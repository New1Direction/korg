"""korg-seal — mint and verify goldseal@v1 certificates of AI-agent sessions.

    korg-seal mint  <session.jsonl> --claim "..."   # produce a signed Gold Seal
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

    rendered = json.dumps(seal, indent=2, ensure_ascii=False)
    if args.out:
        Path(args.out).write_text(rendered + "\n", encoding="utf-8")
        print(f"✓ minted Gold Seal → {args.out}", file=sys.stderr)
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
        f"✓ goldseal VALID — {env.get('event_count')} events, summary re-derived · signed by {pub}…"
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


def main(argv=None) -> int:
    p = argparse.ArgumentParser(
        prog="korg-seal",
        description="Mint and verify goldseal@v1 certificates of AI-agent sessions.",
    )
    sub = p.add_subparsers(dest="cmd", required=True)

    m = sub.add_parser("mint", help="mint a Gold Seal from a session ledger")
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

    v = sub.add_parser("verify", help="verify a Gold Seal (Python; Rust korg-verify is canonical)")
    v.add_argument("seal", help="path to a goldseal@v1 JSON file")
    v.add_argument("--pin", help="require this issuer public key (hex)")
    v.set_defaults(func=_cmd_verify)

    k = sub.add_parser("key", help="print the issuer public key (creating it if absent)")
    k.add_argument("--key", help=f"issuer seed file (default: {keys.DEFAULT_KEY_PATH})")
    k.set_defaults(func=_cmd_key)

    args = p.parse_args(argv)
    return args.func(args)


if __name__ == "__main__":  # pragma: no cover
    raise SystemExit(main())
