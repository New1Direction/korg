---
name: Bug Report
about: Something broke or behaved unexpectedly
labels: bug
---

**Describe the bug**
A clear description of what happened.

**Reproduction**
```bash
korg run "..." # command you ran
```

**Session ledger (if available)**
```
# Paste output of: cat ~/.korg/campaigns/<session-id>/journal.ktrans | tail -20
```

**Expected behavior**
What should have happened.

**Environment**
- OS: [e.g. macOS 14, Ubuntu 22.04]
- Korg version: `korg --version`
- Rust version: `rustc --version`
