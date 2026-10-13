# korg-ledger-py

Pure-Python (stdlib-only) producer and verifier for the **korg-ledger@v1**
tamper-evident ledger format. It is a third independent implementation of the
frozen spec at `spec/korg-ledger-v1/`, pinned to the same conformance vectors
as the Rust and JavaScript references.

`LedgerWriter` produces hash-chained `JournalEvent` JSONL that the Rust
`korg-verify` binary validates byte-for-byte. Every record is redacted under
[`korg-redact@v1`](../../spec/korg-ledger-v1/REDACTION.md) **before** it is hashed
or written (opt out only with `redaction=None`).

    python -m korg_ledger lint ledger.jsonl                 # chain + causal DAG (flat & nested) + redaction
    python -m korg_ledger envelope okto-session S.json --source-version 0.1.0 > env.json
    python -m korg_ledger envelope dazai-log daemon.log --source-version 0.1.0 > env.json
    python -m korg_ledger envelope ningen-sink lifecycle.jsonl > env.json    # dazai/motokano --korg-sink
    python -m korg_ledger import env.json --ledger ledger.jsonl   # korg-import@v1, atomic on rejection
    python -m korg_ledger rechain-redacted old.jsonl new.jsonl    # migrate a journal that leaked a secret

See [`IMPORT.md`](../../spec/korg-ledger-v1/IMPORT.md) for the Okto and
ningen-shikkaku envelope profiles.

    pip install -e adapters/korg-ledger-py
    pytest adapters/korg-ledger-py/tests -v
