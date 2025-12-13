use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use agentsdb_core::embed::hash_embed;
use agentsdb_format::{ChunkInput, ChunkSource, LayerFile, SourceRef};

const TOMBSTONE_KIND: &str = "tombstone";
const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;

const LOGO_PNG: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/logo.png"));
const INDEX_HTML: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/index.html"));
const APP_CSS: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/app.css"));
const APP_JS: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/app.js"));

pub fn serve(root: &str, bind: &str) -> anyhow::Result<()> {
    let root = std::fs::canonicalize(root).with_context(|| format!("canonicalize root {root}"))?;
    let listener = TcpListener::bind(bind).with_context(|| format!("bind {bind}"))?;
    println!("Web: http://{bind}/ (root: {})", root.display());

    let state = Arc::new(Mutex::new(ServerState::new(root)));

    for stream in listener.incoming() {
        let state = Arc::clone(&state);
        let mut stream = match stream {
            Ok(s) => s,
            Err(err) => {
                eprintln!("accept failed: {err}");
                continue;
            }
        };
        std::thread::spawn(move || {
            let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
            let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));
            if let Err(err) = handle_conn(&mut stream, &state) {
                let _ = write_response(
                    &mut stream,
                    500,
                    "text/plain; charset=utf-8",
                    format!("internal error: {err}\n").as_bytes(),
                );
            }
        });
    }

    Ok(())
}

struct ServerState {
    root: PathBuf,
    cache: HashMap<String, LayerCache>,
}

impl ServerState {
    fn new(root: PathBuf) -> Self {
        Self {
            root,
            cache: HashMap::new(),
        }
    }
}

#[derive(Clone)]
struct LayerCache {
    abs_path: PathBuf,
    file_length_bytes: u64,
    modified_unix_ms: u64,
    meta: LayerMeta,
    summaries: Vec<ChunkSummary>,
    removed_ids: HashSet<u32>,
}

#[derive(Debug, Clone, Serialize)]
struct LayerMeta {
    path: String,
    chunk_count: u64,
    file_length_bytes: u64,
    embedding_dim: usize,
    embedding_element_type: String,
    relationship_count: Option<u64>,
    kinds: BTreeMap<String, u64>,
    removed_count: u64,
    confidence_min: f32,
    confidence_max: f32,
    confidence_avg: f32,
}

#[derive(Debug, Clone, Serialize)]
struct ChunkSummary {
    id: u32,
    kind: String,
    author: String,
    confidence: f32,
    created_at_unix_ms: u64,
    source_count: usize,
    removed: bool,
    content_preview: String,
}

#[derive(Debug, Clone, Serialize)]
struct ChunkFull {
    id: u32,
    kind: String,
    author: String,
    confidence: f32,
    created_at_unix_ms: u64,
    sources: Vec<String>,
    content: String,
    removed: bool,
}

