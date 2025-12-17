//! AWS Bedrock embedding backend.

use anyhow::Context;
use std::collections::BTreeMap;
use crate::embedder::{Embedder, EmbedderMetadata, EmbeddingProfile, OutputNorm};
use super::common::{ensure_dim, collect_headers};

pub fn bedrock_embedder(
    dim: usize,
    model: &str,
    api_base: Option<&str>,
    region_env: Option<&str>,
) -> anyhow::Result<Box<dyn Embedder + Send + Sync>> {
    let region = match region_env {
        Some(env_var) => std::env::var(env_var)
            .with_context(|| format!("missing required env var {env_var}"))?,
        None => std::env::var("AWS_REGION")
            .or_else(|_| std::env::var("AWS_DEFAULT_REGION"))
            .context("missing AWS_REGION or AWS_DEFAULT_REGION environment variable")?,
    };

    let access_key = std::env::var("AWS_ACCESS_KEY_ID")
        .context("missing AWS_ACCESS_KEY_ID environment variable")?;
    let secret_key = std::env::var("AWS_SECRET_ACCESS_KEY")
        .context("missing AWS_SECRET_ACCESS_KEY environment variable")?;
    let session_token = std::env::var("AWS_SESSION_TOKEN").ok();

    let api_base = api_base.unwrap_or_else(|| {
        // Default Bedrock endpoint format
        "https://bedrock-runtime.{region}.amazonaws.com"
    });
    let api_base = api_base.replace("{region}", &region);

    Ok(Box::new(BedrockEmbedder::new(
        dim,
        model,
        &api_base,
        region,
        access_key,
        secret_key,
        session_token,
    )?))
}

struct BedrockEmbedder {
    profile: EmbeddingProfile,
    api_base: String,
    region: String,
    access_key: String,
    secret_key: String,
    session_token: Option<String>,
    observed_model: std::sync::Mutex<Option<String>>,
    observed_request: std::sync::Mutex<Option<serde_json::Value>>,
    observed_response: std::sync::Mutex<Option<serde_json::Value>>,
    observed_headers: std::sync::Mutex<Option<BTreeMap<String, String>>>,
}

