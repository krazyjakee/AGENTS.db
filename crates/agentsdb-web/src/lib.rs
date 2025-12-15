use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use agentsdb_core::export::{
    ExportBundleV1, ExportChunkV1, ExportLayerSchemaV1, ExportLayerV1, ExportNdjsonRecordV1,
    ExportSourceV1, ExportToolInfo,
};
use agentsdb_embeddings::config::{
    roll_up_embedding_options_from_paths, standard_layer_paths_for_dir,
};
use agentsdb_embeddings::layer_metadata::LayerMetadataV1;
use agentsdb_format::{ChunkInput, ChunkSource, LayerFile, SourceRef};

const TOMBSTONE_KIND: &str = "tombstone";
const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;
const PROPOSAL_EVENT_KIND: &str = "meta.proposal_event";
const PROPOSAL_EVENT_LAYER: &str = "AGENTS.delta.db";

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
        ("GET", "/api/version") => {
            #[derive(Serialize)]
            struct Out {
                version: &'static str,
            }

            let out = Out {
                version: env!("CARGO_PKG_VERSION"),
            };
            let body = serde_json::to_vec_pretty(&out)?;
            write_response(stream, 200, "application/json", &body).context("write /api/version")
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
                    &[input.id],
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
        ("GET", "/api/export") => {
            let rel_path = req
                .query
                .get("path")
                .context("missing query param: path")?
                .to_string();
            let format = req
                .query
                .get("format")
                .map(String::as_str)
                .unwrap_or("json");
            let redact = req
                .query
                .get("redact")
                .map(String::as_str)
                .unwrap_or("none");
            let (content_type, body) = {
                let st = state.lock().expect("poisoned mutex");
                let abs_path = resolve_layer_path(&st.root, &rel_path)?;
                export_layer(abs_path.as_path(), &rel_path, format, redact)?
            };
            write_response(stream, 200, content_type, &body).context("write /api/export")
        }
        ("POST", "/api/import") => {
            let input: ImportInput =
                serde_json::from_slice(&req.body).context("parse JSON body for import")?;
            let path = input.path.clone();
            let (imported, skipped, dry_run) = {
                let mut st = state.lock().expect("poisoned mutex");
                let abs_path = resolve_layer_path(&st.root, &input.path)?;
                let out = import_into_layer(
                    abs_path.as_path(),
                    &input.scope,
                    input.format.as_deref().unwrap_or("json"),
                    &input.data,
                    input.dry_run.unwrap_or(false),
                    input.dedupe.unwrap_or(false),
                    input.preserve_ids.unwrap_or(false),
                    input.allow_base.unwrap_or(false),
                    input.dim,
                )?;
                if !out.2 {
                    st.cache.remove(&input.path);
                }
                out
            };
            let body = serde_json::to_vec_pretty(&serde_json::json!({
                "ok": true,
                "path": path,
                "imported": imported,
                "skipped": skipped,
                "dry_run": dry_run
            }))?;
            write_response(stream, 200, "application/json", &body).context("write /api/import")
        }
        ("GET", "/api/proposals") => {
            let include_all = req
                .query
                .get("all")
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false);
            let proposals = {
                let mut st = state.lock().expect("poisoned mutex");
                list_proposals(&mut st, include_all)?
            };
            let body = serde_json::to_vec_pretty(&proposals)?;
            write_response(stream, 200, "application/json", &body).context("write /api/proposals")
        }
        ("POST", "/api/proposals/propose") => {
            let input: ProposeInput =
                serde_json::from_slice(&req.body).context("parse JSON body for propose")?;
            let proposal_id = {
                let mut st = state.lock().expect("poisoned mutex");
                record_proposal(&mut st, input)?
            };
            let body = serde_json::to_vec_pretty(
                &serde_json::json!({ "ok": true, "proposal_id": proposal_id }),
            )?;
            write_response(stream, 200, "application/json", &body)
                .context("write /api/proposals/propose")
        }
        ("POST", "/api/proposals/reject") => {
            let input: RejectInput =
                serde_json::from_slice(&req.body).context("parse JSON body for reject")?;
            {
                let mut st = state.lock().expect("poisoned mutex");
                reject_proposals(&mut st, &input.proposal_ids, input.reason.as_deref())?;
            }
            let body = serde_json::to_vec_pretty(&serde_json::json!({ "ok": true }))?;
            write_response(stream, 200, "application/json", &body)
                .context("write /api/proposals/reject")
        }
        ("POST", "/api/proposals/accept") => {
            let input: AcceptInput =
                serde_json::from_slice(&req.body).context("parse JSON body for accept")?;
            let out = {
                let mut st = state.lock().expect("poisoned mutex");
                accept_proposals(&mut st, &input.proposal_ids, input.skip_existing)?
            };
            let body = serde_json::to_vec_pretty(&out)?;
            write_response(stream, 200, "application/json", &body)
                .context("write /api/proposals/accept")
        }
        ("POST", "/api/promote") => {
            let input: PromoteInput =
                serde_json::from_slice(&req.body).context("parse JSON body for promote")?;
            let out = {
                let mut st = state.lock().expect("poisoned mutex");
                promote_delta_to_user(&mut st, &[input.id], input.skip_existing)?
            };
            let body = serde_json::to_vec_pretty(&out)?;
            write_response(stream, 200, "application/json", &body).context("write /api/promote")
        }
        ("POST", "/api/promote/batch") => {
            let input: PromoteBatchInput =
                serde_json::from_slice(&req.body).context("parse JSON body for promote batch")?;
            let out = {
                let mut st = state.lock().expect("poisoned mutex");
                promote_layers(
                    &mut st,
                    &input.from_path,
                    &input.to_path,
                    &input.ids,
                    input.skip_existing,
                )?
            };
            let body = serde_json::to_vec_pretty(&out)?;
            write_response(stream, 200, "application/json", &body)
                .context("write /api/promote/batch")
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
    for (count, ch) in s.chars().enumerate() {
        if count >= max_chars {
            out.push('…');
            break;
        }
        out.push(ch);
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

#[allow(clippy::too_many_arguments)]
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
    #[allow(clippy::too_many_arguments)]
    fn inner(
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
        let dir = path.parent().unwrap_or_else(|| Path::new("."));
        let siblings = standard_layer_paths_for_dir(dir);

        let embedder_for_dim = |dim_usize: usize| -> anyhow::Result<
            Box<dyn agentsdb_embeddings::embedder::Embedder + Send + Sync>,
        > {
            let options = roll_up_embedding_options_from_paths(
                Some(siblings.local.as_path()),
                Some(siblings.user.as_path()),
                Some(siblings.delta.as_path()),
                Some(siblings.base.as_path()),
            )
            .context("roll up options")?;
            if let Some(cfg_dim) = options.dim {
                if cfg_dim != dim_usize {
                    anyhow::bail!(
                    "embedding dim mismatch (layer is dim={dim_usize}, options specify dim={cfg_dim})"
                );
                }
            }
            options
                .into_embedder(dim_usize)
                .context("resolve embedder from options")
        };
        if exists {
            let file = LayerFile::open(path)
                .with_context(|| format!("open for append {}", path.display()))?;
            let dim_usize = file.embedding_dim();

            let mut chunk = ChunkInput {
                id: id.unwrap_or(0), // 0 = auto-assign
                kind: kind.to_string(),
                author: "human".to_string(),
                confidence,
                created_at_unix_ms: now_unix_ms(),
                content: content.to_string(),
                embedding: Vec::new(),
                sources: Vec::new(),
            };
            let embedder = embedder_for_dim(dim_usize)?;
            chunk.embedding = embedder
                .embed(&[chunk.content.clone()])?
                .into_iter()
                .next()
                .unwrap_or_else(|| vec![0.0; dim_usize]);
            let layer_metadata = LayerMetadataV1::new(embedder.profile().clone())
                .with_embedder_metadata(embedder.metadata())
                .with_tool("agentsdb-web", env!("CARGO_PKG_VERSION"));
            let layer_metadata_json = layer_metadata
                .to_json_bytes()
                .context("serialize layer metadata")?;

            for s in sources.iter() {
                chunk.sources.push(ChunkSource::SourceString(s.to_string()));
            }
            for cid in source_chunks.iter() {
                chunk.sources.push(ChunkSource::ChunkId(*cid));
            }

            let mut new_chunks = vec![chunk];
            let assigned = if let Some(existing) = file.layer_metadata_bytes() {
                let existing = LayerMetadataV1::from_json_bytes(existing)
                    .context("parse existing layer metadata")?;
                if existing.embedding_profile != *embedder.profile() {
                    anyhow::bail!(
                        "embedder profile mismatch vs existing layer metadata (existing={:?}, current={:?})",
                        existing.embedding_profile,
                        embedder.profile()
                    );
                }
                agentsdb_format::append_layer_atomic(path, &mut new_chunks, None)
                    .context("append chunk")?
            } else {
                agentsdb_format::append_layer_atomic(
                    path,
                    &mut new_chunks,
                    Some(&layer_metadata_json),
                )
                .context("append chunk")?
            };
            Ok(*assigned.first().unwrap_or(&0))
        } else {
            let dim = dim.context("creating a new layer requires dim")?;
            let assigned = id.unwrap_or(1);
            let mut chunk = ChunkInput {
                id: assigned,
                kind: kind.to_string(),
                author: "human".to_string(),
                confidence,
                created_at_unix_ms: now_unix_ms(),
                content: content.to_string(),
                embedding: Vec::new(),
                sources: Vec::new(),
            };
            let dim_usize = dim as usize;
            let embedder = embedder_for_dim(dim_usize)?;
            chunk.embedding = embedder
                .embed(&[chunk.content.clone()])?
                .into_iter()
                .next()
                .unwrap_or_else(|| vec![0.0; dim_usize]);
            let layer_metadata = LayerMetadataV1::new(embedder.profile().clone())
                .with_embedder_metadata(embedder.metadata())
                .with_tool("agentsdb-web", env!("CARGO_PKG_VERSION"));
            let layer_metadata_json = layer_metadata
                .to_json_bytes()
                .context("serialize layer metadata")?;
            if chunk.id == 0 {
                chunk.id = 1;
            }
            let schema = agentsdb_format::LayerSchema {
                dim,
                element_type: agentsdb_format::EmbeddingElementType::F32,
                quant_scale: 1.0,
            };
            agentsdb_format::write_layer_atomic(
                path,
                &schema,
                &[chunk],
                Some(&layer_metadata_json),
            )
            .context("create layer")?;
            Ok(assigned)
        }
    }
    inner(
        path,
        scope,
        id,
        kind,
        content,
        confidence,
        dim,
        sources,
        source_chunks,
    )
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = vec![0u8; bytes.len() * 2];
    for (i, b) in bytes.iter().enumerate() {
        out[i * 2] = HEX[(b >> 4) as usize];
        out[i * 2 + 1] = HEX[(b & 0x0f) as usize];
    }
    String::from_utf8(out).expect("valid hex")
}

fn content_sha256_hex(content: &str) -> String {
    let digest = agentsdb_embeddings::cache::sha256(content.as_bytes());
    hex_lower(&digest)
}

fn apply_redaction(
    redact: &str,
    content: &str,
    embedding: &[f32],
) -> (Option<String>, Option<Vec<f32>>) {
    match redact {
        "none" => (Some(content.to_string()), Some(embedding.to_vec())),
        "content" => (None, Some(embedding.to_vec())),
        "embeddings" => (Some(content.to_string()), None),
        "all" => (None, None),
        _ => (Some(content.to_string()), Some(embedding.to_vec())),
    }
}

fn logical_layer_for_path(rel_path: &str) -> Option<&'static str> {
    match rel_path {
        "AGENTS.db" => Some("base"),
        "AGENTS.user.db" => Some("user"),
        "AGENTS.delta.db" => Some("delta"),
        "AGENTS.local.db" => Some("local"),
        _ => None,
    }
}

