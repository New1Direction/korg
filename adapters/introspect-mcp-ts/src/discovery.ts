// Discover a binary's callables by execing `<binary> --introspect`
// and validating the korg:introspect@v1 document.

import { spawn } from "node:child_process";
import { promises as fs } from "node:fs";
import { resolve as resolvePath } from "node:path";
import { homedir, platform } from "node:os";
import { delimiter } from "node:path";

export const SUPPORTED_SCHEMA = "korg:introspect@v1";
export const DISCOVERY_TIMEOUT_MS = 15_000;

export class DiscoveryError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "DiscoveryError";
  }
}

export interface DiscoveredCallable {
  command_id: string;
  name: string;
  description: string;
  input_schema: Record<string, unknown>;
  capabilities: Record<string, unknown>;
  surfaces: string[];
}

export interface DiscoveredBinary {
  binary_path: string;
  schema: string;
  binary_name: string;
  version: string;
  callables: DiscoveredCallable[];
  exit_codes: Record<string, string>;
  callables_declared: boolean;
}

export function findCallableByCommandId(
  binary: DiscoveredBinary,
  commandId: string
): DiscoveredCallable | null {
  return binary.callables.find((c) => c.command_id === commandId) ?? null;
}

// ── PATH lookup (Node 18 has no built-in shutil.which) ────────────────

function expanduser(p: string): string {
  if (p.startsWith("~")) return resolvePath(homedir() + p.slice(1));
  return resolvePath(p);
}

async function which(name: string): Promise<string | null> {
  // If it's a path (contains separator), test it directly.
  if (name.includes("/") || name.includes("\\")) {
    const p = expanduser(name);
    try {
      const stat = await fs.stat(p);
      if (stat.isFile()) return p;
    } catch {
      return null;
    }
    return null;
  }
  const pathEnv = process.env["PATH"] ?? "";
  const exts = platform() === "win32" ? (process.env["PATHEXT"] ?? "").split(";") : [""];
  for (const dir of pathEnv.split(delimiter)) {
    if (!dir) continue;
    for (const ext of exts) {
      const full = resolvePath(dir, name + ext);
      try {
        const stat = await fs.stat(full);
        if (stat.isFile()) return full;
      } catch {
        // try next
      }
    }
  }
  return null;
}

export async function resolveBinary(spec: string): Promise<string> {
  if (spec.includes("/") || spec.includes("\\") || spec.startsWith("~")) {
    const p = expanduser(spec);
    try {
      const stat = await fs.stat(p);
      if (stat.isFile()) return p;
    } catch {
      throw new DiscoveryError(`binary not found at path: ${p}`);
    }
    throw new DiscoveryError(`binary not a regular file: ${p}`);
  }
  const found = await which(spec);
  if (!found) {
    throw new DiscoveryError(
      `binary not found on PATH: ${JSON.stringify(spec)}. ` +
        `If you meant a relative path, pass it as ./${spec}`
    );
  }
  return found;
}

// ── Spawn + capture ───────────────────────────────────────────────────

interface SpawnResult {
  stdout: string;
  stderr: string;
  exitCode: number;
}

function execIntrospect(binaryPath: string, timeoutMs: number): Promise<SpawnResult> {
  return new Promise<SpawnResult>((resolve, reject) => {
    const child = spawn(binaryPath, ["--introspect"], {
      stdio: ["ignore", "pipe", "pipe"],
    });
    const stdout: Buffer[] = [];
    const stderr: Buffer[] = [];
    child.stdout.on("data", (b: Buffer) => stdout.push(b));
    child.stderr.on("data", (b: Buffer) => stderr.push(b));

    const timer = setTimeout(() => {
      child.kill("SIGTERM");
      reject(new DiscoveryError(`${binaryPath} --introspect timed out after ${timeoutMs}ms`));
    }, timeoutMs);

    child.on("error", (err) => {
      clearTimeout(timer);
      reject(new DiscoveryError(`could not exec ${binaryPath}: ${err.message}`));
    });
    child.on("close", (code) => {
      clearTimeout(timer);
      resolve({
        stdout: Buffer.concat(stdout).toString("utf8"),
        stderr: Buffer.concat(stderr).toString("utf8"),
        exitCode: code ?? -1,
      });
    });
  });
}

