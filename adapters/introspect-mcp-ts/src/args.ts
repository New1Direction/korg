// Map MCP tool-call arguments to CLI argv. Same convention as the Python
// version: snake_case → kebab-case long flags, bools flag-on-true,
// arrays repeat the flag, command_id dot-segments after the binary name
// become a subcommand path.

export function kebab(name: string): string {
  return name.replace(/_/g, "-");
}

export function valueToArgv(value: unknown): string[] {
  if (typeof value === "boolean") {
    throw new TypeError("bool should not reach valueToArgv; handle at property level");
  }
  if (typeof value === "number") {
    return [String(value)];
  }
  if (typeof value === "string") {
    return [value];
  }
  if (value == null) {
    return [];
  }
  // Fallback for objects: JSON-serialize.
  try {
    return [JSON.stringify(value)];
  } catch {
    return [String(value)];
  }
}

export function buildArgv(
  binaryPath: string,
  commandId: string,
  binaryName: string,
  argumentsMap: Record<string, unknown>
): string[] {
  const argv: string[] = [binaryPath];

  // Subcommand path: split command_id on `.`; segments after the binary
  // name become the subcommand chain.
  const segments = commandId.split(".");
  if (segments.length > 0 && segments[0] === binaryName) {
    argv.push(...segments.slice(1));
  } else {
    argv.push(...segments);
  }

  for (const [key, value] of Object.entries(argumentsMap)) {
    const flag = "--" + kebab(key);
    if (typeof value === "boolean") {
      if (value) argv.push(flag);
      // false → omitted
      continue;
    }
    if (Array.isArray(value)) {
      for (const item of value) {
        argv.push(flag);
        argv.push(...valueToArgv(item));
      }
      continue;
    }
    if (value == null) continue;
    argv.push(flag);
    argv.push(...valueToArgv(value));
  }

  return argv;
}
