//! EmbeddingModel trait + implementations for semantic_entropy in the Evaluator.
//!
//! - FakeEmbeddingModel: deterministic, zero-dependency, always available (CI / tests).
//! - CandleEmbeddingModel: real sentence-transformers BERT (all-MiniLM-L6-v2) via Candle.
//!
//! The real model is loaded when the `candle` feature is enabled and model files (or HF Hub)
//! are available. Otherwise Evaluator falls back to Fake automatically.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

pub trait EmbeddingModel: Send + Sync {
    fn embed(&self, text: &str) -> Result<Vec<f32>, Box<dyn std::error::Error + Send + Sync>>;
}

// ============================================================================
// FakeEmbeddingModel (always works, great for tests)
// ============================================================================

pub struct FakeEmbeddingModel {
    dim: usize,
}

impl Default for FakeEmbeddingModel {
    fn default() -> Self {
        Self { dim: 32 }
    }
}

impl FakeEmbeddingModel {
    pub fn new(dim: usize) -> Self {
        Self { dim }
    }

    fn hash_to_seed(text: &str) -> u64 {
        let mut hasher = DefaultHasher::new();
        text.hash(&mut hasher);
        hasher.finish()
    }
}

impl EmbeddingModel for FakeEmbeddingModel {
    fn embed(&self, text: &str) -> Result<Vec<f32>, Box<dyn std::error::Error + Send + Sync>> {
        if text.trim().is_empty() {
            return Ok(vec![0.0; self.dim]);
        }

        let seed = Self::hash_to_seed(text);
        let bytes = text.as_bytes();

        let mut vec = vec![0.0f32; self.dim];

        for i in 0..4 {
            let byte = ((seed >> (i * 8)) & 0xff) as f32 / 255.0;
            vec[i] = byte * 2.0 - 1.0;
        }

        let len = bytes.len() as f32;
        for (i, &b) in bytes.iter().enumerate() {
            let idx = (i * 7 + (b as usize)) % self.dim;
            let contrib = ((b as f32) / 255.0 - 0.5) * (1.0 + (i as f32 / len));
            vec[idx] += contrib * 0.3;
        }

        let norm: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 1e-8 {
            for v in &mut vec {
                *v /= norm;
            }
        }

        Ok(vec)
    }
}

// ============================================================================
// CandleEmbeddingModel — real production embeddings (all-MiniLM-L6-v2)
// ============================================================================

#[cfg(feature = "candle")]
mod candle_impl {
    use super::EmbeddingModel;
    use candle_core::{DType, Device, Tensor};
    use candle_nn::VarBuilder;
    use candle_transformers::models::bert::{BertModel, Config as BertConfig};
    use std::path::PathBuf;
    use tokenizers::Tokenizer;

    const MODEL_ID: &str = "sentence-transformers/all-MiniLM-L6-v2";
    const EMBED_DIM: usize = 384;

    pub struct CandleEmbeddingModel {
        model: BertModel,
        tokenizer: Tokenizer,
        device: Device,
    }

    impl CandleEmbeddingModel {
        /// Load the real model.
        /// Priority:
        ///   1. KORG_EMBEDDING_MODEL_DIR env var (local files)
        ///   2. ./models/all-MiniLM-L6-v2 (local)
        ///   3. HF Hub download (requires `hf-hub`)
        pub fn load() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
            let device = Device::Cpu;

            let model_dir = Self::resolve_model_dir()?;

            let config_path = model_dir.join("config.json");
            let tokenizer_path = model_dir.join("tokenizer.json");
            let weights_path = model_dir.join("model.safetensors");

            if !config_path.exists() || !tokenizer_path.exists() || !weights_path.exists() {
                return Err(format!(
                    "Candle model files not found in {}. \
                     Set KORG_EMBEDDING_MODEL_DIR or run with model files present.",
                    model_dir.display()
                )
                .into());
            }

            // Load config via serde (most portable across candle-transformers versions)
            let config_str = std::fs::read_to_string(&config_path)?;
            let config: BertConfig = serde_json::from_str(&config_str)?;
            let tokenizer = Tokenizer::from_file(&tokenizer_path)
                .map_err(|e| format!("Failed to load tokenizer: {}", e))?;

            let vb = VarBuilder::from_pth(&weights_path, DType::F32, &device)?;
            let model = BertModel::load(vb, &config)?;

            Ok(Self {
                model,
                tokenizer,
                device,
            })
        }

