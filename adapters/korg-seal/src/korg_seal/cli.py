"""korg-seal — mint and verify korgcert@v1 certificates of AI-agent sessions.

    korg-seal mint  <session.jsonl> --claim "..."   # produce a signed Certificate
    korg-seal verify <seal.json> [--pin <hex>]       # check one (Python; Rust korg-verify is canonical)
    korg-seal key                                     # print the issuer public key

Exit: 0 ok · 1 invalid · 2 usage/IO error.
"""
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

from . import keys
from . import minter


def _key_path(args) -> Path | None:
    return Path(args.key) if getattr(args, "key", None) else None


def _cmd_mint(args) -> int:
    try:
        seed = keys.load_or_create_seed(_key_path(args))
        seal = minter.mint(
            ledger_path=args.ledger,
            claim=args.claim,
            seed=seed,
            issuer_agent=args.issuer,
            issued_at=args.issued_at,
            strict=not args.allow_unverified,
        )
    except (ValueError, OSError, json.JSONDecodeError) as e:
        print(f"korg-seal: {e}", file=sys.stderr)
        return 2

    # --allow-unverified seals a chain that does not verify; never do so silently.
    if args.allow_unverified:
        from korg_ledger import verify_chain, verify_dag

        problems = verify_chain(minter.load_ledger(args.ledger)) + verify_dag(
            minter.load_ledger(args.ledger)
        )
        if problems:
            print(
                f"⚠ WARNING: sealed an UNVERIFIED chain ({len(problems)} problem(s)) "
                "because --allow-unverified was set:",
                file=sys.stderr,
            )
            for p in problems[:8]:
                print(f"    - {p}", file=sys.stderr)

    rendered = json.dumps(seal, indent=2, ensure_ascii=False)
    if args.out:
        Path(args.out).write_text(rendered + "\n", encoding="utf-8")
        print(f"✓ minted Certificate → {args.out}", file=sys.stderr)
    else:
        print(rendered)
    print(f"  issuer {keys.public_key_hex(seed)}", file=sys.stderr)
    print(f"  tip    {seal['tip']}", file=sys.stderr)
    print(f"  events {seal['event_count']}", file=sys.stderr)
    print(
        "  verify anywhere: drop it at https://new1direction.github.io/korg/web/seal.html"
        " · or `korg-verify <seal>`",
        file=sys.stderr,
    )
    return 0


