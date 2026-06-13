# korg-ledger-py

Pure-Python (stdlib-only) producer and verifier for the **korg-ledger@v1**
tamper-evident ledger format. It is a third independent implementation of the
frozen spec at `spec/korg-ledger-v1/`, pinned to the same conformance vectors
as the Rust and JavaScript references.

`LedgerWriter` produces hash-chained `JournalEvent` JSONL that the Rust
`korg-verify` binary validates byte-for-byte.

    pip install -e adapters/korg-ledger-py
    pytest adapters/korg-ledger-py/tests -v
