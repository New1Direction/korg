// Invoke a discovered callable: build argv, exec, format by output_mode.

import { spawn } from "node:child_process";
import { buildArgv } from "./args.js";
import type { DiscoveredCallable } from "./discovery.js";

export const DEFAULT_TIMEOUT_MS = 300_000; // 5 minutes
export const SESSION_NOT_SUPPORTED =
  "[korg-introspect-mcp] this callable declares output_mode=session, " +
  "which requires persistent bidirectional I/O. Not supported in v1 — " +
  "run the binary directly for session-mode operations.";

export interface InvocationResult {
  text: string;
  isError: boolean;
}

interface InvokeOptions {
  binaryPath: string;
  binaryName: string;
  timeoutMs?: number;
}

function exec(
  argv: string[],
  timeoutMs: number
): Promise<{ stdout: string; stderr: string; exitCode: number; timedOut: boolean }> {
  return new Promise((resolve) => {
    const [bin, ...rest] = argv;
    if (!bin) {
      resolve({ stdout: "", stderr: "[korg-introspect-mcp] empty argv", exitCode: -1, timedOut: false });
      return;
    }
    const child = spawn(bin, rest, { stdio: ["ignore", "pipe", "pipe"] });
    const stdoutChunks: Buffer[] = [];
    const stderrChunks: Buffer[] = [];
    let timedOut = false;
    child.stdout.on("data", (b: Buffer) => stdoutChunks.push(b));
    child.stderr.on("data", (b: Buffer) => stderrChunks.push(b));

    const timer = setTimeout(() => {
      timedOut = true;
      child.kill("SIGTERM");
    }, timeoutMs);

    child.on("error", (err) => {
      clearTimeout(timer);
      resolve({
        stdout: "",
        stderr: `[korg-introspect-mcp] could not exec ${bin}: ${err.message}`,
        exitCode: -1,
        timedOut: false,
      });
    });
    child.on("close", (code) => {
      clearTimeout(timer);
      resolve({
        stdout: Buffer.concat(stdoutChunks).toString("utf8"),
        stderr: Buffer.concat(stderrChunks).toString("utf8"),
        exitCode: code ?? -1,
        timedOut,
      });
    });
  });
}

export async function invoke(
  callableDef: DiscoveredCallable,
  argumentsMap: Record<string, unknown>,
  opts: InvokeOptions
): Promise<InvocationResult> {
  const cap = callableDef.capabilities as { output_mode?: string };
  const outputMode = cap.output_mode ?? "envelope";
  if (outputMode === "session") {
    return { text: SESSION_NOT_SUPPORTED, isError: true };
  }

  const argv = buildArgv(
    opts.binaryPath,
    callableDef.command_id,
    opts.binaryName,
    argumentsMap
  );
  const timeoutMs = opts.timeoutMs ?? DEFAULT_TIMEOUT_MS;
  const result = await exec(argv, timeoutMs);

  if (result.timedOut) {
    return {
      text: `[korg-introspect-mcp] ${callableDef.command_id} timed out after ${timeoutMs}ms`,
      isError: true,
    };
  }

  const stdout = result.stdout ?? "";
  const stderr = result.stderr ?? "";
  const isError = result.exitCode !== 0;

  let text: string;
  if (outputMode === "envelope") {
    const body = stdout.trim();
    if (body) {
      try {
        const parsed = JSON.parse(body);
        text = JSON.stringify(parsed, null, 2);
      } catch {
        text = body;
      }
    } else {
      text = "";
    }
  } else if (outputMode === "stream") {
    text = stdout;
  } else if (outputMode === "none") {
    text = stdout.trim() || "ok";
  } else {
    text = stdout;
  }

  if (isError) {
    const suffix: string[] = [];
    if (stderr.trim()) suffix.push(`\n[stderr]\n${stderr.trimEnd()}`);
    suffix.push(`\n[exit_code] ${result.exitCode}`);
    text = (text || "").trimEnd() + suffix.join("");
  }

  return { text, isError };
}
