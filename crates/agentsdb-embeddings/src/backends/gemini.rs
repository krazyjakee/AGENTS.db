//! Google Gemini embedding backend.

use anyhow::Context;
use std::collections::BTreeMap;
use crate::embedder::{Embedder, EmbedderMetadata, EmbeddingProfile, OutputNorm};
use super::common::{ensure_dim, require_env, collect_headers};

pub fn gemini_embedder(
    dim: usize,
    model: &str,
    api_base: Option<&str>,
    api_key_env: Option<&str>,
) -> anyhow::Result<Box<dyn Embedder + Send + Sync>> {
    let api_key_env = api_key_env.unwrap_or("GEMINI_API_KEY");
    let api_key = require_env(api_key_env).context("resolve Gemini API key")?;
    let api_base = api_base.unwrap_or("https://generativelanguage.googleapis.com");
    Ok(Box::new(GeminiEmbedder::new(
        dim, model, api_base, api_key,
    )?))
}

struct GeminiEmbedder {
    profile: EmbeddingProfile,
    api_base: String,
    api_key: String,
    observed_model: std::sync::Mutex<Option<String>>,
    observed_request: std::sync::Mutex<Option<serde_json::Value>>,
    observed_response: std::sync::Mutex<Option<serde_json::Value>>,
    observed_headers: std::sync::Mutex<Option<BTreeMap<String, String>>>,
}

impl GeminiEmbedder {
    fn new(dim: usize, model: &str, api_base: &str, api_key: String) -> anyhow::Result<Self> {
        Ok(Self {
            profile: EmbeddingProfile {
                backend: "gemini".to_string(),
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

impl Embedder for GeminiEmbedder {
    fn profile(&self) -> &EmbeddingProfile {
        &self.profile
    }

    fn metadata(&self) -> EmbedderMetadata {
        EmbedderMetadata {
            provider: Some("gemini".to_string()),
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
            .ok_or_else(|| anyhow::anyhow!("gemini embedder missing model"))?;

        // Gemini uses a different endpoint format with the API key in the URL
        let url = format!("{}/v1/models/{}:embedContent?key={}", self.api_base, model, self.api_key);

        if let Ok(mut g) = self.observed_request.lock() {
            *g = Some(serde_json::json!({
                "endpoint": format!("/v1/models/{}:embedContent", model),
                "model": model,
                "input_count": inputs.len(),
            }));
        }

        // Gemini expects requests with content array
        let mut embeddings = Vec::new();
        for input in inputs {
            let request_body = serde_json::json!({
                "content": {
                    "parts": [{
                        "text": input
                    }]
                }
            });

            let response = ureq::post(&url)
                .set("content-type", "application/json")
                .send_json(request_body)
                .context("gemini embeddings request")?;

            let headers = collect_headers(
                &response,
                &["x-goog-api-key", "date", "server"],
            );
            if !headers.is_empty() {
                if let Ok(mut g) = self.observed_headers.lock() {
                    *g = Some(headers);
                }
            }

            let raw: serde_json::Value = response
                .into_json()
                .context("parse gemini embeddings response")?;

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

            let embedding_obj = raw
                .get("embedding")
                .ok_or_else(|| anyhow::anyhow!("gemini response missing embedding"))?;

            let values = embedding_obj
                .get("values")
                .and_then(|v| v.as_array())
                .ok_or_else(|| anyhow::anyhow!("gemini response missing embedding.values[]"))?;

            let mut vec = Vec::with_capacity(values.len());
            for f in values {
                vec.push(
                    f.as_f64()
                        .ok_or_else(|| anyhow::anyhow!("gemini embedding contains non-number"))?
                        as f32,
                );
            }
            ensure_dim(self.profile.dim, vec.len(), "gemini")?;
            embeddings.push(vec);
        }

        Ok(embeddings)
    }
}