fn export_layer(
    abs_path: &Path,
    rel_path: &str,
    format: &str,
    redact: &str,
) -> anyhow::Result<(&'static str, Vec<u8>)> {
    let file = LayerFile::open(abs_path).with_context(|| format!("open {}", abs_path.display()))?;
    let layer_schema = agentsdb_format::schema_of(&file);
    let schema = ExportLayerSchemaV1 {
        dim: layer_schema.dim,
        element_type: match layer_schema.element_type {
            agentsdb_format::EmbeddingElementType::F32 => "f32".to_string(),
            agentsdb_format::EmbeddingElementType::I8 => "i8".to_string(),
        },
        quant_scale: layer_schema.quant_scale,
    };
    let layer_metadata_json = file
        .layer_metadata_bytes()
        .map(|b| String::from_utf8_lossy(b).to_string());

    let chunks = agentsdb_format::read_all_chunks(&file).context("read chunks")?;
    let mut out_chunks = Vec::with_capacity(chunks.len());
    for c in chunks {
        let (content, embedding) = apply_redaction(redact, &c.content, &c.embedding);
        let sources = c
            .sources
            .into_iter()
            .map(|s| match s {
                agentsdb_format::ChunkSource::ChunkId(id) => ExportSourceV1::ChunkId { id },
                agentsdb_format::ChunkSource::SourceString(v) => {
                    ExportSourceV1::SourceString { value: v }
                }
            })
            .collect();
        let content_sha256 = content.as_deref().map(content_sha256_hex);
        out_chunks.push(ExportChunkV1 {
            id: c.id,
            kind: c.kind,
            content,
            author: c.author,
            confidence: c.confidence,
            created_at_unix_ms: c.created_at_unix_ms,
            sources,
            embedding,
            content_sha256,
        });
    }

    match format {
        "json" => {
            let bundle = ExportBundleV1 {
                format: "agentsdb.export.v1".to_string(),
                tool: ExportToolInfo {
                    name: "agentsdb-web".to_string(),
                    version: env!("CARGO_PKG_VERSION").to_string(),
                },
                layers: vec![ExportLayerV1 {
                    path: rel_path.to_string(),
                    layer: logical_layer_for_path(rel_path).map(|s| s.to_string()),
                    schema,
                    layer_metadata_json,
                    chunks: out_chunks,
                }],
            };
            Ok((
                "application/json",
                serde_json::to_vec_pretty(&bundle).context("serialize JSON")?,
            ))
        }
        "ndjson" => {
            let mut out = Vec::new();
            let header = ExportNdjsonRecordV1::Header {
                format: "agentsdb.export.ndjson.v1".to_string(),
                tool: ExportToolInfo {
                    name: "agentsdb-web".to_string(),
                    version: env!("CARGO_PKG_VERSION").to_string(),
                },
            };
            out.extend_from_slice(serde_json::to_string(&header)?.as_bytes());
            out.push(b'\n');
            let layer_rec = ExportNdjsonRecordV1::Layer {
                path: rel_path.to_string(),
                layer: logical_layer_for_path(rel_path).map(|s| s.to_string()),
                schema,
                layer_metadata_json,
            };
            out.extend_from_slice(serde_json::to_string(&layer_rec)?.as_bytes());
            out.push(b'\n');
            for c in out_chunks {
                let rec = ExportNdjsonRecordV1::Chunk {
                    layer_path: rel_path.to_string(),
                    chunk: c,
                };
                out.extend_from_slice(serde_json::to_string(&rec)?.as_bytes());
                out.push(b'\n');
            }
            Ok(("application/x-ndjson", out))
        }
        _ => anyhow::bail!("format must be json or ndjson"),
    }
}

