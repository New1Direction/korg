# Korg Installation Guide

This document provides step-by-step instructions to install, build, and configure the **Korg / Yvaeh Mode** multi-agent runtime on macOS, Linux, and Docker containers.

---

## 📋 System Prerequisites

Before installing Korg, ensure your machine satisfies the following baseline requirements:

| Dependency | Required Version | Purpose |
|:---|:---|:---|
| **Rust / Cargo** | `1.75.0` or higher | Core compiler toolchain for source builds. |
| **C Compiler** | GCC or Clang | Required to compile `mimalloc` and system wrappers. |
| **OpenSSL / pkg-config** | Latest stable | Required by the HTTP connection client (`reqwest`). |
| **Git** | `2.30.0` or higher | Used by the unified patch manager for patch checkouts. |

---

## 🍺 Option 1: Install via Homebrew (macOS)

For macOS operators, Korg is packaged in a custom Homebrew tap. Run the following commands to tap and install the binary:

```bash
# Tap the official repository
brew tap clubpenguin/korg

# Install Korg
brew install korg

# Verify the installation
korg --version
```

---

## 🐳 Option 2: Run via Docker (Linux / Windows / macOS)

Korg is published as a lightweight, secure scratch container on the GitHub Container Registry.

### 1. Pull the Image
```bash
docker pull ghcr.io/clubpenguin/korg:latest
```

### 2. Run the Swarm Container
Launch Korg interactively, passing your LLM provider keys and mounting your active codebase workspace to the `/workspace` mount point:

```bash
docker run -it \
  -e OPENAI_API_KEY="your-openai-key" \
  -e ANTHROPIC_API_KEY="your-anthropic-key" \
  -v "$(pwd)":/workspace \
  ghcr.io/clubpenguin/korg:latest \
  "Refactor the billing endpoint in /workspace/src/billing.rs"
```

---

## 🛠️ Option 3: Compile from Source (Recommended for Devs)

Compiling from source gives you access to feature switches, custom persona templates, and local optimization profiles.

### 1. Clone the Repository
```bash
git clone https://github.com/clubpenguin/Korg.git
cd Korg
```

### 2. Configure Cognitive Keys
Korg checks your standard shell environment variables. Export the keys for the providers you plan to use:

```bash
# For OpenAI models (gpt-4o, etc.)
export OPENAI_API_KEY="sk-..."

# For Anthropic Claude models (claude-3-5-sonnet, etc.)
export ANTHROPIC_API_KEY="sk-ant-..."

# For xAI Grok models
export GROK_API_KEY="xai-..."

# For Local Ollama (no key required, but ensure ollama server is running)
# default targets http://localhost:11434
```

### 3. Feature Selection & Compilation

Korg compiles in **two separate modes** depending on whether you want a zero-setup offline run or a heavy adversarial run with local BERT sentence embeddings:

#### Mode A: Heavy Adversarial (Default, Real Embeddings)
Compiles Korg with Hugging Face BERT (`all-MiniLM-L6-v2`) via Hugging Face Hub and Rust Candle. This downloads tokenizers and weights locally on the first run to calculate true mathematical semantic cosine similarity vectors:

```bash
# Compile in release mode with candle embeddings enabled
cargo build --release

# The executable will be generated at:
# target/release/korg
```

#### Mode B: Zero-Setup Offline (Lightweight, No BERT)
If you want a lightning-fast build that skips compiling heavy neural network architectures and doesn't download weight files, compile with `--no-default-features`:

```bash
# Compile with fake embeddings fallback
cargo build --release --no-default-features
```

---

## 🧪 Post-Installation Sanity Checks

Validate that your Korg installation is fully operational by executing the built-in test suite:

```bash
# Run the complete unit and integration test suite
cargo test
```
All **21/21 tests should pass cleanly** in less than 3 seconds.

### Launch a Mock Campaign
To confirm TUI raw-terminal setup and event channels function correctly without burning LLM API tokens, run a campaign in TUI mode with mock LLM fallbacks:

```bash
cargo run -- campaign --tui
```
*   Verify the neon 6-pane grid renders.
*   Scrub the playhead backward and forward using your `Left` / `Right` arrow keys.
*   Press `q` to quit safely and restore your terminal settings.

---

## ⚠️ Troubleshooting Common Issues

### Issue 1: Compiler fails on `tokenizers` or `candle-core`
*   **Cause**: Missing standard C++ build utilities or outdated Clang.
*   **Solution**: 
    *   *macOS*: Run `xcode-select --install` to update your developer tools.
    *   *Linux*: Install `build-essential` via your package manager (`sudo apt-get install build-essential`).

### Issue 2: `reqwest` connection errors on startup
*   **Cause**: Missing system OpenSSL certificate registries.
*   **Solution**: Install `pkg-config` and `libssl-dev` (`sudo apt-get install pkg-config libssl-dev`). Alternatively, compile with native TLS wrappers.

---

*You are now fully installed and ready to operate Korg. Proceed next to the [User Guide](USER_GUIDE.md) to launch your first swarm campaign.*
