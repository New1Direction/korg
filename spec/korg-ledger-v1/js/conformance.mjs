// korg-ledger@v1 — JS conformance harness (the executable oracle for verify.mjs).
//
// Mirrors ../conformance.py: reads the same ../conformance.json manifest and the
// same frozen ../vectors/, and is conformant iff it reproduces every intact
// vector's tip_entry_hash and flags every tampered vector. Plus a handful of
// canonicalization edge-case assertions (the place JS and Python most easily
// diverge: non-ASCII and astral-plane escaping).
//
//     node conformance.mjs        # exit 0 = this JS impl reproduces the vectors

import { readFileSync } from "node:fs";
import {
  canonicalize,
  chainHash,
  verifyChain,
  verifyEventSig,
  verifyAnchors,
  verifyGoldSeal,
  deriveSummary,
} from "./verify.mjs";

const enc = new TextEncoder();
const dec = new TextDecoder();
const here = (p) => new URL(p, import.meta.url);

function read(name) {
  const text = readFileSync(here(`../vectors/${name}`), "utf8");
  return text
    .split("\n")
    .filter((l) => l.trim())
    .map((l) => JSON.parse(l));
}

function assertCanon(value, expected, label) {
  const got = dec.decode(canonicalize(value));
  const ok = got === expected;
  console.log(`  [${ok ? "PASS" : "FAIL"}] canon ${label.padEnd(20)} ${ok ? "" : `got ${got} want ${expected}`}`);
  return ok ? 0 : 1;
}