impl BedrockEmbedder {
    fn new(
        dim: usize,
        model: &str,
        api_base: &str,
        region: String,
        access_key: String,
        secret_key: String,
        session_token: Option<String>,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            profile: EmbeddingProfile {
                backend: "bedrock".to_string(),
                model: Some(model.to_string()),
                revision: None,
                dim,
                output_norm: OutputNorm::None,
            },
            api_base: api_base.trim_end_matches('/').to_string(),
            region,
            access_key,
            secret_key,
            session_token,
            observed_model: std::sync::Mutex::new(None),
            observed_request: std::sync::Mutex::new(None),
            observed_response: std::sync::Mutex::new(None),
            observed_headers: std::sync::Mutex::new(None),
        })
    }

    fn sign_request(
        &self,
        method: &str,
        url: &str,
        headers: &BTreeMap<String, String>,
        body: &str,
    ) -> anyhow::Result<BTreeMap<String, String>> {
        use hmac::{Hmac, Mac};
        use sha2::{Digest, Sha256};

        type HmacSha256 = Hmac<Sha256>;

        // Parse URL to get host and path
        let url_parts: Vec<&str> = url.trim_start_matches("https://").splitn(2, '/').collect();
        let host = url_parts[0];
        let path = if url_parts.len() > 1 {
            format!("/{}", url_parts[1])
        } else {
            "/".to_string()
        };

        // Get current timestamp
        let now = time::OffsetDateTime::now_utc();
        let amz_date = now
            .format(&time::format_description::parse("[year][month][day]T[hour][minute][second]Z").unwrap())
            .context("format timestamp")?;
        let date_stamp = now
            .format(&time::format_description::parse("[year][month][day]").unwrap())
            .context("format date")?;

        // Create canonical headers
        let mut canonical_headers = headers.clone();
        canonical_headers.insert("host".to_string(), host.to_string());
        canonical_headers.insert("x-amz-date".to_string(), amz_date.clone());
        if let Some(ref token) = self.session_token {
            canonical_headers.insert("x-amz-security-token".to_string(), token.clone());
        }

        let mut signed_headers_list: Vec<String> = canonical_headers.keys().cloned().collect();
        signed_headers_list.sort();
        let signed_headers = signed_headers_list.join(";");

        let mut canonical_header_str = String::new();
        for key in &signed_headers_list {
            canonical_header_str.push_str(&format!("{}:{}\n", key, canonical_headers[key]));
        }

        // Hash payload
        let mut hasher = Sha256::new();
        hasher.update(body.as_bytes());
        let payload_hash = hex::encode(hasher.finalize());

        // Create canonical request
        let canonical_request = format!(
            "{}\n{}\n\n{}\n{}\n{}",
            method, path, canonical_header_str, signed_headers, payload_hash
        );

        // Create string to sign
        let mut hasher = Sha256::new();
        hasher.update(canonical_request.as_bytes());
        let canonical_request_hash = hex::encode(hasher.finalize());

        let credential_scope = format!("{}/{}/bedrock/aws4_request", date_stamp, self.region);
        let string_to_sign = format!(
            "AWS4-HMAC-SHA256\n{}\n{}\n{}",
            amz_date, credential_scope, canonical_request_hash
        );

        // Calculate signature
        let k_date = HmacSha256::new_from_slice(format!("AWS4{}", self.secret_key).as_bytes())
            .context("create k_date hmac")?
            .chain_update(date_stamp.as_bytes())
            .finalize()
            .into_bytes();

        let k_region = HmacSha256::new_from_slice(&k_date)
            .context("create k_region hmac")?
            .chain_update(self.region.as_bytes())
            .finalize()
            .into_bytes();

        let k_service = HmacSha256::new_from_slice(&k_region)
            .context("create k_service hmac")?
            .chain_update(b"bedrock")
            .finalize()
            .into_bytes();

        let k_signing = HmacSha256::new_from_slice(&k_service)
            .context("create k_signing hmac")?
            .chain_update(b"aws4_request")
            .finalize()
            .into_bytes();

        let signature = HmacSha256::new_from_slice(&k_signing)
            .context("create signature hmac")?
            .chain_update(string_to_sign.as_bytes())
            .finalize()
            .into_bytes();

        let signature_hex = hex::encode(signature);

        // Build authorization header
        let authorization = format!(
            "AWS4-HMAC-SHA256 Credential={}/{}, SignedHeaders={}, Signature={}",
            self.access_key, credential_scope, signed_headers, signature_hex
        );

        let mut result = canonical_headers;
        result.insert("authorization".to_string(), authorization);
        Ok(result)
    }
}

impl Embedder for BedrockEmbedder {
    fn profile(&self) -> &EmbeddingProfile {
        &self.profile
    }

    fn metadata(&self) -> EmbedderMetadata {
        EmbedderMetadata {
            provider: Some("bedrock".to_string()),
            provider_api_base: Some(self.api_base.clone()),
            provider_model: self.profile.model.clone(),
            provider_model_revision: self.observed_model.lock().ok().and_then(|g| g.clone()),
            runtime: Some("http".to_string()),
            runtime_version: crate::build_info::runtime_version_http(),
            provider_request: self.observed_request.lock().ok().and_then(|g| g.clone()),
            provider_response: self.observed_response.lock().ok().and_then(|g| g.clone()),
            provider_response_headers: self.observed_headers.lock().ok().and_then(|g| g.clone()),
            model_sha256: None,
            notes: Some("AWS Bedrock requires AWS credentials (AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY) and uses AWS Signature V4".to_string()),
        }
    }

    fn embed(&self, inputs: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
        let model = self
            .profile
            .model
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("bedrock embedder missing model"))?;

        // AWS Bedrock uses model-specific endpoints
        let url = format!("{}/model/{}/invoke", self.api_base, model);

        if let Ok(mut g) = self.observed_request.lock() {
            *g = Some(serde_json::json!({
                "endpoint": format!("/model/{}/invoke", model),
                "model": model,
                "input_count": inputs.len(),
            }));
        }

