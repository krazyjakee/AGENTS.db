use anyhow::Context;
use serde::Serialize;
use std::path::{Path, PathBuf};

use agentsdb_embeddings::config::{
    roll_up_embedding_options_from_paths, standard_layer_paths_for_dir, AllowlistOp,
    ChecksumAllowlistRecord, EmbeddingOptionsPatch, ModelChecksumPin, OptionsRecord,
    ResolvedEmbeddingOptions, DEFAULT_LOCAL_REVISION, KIND_OPTIONS,
};

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[derive(Debug, Clone)]
/// Represents the resolved paths for the various AGENTS.db layers.
struct ResolvedPaths {
    base: PathBuf,
    user: PathBuf,
    delta: PathBuf,
    local: PathBuf,
}

fn resolve_paths(
    dir: &Path,
    base: Option<&str>,
    user: Option<&str>,
    delta: Option<&str>,
    local: Option<&str>,
) -> ResolvedPaths {
    let standard = standard_layer_paths_for_dir(dir);
    ResolvedPaths {
        base: base.map(PathBuf::from).unwrap_or(standard.base),
        user: user.map(PathBuf::from).unwrap_or(standard.user),
        delta: delta.map(PathBuf::from).unwrap_or(standard.delta),
        local: local.map(PathBuf::from).unwrap_or(standard.local),
    }
}

fn last_options_patch_in_path(path: &Path) -> anyhow::Result<Option<EmbeddingOptionsPatch>> {
    if !path.exists() {
        return Ok(None);
    }
    let file = agentsdb_format::LayerFile::open(path)
        .with_context(|| format!("open {}", path.display()))?;
    let mut last: Option<EmbeddingOptionsPatch> = None;
    for chunk in file.chunks() {
        let chunk = chunk.context("read chunk")?;
        if chunk.kind != KIND_OPTIONS {
            continue;
        }
        let record: OptionsRecord =
            serde_json::from_str(chunk.content).context("parse options JSON")?;
        if let Some(embedding) = record.embedding {
            last = Some(embedding);
        }
    }
    Ok(last)
}

#[derive(Serialize)]
/// Represents an embedding options patch in JSON format.
struct PatchJson {
    backend: Option<String>,
    model: Option<String>,
    revision: Option<String>,
    model_path: Option<String>,
    model_sha256: Option<String>,
    dim: Option<usize>,
    api_base: Option<String>,
    api_key_env: Option<String>,
    cache_enabled: Option<bool>,
    cache_dir: Option<String>,
}

impl From<EmbeddingOptionsPatch> for PatchJson {
    fn from(v: EmbeddingOptionsPatch) -> Self {
        Self {
            backend: v.backend,
            model: v.model,
            revision: v.revision,
            model_path: v.model_path,
            model_sha256: v.model_sha256,
            dim: v.dim,
            api_base: v.api_base,
            api_key_env: v.api_key_env,
            cache_enabled: v.cache_enabled,
            cache_dir: v.cache_dir,
        }
    }
}

#[derive(Serialize)]
/// Represents resolved embedding options in JSON format.
struct ResolvedJson {
    backend: String,
    model: Option<String>,
    revision: Option<String>,
    model_path: Option<String>,
    model_sha256: Option<String>,
    dim: Option<usize>,
    api_base: Option<String>,
    api_key_env: Option<String>,
    cache_enabled: bool,
    cache_dir: Option<String>,
    checksum_allowlist: Vec<ModelChecksumPin>,
}

impl From<ResolvedEmbeddingOptions> for ResolvedJson {
    fn from(v: ResolvedEmbeddingOptions) -> Self {
        let checksum_allowlist = v
            .checksum_allowlist
            .into_iter()
            .map(|(k, sha256)| ModelChecksumPin {
                model: k.model,
                revision: k.revision,
                sha256: Some(sha256),
            })
            .collect();
        Self {
            backend: v.backend,
            model: v.model,
            revision: v.revision,
            model_path: v.model_path,
            model_sha256: v.model_sha256,
            dim: v.dim,
            api_base: v.api_base,
            api_key_env: v.api_key_env,
            cache_enabled: v.cache_enabled,
            cache_dir: v.cache_dir,
            checksum_allowlist,
        }
    }
}

