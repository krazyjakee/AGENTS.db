use agentsdb_core::embed::hash_embed;
use agentsdb_core::types::SearchFilters;
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

#[derive(Debug, Clone, Default)]
pub struct ServerConfig {
    pub base: Option<String>,
    pub user: Option<String>,
    pub delta: Option<String>,
    pub local: Option<String>,
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
}

#[derive(Debug, Deserialize)]
struct ToolCallParams {
    name: String,
    #[serde(default)]
    arguments: Value,
}

pub fn serve_stdio(config: ServerConfig) -> anyhow::Result<()> {
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
            handle_search(config, params).map_err(|e| RpcError::internal_error(e.to_string()))
        }
        TOOL_AGENTS_CONTEXT_WRITE | TOOL_AGENTS_CONTEXT_WRITE_LEGACY => {
            let params: WriteParams = serde_json::from_value(req.params.clone())
                .map_err(|e| RpcError::invalid_params(format!("parse params: {e}")))?;
            handle_write(config, params).map_err(|e| RpcError::internal_error(e.to_string()))
        }
        TOOL_AGENTS_CONTEXT_PROPOSE | TOOL_AGENTS_CONTEXT_PROPOSE_LEGACY => {
            let params: ProposeParams = serde_json::from_value(req.params.clone())
                .map_err(|e| RpcError::invalid_params(format!("parse params: {e}")))?;
            handle_propose(config, params).map_err(|e| RpcError::internal_error(e.to_string()))
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
                "description": "Search across one or more AGENTS.db layers.",
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
                "description": "Append a new chunk to the local or delta layer.",
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
                "description": "Propose promotion of a delta chunk to user.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "context_id": { "type": "integer" },
                        "target": { "type": "string", "enum": ["user"] }
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
            handle_search(config, args).map_err(|e| RpcError::internal_error(e.to_string()))?
        }
        TOOL_AGENTS_CONTEXT_WRITE | TOOL_AGENTS_CONTEXT_WRITE_LEGACY => {
            let args: WriteParams = serde_json::from_value(params.arguments)
                .map_err(|e| RpcError::invalid_params(format!("parse arguments: {e}")))?;
            handle_write(config, args).map_err(|e| RpcError::internal_error(e.to_string()))?
        }
        TOOL_AGENTS_CONTEXT_PROPOSE | TOOL_AGENTS_CONTEXT_PROPOSE_LEGACY => {
            let args: ProposeParams = serde_json::from_value(params.arguments)
                .map_err(|e| RpcError::invalid_params(format!("parse arguments: {e}")))?;
            handle_propose(config, args).map_err(|e| RpcError::internal_error(e.to_string()))?
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

    let opened = layers.open().context("open layers")?;
    if opened.is_empty() {
        anyhow::bail!("no layers configured");
    }
    let dim = opened[0].1.embedding_dim();
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
        None => hash_embed(&params.query, dim),
    };
    let query = SearchQuery {
        embedding: embedding.clone(),
        k,
        filters,
    };
    let results = agentsdb_query::search_layers(&opened, &query).context("search")?;
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
            WriteSource::String(v) => agentsdb_format::ChunkSource::SourceString(v),
            WriteSource::ChunkId { chunk_id } => agentsdb_format::ChunkSource::ChunkId(chunk_id),
        })
        .collect();

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
        chunk.embedding = hash_embed(&chunk.content, file.embedding_dim());
        let mut chunks = vec![chunk];
        let ids = agentsdb_format::append_layer_atomic(path, &mut chunks).context("append")?;
        ids[0]
    } else {
        chunk.id = 1;
        let schema = infer_schema_from_config(config).context("infer schema")?;
        chunk.embedding = hash_embed(&chunk.content, schema.dim as usize);
        agentsdb_format::write_layer_atomic(path, &schema, &[chunk]).context("create layer")?;
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
    let Some(delta_path) = &config.delta else {
        anyhow::bail!("delta layer path not configured");
    };
    let dir = std::path::Path::new(delta_path)
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let proposals = dir.join("AGENTS.proposals.jsonl");
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&proposals)?;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let record = serde_json::json!({
        "context_id": params.context_id,
        "target": "user",
        "created_at_unix_ms": now_ms
    });
    writeln!(f, "{}", serde_json::to_string(&record)?)?;
    f.sync_all()?;
    Ok(serde_json::json!({ "ok": true }))
}