        // Prepare request body based on model type
        let request_body = if model.starts_with("amazon.titan-embed") {
            // Amazon Titan Embeddings format
            serde_json::json!({
                "inputText": inputs.join(" ")
            })
        } else if model.starts_with("cohere.embed") {
            // Cohere Embeddings format
            serde_json::json!({
                "texts": inputs,
                "input_type": "search_document"
            })
        } else {
            // Generic format - try inputText
            serde_json::json!({
                "inputText": inputs.join(" ")
            })
        };

        let body_str = serde_json::to_string(&request_body).context("serialize request body")?;

        // Create initial headers
        let mut headers = BTreeMap::new();
        headers.insert("content-type".to_string(), "application/json".to_string());

        // Sign the request
        let signed_headers = self
            .sign_request("POST", &url, &headers, &body_str)
            .context("sign AWS request")?;

        // Build ureq request with signed headers
        let mut request = ureq::post(&url);
        for (key, value) in &signed_headers {
            request = request.set(key, value);
        }

        let response = request
            .send_string(&body_str)
            .context("bedrock embeddings request")?;

        let headers_to_collect = collect_headers(
            &response,
            &["x-amzn-requestid", "x-amzn-bedrock-invocation-latency", "date", "server"],
        );
        if !headers_to_collect.is_empty() {
            if let Ok(mut g) = self.observed_headers.lock() {
                *g = Some(headers_to_collect);
            }
        }

        let raw: serde_json::Value = response
            .into_json()
            .context("parse bedrock embeddings response")?;

        if let Some(obj) = raw.as_object() {
            let mut meta = serde_json::Map::new();
            for k in ["inputTextTokenCount"] {
                if let Some(v) = obj.get(k) {
                    meta.insert(k.to_string(), v.clone());
                }
            }
            if let Ok(mut g) = self.observed_response.lock() {
                *g = Some(serde_json::Value::Object(meta));
            }
        }

        // Parse response based on model type
        let embeddings = if model.starts_with("amazon.titan-embed") {
            // Amazon Titan response format
            let embedding = raw
                .get("embedding")
                .and_then(|v| v.as_array())
                .ok_or_else(|| anyhow::anyhow!("bedrock response missing embedding[]"))?;

            let mut vec = Vec::with_capacity(embedding.len());
            for f in embedding {
                vec.push(
                    f.as_f64()
                        .ok_or_else(|| anyhow::anyhow!("bedrock embedding contains non-number"))?
                        as f32,
                );
            }
            ensure_dim(self.profile.dim, vec.len(), "bedrock")?;

            // Repeat the same embedding for all inputs (Titan embeds concatenated text)
            vec![vec; inputs.len()]
        } else if model.starts_with("cohere.embed") {
            // Cohere response format
            let embeddings_array = raw
                .get("embeddings")
                .and_then(|v| v.as_array())
                .ok_or_else(|| anyhow::anyhow!("bedrock cohere response missing embeddings[]"))?;

            let mut result = Vec::with_capacity(embeddings_array.len());
            for emb in embeddings_array {
                let arr = emb
                    .as_array()
                    .ok_or_else(|| anyhow::anyhow!("bedrock embedding is not an array"))?;
                let mut vec = Vec::with_capacity(arr.len());
                for f in arr {
                    vec.push(
                        f.as_f64()
                            .ok_or_else(|| anyhow::anyhow!("bedrock embedding contains non-number"))?
                            as f32,
                    );
                }
                ensure_dim(self.profile.dim, vec.len(), "bedrock")?;
                result.push(vec);
            }
            result
        } else {
            // Generic format
            let embedding = raw
                .get("embedding")
                .and_then(|v| v.as_array())
                .ok_or_else(|| anyhow::anyhow!("bedrock response missing embedding[]"))?;

            let mut vec = Vec::with_capacity(embedding.len());
            for f in embedding {
                vec.push(
                    f.as_f64()
                        .ok_or_else(|| anyhow::anyhow!("bedrock embedding contains non-number"))?
                        as f32,
                );
            }
            ensure_dim(self.profile.dim, vec.len(), "bedrock")?;
            vec![vec; inputs.len()]
        };

        Ok(embeddings)
    }
}