#[derive(Serialize)]
/// Represents a layer's options and existence in JSON format.
struct LayerJson {
    path: String,
    exists: bool,
    patch: Option<PatchJson>,
}

pub(crate) fn cmd_options_show(
    dir: &str,
    base: Option<&str>,
    user: Option<&str>,
    delta: Option<&str>,
    local: Option<&str>,
    json: bool,
) -> anyhow::Result<()> {
    let dir = Path::new(dir);
    let paths = resolve_paths(dir, base, user, delta, local);

    let resolved = roll_up_embedding_options_from_paths(
        Some(paths.local.as_path()),
        Some(paths.user.as_path()),
        Some(paths.delta.as_path()),
        Some(paths.base.as_path()),
    )
    .context("roll up options")?;

    let base_patch = last_options_patch_in_path(&paths.base).context("read base options")?;
    let user_patch = last_options_patch_in_path(&paths.user).context("read user options")?;
    let delta_patch = last_options_patch_in_path(&paths.delta).context("read delta options")?;
    let local_patch = last_options_patch_in_path(&paths.local).context("read local options")?;

    if json {
        #[derive(Serialize)]
        struct Out {
            ok: bool,
            resolved: ResolvedJson,
            base: LayerJson,
            user: LayerJson,
            delta: LayerJson,
            local: LayerJson,
        }
        let out = Out {
            ok: true,
            resolved: resolved.into(),
            base: LayerJson {
                path: paths.base.display().to_string(),
                exists: paths.base.exists(),
                patch: base_patch.map(Into::into),
            },
            user: LayerJson {
                path: paths.user.display().to_string(),
                exists: paths.user.exists(),
                patch: user_patch.map(Into::into),
            },
            delta: LayerJson {
                path: paths.delta.display().to_string(),
                exists: paths.delta.exists(),
                patch: delta_patch.map(Into::into),
            },
            local: LayerJson {
                path: paths.local.display().to_string(),
                exists: paths.local.exists(),
                patch: local_patch.map(Into::into),
            },
        };
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    println!("Resolved embedding options:");
    println!(
        "  backend={:?} model={:?} revision={:?} model_path={:?} model_sha256={:?} dim={:?} cache_enabled={:?} cache_dir={:?}",
        resolved.backend,
        resolved.model,
        resolved.revision,
        resolved.model_path,
        resolved.model_sha256,
        resolved.dim,
        resolved.cache_enabled,
        resolved.cache_dir
    );
    if !resolved.checksum_allowlist.is_empty() {
        println!(
            "  checksum_allowlist={} entries",
            resolved.checksum_allowlist.len()
        );
    }
    println!();
    for (label, path, patch) in [
        ("local", &paths.local, local_patch),
        ("user", &paths.user, user_patch),
        ("delta", &paths.delta, delta_patch),
        ("base", &paths.base, base_patch),
    ] {
        if !path.exists() {
            println!("{label}: {} (missing)", path.display());
            continue;
        }
        match patch {
            None => println!("{label}: {} (no options record)", path.display()),
            Some(patch) => println!(
                "{label}: {} (patch backend={:?} model={:?} revision={:?} model_sha256={:?} dim={:?} api_base={:?} api_key_env={:?} cache_enabled={:?} cache_dir={:?})",
                path.display(),
                patch.backend,
                patch.model,
                patch.revision,
                patch.model_sha256,
                patch.dim,
                patch.api_base,
                patch.api_key_env,
                patch.cache_enabled,
                patch.cache_dir
            ),
        }
    }

    Ok(())
}

pub(crate) fn cmd_options_allowlist_list(
    dir: &str,
    base: Option<&str>,
    user: Option<&str>,
    delta: Option<&str>,
    local: Option<&str>,
    json: bool,
) -> anyhow::Result<()> {
    let dir = Path::new(dir);
    let paths = resolve_paths(dir, base, user, delta, local);

    let resolved = roll_up_embedding_options_from_paths(
        Some(paths.local.as_path()),
        Some(paths.user.as_path()),
        Some(paths.delta.as_path()),
        Some(paths.base.as_path()),
    )
    .context("roll up options")?;

    let mut entries: Vec<ModelChecksumPin> = resolved
        .checksum_allowlist
        .into_iter()
        .map(|(k, sha256)| ModelChecksumPin {
            model: k.model,
            revision: k.revision,
            sha256: Some(sha256),
        })
        .collect();
    entries.sort_by(|a, b| {
        (a.model.as_str(), a.revision.as_str()).cmp(&(b.model.as_str(), b.revision.as_str()))
    });

    if json {
        #[derive(Serialize)]
        struct Out {
            ok: bool,
            entries: Vec<ModelChecksumPin>,
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&Out { ok: true, entries })?
        );
        return Ok(());
    }

    if entries.is_empty() {
        println!("No allowlist entries (use `agentsdb options allowlist add ...`).");
        return Ok(());
    }
    println!("Allowlist entries:");
    for e in entries {
        println!(
            "  model={:?} revision={:?} sha256={}",
            e.model,
            e.revision,
            e.sha256.as_deref().unwrap_or("")
        );
    }
    Ok(())
}

