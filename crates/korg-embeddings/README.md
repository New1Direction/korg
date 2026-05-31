# korg-embeddings

Semantic embedding backend for korg — a single `EmbeddingModel` trait with a
zero-dependency fake and a real sentence-transformers BERT implementation behind
a Cargo feature.

It exists to give the rest of the workspace dense text vectors and cosine
similarity **without forcing every consumer to pay for the ML stack**. `candle`
pulls 400MB+ of libraries; isolating it in this crate (rather than folding it
into `korg-llm` or `korg-runtime`) means anyone who wants LLM routing or the
runtime without local embeddings doesn't compile or download any of it. The
feature is on by default but can be turned off.

## What it provides

| Item | Kind | Notes |
|:---|:---|:---|
| `EmbeddingModel` | trait | `fn embed(&self, text: &str) -> Result<Vec<f32>, BoxError>`; `Send + Sync` |
| `FakeEmbeddingModel` | struct | Deterministic, dependency-free. Default 32-dim, configurable via `new(dim)`. |
| `CandleEmbeddingModel` | struct | Real `all-MiniLM-L6-v2` BERT (384-dim) via Candle. Only functional under `--features candle`. |
| `cosine_similarity` | fn | `(&[f32], &[f32]) -> f32`, clamped to `[-1.0, 1.0]`; returns `0.0` on length mismatch / empty. |
| `IndexedCodeBlock` | struct | One embedded source block: path, name, type, line span, content, `embedding: Vec<f32>`. |
| `CodebaseIndex` | struct | `Vec<IndexedCodeBlock>` (Serde + `Default`). The persisted index shape. |

The error type throughout is `Box<dyn std::error::Error + Send + Sync>`.

### `FakeEmbeddingModel`

Hashes the input with `DefaultHasher`, seeds a few leading dimensions, then mixes
each byte into a dimension and L2-normalizes. It is **deterministic** (same text →
same vector) and always available, which is what CI and the test suite rely on.
It is **not semantically meaningful** — unrelated strings are merely unlikely to
collide, not arranged by meaning. Empty/whitespace input returns a zero vector.

### `CandleEmbeddingModel`

Loads `sentence-transformers/all-MiniLM-L6-v2`, runs a BERT forward pass, applies
attention-masked mean pooling, and L2-normalizes (so dot product == cosine).
`CandleEmbeddingModel::load()` resolves model files in this order:

1. `KORG_EMBEDDING_MODEL_DIR` env var (local directory)
2. `./models/all-MiniLM-L6-v2` (relative to cwd)
3. Hugging Face Hub download into `$HF_HOME` / `~/.cache/huggingface`, then
   discovery of the snapshot dir

It expects `config.json`, `tokenizer.json`, and `model.safetensors` in the
resolved directory; if any are missing, `load()` returns an error rather than
falling back. Runs on CPU (`Device::Cpu`) — there is no GPU path here.

When the `candle` feature is **off**, `CandleEmbeddingModel` is compiled as a unit
struct whose `load()` / `embed()` always return "feature not enabled" errors, so
the type name resolves but is non-functional.

## How it fits in the workspace

This crate is a leaf — it depends only on `serde`/`serde_json` (plus the optional
Candle stack) and is consumed by:

- **`korg-runtime`**
  - `evaluator.rs` — the adversarial Evaluator's `semantic_entropy`. It holds an
    `Arc<dyn EmbeddingModel>` and `cosine_similarity` to score how much an agent's
    recent surface text diverges (doom-loop / productive-death signals).
  - `code_indexer.rs` — builds a `CodebaseIndex` of `IndexedCodeBlock`s for
    semantic codebase search.
  - `skills.rs` — clusters/dedupes notes via `cosine_similarity`.
- **`korg-server`** and the top-level **`korg` binary** (`src/main.rs`) — construct an
  embedding model for the same purposes.

Every consumer uses the same pattern: try the real model, fall back to the fake
one if it fails to load. This is the canonical wiring:

```rust
use korg_embeddings::{CandleEmbeddingModel, EmbeddingModel, FakeEmbeddingModel};

let model: Box<dyn EmbeddingModel> = match CandleEmbeddingModel::load() {
    Ok(real) => Box::new(real),
    Err(_) => Box::new(FakeEmbeddingModel::default()),
};
```

The `candle` workspace feature in the root `Cargo.toml` forwards to
`korg-embeddings/candle`, so the whole binary's embedding backend is toggled in
one place.

## Usage

```rust
use korg_embeddings::{cosine_similarity, EmbeddingModel, FakeEmbeddingModel};

let model = FakeEmbeddingModel::default(); // 32-dim, deterministic
let a = model.embed("fix the subtraction bug")?;
let b = model.embed("repair the minus operator")?;
let sim = cosine_similarity(&a, &b); // in [-1.0, 1.0]
```

To use real embeddings, build with the (default) `candle` feature and make the
model files discoverable:

```bash
export KORG_EMBEDDING_MODEL_DIR=/path/to/all-MiniLM-L6-v2  # config.json, tokenizer.json, model.safetensors
cargo build --features candle
```

```rust
use korg_embeddings::{CandleEmbeddingModel, EmbeddingModel};

let model = CandleEmbeddingModel::load()?; // 384-dim, mean-pooled + L2-normalized
let v = model.embed("the auth layer should use JWTs")?;
```

## Status / gaps

- `lib.rs` is the entire crate — no submodules beyond the in-file
  `candle_impl`. Code-block splitting/indexing logic lives in `korg-runtime`'s
  `code_indexer.rs`; this crate only owns the `IndexedCodeBlock` / `CodebaseIndex`
  data shapes and the `embed`/`cosine_similarity` primitives.
- CPU-only inference; no batching (`embed` is one string at a time).
- HF Hub auto-download is best-effort: it downloads the files but then relies on
  scanning the cache snapshot dir to locate them, and recommends setting
  `KORG_EMBEDDING_MODEL_DIR` if discovery fails.
- `FakeEmbeddingModel` is for determinism/availability, not retrieval quality —
  don't read meaning into its similarities.
- Test coverage is one unit test (determinism + normalization of the fake model);
  there is no in-crate test exercising the Candle path.
