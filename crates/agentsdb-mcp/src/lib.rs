use agentsdb_core::types::{LayerId, SearchFilters};
use agentsdb_embeddings::config::{
    roll_up_embedding_options, roll_up_embedding_options_from_paths,
};
use agentsdb_embeddings::layer_metadata::ensure_layer_metadata_compatible_with_embedder;
use agentsdb_embeddings::layer_metadata::LayerMetadataV1;
use agentsdb_query::{LayerSet, SearchQuery};
use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const TOOL_AGENTS_SEARCH: &str = "agents_search";
const TOOL_AGENTS_CONTEXT_WRITE: &str = "agents_context_write";
const TOOL_AGENTS_CONTEXT_PROPOSE: &str = "agents_context_propose";

// Legacy dot-separated names kept for backward compatibility with older clients.
const TOOL_AGENTS_SEARCH_LEGACY: &str = "agents.search";
const TOOL_AGENTS_CONTEXT_WRITE_LEGACY: &str = "agents.context.write";
const TOOL_AGENTS_CONTEXT_PROPOSE_LEGACY: &str = "agents.context.propose";
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub struct ServerConfig {
    pub base: Option<String>,
    pub user: Option<String>,
    pub delta: Option<String>,
    pub local: Option<String>,
}

fn expand_path_vars(path: &str, cwd: &Path) -> anyhow::Result<String> {
    let mut out = path.to_string();

    let cwd_s = cwd.to_string_lossy();
    out = out.replace("${PWD}", &cwd_s);
    out = out.replace("$PWD", &cwd_s);

    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from);
    if let Some(home) = home {
        let home_s = home.to_string_lossy();
        out = out.replace("${HOME}", &home_s);
        out = out.replace("$HOME", &home_s);
        if out == "~" {
            out = home_s.to_string();
        } else if let Some(rest) = out.strip_prefix("~/") {
            out = home.join(rest).to_string_lossy().into_owned();
        }
    }

    Ok(out)
}

