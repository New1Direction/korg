# Contributing to Korg

Thank you for your interest in contributing to **Korg — the first deterministic cognitive runtime**.

---

## Getting Started

```bash
git clone https://github.com/New1Direction/korg
cd korg
cargo build
cargo test  # should show: 130 passed; 0 failed
```

## How to Contribute

- **Bug reports**: Open an issue with a minimal reproduction case and the session ledger from `~/.korg/campaigns/<session-id>/`
- **Feature requests**: Open a discussion in the Issues tab with the label `enhancement`
- **Code contributions**: Fork → branch → PR against `main`

## Architecture Principles

Before contributing, please read the core invariants that **must not be violated**:

1. **The ledger is the only source of truth.** No state mutation occurs outside a ledger append.
2. **Projections are pure folds.** They read events; they never write them.
3. **The CapabilityResolver is the single authority for all runtime state.** No secondary state stores.
4. **Append-only is non-negotiable.** Truncation is a controlled operation (rewind), never a side effect.

## Running Tests

```bash
cargo test                          # All 130 tests
cargo test registry::               # Registry/ledger tests only
cargo test leader::tests::          # Orchestrator tests only
cargo test -- --nocapture           # With full output
```

## License

By contributing, you agree your contributions will be licensed under MIT OR Apache-2.0.