async function run() {
  let failures = 0;

  // §2 canonicalization edge cases (match Python json.dumps(ensure_ascii=True) / Rust).
  failures += assertCanon({ z: [3, 2], a: { y: 1, x: 2 } }, '{"a":{"x":2,"y":1},"z":[3,2]}', "sorted+compact");
  failures += assertCanon({ a: "é" }, '{"a":"\\u00e9"}', "non-ascii");
  failures += assertCanon({ a: "𝄞" }, '{"a":"\\ud834\\udd1e"}', "astral surrogate-pair");

  // event_sig is reserved and excluded from the preimage (Phase-2 signature slot).
  {
    const base = { seq_id: 1, prev_hash: "0".repeat(64), x: "y" };
    const signed = { ...base, event_sig: "ZmFrZS1zaWc=" };
    const ok = (await chainHash(base)) === (await chainHash(signed));
    console.log(`  [${ok ? "PASS" : "FAIL"}] event_sig excluded from preimage`);
    if (!ok) failures++;
  }

  // The frozen vectors — the cross-impl oracle.
  const manifest = JSON.parse(readFileSync(here("../conformance.json"), "utf8"));
  if (manifest.spec_version !== "korg-ledger@v1") throw new Error("unexpected spec_version");

  for (const v of manifest.vectors) {
    const events = read(v.file);
    const key = v.key ? enc.encode(v.key) : null;
    const errors = await verifyChain(events, key);
    let ok = true;
    let detail = "";
    if (v.verify === "intact") {
      if (errors.length) {
        ok = false;
        detail = `expected intact, got ${JSON.stringify(errors)}`;
      } else if ((await chainHash(events[events.length - 1], key)) !== v.tip_entry_hash) {
        ok = false;
        detail = "tip_entry_hash not reproduced";
      }
    } else {
      if (!errors.length) {
        ok = false;
        detail = "expected tampered, verified clean";
      } else if (!errors.some((e) => e.includes(v.error_contains))) {
        ok = false;
        detail = `errors ${JSON.stringify(errors)} missing ${JSON.stringify(v.error_contains)}`;
      }
    }
    console.log(`  [${ok ? "PASS" : "FAIL"}] ${v.file.padEnd(26)} ${v.verify.padEnd(8)} ${detail}`);
    if (!ok) failures++;
  }

  // Cross-impl per-event signatures: the frozen signed-events.jsonl fixture was
  // signed by the Python implementation; JS must verify every event_sig (and a
  // one-byte flip must fail) — proving Python-sign / JS-verify Ed25519 interop.
  {
    const fixDir = "../../../crates/korg-verify/tests/fixtures";
    const pubkey = readFileSync(here(`${fixDir}/signed-events.pubkey`), "utf8").trim();
    const events = readFileSync(here(`${fixDir}/signed-events.jsonl`), "utf8")
      .split("\n").filter((l) => l.trim()).map((l) => JSON.parse(l));
    let allOk = events.length > 0;
    for (const e of events) {
      if (!(await verifyEventSig(pubkey, e, e.event_sig))) allOk = false;
    }
    // a tampered signature must be rejected
    const tamperedOk = await verifyEventSig(pubkey, events[0], "00".repeat(64));
    const ok = allOk && !tamperedOk;
    console.log(`  [${ok ? "PASS" : "FAIL"}] signed-events.jsonl       cross-impl Ed25519 (Python→JS)`);
    if (!ok) failures++;
  }

  // Structural anchor verification (matches the Rust + Python verify_anchors).
  {
    const basic = read("basic-intact.jsonl");
    const tip = basic[basic.length - 1];
    const okAnchor = [{ seq_id: tip.seq_id, entry_hash: tip.entry_hash, anchor_kind: "git-tip" }];
    const badAnchor = [{ seq_id: tip.seq_id, entry_hash: "deadbeef" }];
    const ok = verifyAnchors(basic, okAnchor).length === 0 && verifyAnchors(basic, badAnchor).length > 0;
    console.log(`  [${ok ? "PASS" : "FAIL"}] anchors structural        verify_anchors (entry_hash ↔ chain)`);
    if (!ok) failures++;
  }

  // Cross-impl goldseal@v1: the frozen goldseal-v1.json was MINTED BY PYTHON.
  // JS must (a) verify it valid, (b) re-derive the identical summary, and (c)
  // reject a lying summary + a stripped seal — proving Python-mint / JS-verify.
  {
    const env = JSON.parse(
      readFileSync(here("../../../crates/korg-verify/tests/fixtures/goldseal-v1.json"), "utf8")
    );
    const v = await verifyGoldSeal(env);
    const derived = dec.decode(canonicalize(deriveSummary(env.events)));
    const embedded = dec.decode(canonicalize(env.summary));
    const lying = JSON.parse(JSON.stringify(env));
    lying.summary.files = [];
    const stripped = JSON.parse(JSON.stringify(env));
    delete stripped.seal;
    // anchors are bound into the seal: stripping the (signed) anchor must break it
    const unanchored = JSON.parse(JSON.stringify(env));
    delete unanchored.anchors;
    const ok =
      v.valid &&
      v.kind === "goldseal" &&
      v.summary_ok === true &&
      v.anchors_ok === true &&
      derived === embedded &&
      !(await verifyGoldSeal(lying)).valid &&
      !(await verifyGoldSeal(stripped)).valid &&
      !(await verifyGoldSeal(unanchored)).valid;
    console.log(`  [${ok ? "PASS" : "FAIL"}] goldseal-v1.json          cross-impl seal + summary + bound anchors (Python→JS)`);
    if (!ok) failures++;
  }

  // Adversarial robustness: verifyGoldSeal must never throw on hostile input and
  // never return valid for junk or any single-char hash/sig flip (mirrors the
  // Python Hypothesis + Rust proptest fuzz suites). Seeded LCG → reproducible.
  {
    let seed = 0x9e3779b9;
    const rnd = () => ((seed = (seed * 1103515245 + 12345) & 0x7fffffff), seed / 0x7fffffff);
    const randJson = (d) => {
      const r = rnd();
      if (d <= 0 || r < 0.4) return [null, true, Math.floor(rnd() * 1e6) - 5e5, "x".repeat(Math.floor(rnd() * 8)), rnd() * 1e3][Math.floor(rnd() * 5)];
      if (r < 0.7) { const a = []; const n = Math.floor(rnd() * 5); for (let i = 0; i < n; i++) a.push(randJson(d - 1)); return a; }
      const o = {}; const n = Math.floor(rnd() * 5); for (let i = 0; i < n; i++) o["k" + Math.floor(rnd() * 9)] = randJson(d - 1); return o;
    };
    let ok = true;
    const safe = async (v) => { try { return (await verifyGoldSeal(v)).valid === false; } catch { return false; } };
    const crafted = [null, true, 42, "x", [], {}, [1, 2, 3], { events: "nope" }, { events: [1, 2] }, { schema: "goldseal@v1" }, { schema: "goldseal@v1", events: [{}] }, { schema: "goldseal@v1", events: [null] }];
    for (const c of crafted) ok = ok && (await safe(c));
    for (let i = 0; i < 400; i++) ok = ok && (await safe(randJson(4)));

    const fix = JSON.parse(readFileSync(here("../../../crates/korg-verify/tests/fixtures/goldseal-v1.json"), "utf8"));
    for (let i = 0; i < 64; i++) {
      const m = JSON.parse(JSON.stringify(fix));
      const t = m.tip.split(""); t[i] = t[i] === "0" ? "f" : "0"; m.tip = t.join("");
      ok = ok && (await safe(m));
    }
    for (let i = 0; i < 128; i++) {
      const m = JSON.parse(JSON.stringify(fix));
      const s = m.seal.sig.split(""); s[i] = s[i] === "0" ? "f" : "0"; m.seal.sig = s.join("");
      ok = ok && (await safe(m));
    }
    console.log(`  [${ok ? "PASS" : "FAIL"}] goldseal fuzz             no-throw + junk/mutations rejected`);
    if (!ok) failures++;
  }

  console.log(`\nkorg-ledger@v1 conformance (js): ${failures ? `${failures} FAILURE(S)` : "PASS"}`);
  return failures ? 1 : 0;
}

process.exit(await run());
