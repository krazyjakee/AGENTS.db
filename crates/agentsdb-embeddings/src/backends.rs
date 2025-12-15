#![cfg_attr(
    not(any(
        feature = "openai",
        feature = "voyage",
        feature = "cohere",
        feature = "candle",
        feature = "ort"
    )),
    allow(dead_code, unused_imports)
)]

use anyhow::Context;
#[cfg(any(feature = "openai", feature = "voyage", feature = "cohere"))]
use std::collections::BTreeMap;

use crate::embedder::{Embedder, EmbedderMetadata, EmbeddingProfile, OutputNorm};

fn ensure_dim(expected: usize, got: usize, backend: &str) -> anyhow::Result<()> {
    if expected != got {
        anyhow::bail!("{backend} embedder dimension mismatch (expected {expected}, got {got})");
    }
    Ok(())
}

#[cfg(feature = "candle")]
pub fn local_candle_embedder(
    dim: usize,
    model: &str,
    revision: Option<&str>,
    expected_model_sha256: Option<&str>,
) -> anyhow::Result<Box<dyn Embedder + Send + Sync>> {
    Ok(Box::new(CandleEmbedder::new(
        dim,
        model,
        revision,
        expected_model_sha256,
    )?))
}

#[cfg(feature = "candle")]
struct CandleEmbedder {
    profile: EmbeddingProfile,
    model_sha256: Option<String>,
    model: candle_transformers::models::bert::BertModel,
    tokenizer: tokenizers::Tokenizer,
    device: candle_core::Device,
}

#[cfg(feature = "candle")]
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

#[cfg(feature = "candle")]
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

#[cfg(any(feature = "openai", feature = "voyage", feature = "cohere"))]
fn require_env(key: &str) -> anyhow::Result<String> {
    std::env::var(key).with_context(|| format!("missing required env var {key}"))
}

#[cfg(any(feature = "openai", feature = "voyage", feature = "cohere"))]
fn collect_headers(resp: &ureq::Response, names: &[&str]) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for &name in names {
        if let Some(v) = resp.header(name) {
            out.insert(name.to_string(), v.to_string());
        }
    }
    out
}

#[cfg(feature = "openai")]
pub fn openai_embedder(
    dim: usize,
    model: &str,
    api_base: Option<&str>,
    api_key_env: Option<&str>,
) -> anyhow::Result<Box<dyn Embedder + Send + Sync>> {
    let api_key_env = api_key_env.unwrap_or("OPENAI_API_KEY");
    let api_key = require_env(api_key_env).context("resolve OpenAI API key")?;
    let api_base = api_base.unwrap_or("https://api.openai.com");
    Ok(Box::new(OpenAiEmbedder::new(
        dim, model, api_base, api_key,
    )?))
}

#[cfg(feature = "openai")]
struct OpenAiEmbedder {
    profile: EmbeddingProfile,
    api_base: String,
    api_key: String,
    observed_model: std::sync::Mutex<Option<String>>,
    observed_request: std::sync::Mutex<Option<serde_json::Value>>,
    observed_response: std::sync::Mutex<Option<serde_json::Value>>,
    observed_headers: std::sync::Mutex<Option<BTreeMap<String, String>>>,
}

#[cfg(feature = "openai")]
impl OpenAiEmbedder {
    fn new(dim: usize, model: &str, api_base: &str, api_key: String) -> anyhow::Result<Self> {
        Ok(Self {
            profile: EmbeddingProfile {
                backend: "openai".to_string(),
                model: Some(model.to_string()),
                revision: None,
                dim,
                output_norm: OutputNorm::None,
            },
            api_base: api_base.trim_end_matches('/').to_string(),
            api_key,
            observed_model: std::sync::Mutex::new(None),
            observed_request: std::sync::Mutex::new(None),
            observed_response: std::sync::Mutex::new(None),
            observed_headers: std::sync::Mutex::new(None),
        })
    }
}

#[cfg(feature = "openai")]
impl Embedder for OpenAiEmbedder {
    fn profile(&self) -> &EmbeddingProfile {
        &self.profile
    }