fn find_relative_in_ancestors(cwd: &Path, rel: &Path) -> Option<PathBuf> {
    if !rel.is_relative() {
        return None;
    }
    for ancestor in cwd.ancestors() {
        let candidate = ancestor.join(rel);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

fn normalize_config_with_cwd(mut config: ServerConfig, cwd: &Path) -> anyhow::Result<ServerConfig> {
    config.base = config
        .base
        .as_deref()
        .map(|p| expand_path_vars(p, cwd))
        .transpose()?;
    config.user = config
        .user
        .as_deref()
        .map(|p| expand_path_vars(p, cwd))
        .transpose()?;
    config.delta = config
        .delta
        .as_deref()
        .map(|p| expand_path_vars(p, cwd))
        .transpose()?;
    config.local = config
        .local
        .as_deref()
        .map(|p| expand_path_vars(p, cwd))
        .transpose()?;

    // Best-effort: if base is a relative path and the file exists in an ancestor,
    // resolve it so we can anchor other relative layer paths to the same directory.
    let base_dir = if let Some(base) = config.base.as_deref() {
        let base_path = PathBuf::from(base);
        let resolved_base = if base_path.exists() {
            Some(base_path)
        } else {
            find_relative_in_ancestors(cwd, &base_path)
        };
        resolved_base
            .as_deref()
            .and_then(|p| p.parent())
            .map(Path::to_path_buf)
    } else {
        None
    };

    if let Some(base_dir) = base_dir {
        if let Some(user) = config.user.as_deref() {
            let p = Path::new(user);
            if p.is_relative() {
                config.user = Some(base_dir.join(p).to_string_lossy().into_owned());
            }
        }
        if let Some(delta) = config.delta.as_deref() {
            let p = Path::new(delta);
            if p.is_relative() {
                config.delta = Some(base_dir.join(p).to_string_lossy().into_owned());
            }
        }
        if let Some(local) = config.local.as_deref() {
            let p = Path::new(local);
            if p.is_relative() {
                config.local = Some(base_dir.join(p).to_string_lossy().into_owned());
            }
        }
        if let Some(base) = config.base.as_deref() {
            let p = PathBuf::from(base);
            if !p.exists() {
                if let Some(found) = find_relative_in_ancestors(cwd, &p) {
                    config.base = Some(found.to_string_lossy().into_owned());
                }
            }
        }
    }

    Ok(config)
}

#[derive(Debug, Deserialize)]
struct Request {
    #[allow(dead_code)]
    #[serde(default)]
    pub jsonrpc: Option<String>,
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Serialize)]
struct Response {
    jsonrpc: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<ErrorObj>,
}

#[derive(Debug, Serialize)]
struct ErrorObj {
    code: i64,
    message: String,
}

#[derive(Debug)]
struct RpcError {
    code: i64,
    message: String,
}

impl RpcError {
    fn parse_error(message: impl Into<String>) -> Self {
        Self {
            code: -32700,
            message: message.into(),
        }
    }
    fn invalid_params(message: impl Into<String>) -> Self {
        Self {
            code: -32602,
            message: message.into(),
        }
    }
    fn method_not_found(message: impl Into<String>) -> Self {
        Self {
            code: -32601,
            message: message.into(),
        }
    }
    fn internal_error(message: impl Into<String>) -> Self {
        Self {
            code: -32603,
            message: message.into(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct SearchParams {
    query: String,
    #[serde(default)]
    query_vec: Option<Vec<f32>>,
    #[serde(default)]
    k: Option<usize>,
    #[serde(default)]
    filters: Option<SearchFiltersParams>,
    #[serde(default)]
    layers: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct SearchFiltersParams {
    #[serde(default)]
    kind: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct WriteParams {
    content: String,
    kind: String,
    confidence: f32,
    #[serde(default)]
    sources: Vec<WriteSource>,
    scope: String, // local | delta
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum WriteSource {
    String(String),
    ChunkId { chunk_id: u32 },
}

#[derive(Debug, Deserialize)]
struct ProposeParams {
    context_id: u32,
    target: String, // user
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    why: Option<String>,
    #[serde(default)]
    what: Option<String>,
    #[serde(default, rename = "where")]
    where_: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ToolCallParams {
    name: String,
    #[serde(default)]
    arguments: Value,
}

pub fn serve_stdio(config: ServerConfig) -> anyhow::Result<()> {
    let cwd = std::env::current_dir().context("get current working directory")?;
    let config = normalize_config_with_cwd(config, &cwd).context("normalize layer paths")?;

    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let req: Result<Request, _> = serde_json::from_str(&line);
        let (req, parse_error) = match req {
            Ok(req) => (Some(req), None),
            Err(e) => (
                None,
                Some(RpcError::parse_error(format!("parse error: {e}"))),
            ),
        };

        // JSON-RPC notifications have no id; do not respond.
        let id = req.as_ref().and_then(|r| r.id.as_ref());
        if id.is_none() && parse_error.is_none() {
            continue;
        }

        let out = if let Some(parse_error) = parse_error {
            Response {
                jsonrpc: "2.0",
                id: None,
                result: None,
                error: Some(ErrorObj {
                    code: parse_error.code,
                    message: parse_error.message,
                }),
            }
        } else {
            let req = req.expect("req must exist when no parse_error");
            let res = handle_request(&config, &req);
            match res {
                Ok(result) => Response {
                    jsonrpc: "2.0",
                    id: req.id.clone(),
                    result: Some(result),
                    error: None,
                },
                Err(e) => Response {
                    jsonrpc: "2.0",
                    id: req.id.clone(),
                    result: None,
                    error: Some(ErrorObj {
                        code: e.code,
                        message: e.message,
                    }),
                },
            }
        };

        writeln!(stdout, "{}", serde_json::to_string(&out)?)?;
        stdout.flush()?;
    }

    Ok(())
}

fn handle_request(config: &ServerConfig, req: &Request) -> Result<Value, RpcError> {
    match req.method.as_str() {
        // MCP/JSON-RPC handshake
        "initialize" => Ok(handle_initialize(req.params.clone())),
        "tools/list" => Ok(handle_tools_list()),
        "tools/call" => {
            let params: ToolCallParams = serde_json::from_value(req.params.clone())
                .map_err(|e| RpcError::invalid_params(format!("parse params: {e}")))?;
            handle_tools_call(config, params)
        }
        "resources/list" => Ok(serde_json::json!({ "resources": [] })),
        "prompts/list" => Ok(serde_json::json!({ "prompts": [] })),
        "ping" => Ok(serde_json::json!({})),
        "shutdown" => Ok(Value::Null),

        // Allow calling these as raw methods, in addition to `tools/call`.
        TOOL_AGENTS_SEARCH | TOOL_AGENTS_SEARCH_LEGACY => {
            let params: SearchParams = serde_json::from_value(req.params.clone())
                .map_err(|e| RpcError::invalid_params(format!("parse params: {e}")))?;
            handle_search(config, params).map_err(|e| RpcError::internal_error(format!("{e:#}")))
        }
        TOOL_AGENTS_CONTEXT_WRITE | TOOL_AGENTS_CONTEXT_WRITE_LEGACY => {
            let params: WriteParams = serde_json::from_value(req.params.clone())
                .map_err(|e| RpcError::invalid_params(format!("parse params: {e}")))?;
            handle_write(config, params).map_err(|e| RpcError::internal_error(format!("{e:#}")))
        }
        TOOL_AGENTS_CONTEXT_PROPOSE | TOOL_AGENTS_CONTEXT_PROPOSE_LEGACY => {
            let params: ProposeParams = serde_json::from_value(req.params.clone())
                .map_err(|e| RpcError::invalid_params(format!("parse params: {e}")))?;
            handle_propose(config, params).map_err(|e| RpcError::internal_error(format!("{e:#}")))
        }
        other => Err(RpcError::method_not_found(format!(
            "unknown method: {other}"
        ))),
    }
}

fn handle_initialize(_params: Value) -> Value {
    // Minimal MCP initialize response; clients use this to verify transport and discover tools.
    serde_json::json!({
        "protocolVersion": "2024-11-05",
        "serverInfo": {
            "name": "agentsdb",
            "version": env!("CARGO_PKG_VERSION"),
        },
        "capabilities": {
            "tools": {},
            "resources": {},
            "prompts": {}
        }
    })
}

fn handle_tools_list() -> Value {
    // Tool schemas are intentionally minimal; the server validates params at runtime.
    serde_json::json!({
        "tools": [
            {
                "name": TOOL_AGENTS_SEARCH,
                "description": "Search across knowledge base layers for context on the current project or task.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string" },
                        "query_vec": { "type": "array", "items": { "type": "number" } },
                        "k": { "type": "integer", "minimum": 1 },
                        "filters": {
                            "type": "object",
                            "properties": { "kind": { "type": "array", "items": { "type": "string" } } }
                        },
                        "layers": { "type": "array", "items": { "type": "string" } }
                    },
                    "required": ["query"]
                }
            },
            {
                "name": TOOL_AGENTS_CONTEXT_WRITE,
                "description": "Append a new chunk to the local or delta knowledge base layer.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "content": { "type": "string" },
                        "kind": { "type": "string" },
                        "confidence": { "type": "number" },
                        "sources": {
                            "type": "array",
                            "items": {
                                "oneOf": [
                                    { "type": "string" },
                                    { "type": "object", "properties": { "chunk_id": { "type": "integer" } }, "required": ["chunk_id"] }
                                ]
                            }
                        },
                        "scope": { "type": "string", "enum": ["local", "delta"] }
                    },
                    "required": ["content", "kind", "confidence", "scope"]
                }
            },
            {
                "name": TOOL_AGENTS_CONTEXT_PROPOSE,
                "description": "Propose promotion of a delta chunk to the user layer.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "context_id": { "type": "integer" },
                        "target": { "type": "string", "enum": ["user"] },
                        "title": { "type": "string" },
                        "why": { "type": "string" },
                        "what": { "type": "string" },
                        "where": { "type": "string" }
                    },
                    "required": ["context_id", "target"]
                }
            }
        ]
    })
}

fn handle_tools_call(config: &ServerConfig, params: ToolCallParams) -> Result<Value, RpcError> {
    let result = match params.name.as_str() {
        TOOL_AGENTS_SEARCH | TOOL_AGENTS_SEARCH_LEGACY => {
            let args: SearchParams = serde_json::from_value(params.arguments)
                .map_err(|e| RpcError::invalid_params(format!("parse arguments: {e}")))?;
            handle_search(config, args).map_err(|e| RpcError::internal_error(format!("{e:#}")))?
        }
        TOOL_AGENTS_CONTEXT_WRITE | TOOL_AGENTS_CONTEXT_WRITE_LEGACY => {
            let args: WriteParams = serde_json::from_value(params.arguments)
                .map_err(|e| RpcError::invalid_params(format!("parse arguments: {e}")))?;
            handle_write(config, args).map_err(|e| RpcError::internal_error(format!("{e:#}")))?
        }
        TOOL_AGENTS_CONTEXT_PROPOSE | TOOL_AGENTS_CONTEXT_PROPOSE_LEGACY => {
            let args: ProposeParams = serde_json::from_value(params.arguments)
                .map_err(|e| RpcError::invalid_params(format!("parse arguments: {e}")))?;
            handle_propose(config, args).map_err(|e| RpcError::internal_error(format!("{e:#}")))?
        }
        other => return Err(RpcError::method_not_found(format!("unknown tool: {other}"))),
    };

    Ok(serde_json::json!({
        "content": [
            {
                "type": "text",
                "text": serde_json::to_string(&result).unwrap_or_else(|_| "{\"error\":\"failed to serialize\"}".to_string())
            }
        ]
    }))
}

fn handle_search(config: &ServerConfig, params: SearchParams) -> anyhow::Result<Value> {
    if params.query.trim().is_empty() {
        anyhow::bail!("query must be non-empty");
    }

    let filters = SearchFilters {
        kinds: params.filters.map(|f| f.kind).unwrap_or_default(),
    };
    let k = params.k.unwrap_or(10);

    // Select configured layer paths; `params.layers` filters by layer id.
    let mut layers = LayerSet {
        base: config.base.clone(),
        user: config.user.clone(),
        delta: config.delta.clone(),
        local: config.local.clone(),
    };
    if let Some(selected) = params.layers {
        let keep = |name: &str| selected.iter().any(|v| v == name);
        if !keep("base") {
            layers.base = None;
        }
        if !keep("user") {
            layers.user = None;
        }
        if !keep("delta") {
            layers.delta = None;
        }
        if !keep("local") {
            layers.local = None;
        }
    }

    // Treat missing optional layers as absent. Base is expected to exist if configured.
    if let Some(base) = layers.base.as_deref() {
        if !Path::new(base).exists() {
            anyhow::bail!(
                "base layer not found at {base:?} (configure an absolute path, or run the server with CWD set to your project root)"
            );
        }
    }
    if let Some(user) = layers.user.as_deref() {
        if !Path::new(user).exists() {
            layers.user = None;
        }
    }
    if let Some(delta) = layers.delta.as_deref() {
        if !Path::new(delta).exists() {
            layers.delta = None;
        }
    }
    if let Some(local) = layers.local.as_deref() {
        if !Path::new(local).exists() {
            layers.local = None;
        }
    }

    let opened = layers.open().context("open layers")?;
    if opened.is_empty() {
        anyhow::bail!("no layers configured");
    }
    let dim = opened[0].1.embedding_dim();
    let mut local = None;
    let mut user = None;
    let mut delta = None;
    let mut base = None;
    for (layer_id, file) in &opened {
        match layer_id {
            LayerId::Local => local = Some(file),
            LayerId::User => user = Some(file),
            LayerId::Delta => delta = Some(file),
            LayerId::Base => base = Some(file),
        }
    }
    let options =
        roll_up_embedding_options(&[local, user, delta, base]).context("roll up options")?;
    if let Some(cfg_dim) = options.dim {
        if cfg_dim != dim {
            anyhow::bail!(
                "embedding dim mismatch (layers are dim={dim}, options specify dim={cfg_dim})"
            );
        }
    }
    let embedder = options
        .into_embedder(dim)
        .context("resolve embedder from options")?;
    let embedding = match params.query_vec {
        Some(v) => {
            if v.len() != dim {
                anyhow::bail!(
                    "query_vec dimension mismatch (expected {dim}, got {})",
                    v.len()
                );
            }
            v
        }
        None => embedder
            .embed({
                for (_, file) in &opened {
                    ensure_layer_metadata_compatible_with_embedder(file, embedder.as_ref())
                        .context("validate layer metadata vs embedder")?;
                }
                std::slice::from_ref(&params.query)
            })?
            .into_iter()
            .next()
            .unwrap_or_else(|| vec![0.0; dim]),
    };
    let query = SearchQuery {
        embedding,
        k,
        filters,
    };
    let results = agentsdb_query::search_layers_with_options(
        &opened,
        &query,
        agentsdb_query::SearchOptions { use_index: true },
    )
    .context("search")?;
    Ok(serde_json::to_value(results)?)
}

fn handle_write(config: &ServerConfig, params: WriteParams) -> anyhow::Result<Value> {
    if params.scope != "local" && params.scope != "delta" {
        anyhow::bail!("scope must be 'local' or 'delta'");
    }
    let path = match params.scope.as_str() {
        "local" => config
            .local
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("local layer path not configured"))?,
        "delta" => config
            .delta
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("delta layer path not configured"))?,
        _ => unreachable!(),
    };

    agentsdb_format::ensure_writable_layer_path(path)?;

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let sources = params
        .sources
        .into_iter()
        .map(|s| match s {
            WriteSource::String(v) => Ok(agentsdb_format::ChunkSource::SourceString(v)),
            WriteSource::ChunkId { chunk_id } => {
                if chunk_id == 0 {
                    anyhow::bail!("source chunk_id must be non-zero");
                }
                Ok(agentsdb_format::ChunkSource::ChunkId(chunk_id))
            }
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    let mut chunk = agentsdb_format::ChunkInput {
        id: 0,
        kind: params.kind,
        content: params.content,
        author: "mcp".to_string(),
        confidence: params.confidence,
        created_at_unix_ms: now_ms,
        embedding: Vec::new(),
        sources,
    };

    if !(0.0..=1.0).contains(&chunk.confidence) || !chunk.confidence.is_finite() {
        anyhow::bail!("confidence must be finite and in range 0.0..=1.0");
    }

    let assigned = if std::path::Path::new(path).exists() {
        let file = agentsdb_format::LayerFile::open(path).context("open layer")?;
        let dim = file.embedding_dim();
        let options = roll_up_embedding_options_from_paths(
            config.local.as_deref().map(std::path::Path::new),
            config.user.as_deref().map(std::path::Path::new),
            config.delta.as_deref().map(std::path::Path::new),
            config.base.as_deref().map(std::path::Path::new),
        )
        .context("roll up options")?;
        if let Some(cfg_dim) = options.dim {
            if cfg_dim != dim {
                anyhow::bail!(
                    "embedding dim mismatch (layer is dim={dim}, options specify dim={cfg_dim})"
                );
            }
        }
        let embedder = options
            .into_embedder(dim)
            .context("resolve embedder from options")?;
        chunk.embedding = embedder
            .embed(&[chunk.content.clone()])?
            .into_iter()
            .next()
            .unwrap_or_else(|| vec![0.0; dim]);
        let layer_metadata = LayerMetadataV1::new(embedder.profile().clone())
            .with_embedder_metadata(embedder.metadata())
            .with_tool("agentsdb-mcp", env!("CARGO_PKG_VERSION"));
        let layer_metadata_json = layer_metadata
            .to_json_bytes()
            .context("serialize layer metadata")?;
        let mut chunks = vec![chunk];
        if let Some(existing) = file.layer_metadata_bytes() {
            let existing = LayerMetadataV1::from_json_bytes(existing)
                .context("parse existing layer metadata")?;
            if existing.embedding_profile != *embedder.profile() {
                anyhow::bail!(
                    "embedder profile mismatch vs existing layer metadata (existing={:?}, current={:?})",
                    existing.embedding_profile,
                    embedder.profile()
                );
            }
            let ids =
                agentsdb_format::append_layer_atomic(path, &mut chunks, None).context("append")?;
            ids[0]
        } else {
            let ids =
                agentsdb_format::append_layer_atomic(path, &mut chunks, Some(&layer_metadata_json))
                    .context("append")?;
            ids[0]
        }
    } else {
        chunk.id = 1;
        let schema = infer_schema_from_config(config).context("infer schema")?;
        let dim = schema.dim as usize;
        let options = roll_up_embedding_options_from_paths(
            config.local.as_deref().map(std::path::Path::new),
            config.user.as_deref().map(std::path::Path::new),
            config.delta.as_deref().map(std::path::Path::new),
            config.base.as_deref().map(std::path::Path::new),
        )
        .context("roll up options")?;
        if let Some(cfg_dim) = options.dim {
            if cfg_dim != dim {
                anyhow::bail!(
                    "embedding dim mismatch (schema is dim={dim}, options specify dim={cfg_dim})"
                );
            }
        }
        let embedder = options
            .into_embedder(dim)
            .context("resolve embedder from options")?;
        chunk.embedding = embedder
            .embed(&[chunk.content.clone()])?
            .into_iter()
            .next()
            .unwrap_or_else(|| vec![0.0; dim]);
        let layer_metadata = LayerMetadataV1::new(embedder.profile().clone())
            .with_embedder_metadata(embedder.metadata())
            .with_tool("agentsdb-mcp", env!("CARGO_PKG_VERSION"));
        let layer_metadata_json = layer_metadata
            .to_json_bytes()
            .context("serialize layer metadata")?;
        agentsdb_format::write_layer_atomic(path, &schema, &[chunk], Some(&layer_metadata_json))
            .context("create layer")?;
        1
    };

    Ok(serde_json::json!({ "context_id": assigned }))
}

fn infer_schema_from_config(config: &ServerConfig) -> anyhow::Result<agentsdb_format::LayerSchema> {
    let candidates = [
        config.local.as_deref(),
        config.delta.as_deref(),
        config.user.as_deref(),
        config.base.as_deref(),
    ];
    for p in candidates.into_iter().flatten() {
        let path = std::path::Path::new(p);
        if path.exists() {
            let file =
                agentsdb_format::LayerFile::open(path).with_context(|| format!("open {p}"))?;
            let s = agentsdb_format::schema_of(&file);
            return Ok(agentsdb_format::LayerSchema {
                dim: s.dim,
                element_type: s.element_type,
                quant_scale: s.quant_scale,
            });
        }
    }
    Ok(agentsdb_format::LayerSchema {
        dim: 128,
        element_type: agentsdb_format::EmbeddingElementType::F32,
        quant_scale: 1.0,
    })
}

fn handle_propose(config: &ServerConfig, params: ProposeParams) -> anyhow::Result<Value> {
    if params.target != "user" {
        anyhow::bail!("target must be 'user'");
    }
    const PROPOSAL_EVENT_KIND: &str = "meta.proposal_event";

    let Some(delta_path) = &config.delta else {
        anyhow::bail!("delta layer path not configured");
    };

    let delta_p = std::path::Path::new(delta_path);
    if !delta_p.exists() {
        anyhow::bail!("delta layer file not found at {delta_path}");
    }
    agentsdb_format::ensure_writable_layer_path(delta_p).context("permission check")?;

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let delta_file = agentsdb_format::LayerFile::open(delta_p).context("open delta layer")?;
    let delta_chunks =
        agentsdb_format::read_all_chunks(&delta_file).context("read delta chunks")?;
    let Some(src) = delta_chunks.into_iter().find(|c| c.id == params.context_id) else {
        anyhow::bail!(
            "context_id {} not found in delta layer {}",
            params.context_id,
            delta_path
        );
    };

    let from_label = delta_p
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(delta_path)
        .to_string();
    let to_label = config
        .user
        .as_deref()
        .and_then(|p| std::path::Path::new(p).file_name().and_then(|s| s.to_str()))
        .unwrap_or("AGENTS.user.db")
        .to_string();

    let record = serde_json::json!({
        "action": "propose",
        "context_id": params.context_id,
        "from_path": from_label,
        "to_path": to_label,
        "created_at_unix_ms": now_ms,
        "actor": "mcp",
        "title": params.title,
        "why": params.why,
        "what": params.what,
        "where": params.where_
    });

    let mut event_chunk = agentsdb_format::ChunkInput {
        id: 0,
        kind: PROPOSAL_EVENT_KIND.to_string(),
        content: serde_json::to_string(&record).context("serialize proposal record")?,
        author: "mcp".to_string(),
        confidence: 1.0,
        created_at_unix_ms: now_ms,
        embedding: src.embedding.clone(),
        sources: vec![agentsdb_format::ChunkSource::ChunkId(params.context_id)],
    };
    agentsdb_format::append_layer_atomic(delta_p, std::slice::from_mut(&mut event_chunk), None)
        .context("append proposal event")?;

    Ok(serde_json::json!({ "ok": true }))
}

#[cfg(test)]
fn is_openai_tool_name_compatible(name: &str) -> bool {
    // Matches OpenAI tool name constraints: `^[a-zA-Z0-9_-]+$`.
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_temp_dir(name: &str) -> std::path::PathBuf {
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "agentsdb-mcp-{}-{}-{}",
            name,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn tool_names_are_openai_compatible() {
        let list = handle_tools_list();
        let tools = list
            .get("tools")
            .and_then(|v| v.as_array())
            .expect("tools must be an array");

        for tool in tools {
            let name = tool
                .get("name")
                .and_then(|v| v.as_str())
                .expect("tool name must be a string");
            assert!(
                is_openai_tool_name_compatible(name),
                "tool name not OpenAI-compatible: {name}"
            );
        }
    }

    #[test]
    fn normalize_resolves_base_in_ancestor_and_anchors_rel_layers() {
        let root = make_temp_dir("normalize");
        let nested = root.join("a/b/c");
        std::fs::create_dir_all(&nested).expect("create nested cwd");

        let base = root.join("AGENTS.db");
        std::fs::write(&base, b"").expect("write base placeholder");

        let cfg = ServerConfig {
            base: Some("AGENTS.db".to_string()),
            user: None,
            delta: None,
            local: Some("AGENTS.local.db".to_string()),
        };
        let normalized = normalize_config_with_cwd(cfg, &nested).expect("normalize config");

        assert_eq!(
            normalized.base.as_deref(),
            Some(base.to_string_lossy().as_ref())
        );
        assert_eq!(
            normalized.local.as_deref(),
            Some(root.join("AGENTS.local.db").to_string_lossy().as_ref())
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn normalize_expands_pwd() {
        let root = make_temp_dir("pwd");
        let base = root.join("AGENTS.db");
        std::fs::write(&base, b"").expect("write base placeholder");

        let cfg = ServerConfig {
            base: Some("$PWD/AGENTS.db".to_string()),
            user: None,
            delta: None,
            local: None,
        };
        let normalized = normalize_config_with_cwd(cfg, &root).expect("normalize config");
        assert_eq!(
            normalized.base.as_deref(),
            Some(base.to_string_lossy().as_ref())
        );

        let _ = std::fs::remove_dir_all(&root);
    }
}