fn write_allowlist_record(
    dir: &Path,
    scope: &str,
    record: ChecksumAllowlistRecord,
    json: bool,
) -> anyhow::Result<()> {
    let paths = resolve_paths(dir, None, None, None, None);

    // Only AGENTS.db (base layer) should store options documents.
    // This ensures all operations use the same immutable embedding configuration.
    if scope != "base" {
        anyhow::bail!(
            "allowlist options can only be set on base layer (AGENTS.db); got --scope {scope:?}\n\
             Embedding options must be immutable and stored only in AGENTS.db to ensure consistency.\n\
             Use: agentsdb options allowlist ... --scope base"
        );
    }

    let (target_path, allow_user, allow_base) = match scope {
        "base" => (paths.base.clone(), false, true),
        other => anyhow::bail!("--scope must be 'base' (got {other:?})"),
    };

    if allow_base {
        agentsdb_format::ensure_writable_layer_path_allow_base(&target_path)
            .context("permission check")?;
    } else if allow_user {
        agentsdb_format::ensure_writable_layer_path_allow_user(&target_path)
            .context("permission check")?;
    } else {
        agentsdb_format::ensure_writable_layer_path(&target_path).context("permission check")?;
    }

    let schema = if target_path.exists() {
        let file = agentsdb_format::LayerFile::open(&target_path)
            .with_context(|| format!("open {}", target_path.display()))?;
        agentsdb_format::schema_of(&file)
    } else if paths.base.exists() {
        let file = agentsdb_format::LayerFile::open(&paths.base)
            .with_context(|| format!("open {}", paths.base.display()))?;
        agentsdb_format::schema_of(&file)
    } else {
        agentsdb_format::LayerSchema {
            dim: 128,
            element_type: agentsdb_format::EmbeddingElementType::F32,
            quant_scale: 1.0,
        }
    };

    let record = OptionsRecord {
        embedding: None,
        checksum_allowlist: Some(record),
    };
    let content = serde_json::to_string_pretty(&record).context("serialize allowlist record")?;

    let chunk_id = if target_path.exists() { 0 } else { 1 };
    let chunk = agentsdb_format::ChunkInput {
        id: chunk_id,
        kind: KIND_OPTIONS.to_string(),
        content,
        author: "human".to_string(),
        confidence: 1.0,
        created_at_unix_ms: now_unix_ms(),
        embedding: vec![0.0; schema.dim as usize],
        sources: Vec::new(),
    };

    let (action, assigned_id) = if target_path.exists() {
        let mut chunks = vec![chunk];
        let ids = agentsdb_format::append_layer_atomic(&target_path, &mut chunks, None)
            .context("append")?;
        ("appended", ids[0])
    } else {
        if let Some(parent) = target_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create dir {}", parent.display()))?;
        }
        let mut chunks = [chunk];
        agentsdb_format::write_layer_atomic(&target_path, &schema, &mut chunks, None)
            .context("write")?;
        ("created", chunk_id)
    };

    if json {
        #[derive(Serialize)]
        struct Out {
            ok: bool,
            action: &'static str,
            path: String,
            id: u32,
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&Out {
                ok: true,
                action,
                path: target_path.display().to_string(),
                id: assigned_id
            })?
        );
        return Ok(());
    }

    println!(
        "Allowlist {action} in {} (id={assigned_id})",
        target_path.display()
    );
    Ok(())
}