    fn metadata(&self) -> EmbedderMetadata {
        EmbedderMetadata {
            provider: Some("openai".to_string()),
            provider_api_base: Some(self.api_base.clone()),
            provider_model: self.profile.model.clone(),
            provider_model_revision: self.observed_model.lock().ok().and_then(|g| g.clone()),
            runtime: Some("http".to_string()),
            runtime_version: crate::build_info::runtime_version_http(),
            provider_request: self.observed_request.lock().ok().and_then(|g| g.clone()),
            provider_response: self.observed_response.lock().ok().and_then(|g| g.clone()),
            provider_response_headers: self.observed_headers.lock().ok().and_then(|g| g.clone()),
            model_sha256: None,
            notes: None,
        }
    }

    fn embed(&self, inputs: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
        let model = self
            .profile
            .model
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("openai embedder missing model"))?;
        let url = format!("{}/v1/embeddings", self.api_base);

        if let Ok(mut g) = self.observed_request.lock() {
            *g = Some(serde_json::json!({
                "endpoint": "/v1/embeddings",
                "model": model,
                "input_count": inputs.len(),
            }));
        }

        let response = ureq::post(&url)
            .set("authorization", &format!("Bearer {}", self.api_key))
            .set("content-type", "application/json")
            .send_json(serde_json::json!({ "model": model, "input": inputs }))
            .context("openai embeddings request")?;

        let headers = collect_headers(
            &response,
            &[
                "x-request-id",
                "openai-model",
                "openai-version",
                "openai-processing-ms",
                "date",
                "server",
            ],
        );
        if !headers.is_empty() {
            if let Ok(mut g) = self.observed_headers.lock() {
                *g = Some(headers);
            }
        }

        let raw: serde_json::Value = response
            .into_json()
            .context("parse openai embeddings response")?;

        if let Some(m) = raw
            .get("model")
            .and_then(|v| v.as_str())
            .map(str::to_string)
        {
            if let Ok(mut g) = self.observed_model.lock() {
                *g = Some(m);
            }
        }
        if let Some(obj) = raw.as_object() {
            let mut meta = serde_json::Map::new();
            for k in ["model", "object", "usage"] {
                if let Some(v) = obj.get(k) {
                    meta.insert(k.to_string(), v.clone());
                }
            }
            if let Ok(mut g) = self.observed_response.lock() {
                *g = Some(serde_json::Value::Object(meta));
            }
        }

        let data = raw
            .get("data")
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow::anyhow!("openai response missing data[]"))?;
        let mut out = Vec::with_capacity(data.len());
        for item in data {
            let emb = item
                .get("embedding")
                .and_then(|v| v.as_array())
                .ok_or_else(|| anyhow::anyhow!("openai response item missing embedding[]"))?;
            let mut vec = Vec::with_capacity(emb.len());
            for f in emb {
                vec.push(
                    f.as_f64()
                        .ok_or_else(|| anyhow::anyhow!("openai embedding contains non-number"))?
                        as f32,
                );
            }
            ensure_dim(self.profile.dim, vec.len(), "openai")?;
            out.push(vec);
        }
        Ok(out)
    }
}

#[cfg(feature = "voyage")]
pub fn voyage_embedder(
    dim: usize,
    model: &str,
    api_base: Option<&str>,
    api_key_env: Option<&str>,
) -> anyhow::Result<Box<dyn Embedder + Send + Sync>> {
    let api_key_env = api_key_env.unwrap_or("VOYAGE_API_KEY");
    let api_key = require_env(api_key_env).context("resolve Voyage API key")?;
    let api_base = api_base.unwrap_or("https://api.voyageai.com");
    Ok(Box::new(VoyageEmbedder::new(
        dim, model, api_base, api_key,
    )?))
}

#[cfg(feature = "voyage")]
struct VoyageEmbedder {
    profile: EmbeddingProfile,
    api_base: String,
    api_key: String,
    observed_model: std::sync::Mutex<Option<String>>,
    observed_request: std::sync::Mutex<Option<serde_json::Value>>,
    observed_response: std::sync::Mutex<Option<serde_json::Value>>,
    observed_headers: std::sync::Mutex<Option<BTreeMap<String, String>>>,
}