fn handle_conn(stream: &mut TcpStream, state: &Arc<Mutex<ServerState>>) -> anyhow::Result<()> {
    let req = read_request(stream).context("read request")?;

    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/") => write_response(
            stream,
            200,
            "text/html; charset=utf-8",
            INDEX_HTML.as_bytes(),
        )
        .context("write index"),
        ("GET", "/assets/app.css") => {
            write_response(stream, 200, "text/css; charset=utf-8", APP_CSS.as_bytes())
                .context("write app.css")
        }
        ("GET", "/assets/app.js") => write_response(
            stream,
            200,
            "text/javascript; charset=utf-8",
            APP_JS.as_bytes(),
        )
        .context("write app.js"),
        ("GET", "/logo.png") => {
            write_response(stream, 200, "image/png", LOGO_PNG).context("write /logo.png")
        }
        ("GET", "/favicon.ico") => {
            write_response(stream, 200, "image/png", LOGO_PNG).context("write /favicon.ico")
        }
        ("GET", "/api/layers") => {
            let layers = {
                let st = state.lock().expect("poisoned mutex");
                list_layers(&st.root)?
            };
            let body = serde_json::to_vec_pretty(&layers)?;
            write_response(stream, 200, "application/json", &body).context("write /api/layers")
        }
        ("GET", "/api/layer/meta") => {
            let layer = req
                .query
                .get("path")
                .context("missing query param: path")?
                .to_string();
            let meta = {
                let mut st = state.lock().expect("poisoned mutex");
                get_or_build_cache(&mut st, &layer)?.meta
            };
            let body = serde_json::to_vec_pretty(&meta)?;
            write_response(stream, 200, "application/json", &body).context("write /api/layer/meta")
        }
        ("GET", "/api/layer/chunks") => {
            let layer = req
                .query
                .get("path")
                .context("missing query param: path")?
                .to_string();
            let offset: usize = req
                .query
                .get("offset")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            let limit: usize = req
                .query
                .get("limit")
                .and_then(|v| v.parse().ok())
                .unwrap_or(100);
            let include_removed = req
                .query
                .get("include_removed")
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false);
            let kind_filter = req.query.get("kind").map(|s| s.as_str()).unwrap_or("");

            let (items, total) = {
                let mut st = state.lock().expect("poisoned mutex");
                let cache = get_or_build_cache(&mut st, &layer)?;
                let filtered: Vec<ChunkSummary> = cache
                    .summaries
                    .iter()
                    .filter(|c| include_removed || !c.removed)
                    .filter(|c| kind_filter.is_empty() || c.kind == kind_filter)
                    .cloned()
                    .collect();
                let total = filtered.len();
                let end = offset.saturating_add(limit).min(total);
                let page = if offset >= total {
                    Vec::new()
                } else {
                    filtered[offset..end].to_vec()
                };
                (page, total)
            };

            #[derive(Serialize)]
            struct Out {
                total: usize,
                offset: usize,
                limit: usize,
                items: Vec<ChunkSummary>,
            }
            let out = Out {
                total,
                offset,
                limit,
                items,
            };
            let body = serde_json::to_vec_pretty(&out)?;
            write_response(stream, 200, "application/json", &body)
                .context("write /api/layer/chunks")
        }
        ("GET", "/api/layer/chunk") => {
            let layer = req
                .query
                .get("path")
                .context("missing query param: path")?
                .to_string();
            let id: u32 = req
                .query
                .get("id")
                .context("missing query param: id")?
                .parse()
                .context("invalid id")?;

            let chunk = {
                let mut st = state.lock().expect("poisoned mutex");
                let cache = get_or_build_cache(&mut st, &layer)?;
                read_chunk_full(&cache.abs_path, &cache.removed_ids, id)?
            };

            let body = serde_json::to_vec_pretty(&chunk)?;
            write_response(stream, 200, "application/json", &body).context("write /api/layer/chunk")
        }
        ("POST", "/api/layer/add") => {
            let input: AddInput =
                serde_json::from_slice(&req.body).context("parse JSON body for add")?;
            let (assigned, path) = {
                let mut st = state.lock().expect("poisoned mutex");
                let abs_path = resolve_layer_path(&st.root, &input.path)?;
                let assigned = append_chunk(
                    &abs_path,
                    &input.scope,
                    input.id,
                    &input.kind,
                    &input.content,
                    input.confidence,
                    input.dim,
                    &input.sources,
                    &input.source_chunks,
                )?;
                st.cache.remove(&input.path);
                (assigned, input.path)
            };

            #[derive(Serialize)]
            struct Out {
                ok: bool,
                path: String,
                id: u32,
            }
            let out = Out {
                ok: true,
                path,
                id: assigned,
            };
            let body = serde_json::to_vec_pretty(&out)?;
            write_response(stream, 200, "application/json", &body).context("write add response")
        }
        ("POST", "/api/layer/remove") => {
            let input: RemoveInput =
                serde_json::from_slice(&req.body).context("parse JSON body for remove")?;
            let path = input.path.clone();
            {
                let mut st = state.lock().expect("poisoned mutex");
                let abs_path = resolve_layer_path(&st.root, &input.path)?;
                let _ = append_chunk(
                    &abs_path,
                    &input.scope,
                    None,
                    TOMBSTONE_KIND,
                    &format!("retract chunk id {}", input.id),
                    1.0,
                    None,
                    &Vec::new(),
                    &vec![input.id],
                )?;
                st.cache.remove(&input.path);
            }

            #[derive(Serialize)]
            struct Out {
                ok: bool,
                path: String,
                id: u32,
            }
            let out = Out {
                ok: true,
                path,
                id: input.id,
            };
            let body = serde_json::to_vec_pretty(&out)?;
            write_response(stream, 200, "application/json", &body).context("write remove response")
        }
        _ => write_response(stream, 404, "text/plain; charset=utf-8", b"not found\n")
            .context("write 404"),
    }
}

