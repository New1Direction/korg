// Side-effects gating. Default refuses fs_write / network / ledger_write
// invocations unless KORG_INTROSPECT_MCP_ALLOW opts in.

export const ALWAYS_ALLOWED: ReadonlySet<string> = new Set(["none", "fs_read"]);
export const ALL_EFFECTS: ReadonlySet<string> = new Set([
  "none",
  "fs_read",
  "fs_write",
  "network",
  "ledger_write",
]);

function parseAllowEnv(raw: string | undefined): Set<string> {
  if (!raw) return new Set();
  const v = raw.trim().toLowerCase();
  if (v === "all" || v === "*") return new Set(ALL_EFFECTS);
  return new Set(v.split(",").map((p) => p.trim()).filter(Boolean));
}

export class Policy {
  readonly allowed: ReadonlySet<string>;

  private constructor(allowed: ReadonlySet<string>) {
    this.allowed = allowed;
  }

  static fromEnv(env?: NodeJS.ProcessEnv): Policy {
    const e = env ?? process.env;
    const extra = parseAllowEnv(e["KORG_INTROSPECT_MCP_ALLOW"]);
    const combined = new Set<string>(ALWAYS_ALLOWED);
    for (const v of extra) combined.add(v);
    return new Policy(combined);
  }

  static all(): Policy {
    return new Policy(new Set(ALL_EFFECTS));
  }

  static readOnly(): Policy {
    return new Policy(new Set(ALWAYS_ALLOWED));
  }

  allows(sideEffects: string): boolean {
    return this.allowed.has(sideEffects);
  }

  explainDenial(sideEffects: string): string {
    const sorted = [...this.allowed].sort();
    return (
      `refused: callable declares side_effects=${JSON.stringify(sideEffects)}, ` +
      `which is not in the allow list ${JSON.stringify(sorted)}. ` +
      `To enable, set KORG_INTROSPECT_MCP_ALLOW=${sideEffects} ` +
      `(or KORG_INTROSPECT_MCP_ALLOW=all).`
    );
  }
}