#[cfg(feature = "voyage")]
impl VoyageEmbedder {
    fn new(dim: usize, model: &str, api_base: &str, api_key: String) -> anyhow::Result<Self> {
        Ok(Self {
            profile: EmbeddingProfile {
                backend: "voyage".to_string(),
                model: Some(model.to_string()),
                revision: None,
                dim,
                output_norm: OutputNorm::None,
            },
            api_base: api_base.trim_end_matches('/').to_string(),
            api_key,
            observed_model: std::sync::Mutex::new(None),
            observed_request: std::sync::Mutex::new(None),
            observed_response: std::sync::Mutex::new(None),
            observed_headers: std::sync::Mutex::new(None),
        })
    }
}

#[cfg(feature = "voyage")]
impl Embedder for VoyageEmbedder {
    fn profile(&self) -> &EmbeddingProfile {
        &self.profile
    }

    fn metadata(&self) -> EmbedderMetadata {
        EmbedderMetadata {
            provider: Some("voyage".to_string()),
            provider_api_base: Some(self.api_base.clone()),
            provider_model: self.profile.model.clone(),
            provider_model_revision: self.observed_model.lock().ok().and_then(|g| g.clone()),
            runtime: Some("http".to_string()),
            runtime_version: crate::build_info::runtime_version_http(),
            provider_request: self.observed_request.lock().ok().and_then(|g| g.clone()),
            provider_response: self.observed_response.lock().ok().and_then(|g| g.clone()),
            provider_response_headers: self.observed_headers.lock().ok().and_then(|g| g.clone()),
            model_sha256: None,
            notes: None,
        }
    }

    fn embed(&self, inputs: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
        let model = self
            .profile
            .model
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("voyage embedder missing model"))?;
        let url = format!("{}/v1/embeddings", self.api_base);

        if let Ok(mut g) = self.observed_request.lock() {
            *g = Some(serde_json::json!({
                "endpoint": "/v1/embeddings",
                "model": model,
                "input_count": inputs.len(),
            }));
        }

        let response = ureq::post(&url)
            .set("authorization", &format!("Bearer {}", self.api_key))
            .set("content-type", "application/json")
            .send_json(serde_json::json!({ "model": model, "input": inputs }))
            .context("voyage embeddings request")?;

        let headers = collect_headers(
            &response,
            &["x-request-id", "x-api-version", "date", "server"],
        );
        if !headers.is_empty() {
            if let Ok(mut g) = self.observed_headers.lock() {
                *g = Some(headers);
            }
        }

        let raw: serde_json::Value = response
            .into_json()
            .context("parse voyage embeddings response")?;

        if let Some(m) = raw
            .get("model")
            .and_then(|v| v.as_str())
            .map(str::to_string)
        {
            if let Ok(mut g) = self.observed_model.lock() {
                *g = Some(m);
            }
        }
        if let Some(obj) = raw.as_object() {
            let mut meta = serde_json::Map::new();
            for k in ["model", "object", "usage"] {
                if let Some(v) = obj.get(k) {
                    meta.insert(k.to_string(), v.clone());
                }
            }
            if let Ok(mut g) = self.observed_response.lock() {
                *g = Some(serde_json::Value::Object(meta));
            }
        }

        let data = raw
            .get("data")
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow::anyhow!("voyage response missing data[]"))?;
        let mut out = Vec::with_capacity(data.len());
        for item in data {
            let emb = item
                .get("embedding")
                .and_then(|v| v.as_array())
                .ok_or_else(|| anyhow::anyhow!("voyage response item missing embedding[]"))?;
            let mut vec = Vec::with_capacity(emb.len());
            for f in emb {
                vec.push(
                    f.as_f64()
                        .ok_or_else(|| anyhow::anyhow!("voyage embedding contains non-number"))?
                        as f32,
                );
            }
            ensure_dim(self.profile.dim, vec.len(), "voyage")?;
            out.push(vec);
        }
        Ok(out)
    }
}

#[cfg(feature = "cohere")]
pub fn cohere_embedder(
    dim: usize,
    model: &str,
    api_base: Option<&str>,
    api_key_env: Option<&str>,
) -> anyhow::Result<Box<dyn Embedder + Send + Sync>> {
    let api_key_env = api_key_env.unwrap_or("COHERE_API_KEY");
    let api_key = require_env(api_key_env).context("resolve Cohere API key")?;
    let api_base = api_base.unwrap_or("https://api.cohere.com");
    Ok(Box::new(CohereEmbedder::new(
        dim, model, api_base, api_key,
    )?))
}