export async function runIntrospect(
  binaryPath: string,
  timeoutMs: number = DISCOVERY_TIMEOUT_MS
): Promise<unknown> {
  const result = await execIntrospect(binaryPath, timeoutMs);
  // Some clap-derived CLIs exit non-zero for --introspect (e.g. when other
  // required args are missing). Try to parse stdout regardless; only fail
  // if the parse also fails.
  try {
    return JSON.parse(result.stdout);
  } catch (e) {
    if (result.exitCode !== 0) {
      throw new DiscoveryError(
        `${binaryPath} --introspect exited ${result.exitCode}: ` +
          (result.stderr.trim() || result.stdout.trim() || "(no output)")
      );
    }
    throw new DiscoveryError(
      `${binaryPath} --introspect did not return valid JSON: ${(e as Error).message}. ` +
        `First 200 chars: ${JSON.stringify(result.stdout.slice(0, 200))}`
    );
  }
}

// ── Validation ────────────────────────────────────────────────────────

export function validateDocument(doc: unknown, binaryPath: string): DiscoveredBinary {
  if (typeof doc !== "object" || doc === null) {
    throw new DiscoveryError(`${binaryPath}: introspect document is not an object`);
  }
  const d = doc as Record<string, unknown>;
  const schema = d["schema"];
  if (schema !== SUPPORTED_SCHEMA) {
    throw new DiscoveryError(
      `${binaryPath}: unsupported introspect schema ${JSON.stringify(schema)}. ` +
        `Supported: ${SUPPORTED_SCHEMA}`
    );
  }
  const rawCallables = d["callables"];
  if (!Array.isArray(rawCallables)) {
    throw new DiscoveryError(`${binaryPath}: introspect document has no 'callables' array`);
  }

  const seenIds = new Set<string>();
  const callables: DiscoveredCallable[] = [];
  for (let i = 0; i < rawCallables.length; i++) {
    const c = rawCallables[i];
    if (typeof c !== "object" || c === null) {
      throw new DiscoveryError(`${binaryPath}: callables[${i}] is not an object`);
    }
    const co = c as Record<string, unknown>;
    for (const required of ["command_id", "name", "input_schema", "capabilities"]) {
      if (!(required in co)) {
        throw new DiscoveryError(
          `${binaryPath}: callables[${i}] missing required field ${JSON.stringify(required)}`
        );
      }
    }
    const cid = String(co["command_id"]);
    if (seenIds.has(cid)) {
      throw new DiscoveryError(`${binaryPath}: duplicate command_id ${JSON.stringify(cid)}`);
    }
    seenIds.add(cid);
    const surfacesRaw = co["surfaces"];
    callables.push({
      command_id: cid,
      name: String(co["name"]),
      description: typeof co["description"] === "string" ? co["description"] : "",
      input_schema: co["input_schema"] as Record<string, unknown>,
      capabilities: co["capabilities"] as Record<string, unknown>,
      surfaces: Array.isArray(surfacesRaw) ? surfacesRaw.map((s) => String(s)) : [],
    });
  }

  return {
    binary_path: binaryPath,
    schema,
    binary_name: String(d["binary"] ?? ""),
    version: String(d["version"] ?? "0.0.0"),
    callables,
    exit_codes: (d["exit_codes"] as Record<string, string>) ?? {},
    callables_declared: Boolean(d["callables_declared"]),
  };
}

export async function discover(
  spec: string,
  timeoutMs: number = DISCOVERY_TIMEOUT_MS
): Promise<DiscoveredBinary> {
  const binaryPath = await resolveBinary(spec);
  const rawDoc = await runIntrospect(binaryPath, timeoutMs);
  return validateDocument(rawDoc, binaryPath);
}