fn import_into_layer(
    abs_path: &Path,
    scope: &str,
    format: &str,
    data: &str,
    dry_run: bool,
    dedupe: bool,
    preserve_ids: bool,
    allow_base: bool,
    dim: Option<u32>,
) -> anyhow::Result<(usize, usize, bool)> {
    match scope {
        "local" => {
            if abs_path
                .file_name()
                .and_then(|s| s.to_str())
                .is_some_and(|n| n != "AGENTS.local.db")
            {
                anyhow::bail!("scope local only allowed for AGENTS.local.db");
            }
            agentsdb_format::ensure_writable_layer_path(abs_path).context("permission check")?;
        }
        "delta" => {
            if abs_path
                .file_name()
                .and_then(|s| s.to_str())
                .is_some_and(|n| n != "AGENTS.delta.db")
            {
                anyhow::bail!("scope delta only allowed for AGENTS.delta.db");
            }
            agentsdb_format::ensure_writable_layer_path(abs_path).context("permission check")?;
        }
        "user" => {
            if abs_path
                .file_name()
                .and_then(|s| s.to_str())
                .is_some_and(|n| n != "AGENTS.user.db")
            {
                anyhow::bail!("scope user only allowed for AGENTS.user.db");
            }
            agentsdb_format::ensure_writable_layer_path_allow_user(abs_path)
                .context("permission check")?;
        }
        "base" => {
            if !allow_base {
                anyhow::bail!("refusing to write AGENTS.db without allow_base");
            }
            if abs_path
                .file_name()
                .and_then(|s| s.to_str())
                .is_some_and(|n| n != "AGENTS.db")
            {
                anyhow::bail!("scope base only allowed for AGENTS.db");
            }
            agentsdb_format::ensure_writable_layer_path_allow_base(abs_path)
                .context("permission check")?;
        }
        _ => anyhow::bail!("scope must be local, delta, or user"),
    }

    let mut imported: Vec<ExportChunkV1> = match format {
        "json" => {
            let bundle: ExportBundleV1 = serde_json::from_str(data).context("parse JSON")?;
            bundle.layers.into_iter().flat_map(|l| l.chunks).collect()
        }
        "ndjson" => {
            let mut chunks = Vec::new();
            for (i, line) in data.lines().enumerate() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                let rec: ExportNdjsonRecordV1 = serde_json::from_str(line)
                    .with_context(|| format!("parse NDJSON line {}", i + 1))?;
                if let ExportNdjsonRecordV1::Chunk { chunk, .. } = rec {
                    chunks.push(chunk);
                }
            }
            chunks
        }
        _ => anyhow::bail!("format must be json or ndjson"),
    };

    if imported.is_empty() {
        anyhow::bail!("no chunks found in import");
    }
    for c in &mut imported {
        if c.content.is_none() {
            anyhow::bail!("import contains redacted/missing content; cannot import");
        }
        c.content_sha256 = Some(content_sha256_hex(c.content.as_deref().unwrap_or_default()));
    }

    let dir = abs_path.parent().unwrap_or_else(|| Path::new("."));
    let siblings = standard_layer_paths_for_dir(dir);

    let mut existing_hashes: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut existing_ids: std::collections::HashSet<u32> = std::collections::HashSet::new();
    let (exists, dim_usize, existing_meta) = if abs_path.exists() {
        let file = LayerFile::open(abs_path).context("open target layer")?;
        let chunks = agentsdb_format::read_all_chunks(&file).context("read target chunks")?;
        if dedupe {
            for c in &chunks {
                existing_hashes.insert(content_sha256_hex(&c.content));
            }
        }
        for c in &chunks {
            existing_ids.insert(c.id);
        }
        (
            true,
            file.embedding_dim(),
            file.layer_metadata_bytes().map(|b| b.to_vec()),
        )
    } else {
        (false, 0usize, None)
    };

    let inferred_dim = if exists {
        dim_usize
    } else if let Some(d) = dim {
        d as usize
    } else {
        imported
            .iter()
            .find_map(|c| c.embedding.as_ref().map(|e| e.len()))
            .context("creating a new layer requires dim or input embeddings")?
    };

    let embedder_for_dim = |dim_usize: usize| -> anyhow::Result<
        Box<dyn agentsdb_embeddings::embedder::Embedder + Send + Sync>,
    > {
        let options = roll_up_embedding_options_from_paths(
            Some(siblings.local.as_path()),
            Some(siblings.user.as_path()),
            Some(siblings.delta.as_path()),
            Some(siblings.base.as_path()),
        )
        .context("roll up options")?;
        if let Some(cfg_dim) = options.dim {
            if cfg_dim != dim_usize {
                anyhow::bail!(
                    "embedding dim mismatch (target dim={dim_usize}, options specify dim={cfg_dim})"
                );
            }
        }
        options
            .into_embedder(dim_usize)
            .context("resolve embedder from options")
    };

    let mut layer_metadata_json: Option<Vec<u8>> = None;
    let mut embedder: Option<Box<dyn agentsdb_embeddings::embedder::Embedder + Send + Sync>> = None;

    if !exists && preserve_ids {
        for c in &imported {
            if c.id == 0 {
                anyhow::bail!("preserve_ids requires non-zero ids in input");
            }
            if existing_ids.contains(&c.id) {
                anyhow::bail!("id {} already exists in target", c.id);
            }
            existing_ids.insert(c.id);
        }
    }

    let mut prepared: Vec<agentsdb_format::ChunkInput> = Vec::new();
    let mut skipped = 0usize;
    let mut next_new_id = 1u32;
    for c in imported {
        let content = c.content.as_ref().expect("validated");
        let hash = c.content_sha256.as_deref().unwrap_or_default();
        if dedupe && existing_hashes.contains(hash) {
            skipped += 1;
            continue;
        }
        if dedupe {
            existing_hashes.insert(hash.to_string());
        }

        let embedding = match c.embedding {
            Some(v) => v,
            None => {
                if embedder.is_none() {
                    let e = embedder_for_dim(inferred_dim)?;
                    let meta = LayerMetadataV1::new(e.profile().clone())
                        .with_embedder_metadata(e.metadata())
                        .with_tool("agentsdb-web", env!("CARGO_PKG_VERSION"));
                    layer_metadata_json =
                        Some(meta.to_json_bytes().context("serialize layer metadata")?);
                    embedder = Some(e);
                }
                let e = embedder.as_ref().expect("embedder");
                e.embed(&[content.clone()])?
                    .into_iter()
                    .next()
                    .unwrap_or_else(|| vec![0.0; inferred_dim])
            }
        };
        if embedding.len() != inferred_dim {
            anyhow::bail!(
                "embedding dim mismatch in import chunk id={} (got {}, expected {})",
                c.id,
                embedding.len(),
                inferred_dim
            );
        }

        let id = if exists {
            if preserve_ids {
                if existing_ids.contains(&c.id) {
                    anyhow::bail!("id {} already exists in target", c.id);
                }
                existing_ids.insert(c.id);
                c.id
            } else {
                0
            }
        } else if preserve_ids {
            c.id
        } else {
            while existing_ids.contains(&next_new_id) {
                next_new_id = next_new_id.saturating_add(1);
            }
            existing_ids.insert(next_new_id);
            let assigned = next_new_id;
            next_new_id = next_new_id.saturating_add(1);
            assigned
        };

        let sources = c
            .sources
            .into_iter()
            .map(|s| match s {
                ExportSourceV1::ChunkId { id } => agentsdb_format::ChunkSource::ChunkId(id),
                ExportSourceV1::SourceString { value } => {
                    agentsdb_format::ChunkSource::SourceString(value)
                }
            })
            .collect();

        prepared.push(agentsdb_format::ChunkInput {
            id,
            kind: c.kind,
            content: content.clone(),
            author: c.author,
            confidence: c.confidence,
            created_at_unix_ms: c.created_at_unix_ms,
            embedding,
            sources,
        });
    }

    if let (Some(existing_meta), Some(layer_metadata_json)) =
        (existing_meta.as_ref(), layer_metadata_json.as_ref())
    {
        let existing =
            LayerMetadataV1::from_json_bytes(existing_meta).context("parse layer metadata")?;
        let desired = LayerMetadataV1::from_json_bytes(layer_metadata_json)
            .context("parse layer metadata")?;
        if existing.embedding_profile != desired.embedding_profile {
            anyhow::bail!(
                "embedder profile mismatch vs target layer metadata (existing={:?}, current={:?})",
                existing.embedding_profile,
                desired.embedding_profile
            );
        }
    }

    let prepared_len = prepared.len();
    if dry_run {
        return Ok((prepared_len, skipped, true));
    }

    if prepared.is_empty() {
        return Ok((0, skipped, false));
    }

    if exists {
        let mut new_chunks = prepared;
        let _ = agentsdb_format::append_layer_atomic(
            abs_path,
            &mut new_chunks,
            layer_metadata_json.as_deref(),
        )
        .context("append")?;
    } else {
        let schema = agentsdb_format::LayerSchema {
            dim: inferred_dim as u32,
            element_type: agentsdb_format::EmbeddingElementType::F32,
            quant_scale: 1.0,
        };
        agentsdb_format::write_layer_atomic(
            abs_path,
            &schema,
            &prepared,
            layer_metadata_json.as_deref(),
        )
        .context("create layer")?;
    }

    Ok((prepared_len, skipped, false))
}

