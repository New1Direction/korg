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
import { canonicalize, chainHash, verifyChain } from "./verify.mjs";

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

  console.log(`\nkorg-ledger@v1 conformance (js): ${failures ? `${failures} FAILURE(S)` : "PASS"}`);
  return failures ? 1 : 0;
}

process.exit(await run());