#[derive(Debug)]
struct Request {
    method: String,
    path: String,
    query: HashMap<String, String>,
    body: Vec<u8>,
}

fn read_request(stream: &mut TcpStream) -> anyhow::Result<Request> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    let header_end;
    loop {
        let n = stream.read(&mut tmp).context("read socket")?;
        if n == 0 {
            anyhow::bail!("unexpected EOF");
        }
        buf.extend_from_slice(&tmp[..n]);
        if buf.len() > MAX_BODY_BYTES + 64 * 1024 {
            anyhow::bail!("request too large");
        }
        if let Some(pos) = find_header_end(&buf) {
            header_end = pos;
            break;
        }
    }

    let header_bytes = &buf[..header_end];
    let header_str = std::str::from_utf8(header_bytes).context("headers must be utf-8")?;
    let mut lines = header_str.split("\r\n");
    let request_line = lines.next().context("missing request line")?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().context("missing method")?.to_string();
    let raw_path = parts.next().context("missing path")?.to_string();
    let (path, query) = split_path_query(&raw_path);

    let mut content_length: usize = 0;
    for line in lines {
        if line.is_empty() {
            break;
        }
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        if k.trim().eq_ignore_ascii_case("content-length") {
            content_length = v.trim().parse().context("invalid content-length int")?;
        }
    }
    if content_length > MAX_BODY_BYTES {
        anyhow::bail!("body too large");
    }

    let mut body = Vec::new();
    body.extend_from_slice(&buf[header_end..]);
    while body.len() < content_length {
        let n = stream.read(&mut tmp).context("read body")?;
        if n == 0 {
            anyhow::bail!("unexpected EOF reading body");
        }
        body.extend_from_slice(&tmp[..n]);
        if body.len() > MAX_BODY_BYTES {
            anyhow::bail!("body too large");
        }
    }
    body.truncate(content_length);

    Ok(Request {
        method,
        path,
        query,
        body,
    })
}

fn write_response(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &[u8],
) -> anyhow::Result<()> {
    let status_line = match status {
        200 => "HTTP/1.1 200 OK",
        400 => "HTTP/1.1 400 Bad Request",
        404 => "HTTP/1.1 404 Not Found",
        500 => "HTTP/1.1 500 Internal Server Error",
        _ => "HTTP/1.1 200 OK",
    };
    write!(
        stream,
        "{status_line}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)?;
    Ok(())
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4)
}

fn split_path_query(raw: &str) -> (String, HashMap<String, String>) {
    let mut parts = raw.splitn(2, '?');
    let path = parts.next().unwrap_or("/").to_string();
    let query_str = parts.next().unwrap_or("");
    let mut query = HashMap::new();
    for pair in query_str.split('&').filter(|s| !s.is_empty()) {
        let mut kv = pair.splitn(2, '=');
        let k = kv.next().unwrap_or("");
        let v = kv.next().unwrap_or("");
        if let (Some(k), Some(v)) = (pct_decode(k), pct_decode(v)) {
            query.insert(k, v);
        }
    }
    (path, query)
}

fn pct_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                if i + 2 >= bytes.len() {
                    return None;
                }
                let hi = from_hex(bytes[i + 1])?;
                let lo = from_hex(bytes[i + 2])?;
                out.push((hi << 4) | lo);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

fn from_hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[derive(Debug, Deserialize)]
struct AddInput {
    path: String,
    scope: String, // local|delta
    #[serde(default)]
    id: Option<u32>,
    kind: String,
    content: String,
    confidence: f32,
    #[serde(default)]
    dim: Option<u32>,
    #[serde(default)]
    sources: Vec<String>,
    #[serde(default)]
    source_chunks: Vec<u32>,
}

#[derive(Debug, Deserialize)]
struct RemoveInput {
    path: String,
    scope: String, // local|delta
    id: u32,
}

#[derive(Debug, Serialize)]
struct ListedLayer {
    path: String,
    chunk_count: u64,
    file_length_bytes: u64,
}