#[derive(Debug, Deserialize)]
struct ImportInput {
    path: String,
    scope: String, // local | delta | user | base
    #[serde(default)]
    format: Option<String>, // json | ndjson
    data: String,
    #[serde(default)]
    dry_run: Option<bool>,
    #[serde(default)]
    dedupe: Option<bool>,
    #[serde(default)]
    preserve_ids: Option<bool>,
    #[serde(default)]
    allow_base: Option<bool>,
    #[serde(default)]
    dim: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct ProposeInput {
    context_id: u32,
    #[serde(default)]
    from_path: Option<String>,
    #[serde(default)]
    to_path: Option<String>,
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
struct RejectInput {
    #[serde(rename = "ids")]
    proposal_ids: Vec<u32>,
    #[serde(default)]
    reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AcceptInput {
    #[serde(rename = "ids")]
    proposal_ids: Vec<u32>,
    #[serde(default)]
    skip_existing: bool,
}

#[derive(Debug, Deserialize)]
struct PromoteInput {
    id: u32,
    #[serde(default)]
    skip_existing: bool,
}

#[derive(Debug, Deserialize)]
struct PromoteBatchInput {
    from_path: String,
    to_path: String,
    ids: Vec<u32>,
    #[serde(default)]
    skip_existing: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
enum ProposalStatus {
    Pending,
    Accepted,
    Rejected,
}

#[derive(Debug, Clone, Serialize)]
struct ProposalRow {
    proposal_id: u32,
    context_id: u32,
    from_path: String,
    to_path: String,
    status: ProposalStatus,
    created_at_unix_ms: Option<u64>,
    title: Option<String>,
    why: Option<String>,
    what: Option<String>,
    #[serde(rename = "where")]
    where_: Option<String>,
    exists_in_delta: bool,
    exists_in_user: bool,
    exists_in_source: bool,
    exists_in_target: bool,
    decided_at_unix_ms: Option<u64>,
    decided_by: Option<String>,
    decision_reason: Option<String>,
    decision_outcome: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ProposalEvent {
    #[serde(default)]
    action: Option<String>, // propose | accept | reject
    #[serde(default)]
    proposal_id: Option<u32>, // for accept/reject
    context_id: u32,
    #[serde(default)]
    from_path: Option<String>,
    #[serde(default)]
    to_path: Option<String>,
    #[serde(default)]
    created_at_unix_ms: Option<u64>,

    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    why: Option<String>,
    #[serde(default)]
    what: Option<String>,
    #[serde(default, rename = "where")]
    where_: Option<String>,

    #[serde(default)]
    actor: Option<String>,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    outcome: Option<String>,
}

#[derive(Debug, Clone)]
struct ProposalState {
    proposal_id: u32,
    context_id: u32,
    from_path: String,
    to_path: String,
    status: ProposalStatus,
    created_at_unix_ms: Option<u64>,
    title: Option<String>,
    why: Option<String>,
    what: Option<String>,
    where_: Option<String>,
    decided_at_unix_ms: Option<u64>,
    decided_by: Option<String>,
    decision_reason: Option<String>,
    decision_outcome: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct PromoteOut {
    ok: bool,
    promoted: Vec<u32>,
    skipped: Vec<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    out_path: Option<String>,
}

fn apply_proposal_event(map: &mut BTreeMap<u32, ProposalState>, event_id: u32, ev: ProposalEvent) {
    let action = ev.action.as_deref().unwrap_or("propose");
    match action {
        "propose" => {
            let from_path = ev
                .from_path
                .unwrap_or_else(|| "AGENTS.delta.db".to_string());
            let to_path = ev.to_path.unwrap_or_else(|| "AGENTS.user.db".to_string());
            map.insert(
                event_id,
                ProposalState {
                    proposal_id: event_id,
                    context_id: ev.context_id,
                    from_path,
                    to_path,
                    status: ProposalStatus::Pending,
                    created_at_unix_ms: ev.created_at_unix_ms,
                    title: ev.title,
                    why: ev.why,
                    what: ev.what,
                    where_: ev.where_,
                    decided_at_unix_ms: None,
                    decided_by: None,
                    decision_reason: None,
                    decision_outcome: None,
                },
            );
        }
        "accept" | "reject" => {
            let Some(proposal_id) = ev.proposal_id else {
                return;
            };
            if let Some(s) = map.get_mut(&proposal_id) {
                s.status = if action == "accept" {
                    ProposalStatus::Accepted
                } else {
                    ProposalStatus::Rejected
                };
                s.decided_at_unix_ms = ev.created_at_unix_ms;
                s.decided_by = ev.actor;
                s.decision_reason = ev.reason;
                s.decision_outcome = ev.outcome;
            }
        }
        _other => {}
    }
}

fn read_proposal_events_from_layer(root: &Path) -> anyhow::Result<Vec<(u32, ProposalEvent)>> {
    let path = root.join(PROPOSAL_EVENT_LAYER);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file = LayerFile::open(&path).with_context(|| format!("open {}", path.display()))?;
    let mut out = Vec::new();
    for chunk in file.chunks() {
        let chunk = chunk?;
        if chunk.kind != PROPOSAL_EVENT_KIND {
            continue;
        }
        let ev: ProposalEvent = serde_json::from_str(chunk.content)
            .with_context(|| format!("parse proposal event chunk {}", chunk.id))?;
        out.push((chunk.id, ev));
    }
    Ok(out)
}

fn infer_dim_for_root(root: &Path) -> anyhow::Result<u32> {
    for name in [
        "AGENTS.local.db",
        "AGENTS.user.db",
        "AGENTS.delta.db",
        "AGENTS.db",
    ] {
        let path = root.join(name);
        if !path.exists() {
            continue;
        }
        let file = LayerFile::open(&path).with_context(|| format!("open {}", path.display()))?;
        return Ok(file.embedding_dim() as u32);
    }
    Ok(128)
}

fn append_proposal_event_chunk(
    st: &mut ServerState,
    record: serde_json::Value,
    context_id: u32,
) -> anyhow::Result<u32> {
    let path = st.root.join(PROPOSAL_EVENT_LAYER);
    let dim = if path.exists() {
        None
    } else {
        Some(infer_dim_for_root(&st.root).context("infer dim for proposal layer")?)
    };
    let id = append_chunk(
        &path,
        "delta",
        None,
        PROPOSAL_EVENT_KIND,
        &serde_json::to_string(&record).context("serialize proposal record")?,
        1.0,
        dim,
        &[],
        &[context_id],
    )
    .context("append proposal event chunk")?;
    st.cache.remove(PROPOSAL_EVENT_LAYER);
    Ok(id)
}

fn load_proposal_states(st: &mut ServerState) -> anyhow::Result<BTreeMap<u32, ProposalState>> {
    let events = read_proposal_events_from_layer(&st.root)?;
    let mut map: BTreeMap<u32, ProposalState> = BTreeMap::new();
    for (event_id, ev) in events {
        apply_proposal_event(&mut map, event_id, ev);
    }
    Ok(map)
}

fn list_proposals(st: &mut ServerState, include_all: bool) -> anyhow::Result<Vec<ProposalRow>> {
    let states = load_proposal_states(st)?;
    let mut layer_ids: HashMap<String, HashSet<u32>> = HashMap::new();
    for file in [
        "AGENTS.local.db",
        "AGENTS.user.db",
        "AGENTS.delta.db",
        "AGENTS.db",
    ] {
        if st.root.join(file).exists() {
            let cache = get_or_build_cache(st, file)?;
            layer_ids.insert(
                file.to_string(),
                cache.summaries.iter().map(|c| c.id).collect(),
            );
        } else {
            layer_ids.insert(file.to_string(), HashSet::new());
        }
    }

    let mut out = Vec::new();
    for s in states.values() {
        if !include_all && !matches!(s.status, ProposalStatus::Pending) {
            continue;
        }
        let from_ids = layer_ids.get(&s.from_path).cloned().unwrap_or_default();
        let to_ids = layer_ids.get(&s.to_path).cloned().unwrap_or_default();
        out.push(ProposalRow {
            proposal_id: s.proposal_id,
            context_id: s.context_id,
            from_path: s.from_path.clone(),
            to_path: s.to_path.clone(),
            status: s.status.clone(),
            created_at_unix_ms: s.created_at_unix_ms,
            title: s.title.clone(),
            why: s.why.clone(),
            what: s.what.clone(),
            where_: s.where_.clone(),
            exists_in_delta: layer_ids
                .get("AGENTS.delta.db")
                .map(|ids| ids.contains(&s.context_id))
                .unwrap_or(false),
            exists_in_user: layer_ids
                .get("AGENTS.user.db")
                .map(|ids| ids.contains(&s.context_id))
                .unwrap_or(false),
            exists_in_source: from_ids.contains(&s.context_id),
            exists_in_target: to_ids.contains(&s.context_id),
            decided_at_unix_ms: s.decided_at_unix_ms,
            decided_by: s.decided_by.clone(),
            decision_reason: s.decision_reason.clone(),
            decision_outcome: s.decision_outcome.clone(),
        });
    }
    Ok(out)
}

fn record_proposal(st: &mut ServerState, input: ProposeInput) -> anyhow::Result<u32> {
    let from_path = input
        .from_path
        .as_deref()
        .unwrap_or("AGENTS.delta.db")
        .to_string();
    let to_path = input
        .to_path
        .as_deref()
        .unwrap_or("AGENTS.user.db")
        .to_string();

    let is_allowed = match (from_path.as_str(), to_path.as_str()) {
        ("AGENTS.local.db", "AGENTS.delta.db") => true,
        ("AGENTS.local.db", "AGENTS.user.db") => true,
        ("AGENTS.user.db", "AGENTS.delta.db") => true,
        ("AGENTS.delta.db", "AGENTS.user.db") => true,
        ("AGENTS.delta.db", "AGENTS.db") => true,
        _ => false,
    };
    if !is_allowed {
        anyhow::bail!(
            "proposal flow not permitted (allowed: local->delta|user, user->delta, delta->user|base)"
        );
    }

    let src_cache =
        get_or_build_cache(st, &from_path).with_context(|| format!("open {from_path}"))?;
    if src_cache.removed_ids.contains(&input.context_id) {
        anyhow::bail!("cannot propose removed chunk id {}", input.context_id);
    }
    let exists = src_cache.summaries.iter().any(|c| c.id == input.context_id);
    if !exists {
        anyhow::bail!("chunk id {} not found in {}", input.context_id, from_path);
    }

    let record = serde_json::json!({
        "action": "propose",
        "context_id": input.context_id,
        "from_path": from_path,
        "to_path": to_path,
        "created_at_unix_ms": now_unix_ms(),
        "actor": "web",
        "title": input.title,
        "why": input.why,
        "what": input.what,
        "where": input.where_,
    });
    let id = append_proposal_event_chunk(st, record, input.context_id)
        .context("append proposal event chunk")?;
    Ok(id)
}

fn reject_proposals(
    st: &mut ServerState,
    proposal_ids: &[u32],
    reason: Option<&str>,
) -> anyhow::Result<()> {
    if proposal_ids.is_empty() {
        anyhow::bail!("ids must be non-empty");
    }
    let states = load_proposal_states(st)?;
    for id in proposal_ids {
        let Some(s) = states.get(id) else {
            anyhow::bail!("proposal {id} not found");
        };
        if !matches!(s.status, ProposalStatus::Pending) {
            anyhow::bail!("proposal {id} is not pending");
        }
    }
    for id in proposal_ids {
        let s = states.get(id).context("proposal missing")?;
        let record = serde_json::json!({
            "action": "reject",
            "proposal_id": id,
            "context_id": s.context_id,
            "created_at_unix_ms": now_unix_ms(),
            "actor": "web",
            "outcome": "rejected",
            "reason": reason,
        });
        append_proposal_event_chunk(st, record, s.context_id).context("append reject event")?;
    }
    Ok(())
}

fn accept_proposals(
    st: &mut ServerState,
    proposal_ids: &[u32],
    skip_existing: bool,
) -> anyhow::Result<PromoteOut> {
    if proposal_ids.is_empty() {
        anyhow::bail!("ids must be non-empty");
    }
    let states = load_proposal_states(st)?;
    for id in proposal_ids {
        let Some(s) = states.get(id) else {
            anyhow::bail!("proposal {id} not found");
        };
        if !matches!(s.status, ProposalStatus::Pending) {
            anyhow::bail!("proposal {id} is not pending");
        }
        let is_allowed = match (s.from_path.as_str(), s.to_path.as_str()) {
            ("AGENTS.local.db", "AGENTS.delta.db") => true,
            ("AGENTS.local.db", "AGENTS.user.db") => true,
            ("AGENTS.user.db", "AGENTS.delta.db") => true,
            ("AGENTS.delta.db", "AGENTS.user.db") => true,
            ("AGENTS.delta.db", "AGENTS.db") => true,
            _ => false,
        };
        if !is_allowed {
            anyhow::bail!("proposal {id} flow is not permitted");
        }
    }

    let out = promote_from_to(st, &states, proposal_ids, skip_existing)?;
    let promoted: HashSet<u32> = out.promoted.iter().copied().collect();
    let skipped: HashSet<u32> = out.skipped.iter().copied().collect();

    for id in proposal_ids {
        let s = states.get(id).context("proposal missing")?;
        let outcome = if promoted.contains(&s.context_id) {
            "promoted"
        } else if skipped.contains(&s.context_id) {
            "skipped_existing"
        } else {
            "unknown"
        };
        let record = serde_json::json!({
            "action": "accept",
            "proposal_id": id,
            "context_id": s.context_id,
            "created_at_unix_ms": now_unix_ms(),
            "actor": "web",
            "outcome": outcome,
            "out_path": out.out_path.clone(),
        });
        append_proposal_event_chunk(st, record, s.context_id).context("append accept event")?;
    }

    Ok(out)
}

fn promote_from_to(
    st: &mut ServerState,
    states: &BTreeMap<u32, ProposalState>,
    proposal_ids: &[u32],
    skip_existing: bool,
) -> anyhow::Result<PromoteOut> {
    let mut promoted_all = Vec::new();
    let mut skipped_all = Vec::new();
    let mut out_path: Option<String> = None;

    let mut by_pair: BTreeMap<(String, String), Vec<u32>> = BTreeMap::new();
    for id in proposal_ids {
        let s = states.get(id).context("proposal state missing")?;
        by_pair
            .entry((s.from_path.clone(), s.to_path.clone()))
            .or_default()
            .push(s.context_id);
    }
    for ((from_path, to_path), mut group_ids) in by_pair {
        group_ids.sort_unstable();
        group_ids.dedup();
        let out = promote_layers(st, &from_path, &to_path, &group_ids, skip_existing)?;
        promoted_all.extend(out.promoted);
        skipped_all.extend(out.skipped);
        if let Some(p) = out.out_path {
            match out_path.as_deref() {
                None => out_path = Some(p),
                Some(existing) if existing == p => {}
                Some(existing) => {
                    anyhow::bail!("multiple promote output paths ({existing} vs {p})");
                }
            }
        }
    }

    promoted_all.sort_unstable();
    promoted_all.dedup();
    skipped_all.sort_unstable();
    skipped_all.dedup();

    Ok(PromoteOut {
        ok: true,
        promoted: promoted_all,
        skipped: skipped_all,
        out_path,
    })
}

fn promote_delta_to_user(
    st: &mut ServerState,
    ids: &[u32],
    skip_existing: bool,
) -> anyhow::Result<PromoteOut> {
    if ids.is_empty() {
        anyhow::bail!("ids must be non-empty");
    }
    let delta_path = st.root.join("AGENTS.delta.db");
    let user_path = st.root.join("AGENTS.user.db");

    if !delta_path.exists() {
        anyhow::bail!("AGENTS.delta.db not found under root");
    }
    if user_path.file_name().and_then(|s| s.to_str()) != Some("AGENTS.user.db") {
        anyhow::bail!("invalid user layer path");
    }
    agentsdb_format::ensure_writable_layer_path_allow_user(&user_path)
        .context("permission check")?;

    let delta_file = agentsdb_format::LayerFile::open(&delta_path)
        .with_context(|| format!("open {}", delta_path.display()))?;
    let delta_schema = agentsdb_format::schema_of(&delta_file);
    let delta_metadata = delta_file.layer_metadata_bytes().map(|b| b.to_vec());
    let delta_chunks = agentsdb_format::read_all_chunks(&delta_file)?;
    let delta_by_id: HashMap<u32, agentsdb_format::ChunkInput> =
        delta_chunks.into_iter().map(|c| (c.id, c)).collect();

    let mut existing_user_ids: HashSet<u32> = HashSet::new();
    if user_path.exists() {
        let user_file = agentsdb_format::LayerFile::open(&user_path)
            .with_context(|| format!("open {}", user_path.display()))?;
        let user_schema = agentsdb_format::schema_of(&user_file);
        if user_schema.dim != delta_schema.dim
            || user_schema.element_type != delta_schema.element_type
            || user_schema.quant_scale.to_bits() != delta_schema.quant_scale.to_bits()
        {
            anyhow::bail!("schema mismatch between AGENTS.delta.db and AGENTS.user.db");
        }
        existing_user_ids = agentsdb_format::read_all_chunks(&user_file)?
            .into_iter()
            .map(|c| c.id)
            .collect();
    }

    let mut promote_ids = Vec::new();
    let mut skipped = Vec::new();
    for id in ids {
        if existing_user_ids.contains(id) {
            if skip_existing {
                skipped.push(*id);
                continue;
            }
            anyhow::bail!(
                "AGENTS.user.db already contains id {id} (use skip_existing to skip duplicates)"
            );
        }
        promote_ids.push(*id);
    }
    promote_ids.sort_unstable();
    promote_ids.dedup();

    let mut promote = Vec::new();
    for id in &promote_ids {
        let Some(c) = delta_by_id.get(id) else {
            anyhow::bail!("chunk id {id} not found in AGENTS.delta.db");
        };
        let mut c = c.clone();
        if c.author != "human" {
            c.author = "human".to_string();
        }
        promote.push(c);
    }

    if !promote.is_empty() {
        if user_path.exists() {
            agentsdb_format::append_layer_atomic(&user_path, &mut promote, None)
                .context("append")?;
        } else {
            agentsdb_format::write_layer_atomic(
                &user_path,
                &delta_schema,
                &promote,
                delta_metadata.as_deref(),
            )
            .context("write")?;
        }
        st.cache.remove("AGENTS.user.db");
    }

    Ok(PromoteOut {
        ok: true,
        promoted: promote_ids,
        skipped,
        out_path: None,
    })
}

fn promote_layers(
    st: &mut ServerState,
    from_path: &str,
    to_path: &str,
    ids: &[u32],
    skip_existing: bool,
) -> anyhow::Result<PromoteOut> {
    if ids.is_empty() {
        anyhow::bail!("ids must be non-empty");
    }
    if from_path == to_path {
        anyhow::bail!("from_path and to_path must differ");
    }

    let allowed = match (from_path, to_path) {
        ("AGENTS.local.db", "AGENTS.delta.db") => true,
        ("AGENTS.local.db", "AGENTS.user.db") => true,
        ("AGENTS.user.db", "AGENTS.delta.db") => true,
        ("AGENTS.delta.db", "AGENTS.user.db") => true,
        ("AGENTS.delta.db", "AGENTS.db") => true,
        _ => false,
    };
    if !allowed {
        anyhow::bail!(
            "promotion flow not permitted (allowed: local->delta|user, user->delta, delta->user|base)"
        );
    }

    if to_path == "AGENTS.db" {
        return promote_delta_to_base_new(st, ids, skip_existing);
    }

    let from_abs = resolve_layer_path(&st.root, from_path)?;
    let to_abs = st.root.join(to_path);
    agentsdb_format::ensure_writable_layer_path_allow_user(&to_abs).context("permission check")?;

    let from_file =
        agentsdb_format::LayerFile::open(&from_abs).with_context(|| format!("open {from_path}"))?;
    let from_schema = agentsdb_format::schema_of(&from_file);
    let from_metadata = from_file.layer_metadata_bytes().map(|b| b.to_vec());
    let from_chunks = agentsdb_format::read_all_chunks(&from_file)?;
    let by_id: HashMap<u32, agentsdb_format::ChunkInput> =
        from_chunks.into_iter().map(|c| (c.id, c)).collect();

    let mut to_existing_ids: HashSet<u32> = HashSet::new();
    if to_abs.exists() {
        let to_file =
            agentsdb_format::LayerFile::open(&to_abs).with_context(|| format!("open {to_path}"))?;
        let to_schema = agentsdb_format::schema_of(&to_file);
        if to_schema.dim != from_schema.dim
            || to_schema.element_type != from_schema.element_type
            || to_schema.quant_scale.to_bits() != from_schema.quant_scale.to_bits()
        {
            anyhow::bail!("schema mismatch between {from_path} and {to_path}");
        }
        to_existing_ids = agentsdb_format::read_all_chunks(&to_file)?
            .into_iter()
            .map(|c| c.id)
            .collect();
    }

    let mut promote_ids = Vec::new();
    let mut skipped = Vec::new();
    for id in ids {
        if to_existing_ids.contains(id) {
            if skip_existing {
                skipped.push(*id);
                continue;
            }
            anyhow::bail!("destination already contains id {id}");
        }
        promote_ids.push(*id);
    }
    promote_ids.sort_unstable();
    promote_ids.dedup();

    let mut promote = Vec::new();
    for id in &promote_ids {
        let Some(c) = by_id.get(id) else {
            anyhow::bail!("chunk id {id} not found in {from_path}");
        };
        let mut c = c.clone();
        if c.author != "human" {
            c.author = "human".to_string();
        }
        promote.push(c);
    }

    if !promote.is_empty() {
        if to_abs.exists() {
            agentsdb_format::append_layer_atomic(&to_abs, &mut promote, None).context("append")?;
        } else {
            agentsdb_format::write_layer_atomic(
                &to_abs,
                &from_schema,
                &promote,
                from_metadata.as_deref(),
            )
            .context("write")?;
        }
        st.cache.remove(to_path);
    }

    Ok(PromoteOut {
        ok: true,
        promoted: promote_ids,
        skipped,
        out_path: None,
    })
}

fn chunks_equal(a: &agentsdb_format::ChunkInput, b: &agentsdb_format::ChunkInput) -> bool {
    a.id == b.id
        && a.kind == b.kind
        && a.content == b.content
        && a.author == b.author
        && a.confidence.to_bits() == b.confidence.to_bits()
        && a.created_at_unix_ms == b.created_at_unix_ms
        && a.embedding.len() == b.embedding.len()
        && a.embedding
            .iter()
            .zip(b.embedding.iter())
            .all(|(x, y)| x.to_bits() == y.to_bits())
        && sources_equal(&a.sources, &b.sources)
}

fn sources_equal(a: &[agentsdb_format::ChunkSource], b: &[agentsdb_format::ChunkSource]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    for (x, y) in a.iter().zip(b.iter()) {
        match (x, y) {
            (
                agentsdb_format::ChunkSource::ChunkId(ax),
                agentsdb_format::ChunkSource::ChunkId(by),
            ) => {
                if ax != by {
                    return false;
                }
            }
            (
                agentsdb_format::ChunkSource::SourceString(ax),
                agentsdb_format::ChunkSource::SourceString(by),
            ) => {
                if ax != by {
                    return false;
                }
            }
            _ => return false,
        }
    }
    true
}

fn promote_delta_to_base_new(
    st: &mut ServerState,
    ids: &[u32],
    skip_existing: bool,
) -> anyhow::Result<PromoteOut> {
    let base_path = st.root.join("AGENTS.db");
    let delta_path = st.root.join("AGENTS.delta.db");
    if !base_path.exists() {
        anyhow::bail!("AGENTS.db not found under root");
    }
    if !delta_path.exists() {
        anyhow::bail!("AGENTS.delta.db not found under root");
    }

    let base_file = agentsdb_format::LayerFile::open(&base_path)
        .with_context(|| format!("open {}", base_path.display()))?;
    let base_schema = agentsdb_format::schema_of(&base_file);
    let base_metadata = base_file.layer_metadata_bytes().map(|b| b.to_vec());
    let mut by_id: BTreeMap<u32, agentsdb_format::ChunkInput> =
        agentsdb_format::read_all_chunks(&base_file)?
            .into_iter()
            .map(|c| (c.id, c))
            .collect();

    let delta_file = agentsdb_format::LayerFile::open(&delta_path)
        .with_context(|| format!("open {}", delta_path.display()))?;
    let delta_schema = agentsdb_format::schema_of(&delta_file);
    if delta_schema.dim != base_schema.dim
        || delta_schema.element_type != base_schema.element_type
        || delta_schema.quant_scale.to_bits() != base_schema.quant_scale.to_bits()
    {
        anyhow::bail!("schema mismatch between AGENTS.delta.db and AGENTS.db");
    }
    let delta_by_id: HashMap<u32, agentsdb_format::ChunkInput> =
        agentsdb_format::read_all_chunks(&delta_file)?
            .into_iter()
            .map(|c| (c.id, c))
            .collect();

    let mut promoted = Vec::new();
    let mut skipped = Vec::new();

    for id in ids {
        let Some(c) = delta_by_id.get(id) else {
            anyhow::bail!("chunk id {id} not found in AGENTS.delta.db");
        };
        if c.kind == PROPOSAL_EVENT_KIND {
            anyhow::bail!("cannot promote proposal event chunk id {id} into base");
        }
        if c.kind == TOMBSTONE_KIND {
            anyhow::bail!("cannot promote tombstone chunk id {id} into base");
        }
        if let Some(existing) = by_id.get(id) {
            if chunks_equal(existing, c) {
                skipped.push(*id);
                continue;
            }
            if skip_existing {
                anyhow::bail!(
                    "base already contains id {id} with different content (cannot skip conflicts)"
                );
            }
            anyhow::bail!("base already contains id {id} with different content");
        }
        let mut c = c.clone();
        if c.author != "human" {
            c.author = "human".to_string();
        }
        by_id.insert(*id, c);
        promoted.push(*id);
    }

    promoted.sort_unstable();
    promoted.dedup();
    skipped.sort_unstable();
    skipped.dedup();
    if promoted.is_empty() {
        return Ok(PromoteOut {
            ok: true,
            promoted,
            skipped,
            out_path: None,
        });
    }

    let out_path = st.root.join("AGENTS.db.new");
    let mut chunks: Vec<agentsdb_format::ChunkInput> = by_id.into_values().collect();
    chunks.sort_by_key(|c| c.id);
    agentsdb_format::write_layer_atomic(&out_path, &base_schema, &chunks, base_metadata.as_deref())
        .context("write AGENTS.db.new")?;

    Ok(PromoteOut {
        ok: true,
        promoted,
        skipped,
        out_path: Some(out_path.to_string_lossy().into_owned()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentsdb_embeddings::embedder::{EmbeddingProfile, OutputNorm};
    use agentsdb_embeddings::layer_metadata::LayerMetadataV1;

    fn write_layer_with_custom_profile(path: &Path, dim: u32, output_norm: OutputNorm) {
        let schema = agentsdb_format::LayerSchema {
            dim,
            element_type: agentsdb_format::EmbeddingElementType::F32,
            quant_scale: 1.0,
        };
        let profile = EmbeddingProfile {
            backend: "hash".to_string(),
            model: None,
            revision: None,
            dim: dim as usize,
            output_norm,
        };
        let metadata = LayerMetadataV1::new(profile)
            .to_json_bytes()
            .expect("metadata json");
        let chunk = agentsdb_format::ChunkInput {
            id: 1,
            kind: "note".to_string(),
            content: "seed".to_string(),
            author: "human".to_string(),
            confidence: 1.0,
            created_at_unix_ms: 0,
            embedding: vec![0.0; dim as usize],
            sources: Vec::new(),
        };
        agentsdb_format::write_layer_atomic(path, &schema, &[chunk], Some(&metadata))
            .expect("write layer");
    }

    #[test]
    fn web_write_rejects_embedder_profile_mismatch() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("AGENTS.local.db");
        write_layer_with_custom_profile(&path, 8, OutputNorm::L2);

        let err = append_chunk(&path, "local", None, "note", "hello", 1.0, None, &[], &[])
            .expect_err("expected mismatch error");
        assert!(
            err.to_string().contains("embedder profile mismatch"),
            "{err}"
        );
    }

    #[test]
    fn web_promote_copies_delta_to_user_and_records_ids() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();

        let delta = root.join("AGENTS.delta.db");
        write_layer_with_custom_profile(&delta, 8, OutputNorm::None);

        // Add a second chunk with a stable id to promote.
        let _ = append_chunk(
            &delta,
            "delta",
            Some(9),
            "note",
            "promote me",
            0.9,
            None,
            &[],
            &[],
        )
        .expect("append delta chunk");

        let mut st = ServerState::new(root.to_path_buf());
        let out = promote_delta_to_user(&mut st, &[9], false).expect("promote");
        assert_eq!(out.promoted, vec![9]);
        assert!(root.join("AGENTS.user.db").exists());
    }

    #[test]
    fn web_proposal_states_ignore_missing_layer() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut st = ServerState::new(dir.path().to_path_buf());
        let states = load_proposal_states(&mut st).expect("load states");
        assert!(states.is_empty());
    }
}