#[cfg(feature = "cohere")]
struct CohereEmbedder {
    profile: EmbeddingProfile,
    api_base: String,
    api_key: String,
    observed_model: std::sync::Mutex<Option<String>>,
    observed_request: std::sync::Mutex<Option<serde_json::Value>>,
    observed_response: std::sync::Mutex<Option<serde_json::Value>>,
    observed_headers: std::sync::Mutex<Option<BTreeMap<String, String>>>,
}

#[cfg(feature = "cohere")]
impl CohereEmbedder {
    fn new(dim: usize, model: &str, api_base: &str, api_key: String) -> anyhow::Result<Self> {
        Ok(Self {
            profile: EmbeddingProfile {
                backend: "cohere".to_string(),
                model: Some(model.to_string()),
                revision: None,
                dim,
                output_norm: OutputNorm::None,
            },
            api_base: api_base.trim_end_matches('/').to_string(),
            api_key,
            observed_model: std::sync::Mutex::new(None),
            observed_request: std::sync::Mutex::new(None),
            observed_response: std::sync::Mutex::new(None),
            observed_headers: std::sync::Mutex::new(None),
        })
    }
}

#[cfg(feature = "cohere")]
impl Embedder for CohereEmbedder {
    fn profile(&self) -> &EmbeddingProfile {
        &self.profile
    }

    fn metadata(&self) -> EmbedderMetadata {
        EmbedderMetadata {
            provider: Some("cohere".to_string()),
            provider_api_base: Some(self.api_base.clone()),
            provider_model: self.profile.model.clone(),
            provider_model_revision: self.observed_model.lock().ok().and_then(|g| g.clone()),
            runtime: Some("http".to_string()),
            runtime_version: crate::build_info::runtime_version_http(),
            provider_request: self.observed_request.lock().ok().and_then(|g| g.clone()),
            provider_response: self.observed_response.lock().ok().and_then(|g| g.clone()),
            provider_response_headers: self.observed_headers.lock().ok().and_then(|g| g.clone()),
            model_sha256: None,
            notes: None,
        }
    }

    fn embed(&self, inputs: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
        let model = self
            .profile
            .model
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("cohere embedder missing model"))?;
        let url = format!("{}/v1/embed", self.api_base);

        if let Ok(mut g) = self.observed_request.lock() {
            *g = Some(serde_json::json!({
                "endpoint": "/v1/embed",
                "model": model,
                "input_count": inputs.len(),
            }));
        }

        let response = ureq::post(&url)
            .set("authorization", &format!("Bearer {}", self.api_key))
            .set("content-type", "application/json")
            .send_json(serde_json::json!({ "model": model, "texts": inputs }))
            .context("cohere embeddings request")?;

        let headers = collect_headers(
            &response,
            &["x-request-id", "x-api-version", "date", "server"],
        );
        if !headers.is_empty() {
            if let Ok(mut g) = self.observed_headers.lock() {
                *g = Some(headers);
            }
        }

        let raw: serde_json::Value = response
            .into_json()
            .context("parse cohere embeddings response")?;

        if let Some(m) = raw
            .get("model")
            .and_then(|v| v.as_str())
            .map(str::to_string)
        {
            if let Ok(mut g) = self.observed_model.lock() {
                *g = Some(m);
            }
        }
        if let Some(obj) = raw.as_object() {
            let mut meta = serde_json::Map::new();
            for k in ["model"] {
                if let Some(v) = obj.get(k) {
                    meta.insert(k.to_string(), v.clone());
                }
            }
            if let Ok(mut g) = self.observed_response.lock() {
                *g = Some(serde_json::Value::Object(meta));
            }
        }

        let embeddings = raw
            .get("embeddings")
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow::anyhow!("cohere response missing embeddings[]"))?;
        let mut out = Vec::with_capacity(embeddings.len());
        for emb in embeddings {
            let arr = emb
                .as_array()
                .ok_or_else(|| anyhow::anyhow!("cohere embedding is not an array"))?;
            let mut vec = Vec::with_capacity(arr.len());
            for f in arr {
                vec.push(
                    f.as_f64()
                        .ok_or_else(|| anyhow::anyhow!("cohere embedding contains non-number"))?
                        as f32,
                );
            }
            ensure_dim(self.profile.dim, vec.len(), "cohere")?;
            out.push(vec);
        }
        Ok(out)
    }
}

