// Verify korg Certificates / ledgers, render a rich Markdown report into the job
// summary, and (on a pull_request) upsert a single sticky PR comment. Reuses the
// repo's conformance-pinned verify.mjs — zero extra deps. Exit 0 iff all valid.
import { appendFileSync, readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const MARKER = "<!-- korg-gold-seal-check -->";
const here = dirname(fileURLToPath(import.meta.url));
const verifyPath = resolve(here, "../../../spec/korg-ledger-v1/js/verify.mjs");
const { verifyText } = await import(verifyPath);

const files = process.argv.slice(2);
const pin = process.env.KORG_PIN || null;

// Neutralize Markdown/HTML so attacker-controlled seal fields (claim, file
// paths, issuer, anchor repo/commit — the seal may even be INVALID) cannot inject
// into the privileged PR comment: escapes code spans (`), tables (|), links/HTML
// (<>[]()), @-mentions, and backslashes, and collapses control chars/newlines.
// Angle-escaping also defangs HTML comments, so a forged MARKER can't be smuggled in.
const esc = (s) =>
  String(s)
    .replace(/[\x00-\x1f]+/g, " ")
    .replace(/[`|<>[\]()@\\]/g, (c) => "\\" + c);

const results = [];
for (const f of files) {
  let text;
  try {
    text = readFileSync(f, "utf8");
  } catch (e) {
    results.push({ f, error: `cannot read: ${e.message}` });
    continue;
  }
  let env = null;
  try {
    env = JSON.parse(text);
  } catch {
    /* journals are JSONL, not a single object */
  }
  try {
    const verdict = await verifyText(text, { pinPubkey: pin });
    results.push({ f, verdict, env });
  } catch (e) {
    results.push({ f, error: `parse: ${e.message}` });
  }
}

const allValid = results.every((r) => r.verdict && r.verdict.valid);

const L = [];
L.push(allValid ? "## 🛡️ ✅ Certificate verified" : "## 🛡️ ❌ Certificate verification FAILED");
L.push("");
L.push("_Independently verified — zero trust in the tool that produced it._");
L.push("");

for (const r of results) {
  if (r.error) {
    L.push(`### ⚠️ \`${r.f}\` — ${r.error}`);
    L.push("");
    continue;
  }
  const v = r.verdict;
  const e = r.env || {};
  L.push(`### ${v.valid ? "✅" : "❌"} \`${r.f}\` — ${v.kind} ${v.valid ? "VALID" : "INVALID"}`);
  L.push("");
  if (v.kind === "korgcert") {
    const s = e.summary || {};
    // Every field below is seal-derived (untrusted) → esc() before interpolation,
    // and NOT wrapped in backticks (esc neutralizes them, which would break a span).
    const who = v.signer ? esc(String(v.signer).slice(0, 16)) + "…" : "unsigned";
    const tools = s.by_tool && typeof s.by_tool === "object"
      ? Object.keys(s.by_tool).sort().map((k) => `${esc(k)}×${esc(s.by_tool[k])}`).join(" ")
      : "";
    const filesList = (Array.isArray(s.files) ? s.files : []).map((x) => esc(x)).join(", ");
    const anchors = (Array.isArray(e.anchors) ? e.anchors : [])
      .map((a) => esc(`${a?.anchor_proof?.repo || "?"}@${String(a?.anchor_proof?.commit || "").slice(0, 10)}`))
      .join(", ");
    L.push("| | |");
    L.push("|---|---|");
    L.push(`| **claim** | ${e.claim ? esc(e.claim) : "—"} |`);
    L.push(`| **who** (issuer) | ${who} |`);
    L.push(`| **what** | ${esc(v.event_count)} events · ${tools} |`);
    if (filesList) L.push(`| **files** | ${filesList} |`);
    if (anchors) L.push(`| **when** (anchor) | ${anchors} — run \`korg-seal resolve\` to confirm the date |`);
    L.push(
      `| **integrity** | chain ${v.chain_ok ? "✓" : "✗"} · summary ${v.summary_ok ? "re-derived ✓" : "✗"} · seal ${v.seal_ok ? "✓" : "✗"} |`
    );
    L.push("");
  }
  if (!v.valid && v.errors?.length) {
    L.push("**problems:**");
    for (const err of v.errors.slice(0, 6)) L.push(`- ${esc(err)}`);
    L.push("");
  }
}

L.push(
  "<sub>Verified by the independent [korg](https://github.com/New1Direction/korg) verifier. " +
    "Re-check in a browser: [seal.html](https://new1direction.github.io/korg/web/seal.html).</sub>"
);
L.push("");
L.push(MARKER);
const body = L.join("\n");

if (process.env.GITHUB_STEP_SUMMARY) {
  appendFileSync(process.env.GITHUB_STEP_SUMMARY, body + "\n");
}

// ── sticky PR comment (pull_request events only) ────────────────────────────
const token = process.env.GH_TOKEN;
const repo = process.env.GITHUB_REPOSITORY;
const wantComment = (process.env.KORG_COMMENT || "true") !== "false";
let pr = null;
if (process.env.GITHUB_EVENT_PATH) {
  try {
    const ev = JSON.parse(readFileSync(process.env.GITHUB_EVENT_PATH, "utf8"));
    pr = ev.pull_request?.number ?? ev.issue?.number ?? null;
  } catch {
    /* not a PR event */
  }
}

if (wantComment && token && repo && pr) {
  const h = {
    Authorization: `Bearer ${token}`,
    Accept: "application/vnd.github+json",
    "User-Agent": "korg-gold-seal",
    "Content-Type": "application/json",
  };
  const issueComments = `https://api.github.com/repos/${repo}/issues/${pr}/comments`;
  try {
    const list = await (await fetch(`${issueComments}?per_page=100`, { headers: h })).json();
    // Only ever update OUR OWN sticky comment: a Bot-authored comment whose
    // trailing line is the marker. A bare body.includes(MARKER) could be tricked
    // into editing an unrelated user comment that merely quotes the marker.
    const existing = Array.isArray(list)
      ? list.find((c) => c.user?.type === "Bot" && (c.body || "").trimEnd().endsWith(MARKER))
      : null;
    if (existing) {
      await fetch(`https://api.github.com/repos/${repo}/issues/comments/${existing.id}`, {
        method: "PATCH",
        headers: h,
        body: JSON.stringify({ body }),
      });
      console.log(`korg: updated sticky Certificate comment ${existing.id} on PR #${pr}`);
    } else {
      await fetch(issueComments, { method: "POST", headers: h, body: JSON.stringify({ body }) });
      console.log(`korg: posted Certificate comment on PR #${pr}`);
    }
  } catch (e) {
    console.log(`::warning::korg could not upsert the PR comment: ${e.message}`);
  }
}

process.exit(allValid ? 0 : 1);