fn list_layers(root: &Path) -> anyhow::Result<Vec<ListedLayer>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(root).with_context(|| format!("read dir {}", root.display()))? {
        let entry = entry?;
        let path = entry.path();
        let ty = entry
            .file_type()
            .with_context(|| format!("stat {}", path.display()))?;
        if !ty.is_file() {
            continue;
        }
        if path.extension().and_then(|s| s.to_str()) != Some("db") {
            continue;
        }
        let file_name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();

        match agentsdb_format::LayerFile::open(&path) {
            Ok(f) => out.push(ListedLayer {
                path: file_name,
                chunk_count: f.chunk_count,
                file_length_bytes: f.header.file_length_bytes,
            }),
            Err(_) => continue,
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

fn resolve_layer_path(root: &Path, file_name: &str) -> anyhow::Result<PathBuf> {
    if file_name.contains(std::path::MAIN_SEPARATOR)
        || file_name.contains('/')
        || file_name.contains('\\')
    {
        anyhow::bail!("path must be a file name under root");
    }
    if Path::new(file_name).extension().and_then(|s| s.to_str()) != Some("db") {
        anyhow::bail!("path must end with .db");
    }
    let abs = root.join(file_name);
    let abs = std::fs::canonicalize(&abs).unwrap_or(abs);
    if !abs.starts_with(root) {
        anyhow::bail!("path escapes root");
    }
    Ok(abs)
}

fn modified_unix_ms(path: &Path) -> anyhow::Result<u64> {
    let meta = std::fs::metadata(path).with_context(|| format!("stat {}", path.display()))?;
    let m = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
    let ms = m
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    Ok(ms)
}

fn get_or_build_cache(st: &mut ServerState, file_name: &str) -> anyhow::Result<LayerCache> {
    let abs = resolve_layer_path(&st.root, file_name)?;
    let meta = std::fs::metadata(&abs).with_context(|| format!("stat {}", abs.display()))?;
    let file_length_bytes = meta.len();
    let modified_unix_ms = modified_unix_ms(&abs)?;

    let needs_rebuild = match st.cache.get(file_name) {
        Some(c) => {
            c.file_length_bytes != file_length_bytes || c.modified_unix_ms != modified_unix_ms
        }
        None => true,
    };
    if needs_rebuild {
        let cache = build_cache(file_name.to_string(), abs)?;
        st.cache.insert(file_name.to_string(), cache);
    }
    Ok(st
        .cache
        .get(file_name)
        .context("cache missing after rebuild")?
        .clone())
}

fn build_cache(path_label: String, abs_path: PathBuf) -> anyhow::Result<LayerCache> {
    let file =
        LayerFile::open(&abs_path).with_context(|| format!("open {}", abs_path.display()))?;
    let modified_ms = modified_unix_ms(&abs_path)?;
    let mut kinds: BTreeMap<String, u64> = BTreeMap::new();
    let mut removed_ids: HashSet<u32> = HashSet::new();
    let mut summaries = Vec::with_capacity(file.chunk_count as usize);

    let mut conf_min = 1.0f32;
    let mut conf_max = 0.0f32;
    let mut conf_sum = 0.0f64;
    let mut conf_n = 0u64;

    for chunk in file.chunks() {
        let chunk = chunk?;
        *kinds.entry(chunk.kind.to_string()).or_insert(0) += 1;

        conf_min = conf_min.min(chunk.confidence);
        conf_max = conf_max.max(chunk.confidence);
        conf_sum += chunk.confidence as f64;
        conf_n += 1;

        let sources = file.sources_for(chunk.rel_start, chunk.rel_count)?;
        if chunk.kind == TOMBSTONE_KIND {
            for s in sources.iter() {
                if let SourceRef::ChunkId(id) = s {
                    removed_ids.insert(*id);
                }
            }
        }

        let source_count = sources.len();
        let content_preview = truncate_preview(chunk.content, 240);

        summaries.push(ChunkSummary {
            id: chunk.id,
            kind: chunk.kind.to_string(),
            author: chunk.author.to_string(),
            confidence: chunk.confidence,
            created_at_unix_ms: chunk.created_at_unix_ms,
            source_count,
            removed: false,
            content_preview,
        });
    }

    for s in summaries.iter_mut() {
        if removed_ids.contains(&s.id) {
            s.removed = true;
        }
    }

    let confidence_avg = if conf_n == 0 {
        0.0
    } else {
        (conf_sum / (conf_n as f64)) as f32
    };
    let meta = LayerMeta {
        path: path_label,
        chunk_count: file.chunk_count,
        file_length_bytes: file.header.file_length_bytes,
        embedding_dim: file.embedding_dim(),
        embedding_element_type: format!("{:?}", file.embedding_matrix.element_type).to_lowercase(),
        relationship_count: file.relationship_count,
        kinds,
        removed_count: removed_ids.len() as u64,
        confidence_min: if conf_n == 0 { 0.0 } else { conf_min },
        confidence_max: if conf_n == 0 { 0.0 } else { conf_max },
        confidence_avg,
    };

    Ok(LayerCache {
        abs_path,
        file_length_bytes: file.header.file_length_bytes,
        modified_unix_ms: modified_ms,
        meta,
        summaries,
        removed_ids,
    })
}

fn truncate_preview(s: &str, max_chars: usize) -> String {
    let mut out = String::new();
    let mut count = 0usize;
    for ch in s.chars() {
        if count >= max_chars {
            out.push('…');
            break;
        }
        out.push(ch);
        count += 1;
    }
    out
}

fn read_chunk_full(path: &Path, removed: &HashSet<u32>, id: u32) -> anyhow::Result<ChunkFull> {
    let file = LayerFile::open(path).with_context(|| format!("open {}", path.display()))?;
    for chunk in file.chunks() {
        let chunk = chunk?;
        if chunk.id != id {
            continue;
        }
        let sources = file.sources_for(chunk.rel_start, chunk.rel_count)?;
        let sources: Vec<String> = sources.iter().map(|s| format!("{s:?}")).collect();
        return Ok(ChunkFull {
            id: chunk.id,
            kind: chunk.kind.to_string(),
            author: chunk.author.to_string(),
            confidence: chunk.confidence,
            created_at_unix_ms: chunk.created_at_unix_ms,
            sources,
            content: chunk.content.to_string(),
            removed: removed.contains(&chunk.id),
        });
    }
    anyhow::bail!("chunk id {id} not found");
}

fn append_chunk(
    path: &Path,
    scope: &str,
    id: Option<u32>,
    kind: &str,
    content: &str,
    confidence: f32,
    dim: Option<u32>,
    sources: &[String],
    source_chunks: &[u32],
) -> anyhow::Result<u32> {
    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    if !matches!(file_name, "AGENTS.local.db" | "AGENTS.delta.db") {
        anyhow::bail!("writes are only allowed for AGENTS.local.db / AGENTS.delta.db");
    }
    if scope == "local" && file_name != "AGENTS.local.db" {
        anyhow::bail!("scope local only allowed for AGENTS.local.db");
    }
    if scope == "delta" && file_name != "AGENTS.delta.db" {
        anyhow::bail!("scope delta only allowed for AGENTS.delta.db");
    }

    let exists = path.exists();
    if exists {
        let file =
            LayerFile::open(path).with_context(|| format!("open for append {}", path.display()))?;
        let dim_usize = file.embedding_dim();

        let mut chunk = ChunkInput {
            id: id.unwrap_or(0), // 0 = auto-assign
            kind: kind.to_string(),
            author: "web".to_string(),
            confidence,
            created_at_unix_ms: now_unix_ms(),
            content: content.to_string(),
            embedding: Vec::new(),
            sources: Vec::new(),
        };
        chunk.embedding = hash_embed(&chunk.content, dim_usize);

        for s in sources.iter() {
            chunk.sources.push(ChunkSource::SourceString(s.to_string()));
        }
        for cid in source_chunks.iter() {
            chunk.sources.push(ChunkSource::ChunkId(*cid));
        }

        let mut new_chunks = vec![chunk];
        let assigned =
            agentsdb_format::append_layer_atomic(path, &mut new_chunks).context("append chunk")?;
        Ok(*assigned.first().unwrap_or(&0))
    } else {
        let dim = dim.context("creating a new layer requires dim")?;
        let assigned = id.unwrap_or(1);
        let mut chunk = ChunkInput {
            id: assigned,
            kind: kind.to_string(),
            author: "web".to_string(),
            confidence,
            created_at_unix_ms: now_unix_ms(),
            content: content.to_string(),
            embedding: Vec::new(),
            sources: Vec::new(),
        };
        chunk.embedding = hash_embed(&chunk.content, dim as usize);
        if chunk.id == 0 {
            chunk.id = 1;
        }
        let schema = agentsdb_format::LayerSchema {
            dim: dim as u32,
            element_type: agentsdb_format::EmbeddingElementType::F32,
            quant_scale: 1.0,
        };
        agentsdb_format::write_layer_atomic(path, &schema, &[chunk]).context("create layer")?;
        Ok(assigned)
    }
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