pub(crate) fn cmd_options_allowlist_add(
    dir: &str,
    scope: &str,
    model: &str,
    revision: Option<&str>,
    sha256: &str,
    json: bool,
) -> anyhow::Result<()> {
    agentsdb_embeddings::verification::ensure_sha256_hex(sha256).context("validate sha256")?;
    let dir = Path::new(dir);
    let record = ChecksumAllowlistRecord {
        op: AllowlistOp::Add,
        entries: vec![ModelChecksumPin {
            model: model.to_string(),
            revision: revision.unwrap_or(DEFAULT_LOCAL_REVISION).to_string(),
            sha256: Some(sha256.to_string()),
        }],
    };
    write_allowlist_record(dir, scope, record, json)
}

pub(crate) fn cmd_options_allowlist_remove(
    dir: &str,
    scope: &str,
    model: &str,
    revision: Option<&str>,
    json: bool,
) -> anyhow::Result<()> {
    let dir = Path::new(dir);
    let record = ChecksumAllowlistRecord {
        op: AllowlistOp::Remove,
        entries: vec![ModelChecksumPin {
            model: model.to_string(),
            revision: revision.unwrap_or(DEFAULT_LOCAL_REVISION).to_string(),
            sha256: None,
        }],
    };
    write_allowlist_record(dir, scope, record, json)
}

