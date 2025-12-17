//! Candle-based local embedding backend.

use anyhow::Context;
use crate::embedder::{Embedder, EmbedderMetadata, EmbeddingProfile, OutputNorm};
use super::common::{ensure_dim, hex_lower};

pub fn local_candle_embedder(
    dim: usize,
    model: &str,
    revision: Option<&str>,
    expected_model_sha256: Option<&str>,
) -> anyhow::Result<Box<dyn Embedder + Send + Sync>> {
    // Creates a new `Embedder` instance using the Candle backend for local inference.
    //
    // This function downloads and loads a specified BERT-based model via `hf-hub`
    // and initializes it for embedding tasks.
    Ok(Box::new(CandleEmbedder::new(
        dim,
        model,
        revision,
        expected_model_sha256,
    )?))
}

struct CandleEmbedder {
    /// An `Embedder` implementation that uses the Candle machine learning framework
    /// for local, on-device embedding inference.
    profile: EmbeddingProfile,
    model_sha256: Option<String>,
    model: candle_transformers::models::bert::BertModel,
    tokenizer: tokenizers::Tokenizer,
    device: candle_core::Device,
}

impl CandleEmbedder {
    fn new(
        dim: usize,
        model: &str,
        revision: Option<&str>,
        expected_model_sha256: Option<&str>,
    ) -> anyhow::Result<Self> {
        let revision = revision.unwrap_or(crate::config::DEFAULT_LOCAL_REVISION);

        let (model_repo, model_file) = match model {
            "all-minilm-l6-v2" | "all-MiniLM-L6-v2" => (
                "sentence-transformers/all-MiniLM-L6-v2",
                "model.safetensors",
            ),
            other => {
                anyhow::bail!("unknown local model {other:?} (supported: \"all-minilm-l6-v2\")")
            }
        };

        let device = candle_core::Device::Cpu;

        let api = hf_hub::api::sync::ApiBuilder::new()
            .with_progress(false)
            .build()
            .context("init hf-hub client")?;
        let repo = api.repo(hf_hub::Repo::with_revision(
            model_repo.to_string(),
            hf_hub::RepoType::Model,
            revision.to_string(),
        ));

        let model_path = repo.get(model_file).context("download safetensors model")?;
        let model_bytes =
            std::fs::read(&model_path).with_context(|| format!("read {}", model_path.display()))?;
        let model_sha = crate::cache::sha256(&model_bytes);
        let model_sha_hex = hex_lower(&model_sha);
        crate::verification::verify_model_sha256(expected_model_sha256, &model_sha_hex)
            .context("verify downloaded model checksum")?;

        let config_path = repo.get("config.json").context("download config.json")?;
        let config_bytes = std::fs::read(&config_path)
            .with_context(|| format!("read {}", config_path.display()))?;
        let config: candle_transformers::models::bert::Config =
            serde_json::from_slice(&config_bytes).context("parse bert config")?;

        let tokenizer_path = repo
            .get("tokenizer.json")
            .context("download tokenizer.json")?;
        let tokenizer = tokenizers::Tokenizer::from_file(&tokenizer_path)
            .map_err(|e| anyhow::anyhow!("load tokenizer: {e}"))?;

        let tensors = candle_core::safetensors::load(&model_path, &device)
            .with_context(|| format!("load safetensors {}", model_path.display()))?;
        let vb = candle_nn::VarBuilder::from_tensors(tensors, candle_core::DType::F32, &device);
        let bert = candle_transformers::models::bert::BertModel::load(vb, &config)
            .context("init bert model")?;

        ensure_dim(dim, config.hidden_size, "candle")?;

        Ok(Self {
            profile: EmbeddingProfile {
                backend: "candle".to_string(),
                model: Some(model.to_string()),
                revision: Some(revision.to_string()),
                dim,
                output_norm: OutputNorm::None,
            },
            model_sha256: Some(model_sha_hex),
            model: bert,
            tokenizer,
            device,
        })
    }