#[cfg(feature = "ort")]
pub fn local_fastembed_embedder(
    backend: &str,
    dim: usize,
    model: &str,
    revision: Option<&str>,
    model_path: Option<&str>,
    expected_model_sha256: Option<&str>,
) -> anyhow::Result<Box<dyn Embedder + Send + Sync>> {
    let backend = backend.to_string();
    Ok(Box::new(FastembedEmbedder::new(
        backend,
        dim,
        model,
        revision,
        model_path,
        expected_model_sha256,
    )?))
}

#[cfg(feature = "ort")]
struct FastembedEmbedder {
    profile: EmbeddingProfile,
    inner: fastembed::TextEmbedding,
    model_sha256: Option<String>,
    notes: Option<String>,
}

#[cfg(feature = "ort")]
impl FastembedEmbedder {
    fn new(
        backend: String,
        dim: usize,
        model: &str,
        revision: Option<&str>,
        model_path: Option<&str>,
        expected_model_sha256: Option<&str>,
    ) -> anyhow::Result<Self> {
        let model_enum = parse_fastembed_model(model)?;
        let expected_dim = fastembed_model_dim(&model_enum);
        ensure_dim(dim, expected_dim, "fastembed")?;

        let revision = revision.unwrap_or("main");
        let (onnx_bytes, tokenizer_files, model_sha256, notes) = match model_path {
            Some(path) => {
                let (onnx_bytes, tokenizer_files, model_sha256) =
                    load_fastembed_model_from_path(std::path::Path::new(path))
                        .context("load model from path")?;
                (
                    onnx_bytes,
                    tokenizer_files,
                    model_sha256,
                    Some(format!(
                        "onnxruntime via fastembed (model loaded from local path: {path})"
                    )),
                )
            }
            None => {
                let (onnx_bytes, tokenizer_files, model_sha256) =
                    download_fastembed_model(model_enum, revision).context("download model")?;
                (
                    onnx_bytes,
                    tokenizer_files,
                    model_sha256,
                    Some(
                        "onnxruntime via fastembed (model downloaded via hf-hub into the HF cache)"
                            .to_string(),
                    ),
                )
            }
        };
        if let Some(actual) = model_sha256.as_deref() {
            crate::verification::verify_model_sha256(expected_model_sha256, actual)
                .context("verify model checksum")?;
        }
        let user = fastembed::UserDefinedEmbeddingModel::new(onnx_bytes, tokenizer_files);
        let inner = fastembed::TextEmbedding::try_new_from_user_defined(
            user,
            fastembed::InitOptionsUserDefined::new(),
        )
        .context("init fastembed model (user-defined)")?;

        Ok(Self {
            profile: EmbeddingProfile {
                backend,
                model: Some(model.to_string()),
                revision: Some(revision.to_string()),
                dim,
                output_norm: OutputNorm::None,
            },
            inner,
            model_sha256,
            notes,
        })
    }
}

#[cfg(feature = "ort")]
impl Embedder for FastembedEmbedder {
    fn profile(&self) -> &EmbeddingProfile {
        &self.profile
    }

    fn metadata(&self) -> EmbedderMetadata {
        EmbedderMetadata {
            provider: None,
            provider_api_base: None,
            provider_model: self.profile.model.clone(),
            provider_model_revision: self.profile.revision.clone(),
            runtime: Some("onnxruntime".to_string()),
            runtime_version: crate::build_info::runtime_version_fastembed(),
            provider_request: None,
            provider_response: None,
            provider_response_headers: None,
            model_sha256: self.model_sha256.clone(),
            notes: self.notes.clone(),
        }
    }

    fn embed(&self, inputs: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
        let out: Vec<Vec<f32>> = self
            .inner
            .embed(inputs.to_vec(), None)
            .context("fastembed embed")
            .map(|v| v.into_iter().map(|e| e.to_vec()).collect())?;

        for emb in &out {
            ensure_dim(self.profile.dim, emb.len(), "fastembed")?;
        }
        Ok(out)
    }
}

#[cfg(feature = "ort")]
fn parse_fastembed_model(model: &str) -> anyhow::Result<fastembed::EmbeddingModel> {
    match model {
        // Keep this list intentionally small; add more models as needed.
        "all-minilm-l6-v2" | "all-MiniLM-L6-v2" => Ok(fastembed::EmbeddingModel::AllMiniLML6V2),
        other => anyhow::bail!("unknown local model {other:?} (supported: \"all-minilm-l6-v2\")"),
    }
}

