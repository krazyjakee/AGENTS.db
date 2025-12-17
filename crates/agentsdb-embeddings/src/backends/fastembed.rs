//! FastEmbed (ONNX Runtime) local embedding backend.

use anyhow::Context;
use crate::embedder::{Embedder, EmbedderMetadata, EmbeddingProfile, OutputNorm};
use super::common::{ensure_dim, hex_lower};

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

struct FastembedEmbedder {
    profile: EmbeddingProfile,
    inner: fastembed::TextEmbedding,
    model_sha256: Option<String>,
    notes: Option<String>,
}

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

fn parse_fastembed_model(model: &str) -> anyhow::Result<fastembed::EmbeddingModel> {
    match model {
        // Keep this list intentionally small; add more models as needed.
        "all-minilm-l6-v2" | "all-MiniLM-L6-v2" => Ok(fastembed::EmbeddingModel::AllMiniLML6V2),
        other => anyhow::bail!("unknown local model {other:?} (supported: \"all-minilm-l6-v2\")"),
    }
}

fn fastembed_model_dim(model: &fastembed::EmbeddingModel) -> usize {
    match model {
        fastembed::EmbeddingModel::AllMiniLML6V2 => 384,
        // Conservative default for any future models we add to `parse_fastembed_model`.
        _ => 384,
    }
}

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

fn read_hf_bytes(repo: &hf_hub::api::sync::ApiRepo, filename: &str) -> anyhow::Result<Vec<u8>> {
    let path = repo
        .get(filename)
        .with_context(|| format!("download {filename}"))?;
    std::fs::read(&path).with_context(|| format!("read {}", path.display()))
}

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