        fn resolve_model_dir() -> Result<PathBuf, Box<dyn std::error::Error + Send + Sync>> {
            // 1. Explicit env var
            if let Ok(dir) = std::env::var("KORG_EMBEDDING_MODEL_DIR") {
                return Ok(PathBuf::from(dir));
            }

            // 2. Local ./models/all-MiniLM-L6-v2
            let local = PathBuf::from("models/all-MiniLM-L6-v2");
            if local.exists() {
                return Ok(local);
            }

            // 3. HF Hub download (best effort)
            #[cfg(feature = "candle")]
            {
                use hf_hub::api::sync::Api;
                let api = Api::new()?;
                let repo = api.model(MODEL_ID.to_string());

                // These are the files we need for the sentence-transformers BERT variant
                let _ = repo.get("config.json")?;
                let _ = repo.get("tokenizer.json")?;
                let _ = repo.get("model.safetensors")?;

                // hf-hub places them in ~/.cache/huggingface/hub/...
                // For simplicity we return a conventional path and let the user
                // set KORG_EMBEDDING_MODEL_DIR if the auto-discovery fails.
                let cache = std::env::var("HF_HOME").unwrap_or_else(|_| {
                    std::env::var("HOME").unwrap_or_default() + "/.cache/huggingface"
                });
                let hub = PathBuf::from(cache)
                    .join("hub/models--sentence-transformers--all-MiniLM-L6-v2/snapshots");
                if let Ok(entries) = std::fs::read_dir(&hub) {
                    for entry in entries.flatten() {
                        if entry.path().join("model.safetensors").exists() {
                            return Ok(entry.path());
                        }
                    }
                }
            }

            Err("Could not locate or download all-MiniLM-L6-v2 model files".into())
        }
    }

    impl EmbeddingModel for CandleEmbeddingModel {
        fn embed(&self, text: &str) -> Result<Vec<f32>, Box<dyn std::error::Error + Send + Sync>> {
            if text.trim().is_empty() {
                return Ok(vec![0.0; EMBED_DIM]);
            }

            let encoding = self
                .tokenizer
                .encode(text, true)
                .map_err(|e| format!("tokenization failed: {}", e))?;

            let input_ids = Tensor::new(encoding.get_ids(), &self.device)?.unsqueeze(0)?;
            let attention_mask =
                Tensor::new(encoding.get_attention_mask(), &self.device)?.unsqueeze(0)?;

            // Forward pass — third arg is usually Option<encoder_hidden_states>
            let last_hidden_state = self.model.forward(&input_ids, &attention_mask, None)?;

            // Mean pooling with attention mask
            let mask = attention_mask.unsqueeze(2)?.to_dtype(DType::F32)?;
            let sum_embeddings = last_hidden_state.broadcast_mul(&mask)?.sum(1)?;
            let sum_mask = mask.sum(1)?.clamp(1e-9, f32::MAX as f64)?;
            let mean_pooled = sum_embeddings.broadcast_div(&sum_mask)?;

            let vec = mean_pooled.squeeze(0)?.to_vec1::<f32>()?;

            // L2 normalize (important for cosine similarity / entropy calculation)
            let norm: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
            let normalized = if norm > 1e-8 {
                vec.iter().map(|x| x / norm).collect()
            } else {
                vec
            };

            Ok(normalized)
        }
    }
}

#[cfg(feature = "candle")]
pub use candle_impl::CandleEmbeddingModel;

#[cfg(not(feature = "candle"))]
pub struct CandleEmbeddingModel;

#[cfg(not(feature = "candle"))]
impl CandleEmbeddingModel {
    pub fn load() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        Err("Candle feature not enabled. Rebuild with --features candle".into())
    }
}

#[cfg(not(feature = "candle"))]
impl EmbeddingModel for CandleEmbeddingModel {
    fn embed(&self, _text: &str) -> Result<Vec<f32>, Box<dyn std::error::Error + Send + Sync>> {
        Err("Candle feature not enabled".into())
    }
}

// ============================================================================
// Cosine similarity helper (used by semantic_entropy)
// ============================================================================

pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for (x, y) in a.iter().zip(b.iter()) {
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    let denom = (na.sqrt() * nb.sqrt()).max(1e-8);
    (dot / denom).clamp(-1.0, 1.0)
}

// ============================================================================
// Codebase Indexing Data Structures
// ============================================================================

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct IndexedCodeBlock {
    pub file_path: String,
    pub block_name: String,
    pub block_type: String, // "struct", "fn", "impl", "module", "generic"
    pub start_line: usize,
    pub end_line: usize,
    pub content: String,
    pub embedding: Vec<f32>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct CodebaseIndex {
    pub blocks: Vec<IndexedCodeBlock>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_embeddings_are_normalized_and_deterministic() {
        let m = FakeEmbeddingModel::default();
        let e1 = m.embed("hello world semantic test").unwrap();
        let e2 = m.embed("hello world semantic test").unwrap();
        assert_eq!(e1.len(), 32);
        assert!((e1.iter().map(|x| x * x).sum::<f32>().sqrt() - 1.0).abs() < 1e-5);
        assert_eq!(e1, e2);

        let e3 = m
            .embed("completely different sentence about rust and agents")
            .unwrap();
        let sim = cosine_similarity(&e1, &e3);
        assert!(sim < 0.95, "different texts should not be almost identical");
    }
}