pub(crate) fn cmd_options_allowlist_clear(
    dir: &str,
    scope: &str,
    json: bool,
) -> anyhow::Result<()> {
    let dir = Path::new(dir);
    let record = ChecksumAllowlistRecord {
        op: AllowlistOp::Clear,
        entries: Vec::new(),
    };
    write_allowlist_record(dir, scope, record, json)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn cmd_options_set(
    dir: &str,
    scope: &str,
    backend: Option<&str>,
    model: Option<&str>,
    revision: Option<&str>,
    model_path: Option<&str>,
    model_sha256: Option<&str>,
    dim: Option<u32>,
    api_base: Option<&str>,
    api_key_env: Option<&str>,
    cache_enabled: Option<bool>,
    cache_dir: Option<&str>,
    json: bool,
) -> anyhow::Result<()> {
    let dir = Path::new(dir);
    let paths = resolve_paths(dir, None, None, None, None);

    if backend.is_none()
        && model.is_none()
        && revision.is_none()
        && model_path.is_none()
        && model_sha256.is_none()
        && dim.is_none()
        && api_base.is_none()
        && api_key_env.is_none()
        && cache_enabled.is_none()
        && cache_dir.is_none()
    {
        anyhow::bail!("no fields provided (use one or more of --backend/--model/--revision/--model-path/--model-sha256/--dim/--api-base/--api-key-env/--cache/--cache-dir)");
    }

    // Only AGENTS.db (base layer) should store options documents.
    // This ensures all operations use the same immutable embedding configuration.
    if scope != "base" {
        anyhow::bail!(
            "options can only be set on base layer (AGENTS.db); got --scope {scope:?}\n\
             Embedding options must be immutable and stored only in AGENTS.db to ensure consistency.\n\
             Use: agentsdb options set --scope base ..."
        );
    }

    let (target_path, allow_user, allow_base) = match scope {
        "base" => (paths.base.clone(), false, true),
        other => anyhow::bail!("--scope must be 'base' (got {other:?})"),
    };

    if allow_base {
        agentsdb_format::ensure_writable_layer_path_allow_base(&target_path)
            .context("permission check")?;
    } else if allow_user {
        agentsdb_format::ensure_writable_layer_path_allow_user(&target_path)
            .context("permission check")?;
    } else {
        agentsdb_format::ensure_writable_layer_path(&target_path).context("permission check")?;
    }

    let schema = if target_path.exists() {
        let file = agentsdb_format::LayerFile::open(&target_path)
            .with_context(|| format!("open {}", target_path.display()))?;
        agentsdb_format::schema_of(&file)
    } else if paths.base.exists() {
        let file = agentsdb_format::LayerFile::open(&paths.base)
            .with_context(|| format!("open {}", paths.base.display()))?;
        agentsdb_format::schema_of(&file)
    } else {
        agentsdb_format::LayerSchema {
            dim: dim.unwrap_or(128),
            element_type: agentsdb_format::EmbeddingElementType::F32,
            quant_scale: 1.0,
        }
    };

    if let Some(cfg_dim) = dim {
        if cfg_dim != schema.dim {
            anyhow::bail!(
                "embedding dim mismatch (target schema is dim={}, options specify dim={cfg_dim})",
                schema.dim
            );
        }
    }

    let patch = EmbeddingOptionsPatch {
        backend: backend.map(str::to_string),
        model: model.map(str::to_string),
        revision: revision.map(str::to_string),
        model_path: model_path.map(str::to_string),
        model_sha256: model_sha256.map(str::to_string),
        dim: dim.map(|d| d as usize),
        api_base: api_base.map(str::to_string),
        api_key_env: api_key_env.map(str::to_string),
        cache_enabled,
        cache_dir: cache_dir.map(str::to_string),
    };
    let record = OptionsRecord {
        embedding: Some(patch),
        checksum_allowlist: None,
    };
    let content = serde_json::to_string_pretty(&record).context("serialize options")?;

    let chunk_id = if target_path.exists() { 0 } else { 1 };
    let chunk = agentsdb_format::ChunkInput {
        id: chunk_id,
        kind: KIND_OPTIONS.to_string(),
        content,
        author: "human".to_string(),
        confidence: 1.0,
        created_at_unix_ms: now_unix_ms(),
        embedding: vec![0.0; schema.dim as usize],
        sources: Vec::new(),
    };

    let (action, assigned_id) = if target_path.exists() {
        let mut chunks = vec![chunk];
        let ids = agentsdb_format::append_layer_atomic(&target_path, &mut chunks, None)
            .context("append")?;
        ("appended", ids[0])
    } else {
        if let Some(parent) = target_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create dir {}", parent.display()))?;
        }
        let mut chunks = [chunk];
        agentsdb_format::write_layer_atomic(&target_path, &schema, &mut chunks, None)
            .context("write")?;
        ("created", chunk_id)
    };

    if json {
        #[derive(Serialize)]
        struct Out {
            ok: bool,
            action: &'static str,
            path: String,
            id: u32,
            schema_dim: u32,
        }
        let out = Out {
            ok: true,
            action,
            path: target_path.display().to_string(),
            id: assigned_id,
            schema_dim: schema.dim,
        };
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    println!(
        "Options {action} in {} (id={assigned_id})",
        target_path.display()
    );
    Ok(())
}

fn prompt_line(label: &str, default: Option<&str>) -> anyhow::Result<String> {
    use std::io::Write;
    let mut stdout = std::io::stdout();
    match default {
        Some(d) if !d.is_empty() => write!(stdout, "{label} [{d}]: ")?,
        _ => write!(stdout, "{label}: ")?,
    }
    stdout.flush()?;
    let mut s = String::new();
    std::io::stdin().read_line(&mut s)?;
    let s = s.trim().to_string();
    if s.is_empty() {
        Ok(default.unwrap_or_default().to_string())
    } else {
        Ok(s)
    }
}

pub(crate) fn cmd_options_wizard(dir: &str, json: bool) -> anyhow::Result<()> {
    if json {
        anyhow::bail!("--json is not supported for options wizard");
    }
    let dir = Path::new(dir);

    let standard = standard_layer_paths_for_dir(dir);
    let existing_schema_dim = standard.base.exists()
        .then(|| agentsdb_format::LayerFile::open(&standard.base).ok())
        .flatten()
        .map(|f| agentsdb_format::schema_of(&f).dim);

    println!("Embedding options wizard (stores immutable options in AGENTS.db).");
    println!("Note: backends other than `hash` require rebuilding `agentsdb` with the matching Cargo feature.");

    let backend = prompt_line(
        "Backend (hash|ort|candle|openai|voyage|cohere|anthropic|bedrock|gemini)",
        Some("candle"),
    )?;

    let schema_dim = existing_schema_dim.unwrap_or(match backend.as_str() {
        "hash" => 128,
        _ => 384,
    });
    let dim_s = prompt_line("Embedding dim", Some(&schema_dim.to_string()))?;
    let dim: u32 = dim_s.parse().context("parse dim")?;

    let model_default = match backend.as_str() {
        "ort" | "candle" => Some(agentsdb_embeddings::config::DEFAULT_LOCAL_MODEL),
        "openai" => Some("text-embedding-3-small"),
        "voyage" => Some("voyage-3"),
        "cohere" => Some("embed-english-v3.0"),
        "anthropic" => Some("voyage-3"),
        "bedrock" => Some("amazon.titan-embed-text-v1"),
        "gemini" => Some("text-embedding-004"),
        _ => None,
    };
    let model = if model_default.is_some() {
        let s = prompt_line("Model (optional)", model_default)?;
        (!s.trim().is_empty()).then_some(s)
    } else {
        None
    };

    let model_path = match backend.as_str() {
        "ort" => {
            let s = prompt_line("Local model path (optional; dir or .onnx file)", Some(""))?;
            (!s.trim().is_empty()).then_some(s)
        }
        _ => None,
    };

    let model_sha256 = match backend.as_str() {
        "ort" | "candle" => {
            let s = prompt_line("Expected model sha256 (optional)", Some(""))?;
            (!s.trim().is_empty()).then_some(s)
        }
        _ => None,
    };

    let (api_base, api_key_env) = match backend.as_str() {
        "openai" => {
            let base = prompt_line("API base (optional)", Some("https://api.openai.com"))?;
            let key_env = prompt_line("API key env var", Some("OPENAI_API_KEY"))?;
            (Some(base), Some(key_env))
        }
        "voyage" => {
            let base = prompt_line("API base (optional)", Some("https://api.voyageai.com"))?;
            let key_env = prompt_line("API key env var", Some("VOYAGE_API_KEY"))?;
            (Some(base), Some(key_env))
        }
        "cohere" => {
            let base = prompt_line("API base (optional)", Some("https://api.cohere.com"))?;
            let key_env = prompt_line("API key env var", Some("COHERE_API_KEY"))?;
            (Some(base), Some(key_env))
        }
        "anthropic" => {
            let base = prompt_line("API base (optional)", Some("https://api.anthropic.com"))?;
            let key_env = prompt_line("API key env var", Some("ANTHROPIC_API_KEY"))?;
            (Some(base), Some(key_env))
        }
        "bedrock" => {
            let base = prompt_line("API base (optional)", Some("https://bedrock-runtime.{region}.amazonaws.com"))?;
            let key_env = prompt_line("Region env var (optional)", Some("AWS_REGION"))?;
            (Some(base), Some(key_env))
        }
        "gemini" => {
            let base = prompt_line("API base (optional)", Some("https://generativelanguage.googleapis.com"))?;
            let key_env = prompt_line("API key env var", Some("GEMINI_API_KEY"))?;
            (Some(base), Some(key_env))
        }
        _ => (None, None),
    };

    let cache = prompt_line("Enable cache? (y/n)", Some("n"))?;
    let cache_enabled = matches!(
        cache.to_ascii_lowercase().as_str(),
        "y" | "yes" | "1" | "true"
    );
    let cache_dir = if cache_enabled {
        let s = prompt_line("Cache dir (optional)", Some(""))?;
        (!s.trim().is_empty()).then_some(s)
    } else {
        None
    };

    // Always write to base layer (AGENTS.db) for immutable embedding configuration
    cmd_options_set(
        dir.to_string_lossy().as_ref(),
        "base",
        Some(backend.as_str()),
        model.as_deref(),
        None,
        model_path.as_deref(),
        model_sha256.as_deref(),
        Some(dim),
        api_base.as_deref(),
        api_key_env.as_deref(),
        Some(cache_enabled),
        cache_dir.as_deref(),
        false,
    )
}
