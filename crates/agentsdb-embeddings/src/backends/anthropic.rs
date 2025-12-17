//! Anthropic embedding backend.

use anyhow::Context;
use std::collections::BTreeMap;
use crate::embedder::{Embedder, EmbedderMetadata, EmbeddingProfile, OutputNorm};
use super::common::{ensure_dim, require_env, collect_headers};

pub fn anthropic_embedder(
    dim: usize,
    model: &str,
    api_base: Option<&str>,
    api_key_env: Option<&str>,
) -> anyhow::Result<Box<dyn Embedder + Send + Sync>> {
    let api_key_env = api_key_env.unwrap_or("ANTHROPIC_API_KEY");
    let api_key = require_env(api_key_env).context("resolve Anthropic API key")?;
    let api_base = api_base.unwrap_or("https://api.anthropic.com");
    Ok(Box::new(AnthropicEmbedder::new(
        dim, model, api_base, api_key,
    )?))
}

struct AnthropicEmbedder {
    profile: EmbeddingProfile,
    api_base: String,
    api_key: String,
    observed_model: std::sync::Mutex<Option<String>>,
    observed_request: std::sync::Mutex<Option<serde_json::Value>>,
    observed_response: std::sync::Mutex<Option<serde_json::Value>>,
    observed_headers: std::sync::Mutex<Option<BTreeMap<String, String>>>,
}

impl AnthropicEmbedder {
    fn new(dim: usize, model: &str, api_base: &str, api_key: String) -> anyhow::Result<Self> {
        Ok(Self {
            profile: EmbeddingProfile {
                backend: "anthropic".to_string(),
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

impl Embedder for AnthropicEmbedder {
    fn profile(&self) -> &EmbeddingProfile {
        &self.profile
    }

    fn metadata(&self) -> EmbedderMetadata {
        EmbedderMetadata {
            provider: Some("anthropic".to_string()),
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
            .ok_or_else(|| anyhow::anyhow!("anthropic embedder missing model"))?;
        let url = format!("{}/v1/embeddings", self.api_base);

        if let Ok(mut g) = self.observed_request.lock() {
            *g = Some(serde_json::json!({
                "endpoint": "/v1/embeddings",
                "model": model,
                "input_count": inputs.len(),
            }));
        }

        let response = ureq::post(&url)
            .set("x-api-key", &self.api_key)
            .set("content-type", "application/json")
            .send_json(serde_json::json!({ "model": model, "input": inputs }))
            .context("anthropic embeddings request")?;

        let headers = collect_headers(
            &response,
            &["request-id", "anthropic-version", "date", "server"],
        );
        if !headers.is_empty() {
            if let Ok(mut g) = self.observed_headers.lock() {
                *g = Some(headers);
            }
        }

        let raw: serde_json::Value = response
            .into_json()
            .context("parse anthropic embeddings response")?;

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
            .ok_or_else(|| anyhow::anyhow!("anthropic response missing data[]"))?;
        let mut out = Vec::with_capacity(data.len());
        for item in data {
            let emb = item
                .get("embedding")
                .and_then(|v| v.as_array())
                .ok_or_else(|| anyhow::anyhow!("anthropic response item missing embedding[]"))?;
            let mut vec = Vec::with_capacity(emb.len());
            for f in emb {
                vec.push(
                    f.as_f64()
                        .ok_or_else(|| anyhow::anyhow!("anthropic embedding contains non-number"))?
                        as f32,
                );
            }
            ensure_dim(self.profile.dim, vec.len(), "anthropic")?;
            out.push(vec);
        }
        Ok(out)
    }
}
