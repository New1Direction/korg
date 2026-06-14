"""Resolve a git-tip anchor against the public git history — the *when* step.

The offline verifiers prove WHAT happened (chain + re-derived summary) and WHO
attested (the Ed25519 seal). They cannot prove WHEN — Ed25519 carries no time.
This module closes that gap: it fetches the anchor's public commit and confirms
the commit witnesses the anchored ``entry_hash``. A public commit is immutable
once pushed and mirrored, so if it introduced the hash, the chain demonstrably
existed by that commit's date — a "published no later than" bound.

Deliberately separate from the hermetic verifiers (it needs the network) and
stdlib-only (``urllib``), so the dependency-light verification path is untouched.
"""
from __future__ import annotations

import json
import urllib.error
import urllib.request
from dataclasses import dataclass

GITHUB_API = "https://api.github.com"
ANCHOR_KIND_GIT_TIP = "git-tip"


@dataclass
class AnchorResult:
    seq_id: object
    entry_hash: str
    repo: str
    commit: str
    witnessed: bool
    committed_at: str | None
    detail: str


def parse_github_repo(url: str) -> tuple[str, str]:
    """Map a repo URL to ``(owner, name)``. Accepts ``https://github.com/o/n``,
    ``github.com/o/n(.git)``, or ``o/n``."""
    u = url.strip().rstrip("/")
    if u.endswith(".git"):
        u = u[:-4]
    u = u.replace("https://", "").replace("http://", "")
    parts = [p for p in u.split("/") if p]
    # Exact host match — a suffix test like endswith("github.com") would accept a
    # spoofed witness host such as notgithub.com/owner/repo.
    if len(parts) >= 3 and parts[0] in ("github.com", "www.github.com"):
        return parts[1], parts[2]
    if len(parts) == 2 and "." not in parts[0]:
        return parts[0], parts[1]
    raise ValueError(f"unrecognized GitHub repo URL: {url!r}")


def _default_fetch(owner: str, name: str, sha: str, timeout: int = 15) -> dict:
    """Fetch a commit (with file patches + dates) from the public GitHub API."""
    url = f"{GITHUB_API}/repos/{owner}/{name}/commits/{sha}"
    req = urllib.request.Request(
        url,
        headers={"Accept": "application/vnd.github+json", "User-Agent": "korg-seal"},
    )
    with urllib.request.urlopen(req, timeout=timeout) as r:  # noqa: S310 (fixed https host)
        return json.loads(r.read().decode("utf-8"))


def resolve_anchor(anchor: dict, fetch=None) -> AnchorResult:
    """Resolve one git-tip anchor: fetch its commit and confirm it introduced the
    anchored ``entry_hash``. ``fetch(owner, name, sha) -> commit dict`` is injectable
    so this is unit-testable without the network (defaults to the live GitHub API)."""
    if fetch is None:
        fetch = _default_fetch
    seq = anchor.get("seq_id")
    entry_hash = anchor.get("entry_hash")
    proof = anchor.get("anchor_proof") or {}
    repo = str(proof.get("repo", ""))
    commit = str(proof.get("commit", ""))
    res = AnchorResult(seq, entry_hash, repo, commit, False, None, "")

    if anchor.get("anchor_kind") != ANCHOR_KIND_GIT_TIP:
        res.detail = f"unsupported anchor_kind {anchor.get('anchor_kind')!r}"
        return res
    if not isinstance(entry_hash, str) or not commit:
        res.detail = "anchor missing entry_hash or commit"
        return res
    try:
        owner, name = parse_github_repo(repo)
    except ValueError as e:
        res.detail = str(e)
        return res
    try:
        data = fetch(owner, name, commit)
    except urllib.error.HTTPError as e:
        res.detail = f"commit not found / API error ({e.code})"
        return res
    except Exception as e:  # noqa: BLE001 — network/parse errors all surface as a failed resolve
        res.detail = f"fetch failed: {e}"
        return res

    res.committed_at = (((data.get("commit") or {}).get("committer") or {}).get("date"))
    patches = "".join((f.get("patch") or "") for f in (data.get("files") or []))
    res.witnessed = entry_hash in patches
    res.detail = "witnessed" if res.witnessed else "commit found but does not introduce the entry_hash"
    return res


def resolve_seal(env: dict, fetch=None) -> list[AnchorResult]:
    """Resolve every git-tip anchor embedded in a Gold Seal."""
    anchors = env.get("anchors") or []
    return [
        resolve_anchor(a, fetch=fetch)
        for a in anchors
        if isinstance(a, dict) and a.get("anchor_kind") == ANCHOR_KIND_GIT_TIP
    ]