def _cmd_verify(args) -> int:
    from korg_ledger.signing import verify_seal

    try:
        env = json.loads(Path(args.seal).read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as e:
        print(f"korg-seal: {e}", file=sys.stderr)
        return 2

    errors = verify_seal(env, pin_pubkey=args.pin)
    if errors:
        print(f"✗ INVALID — {len(errors)} problem(s):", file=sys.stderr)
        for e in errors[:8]:
            print(f"    - {e}", file=sys.stderr)
        return 1

    seal = env.get("seal", {})
    pub = str(seal.get("pubkey", ""))[:16]
    print(
        f"✓ korgcert VALID — {env.get('event_count')} events, summary re-derived · signed by {pub}…"
    )
    print(f"  claim: {env.get('claim')}")
    return 0


def _cmd_key(args) -> int:
    try:
        seed = keys.load_or_create_seed(_key_path(args))
    except (ValueError, OSError) as e:
        print(f"korg-seal: {e}", file=sys.stderr)
        return 2
    print(keys.public_key_hex(seed))
    return 0


def _cmd_anchor(args) -> int:
    try:
        seal = json.loads(Path(args.seal).read_text(encoding="utf-8"))
        seed = keys.load_or_create_seed(_key_path(args))
        anchored = minter.anchor(
            seal=seal, repo=args.repo, commit=args.commit, seed=seed, seq_id=args.seq
        )
    except (ValueError, OSError, json.JSONDecodeError) as e:
        print(f"korg-seal: {e}", file=sys.stderr)
        return 2
    rendered = json.dumps(anchored, indent=2, ensure_ascii=False)
    out = args.out or args.seal
    Path(out).write_text(rendered + "\n", encoding="utf-8")
    print(f"✓ anchored to {args.repo}@{args.commit[:10]} → {out}", file=sys.stderr)
    print("  resolve it (proves WHEN): korg-seal resolve " + out, file=sys.stderr)
    return 0


def _cmd_resolve(args) -> int:
    from . import resolve as resolver

    try:
        env = json.loads(Path(args.seal).read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as e:
        print(f"korg-seal: {e}", file=sys.stderr)
        return 2

    results = resolver.resolve_seal(env)
    if not results:
        print("korg-seal: no git-tip anchors to resolve", file=sys.stderr)
        return 2

    all_ok = True
    for r in results:
        if r.witnessed:
            print(
                f"✓ seq {r.seq_id} · {r.entry_hash[:12]}… witnessed by "
                f"{r.repo}@{r.commit[:10]}"
            )
            # The committer date is self-asserted (git lets you set it to anything),
            # so it is NOT a trusted clock. The real bound is the publish-before-fetch
            # property of an immutable public commit (SPEC §8.2).
            print(
                f"  → the tip is committed to public git (commit's self-asserted date: {r.committed_at})"
            )
            print(
                "    a public commit proves the chain existed before any third party fetched it —"
                " confirm the commit's first-seen time out of band for a hard bound"
            )
        else:
            all_ok = False
            print(f"✗ seq {r.seq_id} · {r.repo}@{r.commit[:10]}: {r.detail}")
    return 0 if all_ok else 1


def main(argv=None) -> int:
    p = argparse.ArgumentParser(
        prog="korg-seal",
        description="Mint and verify korgcert@v1 certificates of AI-agent sessions.",
    )
    sub = p.add_subparsers(dest="cmd", required=True)

    m = sub.add_parser("mint", help="mint a Certificate from a session ledger")
    m.add_argument("ledger", help="path to a korg-ledger@v1 session (JSONL or JSON array)")
    m.add_argument("--claim", required=True, help="one-line description the issuer attests to")
    m.add_argument("-o", "--out", help="write the seal here (default: stdout)")
    m.add_argument("--issuer", help="issuer agent label (default: derived from the key)")
    m.add_argument("--key", help=f"issuer seed file (default: {keys.DEFAULT_KEY_PATH})")
    m.add_argument("--issued-at", type=int, dest="issued_at", help="Unix seconds (default: now)")
    m.add_argument(
        "--allow-unverified",
        action="store_true",
        help="seal even if the chain does not verify (NOT recommended)",
    )
    m.set_defaults(func=_cmd_mint)

    v = sub.add_parser("verify", help="verify a Certificate (Python; Rust korg-verify is canonical)")
    v.add_argument("seal", help="path to a korgcert@v1 JSON file")
    v.add_argument("--pin", help="require this issuer public key (hex)")
    v.set_defaults(func=_cmd_verify)

    k = sub.add_parser("key", help="print the issuer public key (creating it if absent)")
    k.add_argument("--key", help=f"issuer seed file (default: {keys.DEFAULT_KEY_PATH})")
    k.set_defaults(func=_cmd_key)

    a = sub.add_parser("anchor", help="bind a git-tip time anchor to a seal (re-signs)")
    a.add_argument("seal", help="path to the korgcert@v1 to anchor")
    a.add_argument("--repo", required=True, help="public repo URL the seal was published to")
    a.add_argument("--commit", required=True, help="commit SHA that witnesses the tip")
    a.add_argument("--seq", type=int, help="anchor a specific seq_id (default: the tip)")
    a.add_argument("--key", help=f"issuer seed file (default: {keys.DEFAULT_KEY_PATH})")
    a.add_argument("-o", "--out", help="write here (default: in place)")
    a.set_defaults(func=_cmd_anchor)

    r = sub.add_parser("resolve", help="resolve git-tip anchors over the network — proves WHEN")
    r.add_argument("seal", help="path to a korgcert@v1 with git-tip anchors")
    r.set_defaults(func=_cmd_resolve)

    args = p.parse_args(argv)
    return args.func(args)


if __name__ == "__main__":  # pragma: no cover
    raise SystemExit(main())
