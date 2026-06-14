// korg-ledger@v1 — independent JavaScript verifier (the third implementation).
//
// Dependency-free and isomorphic: runs in Node (>=18) and the browser using only
// the Web Crypto standard (`globalThis.crypto.subtle`) — no npm packages, no
// build step. It reproduces the spec's canonicalization + hash-chain from scratch
// (canonical text: ../SPEC.md) and is conformant iff it reproduces the frozen
// tip hashes in ../conformance.json byte-for-byte. The Rust core (korg-registry /
// korg-verify) and the Python reference (../conformance.py) reproduce the same
// vectors — three independent codepaths, one shared oracle.
//
// CLI:  node verify.mjs <receipt.json | journal.jsonl> [--key <str>] [--pubkey <hex>] [--json]
//       exit 0 = valid · 1 = invalid/tampered · 2 = usage/parse error

const GENESIS = "0".repeat(64);
const HASH_FIELDS = ["entry_hash", "event_sig"]; // excluded from preimage (event_sig = reserved Phase-2 signature slot)

const subtle = globalThis.crypto && globalThis.crypto.subtle;
const enc = new TextEncoder();

// ── §2 Canonicalization ─────────────────────────────────────────────────────
// JSON, object keys sorted by code point, no insignificant whitespace, non-ASCII
// (and anything outside printable 0x20..0x7e) escaped as lowercase \uXXXX. Output
// is pure ASCII, so there is no UTF-8 encoding ambiguity across languages.

function canonicalJsonString(s) {
  let out = '"';
  for (let i = 0; i < s.length; i++) {
    const c = s.charCodeAt(i); // UTF-16 code unit (surrogate pairs → two \uXXXX, matching Python ensure_ascii)
    switch (c) {
      case 0x22: out += '\\"'; break;
      case 0x5c: out += "\\\\"; break;
      case 0x0a: out += "\\n"; break;
      case 0x0d: out += "\\r"; break;
      case 0x09: out += "\\t"; break;
      case 0x08: out += "\\b"; break;
      case 0x0c: out += "\\f"; break;
      default:
        if (c >= 0x20 && c <= 0x7e) out += s[i];
        else out += "\\u" + c.toString(16).padStart(4, "0");
    }
  }
  return out + '"';
}

function canonicalString(value) {
  if (value === null) return "null";
  if (value === true) return "true";
  if (value === false) return "false";
  const t = typeof value;
  if (t === "number") {
    if (!Number.isInteger(value)) {
      throw new Error(`floats are out of korg-ledger@v1 canonicalization scope: ${value}`);
    }
    return String(value);
  }
  if (t === "string") return canonicalJsonString(value);
  if (Array.isArray(value)) return "[" + value.map(canonicalString).join(",") + "]";
  if (t === "object") {
    // Default sort is by UTF-16 code unit, which equals code-point order for the
    // BMP identifier keys korg emits (matches Python sort_keys / Rust keys.sort()).
    const keys = Object.keys(value).sort();
    return "{" + keys.map((k) => canonicalJsonString(k) + ":" + canonicalString(value[k])).join(",") + "}";
  }
  throw new Error(`unsupported JSON value of type ${t}`);
}

/** Canonical byte encoding of a JSON value (pure ASCII). */
export function canonicalize(value) {
  return enc.encode(canonicalString(value));
}

function toHex(buf) {
  const b = new Uint8Array(buf);
  let s = "";
  for (const x of b) s += x.toString(16).padStart(2, "0");
  return s;
}

function hexToBytes(hex) {
  if (typeof hex !== "string" || hex.length % 2 !== 0) return null;
  const out = new Uint8Array(hex.length / 2);
  for (let i = 0; i < out.length; i++) {
    const v = parseInt(hex.substr(i * 2, 2), 16);
    if (Number.isNaN(v)) return null;
    out[i] = v;
  }
  return out;
}

function bytesEqual(a, b) {
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) if (a[i] !== b[i]) return false;
  return true;
}

// ── §3 entry_hash ────────────────────────────────────────────────────────────
// preimage = canonicalize(event minus entry_hash); SHA-256, or HMAC-SHA256 when a
// key is present. `prev_hash` is kept in the preimage — that is the chain link.