    fn encode_batch(&self, inputs: &[String]) -> anyhow::Result<Vec<tokenizers::Encoding>> {
        let mut tokenizer = self.tokenizer.clone();
        tokenizer.with_padding(Some(tokenizers::PaddingParams {
            strategy: tokenizers::PaddingStrategy::BatchLongest,
            ..Default::default()
        }));
        tokenizer
            .with_truncation(Some(tokenizers::TruncationParams {
                max_length: 256,
                ..Default::default()
            }))
            .map_err(|e| anyhow::anyhow!("configure tokenizer truncation: {e}"))?;
        tokenizer
            .encode_batch(inputs.to_vec(), true)
            .map_err(|e| anyhow::anyhow!("tokenize batch: {e}"))
    }
}

impl Embedder for CandleEmbedder {
    fn profile(&self) -> &EmbeddingProfile {
        &self.profile
    }

    fn metadata(&self) -> EmbedderMetadata {
        EmbedderMetadata {
            provider: None,
            provider_api_base: None,
            provider_model: self.profile.model.clone(),
            provider_model_revision: self.profile.revision.clone(),
            runtime: Some("candle".to_string()),
            runtime_version: crate::build_info::runtime_version_candle(),
            provider_request: None,
            provider_response: None,
            provider_response_headers: None,
            model_sha256: self.model_sha256.clone(),
            notes: Some(
                "candle-native bert inference (model downloaded via hf-hub into the HF cache)"
                    .to_string(),
            ),
        }
    }

    fn embed(&self, inputs: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
        let enc = self.encode_batch(inputs).context("tokenize inputs")?;
        let batch = enc.len();
        let seq_len = enc.iter().map(|e| e.get_ids().len()).max().unwrap_or(0);

        let mut ids: Vec<i64> = Vec::with_capacity(batch * seq_len);
        let mut type_ids: Vec<i64> = Vec::with_capacity(batch * seq_len);
        let mut mask: Vec<i64> = Vec::with_capacity(batch * seq_len);
        for e in &enc {
            let e_ids = e.get_ids();
            let e_type_ids = e.get_type_ids();
            let e_mask = e.get_attention_mask();
            ids.extend(e_ids.iter().map(|&v| v as i64));
            type_ids.extend(e_type_ids.iter().map(|&v| v as i64));
            mask.extend(e_mask.iter().map(|&v| v as i64));
        }

        let input_ids =
            candle_core::Tensor::from_vec(ids, (batch, seq_len), &self.device).context("ids")?;
        let token_type_ids =
            candle_core::Tensor::from_vec(type_ids, (batch, seq_len), &self.device)
                .context("type ids")?;
        let attention_mask =
            candle_core::Tensor::from_vec(mask, (batch, seq_len), &self.device).context("mask")?;

        let token_embeddings = self
            .model
            .forward(&input_ids, &token_type_ids, Some(&attention_mask))
            .context("bert forward")?;
        // Mean pooling over the sequence with attention mask.
        let mask_f = attention_mask
            .to_dtype(candle_core::DType::F32)
            .context("mask to f32")?
            .unsqueeze(2)
            .context("mask unsqueeze")?;
        let masked = token_embeddings
            .broadcast_mul(&mask_f)
            .context("mask embeddings")?;
        let sum = masked.sum(1).context("sum")?;
        let denom = mask_f.sum(1).context("mask sum")?;
        let mean = sum.broadcast_div(&denom).context("mean pool")?;

        let mut out: Vec<Vec<f32>> = Vec::with_capacity(batch);
        for i in 0..batch {
            let row = mean.get(i).context("select embedding")?;
            let v: Vec<f32> = row.to_vec1().context("embedding to vec")?;
            ensure_dim(self.profile.dim, v.len(), "candle")?;
            out.push(v);
        }
        Ok(out)
    }
}
