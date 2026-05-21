# Korg Swarm Security Policy

This file governs the autonomous execution permissions for Korg persona swarms. The Zero-Trust Policy Engine intercepts all agentic tool calls and matches them against this ledger.

## 1. Whitelisted Shell Commands
The following shell commands and their arguments are permitted for execution:
- `cargo check`
- `cargo test`
- `cargo build`
- `git diff`
- `git status`
- `echo`

## 2. Whitelisted Paths
Agents may only read and write within the following directory trees:
- The Korg project root (determined dynamically)
- `/tmp/korg` and subdirectories

## 3. Blacklisted Targets
Any attempt to access or modify the following files or patterns is strictly forbidden:
- `/etc/passwd`
- `/etc/shadow`
- `~/.ssh`
- `id_rsa`
- `.env` files containing secrets

## 4. Swarm Resource Limits
- Max Concurrency: 16 concurrent workers
- Max Token Churn: 50,000 tokens/sec
- Semantic Entropy Threshold: 0.78