/** @param {object} event @param {Uint8Array|null} keyBytes @returns {Promise<string>} lowercase hex */
export async function chainHash(event, keyBytes = null) {
  const obj = { ...event };
  for (const f of HASH_FIELDS) delete obj[f];
  const data = canonicalize(obj);
  if (keyBytes != null) {
    const key = await subtle.importKey("raw", keyBytes, { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
    return toHex(await subtle.sign("HMAC", key, data));
  }
  return toHex(await subtle.digest("SHA-256", data));
}

// ── §5 Verification ──────────────────────────────────────────────────────────

/** Recompute the hash-chain. Returns [] iff intact; each error names a seq_id. */
export async function verifyChain(events, keyBytes = null) {
  const errors = [];
  let expectedPrev = GENESIS;
  for (const e of events) {
    const sid = e.seq_id;
    const stored = e.entry_hash;
    if (stored == null) {
      errors.push(`seq ${sid}: missing entry_hash (event is not chained)`);
      expectedPrev = null;
      continue;
    }
    if (e.prev_hash !== expectedPrev) {
      errors.push(`seq ${sid}: prev_hash breaks the chain (an event was inserted, deleted, or reordered)`);
    }
    if ((await chainHash(e, keyBytes)) !== stored) {
      errors.push(`seq ${sid}: entry_hash mismatch (content was tampered)`);
    }
    expectedPrev = stored;
  }
  return errors;
}

/** Check the causal DAG: unique seq_ids and strictly-earlier triggered_by links. */
export function verifyDag(events) {
  const errors = [];
  const seqs = events.map((e) => e.seq_id).filter((s) => typeof s === "number");
  const seqset = new Set(seqs);
  if (seqset.size !== seqs.length) errors.push("duplicate seq_id present");
  for (const e of events) {
    const tb = e.triggered_by;
    if (typeof tb !== "number") continue;
    const sid = e.seq_id;
    if (!seqset.has(tb)) errors.push(`seq ${sid}: triggered_by ${tb} does not exist`);
    else if (typeof sid === "number" && tb >= sid) {
      errors.push(`seq ${sid}: triggered_by ${tb} is not strictly earlier`);
    }
  }
  return errors;
}

/**
 * Verify an Ed25519 signature over the RAW tip-hash bytes (the 32 decoded bytes,
 * not the hex string) — matching the Rust verifier and `sign_tip`. Any malformed
 * input or unsupported algorithm returns false rather than throwing.
 */
export async function verifyTipSig(pubkeyHex, tipHex, sigHex) {
  try {
    const pk = hexToBytes(pubkeyHex);
    const msg = hexToBytes(tipHex);
    const sig = hexToBytes(sigHex);
    if (!pk || !msg || !sig || pk.length !== 32 || sig.length !== 64) return false;
    const key = await subtle.importKey("raw", pk, { name: "Ed25519" }, false, ["verify"]);
    return await subtle.verify({ name: "Ed25519" }, key, sig, msg);
  } catch {
    return false;
  }
}

/**
 * Verify an Ed25519 `event_sig` over an event's canonical preimage (the event
 * minus HASH_FIELDS, canonicalized) — the per-event analogue of verifyTipSig.
 * Byte-identical message to the Rust `verify_event_sig` and Python signing.
 * Any malformed input or unsupported algorithm returns false rather than throwing.
 */
export async function verifyEventSig(pubkeyHex, event, sigHex) {
  try {
    const pk = hexToBytes(pubkeyHex);
    const sig = hexToBytes(sigHex);
    if (!pk || !sig || pk.length !== 32 || sig.length !== 64) return false;
    const obj = { ...event };
    for (const f of HASH_FIELDS) delete obj[f];
    const msg = canonicalize(obj);
    const key = await subtle.importKey("raw", pk, { name: "Ed25519" }, false, ["verify"]);
    return await subtle.verify({ name: "Ed25519" }, key, sig, msg);
  } catch {
    return false;
  }
}

/**
 * Structural verification of an anchors.jsonl sidecar against a verified chain:
 * each anchor's `entry_hash` must match the chain event at its `seq_id`. Always
 * hermetic (no network). Returns [] iff every anchor matches. The external
 * git-tip proof (the actual owner-rewrite defense) is checked by the Rust verifier.
 */
export function verifyAnchors(chain, anchors) {
  const errors = [];
  const bySeq = new Map(chain.map((e) => [e.seq_id, e]));
  for (const a of anchors) {
    const seq = a.seq_id;
    const want = a.entry_hash;
    if (seq == null || want == null) {
      errors.push("anchor record missing seq_id or entry_hash");
      continue;
    }
    const e = bySeq.get(seq);
    if (!e) errors.push(`anchor seq ${seq}: no event with that seq_id in the chain`);
    else if (e.entry_hash !== want) errors.push(`anchor seq ${seq}: entry_hash does not match the chain`);
  }
  return errors;
}

/** Verify a list of events as a journal: hash chain + causal DAG. */
export async function verifyJournal(events, keyBytes = null) {
  const errors = await verifyChain(events, keyBytes);
  const dag = verifyDag(events);
  const chainOk = errors.length === 0;
  const dagOk = dag.length === 0;
  return {
    valid: chainOk && dagOk,
    kind: "journal",
    event_count: events.length,
    chain_ok: chainOk,
    dag_ok: dagOk,
    tip_ok: true,
    signature_ok: null,
    signer: null,
    errors: errors.concat(dag),
  };
}

/**
 * Verify a receipt object: embedded events (chain + DAG), the recorded tip matches
 * the chain head, and — if signed — the Ed25519 signature is valid for that tip.
 * `pinPubkey` requires the signer to equal a key the relying party already trusts.
 */
export async function verifyReceipt(receipt, { key = null, pinPubkey = null } = {}) {
  const events = Array.isArray(receipt.events) ? receipt.events : [];
  const errors = await verifyChain(events, key);
  const dag = verifyDag(events);
  const chainOk = errors.length === 0;
  const dagOk = dag.length === 0;
  for (const e of dag) errors.push(e);

  const claimedTip = typeof receipt.tip === "string" ? receipt.tip : null;
  const head = events.length ? events[events.length - 1].entry_hash : null;
  let tipOk;
  if (claimedTip == null) tipOk = true;
  else if (head == null) tipOk = false;
  else tipOk = claimedTip === head;
  if (!tipOk) errors.push("recorded tip does not match the chain head");

  let signatureOk = null;
  let signer = null;
  if (receipt.signature) {
    const pubkey = receipt.signature.pubkey || "";
    const sigHex = receipt.signature.sig || "";
    let ok = await verifyTipSig(pubkey, claimedTip || "", sigHex);
    signer = pubkey;
    if (!ok) errors.push("signature does not verify for the recorded tip");
    if (pinPubkey != null && pinPubkey !== pubkey) {
      ok = false;
      errors.push(`signer ${pubkey} does not match the pinned key ${pinPubkey}`);
    }
    signatureOk = ok;
  } else if (pinPubkey != null) {
    signatureOk = false;
    errors.push(`receipt is unsigned but signer ${pinPubkey} was required`);
  }

  const valid = chainOk && dagOk && tipOk && signatureOk !== false;
  return {
    valid,
    kind: "receipt",
    event_count: events.length,
    chain_ok: chainOk,
    dag_ok: dagOk,
    tip_ok: tipOk,
    signature_ok: signatureOk,
    signer,
    errors,
  };
}

// ── goldseal@v1 ──────────────────────────────────────────────────────────────
// A Gold Seal is a public, independently-verifiable certificate: a receipt
// superset that additionally binds a human-legible summary (re-derived from the
// events, so it cannot lie) and an issuer Ed25519 seal over the canonical header.
// Byte-identical derivation + header to the Python (korg_ledger.goldseal) and
// Rust (korg-verify) implementations.

const GOLDSEAL_NON_HEADER = ["events", "seal", "anchors"];

function eventView(e) {
  const inner = e && e.event;
  if (inner && typeof inner === "object" && !Array.isArray(inner)) {
    return [inner.source_agent, inner.tool_name, inner.args];
  }
  return [e.source_agent, e.tool_name, e.args];
}

/** Deterministically derive the bound summary from the event chain. */
export function deriveSummary(events) {
  const byTool = {};
  const files = new Set();
  const agents = new Set();
  for (const e of events) {
    const [agent, tool, args] = eventView(e);
    if (typeof tool === "string") byTool[tool] = (byTool[tool] || 0) + 1;
    if (typeof agent === "string") agents.add(agent);
    if (args && typeof args === "object" && !Array.isArray(args)) {
      for (const k of ["file_path", "path"]) {
        if (typeof args[k] === "string") files.add(args[k]);
      }
    }
  }
  const seqs = events.map((e) => e.seq_id).filter((s) => typeof s === "number" && Number.isInteger(s));
  const byToolSorted = {};
  for (const k of Object.keys(byTool).sort()) byToolSorted[k] = byTool[k];
  return {
    agents: [...agents].sort(),
    by_tool: byToolSorted,
    files: [...files].sort(),
    seq_first: seqs.length ? Math.min(...seqs) : 0,
    seq_last: seqs.length ? Math.max(...seqs) : 0,
  };
}

/** The signed portion of a Gold Seal: the envelope minus events/seal/anchors. */
export function sealHeader(envelope) {
  const h = { ...envelope };
  for (const k of GOLDSEAL_NON_HEADER) delete h[k];
  return h;
}

/**
 * Verify an Ed25519 seal signature over a header's canonical bytes — the
 * seal-level analogue of verifyEventSig. False on any error.
 */
export async function verifySealSig(pubkeyHex, header, sigHex) {
  try {
    const pk = hexToBytes(pubkeyHex);
    const sig = hexToBytes(sigHex);
    if (!pk || !sig || pk.length !== 32 || sig.length !== 64) return false;
    const msg = canonicalize(header);
    const key = await subtle.importKey("raw", pk, { name: "Ed25519" }, false, ["verify"]);
    return await subtle.verify({ name: "Ed25519" }, key, sig, msg);
  } catch {
    return false;
  }
}

/**
 * Verify a goldseal@v1 envelope: chain + DAG, tip matches head, event_count,
 * the re-derived summary matches byte-for-byte, and the Ed25519 seal signature.
 * `pinPubkey` requires the issuer to equal a key the relying party trusts.
 */
export async function verifyGoldSeal(envelope, { pinPubkey = null } = {}) {
  const events = Array.isArray(envelope.events) ? envelope.events : [];
  const errors = await verifyChain(events);
  const dag = verifyDag(events);
  const chainOk = errors.length === 0;
  const dagOk = dag.length === 0;
  for (const e of dag) errors.push(e);

  const claimedTip = typeof envelope.tip === "string" ? envelope.tip : null;
  const head = events.length ? events[events.length - 1].entry_hash : null;
  const tipOk = claimedTip == null ? false : head != null && claimedTip === head;
  if (!tipOk) errors.push("recorded tip does not match the chain head");

  const countOk = envelope.event_count === events.length;
  if (!countOk) {
    errors.push(`event_count ${envelope.event_count} does not match the ${events.length} embedded events`);
  }

  let summaryOk = false;
  try {
    summaryOk = bytesEqual(canonicalize(envelope.summary ?? null), canonicalize(deriveSummary(events)));
  } catch {
    summaryOk = false;
  }
  if (!summaryOk) errors.push("summary does not match the events (re-derivation mismatch)");

  let anchorsOk = null;
  if (Array.isArray(envelope.anchors) && envelope.anchors.length) {
    const ae = verifyAnchors(events, envelope.anchors);
    anchorsOk = ae.length === 0;
    for (const e of ae) errors.push(e);
  }

  let sealOk = null;
  let signer = null;
  const seal = envelope.seal;
  if (seal && typeof seal === "object") {
    signer = typeof seal.pubkey === "string" ? seal.pubkey : "";
    let ok = await verifySealSig(signer, sealHeader(envelope), seal.sig || "");
    if (!ok) errors.push("seal signature does not verify for the header");
    if (pinPubkey != null && pinPubkey !== signer) {
      ok = false;
      errors.push(`issuer ${signer} does not match the pinned key ${pinPubkey}`);
    }
    sealOk = ok;
  } else {
    // A goldseal@v1 envelope MUST carry a seal — a stripped seal is a downgrade,
    // not a valid (merely unsigned) artifact. Fails the verdict in all impls.
    sealOk = false;
    errors.push(
      pinPubkey != null
        ? `seal is absent but signer ${pinPubkey} was required`
        : "seal is absent (unsigned Gold Seal)"
    );
  }

  const valid = chainOk && dagOk && tipOk && countOk && summaryOk && sealOk !== false && anchorsOk !== false;
  return {
    valid,
    kind: "goldseal",
    event_count: events.length,
    chain_ok: chainOk,
    dag_ok: dagOk,
    tip_ok: tipOk,
    summary_ok: summaryOk,
    seal_ok: sealOk,
    signature_ok: sealOk, // alias so the generic CLI line works
    anchors_ok: anchorsOk,
    signer,
    claim: typeof envelope.claim === "string" ? envelope.claim : null,
    summary: envelope.summary ?? null,
    errors,
  };
}

/** Parse a journal from a JSON array or JSON Lines. */
export function loadEvents(text) {
  const trimmed = text.trimStart();
  if (trimmed.startsWith("[")) return JSON.parse(text);
  const out = [];
  text.split("\n").forEach((line, i) => {
    const t = line.trim();
    if (!t) return;
    try {
      out.push(JSON.parse(t));
    } catch (e) {
      throw new Error(`line ${i + 1}: ${e.message}`);
    }
  });
  return out;
}

/** Auto-detect a receipt vs a journal and verify accordingly. */
export async function verifyText(text, { key = null, pinPubkey = null } = {}) {
  const trimmed = text.trimStart();
  if (trimmed.startsWith("{")) {
    let v;
    try {
      v = JSON.parse(text);
    } catch {
      v = null;
    }
    if (v && typeof v === "object" && !Array.isArray(v)) {
      if (typeof v.schema === "string" && v.schema.startsWith("goldseal")) {
        return verifyGoldSeal(v, { pinPubkey });
      }
      const isReceipt =
        v.events !== undefined || (typeof v.schema === "string" && v.schema.startsWith("korgex-receipt"));
      if (isReceipt) return verifyReceipt(v, { key, pinPubkey });
      return verifyJournal([v], key);
    }
  }
  return verifyJournal(loadEvents(text), key);
}

// ── CLI (Node only) ──────────────────────────────────────────────────────────

export async function cli(argv) {
  const { readFileSync } = await import("node:fs");
  let file = null;
  let key = null;
  let pin = null;
  let jsonOut = false;
  const HELP =
    "korg-verify (js) — verify a korg receipt or journal (hash chain + causal DAG + Ed25519)\n\n" +
    "USAGE:\n  node verify.mjs <file> [--key <str>] [--pubkey <hex>] [--json]\n\n" +
    "EXIT:  0 VALID    1 INVALID/tampered    2 usage/parse error\n";
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--key") key = enc.encode(argv[++i] ?? "");
    else if (a === "--pubkey") pin = argv[++i] ?? null;
    else if (a === "--json") jsonOut = true;
    else if (a === "-h" || a === "--help") {
      process.stdout.write(HELP);
      return 2;
    } else if (!a.startsWith("-") && file === null) file = a;
    else {
      process.stderr.write(`unknown argument: ${a}\n\n${HELP}`);
      return 2;
    }
  }
  if (!file) {
    process.stderr.write(HELP);
    return 2;
  }
  let text;
  try {
    text = readFileSync(file, "utf8");
  } catch (e) {
    process.stderr.write(`cannot read ${file}: ${e.message}\n`);
    return 2;
  }
  let v;
  try {
    v = await verifyText(text, { key, pinPubkey: pin });
  } catch (e) {
    process.stderr.write(`parse error: ${e.message}\n`);
    return 2;
  }
  if (jsonOut) {
    process.stdout.write(JSON.stringify(v) + "\n");
  } else if (v.valid) {
    const signed = v.signer && v.signature_ok ? ` · signed by ${v.signer.slice(0, 16)}…` : "";
    process.stdout.write(`  ✓ ${v.kind} VALID — ${v.event_count} events, hash-chain + DAG intact${signed}\n    ${file}\n`);
  } else {
    process.stdout.write(`  ✗ ${v.kind} INVALID — ${v.errors.length} problem(s):\n`);
    for (const e of v.errors.slice(0, 8)) process.stdout.write(`      - ${e}\n`);
  }
  return v.valid ? 0 : 1;
}

// Run the CLI only when invoked directly as a Node script (browser/import-safe).
if (typeof process !== "undefined" && process.argv?.[1]) {
  const { pathToFileURL } = await import("node:url");
  if (import.meta.url === pathToFileURL(process.argv[1]).href) {
    process.exit(await cli(process.argv.slice(2)));
  }
}