#[cfg(feature = "ort")]
fn fastembed_model_dim(model: &fastembed::EmbeddingModel) -> usize {
    match model {
        fastembed::EmbeddingModel::AllMiniLML6V2 => 384,
        // Conservative default for any future models we add to `parse_fastembed_model`.
        _ => 384,
    }
}

#[cfg(feature = "ort")]
fn download_fastembed_model(
    model: fastembed::EmbeddingModel,
    revision: &str,
) -> anyhow::Result<(Vec<u8>, fastembed::TokenizerFiles, Option<String>)> {
    use hf_hub::api::sync::ApiBuilder;
    use hf_hub::{Repo, RepoType};

    let (model_code, model_file) = match model {
        fastembed::EmbeddingModel::AllMiniLML6V2 => ("Qdrant/all-MiniLM-L6-v2-onnx", "model.onnx"),
        _ => anyhow::bail!("unsupported fastembed model for download"),
    };

    let api = ApiBuilder::new()
        .with_progress(false)
        .build()
        .context("init hf-hub client")?;
    let repo = api.repo(Repo::with_revision(
        model_code.to_string(),
        RepoType::Model,
        revision.to_string(),
    ));

    let onnx_path = repo.get(model_file).context("download onnx model")?;
    let onnx_bytes =
        std::fs::read(&onnx_path).with_context(|| format!("read {}", onnx_path.display()))?;
    let sha = crate::cache::sha256(&onnx_bytes);
    let sha_hex = hex_lower(&sha);

    let tokenizer_file = read_hf_bytes(&repo, "tokenizer.json")?;
    let config_file = read_hf_bytes(&repo, "config.json")?;
    let special_tokens_map_file = read_hf_bytes(&repo, "special_tokens_map.json")?;
    let tokenizer_config_file = read_hf_bytes(&repo, "tokenizer_config.json")?;

    Ok((
        onnx_bytes,
        fastembed::TokenizerFiles {
            tokenizer_file,
            config_file,
            special_tokens_map_file,
            tokenizer_config_file,
        },
        Some(sha_hex),
    ))
}

#[cfg(feature = "ort")]
fn read_hf_bytes(repo: &hf_hub::api::sync::ApiRepo, filename: &str) -> anyhow::Result<Vec<u8>> {
    let path = repo
        .get(filename)
        .with_context(|| format!("download {filename}"))?;
    std::fs::read(&path).with_context(|| format!("read {}", path.display()))
}

#[cfg(feature = "ort")]
fn load_fastembed_model_from_path(
    path: &std::path::Path,
) -> anyhow::Result<(Vec<u8>, fastembed::TokenizerFiles, Option<String>)> {
    let (onnx_path, dir) = if path.is_dir() {
        (path.join("model.onnx"), path)
    } else {
        let dir = path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("model path has no parent directory"))?;
        (path.to_path_buf(), dir)
    };

    let onnx_bytes =
        std::fs::read(&onnx_path).with_context(|| format!("read {}", onnx_path.display()))?;
    let sha = crate::cache::sha256(&onnx_bytes);
    let sha_hex = hex_lower(&sha);

    let read_required = |name: &str| -> anyhow::Result<Vec<u8>> {
        let p = dir.join(name);
        std::fs::read(&p).with_context(|| format!("read {}", p.display()))
    };

    let tokenizer_file = read_required("tokenizer.json")?;
    let config_file = read_required("config.json")?;
    let special_tokens_map_file = read_required("special_tokens_map.json")?;
    let tokenizer_config_file = read_required("tokenizer_config.json")?;

    Ok((
        onnx_bytes,
        fastembed::TokenizerFiles {
            tokenizer_file,
            config_file,
            special_tokens_map_file,
            tokenizer_config_file,
        },
        Some(sha_hex),
    ))
}

#[cfg(any(feature = "ort", feature = "candle"))]
fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = vec![0u8; bytes.len() * 2];
    for (i, b) in bytes.iter().enumerate() {
        out[i * 2] = HEX[(b >> 4) as usize];
        out[i * 2 + 1] = HEX[(b & 0x0f) as usize];
    }
    String::from_utf8(out).expect("valid hex")
}
