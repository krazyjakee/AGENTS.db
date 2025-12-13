use anyhow::Context;
use clap::{Parser, Subcommand};
use serde::Serialize;

use agentsdb_core::embed::hash_embed;
use agentsdb_core::types::{LayerId, SearchFilters};
use agentsdb_query::{LayerSet, SearchQuery};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(
    name = "agentsdb",
    version,
    about = "AGENTS.db tooling (v0.1)",
    long_about = "Tools for creating, inspecting, and querying AGENTS.db layers.\n\nNotes:\n  - Layers are treated as append-only. Writes append new chunks.\n  - `search --query` uses a deterministic hash embedding (not a semantic model)."
)]
struct Cli {
    /// Emit machine-readable JSON instead of human output.
    #[arg(long)]
    json: bool,

    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Collect files and emit a compile JSON payload.
    Collect {
        /// Root directory to search for files.
        #[arg(long, default_value = ".")]
        root: String,
        /// File names to include (repeatable).
        #[arg(long = "include", default_value = "AGENTS.md")]
        includes: Vec<String>,
        /// Output path for the generated JSON.
        #[arg(long)]
        out: String,
        /// Chunk kind to assign to collected files.
        #[arg(long, default_value = "canonical")]
        kind: String,
        /// Embedding dimension for the emitted schema.
        #[arg(long, default_value_t = 128)]
        dim: u32,
        /// Embedding element type: `f32` or `i8`.
        #[arg(long, default_value = "f32")]
        element_type: String,
        /// Quantization scale (only used when `--element-type i8`).
        #[arg(long)]
        quant_scale: Option<f32>,
    },
    /// Validate that a layer file is readable and well-formed.
    Validate {
        /// Layer path (e.g. `AGENTS.base.db`).
        path: String,
    },
    /// Inspect a layer file header/sections, or print a chunk by id.
    Inspect {
        /// Layer path to inspect (alternative to providing PATH).
        #[arg(long)]
        layer: Option<String>,
        /// Chunk id to print (prints layer metadata if omitted).
        #[arg(long)]
        id: Option<u32>,
        /// Layer path to inspect (positional alternative to `--layer`).
        #[arg(value_name = "PATH")]
        path: Option<String>,
    },
    /// Run the MCP server over stdio.
    Serve {
        /// Path to a base layer (usually `AGENTS.base.db`).
        #[arg(long)]
        base: Option<String>,
        /// Path to a user layer (usually `AGENTS.user.db`).
        #[arg(long)]
        user: Option<String>,
        /// Path to a delta layer (usually `AGENTS.delta.db`).
        #[arg(long)]
        delta: Option<String>,
        /// Path to a local layer (usually `AGENTS.local.db`).
        #[arg(long)]
        local: Option<String>,
    },
    /// Compile a JSON payload into an on-disk layer file.
    Compile {
        /// Input JSON path (from `collect` or manually authored).
        #[arg(long = "in")]
        input: String,
        /// Output layer path to write.
        #[arg(long)]
        out: String,
    },
    /// Append a chunk to a writable layer file.
    Write {
        /// Destination layer path (must be `AGENTS.local.db` or `AGENTS.delta.db`).
        path: String,
        /// Target scope for permission checks: `local` or `delta`.
        #[arg(long)]
        scope: String, // local | delta
        /// Chunk id to write (if omitted, an id is assigned).
        #[arg(long)]
        id: Option<u32>,
        /// Chunk kind (e.g. `canonical`, `note`, etc).
        #[arg(long)]
        kind: String,
        /// Chunk content (the text to store).
        #[arg(long)]
        content: String,
        /// Confidence score in [0, 1].
        #[arg(long)]
        confidence: f32,
        /// Embedding JSON array (e.g. `[0.1, 0.2, ...]`); if omitted, uses hash embedding.
        #[arg(long)]
        embedding: Option<String>, // JSON array; if omitted, uses hash embed
        /// Embedding dimension (required when creating a new layer and `--embedding` is omitted).
        #[arg(long)]
        dim: Option<u32>, // required when creating a new layer and embedding omitted
        /// Source references like `path/to/file:line` (repeatable).
        #[arg(long = "source")]
        sources: Vec<String>, // file:line-style strings
        /// Source chunk ids (repeatable).
        #[arg(long = "source-chunk")]
        source_chunks: Vec<u32>,
    },
    /// Search one or more layers using vector similarity.
    #[command(
        after_help = "Examples:\n  agentsdb search --base AGENTS.base.db --query \"how do I run tests?\"\n  agentsdb search --user AGENTS.user.db --query-vec-file query.json -k 5\n  agentsdb search --base AGENTS.base.db --delta AGENTS.delta.db --query \"rustfmt\" --kind canonical --kind note\n\nQuery modes:\n  - --query: text hashed into a deterministic embedding (fast, but not semantic).\n  - --query-vec/--query-vec-file: provide an explicit embedding as a JSON array of numbers."
    )]
    Search {
        /// Path to a base layer (usually `AGENTS.base.db`).
        #[arg(long)]
        base: Option<String>,
        /// Path to a user layer (usually `AGENTS.user.db`).
        #[arg(long)]
        user: Option<String>,
        /// Path to a delta layer (usually `AGENTS.delta.db`).
        #[arg(long)]
        delta: Option<String>,
        /// Path to a local layer (usually `AGENTS.local.db`).
        #[arg(long)]
        local: Option<String>,

        /// Text query (hashed into an embedding).
        #[arg(long)]
        query: Option<String>,
        /// Explicit embedding as a JSON array (e.g. `[0.1, 0.2, ...]`).
        #[arg(long)]
        query_vec: Option<String>,
        /// Path to a file containing a JSON array embedding.
        #[arg(long)]
        query_vec_file: Option<String>,

        /// Number of nearest neighbors to return.
        #[arg(short, long, default_value_t = 10)]
        k: usize,

        /// Filter results by chunk kind (repeatable).
        #[arg(long = "kind")]
        kinds: Vec<String>,
    },
    /// Compare a base layer to a delta layer by id.
    Diff {
        /// Path to the base layer.
        #[arg(long)]
        base: String,
        /// Path to the delta layer.
        #[arg(long)]
        delta: String,
    },
    /// Copy selected chunks from one layer into another.
    Promote {
        /// Source layer path.
        #[arg(long = "from")]
        from_path: String,
        /// Destination layer path (must be writable).
        #[arg(long = "to")]
        to_path: String,
        /// Comma-separated chunk ids to promote (e.g. `1,2,3`).
        #[arg(long)]
        ids: String, // comma-separated
    },
    /// (Not implemented) Rewrite and deduplicate layers.
    Compact {
        /// Path to a base layer.
        #[arg(long)]
        base: Option<String>,
        /// Path to a user layer.
        #[arg(long)]
        user: Option<String>,
        /// Output path for the compacted layer.
        #[arg(long)]
        out: Option<String>,
    },
}

#[derive(Serialize)]
struct ValidateJson<'a> {
    ok: bool,
    path: &'a str,
    error: Option<String>,
}

#[derive(Serialize)]
struct InspectJson<'a> {
    path: &'a str,
    header: HeaderJson,
    sections: Vec<SectionJson>,
    string_count: u64,
    chunk_count: u64,
    embedding: EmbeddingJson,
    relationships: Option<u64>,
}

#[derive(Serialize)]
struct HeaderJson {
    magic: u32,
    version_major: u16,
    version_minor: u16,
    file_length_bytes: u64,
    section_count: u64,
    sections_offset: u64,
    flags: u64,
}

#[derive(Serialize)]
struct SectionJson {
    kind: String,
    offset: u64,
    length: u64,
}

#[derive(Serialize)]
struct EmbeddingJson {
    row_count: u64,
    dim: u32,
    element_type: String,
    data_offset: u64,
    data_length: u64,
    quant_scale: f32,
}

#[derive(Serialize)]
struct SearchJson {
    query_dim: usize,
    k: usize,
    results: Vec<SearchResultJson>,
}

#[derive(Serialize)]
struct SearchResultJson {
    layer: String,
    id: u32,
    kind: String,
    score: f32,
    author: String,
    confidence: f32,
    created_at_unix_ms: u64,
    sources: Vec<String>,
    hidden_layers: Vec<String>,
    content: String,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.cmd {
        Command::Collect {
            root,
            includes,
            out,
            kind,
            dim,
            element_type,
            quant_scale,
        } => cmd_collect(
            &root,
            &includes,
            &out,
            &kind,
            dim,
            &element_type,
            quant_scale,
            cli.json,
        ),
        Command::Validate { path } => cmd_validate(&path, cli.json),
        Command::Inspect { layer, id, path } => {
            cmd_inspect(layer.as_deref(), path.as_deref(), id, cli.json)
        }
        Command::Serve {
            base,
            user,
            delta,
            local,
        } => {
            if cli.json {
                anyhow::bail!("--json is not supported for serve");
            }
            agentsdb_mcp::serve_stdio(agentsdb_mcp::ServerConfig {
                base,
                user,
                delta,
                local,
            })
        }
        Command::Compile { input, out } => cmd_compile(&input, &out, cli.json),
        Command::Write {
            path,
            scope,
            id,
            kind,
            content,
            confidence,
            embedding,
            dim,
            sources,
            source_chunks,
        } => cmd_write(
            &path,
            &scope,
            id,
            &kind,
            &content,
            confidence,
            embedding.as_deref(),
            dim,
            &sources,
            &source_chunks,
            cli.json,
        ),
        Command::Search {
            base,
            user,
            delta,
            local,
            query,
            query_vec,
            query_vec_file,
            k,
            kinds,
        } => cmd_search(
            LayerSet {
                base,
                user,
                delta,
                local,
            },
            query,
            query_vec,
            query_vec_file,
            k,
            kinds,
            cli.json,
        ),
        Command::Diff { base, delta } => cmd_diff(&base, &delta, cli.json),
        Command::Promote {
            from_path,
            to_path,
            ids,
        } => cmd_promote(&from_path, &to_path, &ids, cli.json),
        Command::Compact { .. } => {
            anyhow::bail!("compact is optional for v0.1 and is not implemented yet")
        }
    }
}

#[derive(serde::Deserialize)]
struct CompileInput {
    schema: CompileSchema,
    chunks: Vec<CompileChunk>,
}

#[derive(serde::Deserialize)]
struct CompileSchema {
    dim: u32,
    element_type: String, // "f32" | "i8"
    quant_scale: Option<f32>,
}

#[derive(serde::Deserialize)]
struct CompileChunk {
    id: u32,
    kind: String,
    content: String,
    author: String,
    confidence: f32,
    created_at_unix_ms: u64,
    #[serde(default)]
    embedding: Option<Vec<f32>>,
    #[serde(default)]
    sources: Vec<CompileSource>,
}

#[derive(serde::Deserialize)]
#[serde(untagged)]
enum CompileSource {
    String(String),
    Chunk { chunk_id: u32 },
}

#[derive(serde::Serialize)]
struct CollectOutput {
    schema: CompileSchemaOut,
    chunks: Vec<CollectChunk>,
}

#[derive(serde::Serialize)]
struct CompileSchemaOut {
    dim: u32,
    element_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    quant_scale: Option<f32>,
}

#[derive(serde::Serialize)]
struct CollectChunk {
    id: u32,
    kind: String,
    content: String,
    author: String,
    confidence: f32,
    created_at_unix_ms: u64,
    sources: Vec<CollectSource>,
}

#[derive(serde::Serialize)]
#[serde(untagged)]
enum CollectSource {
    String(String),
}

#[allow(clippy::too_many_arguments)]
fn cmd_collect(
    root: &str,
    includes: &[String],
    out: &str,
    kind: &str,
    dim: u32,
    element_type: &str,
    quant_scale: Option<f32>,
    json: bool,
) -> anyhow::Result<()> {
    if dim == 0 {
        anyhow::bail!("--dim must be non-zero");
    }
    if element_type != "f32" && element_type != "i8" {
        anyhow::bail!("--element-type must be 'f32' or 'i8'");
    }

    let root_path = Path::new(root);
    let files = collect_files(root_path, includes)?;

    let mut used_ids = BTreeSet::new();
    let mut chunks = Vec::with_capacity(files.len());
    for rel in files {
        let abs = root_path.join(&rel);
        let content =
            std::fs::read_to_string(&abs).with_context(|| format!("read {}", abs.display()))?;
        let id = assign_stable_id(&rel, &content, &mut used_ids);
        chunks.push(CollectChunk {
            id,
            kind: kind.to_string(),
            content,
            author: "human".to_string(),
            confidence: 1.0,
            created_at_unix_ms: 0,
            sources: vec![CollectSource::String(format!("{}:1", rel.display()))],
        });
    }

    let output = CollectOutput {
        schema: CompileSchemaOut {
            dim,
            element_type: element_type.to_string(),
            quant_scale: quant_scale.or_else(|| (element_type == "i8").then_some(1.0)),
        },
        chunks,
    };

    let s = serde_json::to_string_pretty(&output)?;
    let out_path = Path::new(out);
    if let Some(parent) = out_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create dir {}", parent.display()))?;
        }
    }
    std::fs::write(out_path, s.as_bytes()).with_context(|| format!("write {out}"))?;

    if json {
        #[derive(Serialize)]
        struct Out<'a> {
            ok: bool,
            out: &'a str,
            chunks: usize,
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&Out {
                ok: true,
                out,
                chunks: output.chunks.len(),
            })?
        );
    } else {
        println!("Wrote {out} ({} chunks)", output.chunks.len());
    }

    Ok(())
}

fn cmd_validate(path: &str, json: bool) -> anyhow::Result<()> {
    let res = agentsdb_format::LayerFile::open(path);
    match res {
        Ok(_) => {
            if json {
                let out = ValidateJson {
                    ok: true,
                    path,
                    error: None,
                };
                println!("{}", serde_json::to_string_pretty(&out)?);
            } else {
                println!("OK: {path}");
            }
            Ok(())
        }
        Err(e) => {
            if json {
                let out = ValidateJson {
                    ok: false,
                    path,
                    error: Some(e.to_string()),
                };
                println!("{}", serde_json::to_string_pretty(&out)?);
                std::process::exit(1);
            } else {
                anyhow::bail!("INVALID: {path}: {e}");
            }
        }
    }
}

fn cmd_inspect(
    layer: Option<&str>,
    path: Option<&str>,
    id: Option<u32>,
    json: bool,
) -> anyhow::Result<()> {
    let p = layer
        .or(path)
        .ok_or_else(|| anyhow::anyhow!("missing layer path (use --layer PATH or provide PATH)"))?;
    let file = agentsdb_format::LayerFile::open(p).with_context(|| format!("open {p}"))?;

    if let Some(id) = id {
        let mut found: Option<agentsdb_format::ChunkView<'_>> = None;
        for c in file.chunks() {
            let c = c?;
            if c.id == id {
                found = Some(c);
                break;
            }
        }
        let Some(c) = found else {
            anyhow::bail!("chunk id {id} not found in {p}");
        };
        let sources = file
            .sources_for(c.rel_start, c.rel_count)?
            .into_iter()
            .map(|s| match s {
                agentsdb_format::SourceRef::ChunkId(v) => format!("chunk:{v}"),
                agentsdb_format::SourceRef::String(v) => v.to_string(),
            })
            .collect::<Vec<_>>();

        if json {
            #[derive(Serialize)]
            struct Out<'a> {
                layer: &'a str,
                id: u32,
                kind: &'a str,
                author: &'a str,
                confidence: f32,
                created_at_unix_ms: u64,
                sources: Vec<String>,
                content: &'a str,
            }
            println!(
                "{}",
                serde_json::to_string_pretty(&Out {
                    layer: p,
                    id: c.id,
                    kind: c.kind,
                    author: c.author,
                    confidence: c.confidence,
                    created_at_unix_ms: c.created_at_unix_ms,
                    sources,
                    content: c.content,
                })?
            );
        } else {
            println!("Layer: {p}");
            println!(
                "Chunk: id={} kind={} author={} conf={:.3} created_at_unix_ms={}",
                c.id, c.kind, c.author, c.confidence, c.created_at_unix_ms
            );
            for s in sources {
                println!("  source: {s}");
            }
            println!();
            println!("{}", c.content);
        }
        return Ok(());
    }

    if json {
        let header = HeaderJson {
            magic: file.header.magic,
            version_major: file.header.version_major,
            version_minor: file.header.version_minor,
            file_length_bytes: file.header.file_length_bytes,
            section_count: file.header.section_count,
            sections_offset: file.header.sections_offset,
            flags: file.header.flags,
        };
        let sections = file
            .sections
            .iter()
            .map(|s| SectionJson {
                kind: format!("{:?}", s.kind),
                offset: s.offset,
                length: s.length,
            })
            .collect();

        let embedding = EmbeddingJson {
            row_count: file.embedding_matrix.row_count,
            dim: file.embedding_matrix.dim,
            element_type: format!("{:?}", file.embedding_matrix.element_type),
            data_offset: file.embedding_matrix.data_offset,
            data_length: file.embedding_matrix.data_length,
            quant_scale: file.embedding_matrix.quant_scale,
        };

        let out = InspectJson {
            path: p,
            header,
            sections,
            string_count: file.string_dictionary.string_count,
            chunk_count: file.chunk_count,
            embedding,
            relationships: file.relationship_count,
        };
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        println!("Path: {p}");
        println!(
            "Header: magic=0x{:08x} version={}.{} file_len={} sections={} sections_offset={} flags={}",
            file.header.magic,
            file.header.version_major,
            file.header.version_minor,
            file.header.file_length_bytes,
            file.header.section_count,
            file.header.sections_offset,
            file.header.flags
        );
        println!("Sections:");
        for s in &file.sections {
            println!(
                "  - kind={:?} offset={} length={}",
                s.kind, s.offset, s.length
            );
        }
        println!(
            "StringDictionary: string_count={}",
            file.string_dictionary.string_count
        );
        println!("ChunkTable: chunk_count={}", file.chunk_count);
        println!(
            "EmbeddingMatrix: rows={} dim={} type={:?} data_offset={} data_length={} quant_scale={}",
            file.embedding_matrix.row_count,
            file.embedding_matrix.dim,
            file.embedding_matrix.element_type,
            file.embedding_matrix.data_offset,
            file.embedding_matrix.data_length,
            file.embedding_matrix.quant_scale
        );
        println!(
            "Relationships: {}",
            file.relationship_count
                .map(|v| v.to_string())
                .unwrap_or_else(|| "absent".to_string())
        );
    }

    Ok(())
}

fn cmd_compile(input: &str, out: &str, json: bool) -> anyhow::Result<()> {
    let s = std::fs::read_to_string(input).with_context(|| format!("read {input}"))?;
    let mut input: CompileInput = serde_json::from_str(&s).context("parse compile input JSON")?;

    if input.schema.dim == 0 {
        anyhow::bail!("schema.dim must be non-zero");
    }

    let element_type = match input.schema.element_type.as_str() {
        "f32" => agentsdb_format::EmbeddingElementType::F32,
        "i8" => agentsdb_format::EmbeddingElementType::I8,
        other => anyhow::bail!("schema.element_type must be 'f32' or 'i8' (got {other:?})"),
    };
    let quant_scale = match element_type {
        agentsdb_format::EmbeddingElementType::F32 => 1.0,
        agentsdb_format::EmbeddingElementType::I8 => input.schema.quant_scale.unwrap_or(1.0),
    };
    let schema = agentsdb_format::LayerSchema {
        dim: input.schema.dim,
        element_type,
        quant_scale,
    };

    input.chunks.sort_by_key(|c| c.id);
    let dim = schema.dim as usize;
    let chunks: Vec<agentsdb_format::ChunkInput> = input
        .chunks
        .into_iter()
        .map(|c| {
            let content = c.content;
            let embedding = match c.embedding {
                Some(v) => v,
                None => hash_embed(&content, dim),
            };
            agentsdb_format::ChunkInput {
                id: c.id,
                kind: c.kind,
                content,
                author: c.author,
                confidence: c.confidence,
                created_at_unix_ms: c.created_at_unix_ms,
                embedding,
                sources: c
                    .sources
                    .into_iter()
                    .map(|s| match s {
                        CompileSource::String(v) => agentsdb_format::ChunkSource::SourceString(v),
                        CompileSource::Chunk { chunk_id } => {
                            agentsdb_format::ChunkSource::ChunkId(chunk_id)
                        }
                    })
                    .collect(),
            }
        })
        .collect();

    agentsdb_format::write_layer_atomic(out, &schema, &chunks).context("write layer")?;

    if json {
        #[derive(Serialize)]
        struct Out<'a> {
            ok: bool,
            out: &'a str,
            chunks: usize,
        }
        let out = Out {
            ok: true,
            out,
            chunks: chunks.len(),
        };
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        println!("Wrote {out} ({} chunks)", chunks.len());
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn cmd_write(
    path: &str,
    scope: &str,
    id: Option<u32>,
    kind: &str,
    content: &str,
    confidence: f32,
    embedding_json: Option<&str>,
    dim: Option<u32>,
    sources: &[String],
    source_chunks: &[u32],
    json: bool,
) -> anyhow::Result<()> {
    if scope != "local" && scope != "delta" {
        anyhow::bail!("--scope must be 'local' or 'delta'");
    }
    let expected_name = match scope {
        "local" => "AGENTS.local.db",
        "delta" => "AGENTS.delta.db",
        _ => unreachable!(),
    };
    if std::path::Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .is_some_and(|n| n != expected_name)
    {
        anyhow::bail!("scope {scope:?} expects file named {expected_name}");
    }

    agentsdb_format::ensure_writable_layer_path(path).context("permission check")?;

    let embedding = match embedding_json {
        Some(v) => parse_vec_json(v)?,
        None => Vec::new(),
    };
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let mut chunk = agentsdb_format::ChunkInput {
        id: id.unwrap_or(0),
        kind: kind.to_string(),
        content: content.to_string(),
        author: "mcp".to_string(),
        confidence,
        created_at_unix_ms: now_ms,
        embedding: embedding.clone(),
        sources: sources
            .iter()
            .cloned()
            .map(agentsdb_format::ChunkSource::SourceString)
            .chain(
                source_chunks
                    .iter()
                    .copied()
                    .map(agentsdb_format::ChunkSource::ChunkId),
            )
            .collect(),
    };

    let p = std::path::Path::new(path);
    let assigned = if p.exists() {
        if embedding.is_empty() {
            let file = agentsdb_format::LayerFile::open(path).context("open layer")?;
            chunk.embedding = hash_embed(&chunk.content, file.embedding_dim());
        }
        let mut chunks = vec![chunk];
        let ids = agentsdb_format::append_layer_atomic(path, &mut chunks).context("append")?;
        ids[0]
    } else {
        let dim = match (dim, embedding.is_empty()) {
            (Some(d), _) => d as usize,
            (None, true) => {
                anyhow::bail!("creating a new layer without --embedding requires --dim")
            }
            (None, false) => embedding.len(),
        };
        if chunk.embedding.is_empty() {
            chunk.embedding = hash_embed(&chunk.content, dim);
        }
        if chunk.id == 0 {
            chunk.id = 1;
        }
        let schema = agentsdb_format::LayerSchema {
            dim: dim as u32,
            element_type: agentsdb_format::EmbeddingElementType::F32,
            quant_scale: 1.0,
        };
        agentsdb_format::write_layer_atomic(path, &schema, &[chunk]).context("create layer")?;
        id.unwrap_or(1)
    };

    if json {
        #[derive(Serialize)]
        struct Out<'a> {
            ok: bool,
            path: &'a str,
            id: u32,
        }
        let out = Out {
            ok: true,
            path,
            id: assigned,
        };
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        println!("Appended id={assigned} to {path}");
    }

    Ok(())
}

fn cmd_search(
    layers: LayerSet,
    query: Option<String>,
    query_vec: Option<String>,
    query_vec_file: Option<String>,
    k: usize,
    kinds: Vec<String>,
    json: bool,
) -> anyhow::Result<()> {
    let opened = layers.open().context("open layers")?;
    if opened.is_empty() {
        anyhow::bail!("no layers provided (use --base/--user/--delta/--local)");
    }

    let dim = opened[0].1.embedding_dim();
    let embedding = match (query, query_vec, query_vec_file) {
        (Some(q), None, None) => {
            if q.trim().is_empty() {
                anyhow::bail!("--query must be non-empty");
            }
            hash_embed(&q, dim)
        }
        (None, Some(v), None) => parse_vec_json(&v)?,
        (None, None, Some(path)) => {
            let s = std::fs::read_to_string(&path).with_context(|| format!("read {path}"))?;
            parse_vec_json(&s)?
        }
        (Some(_), Some(_), _) | (Some(_), _, Some(_)) | (None, Some(_), Some(_)) => {
            anyhow::bail!("provide only one of --query, --query-vec, or --query-vec-file")
        }
        (None, None, None) => {
            anyhow::bail!("missing query (use --query or --query-vec/--query-vec-file)")
        }
    };

    let query = SearchQuery {
        embedding: embedding.clone(),
        k,
        filters: SearchFilters { kinds },
    };

    let results = agentsdb_query::search_layers(&opened, &query).context("search")?;

    if json {
        let out = SearchJson {
            query_dim: embedding.len(),
            k,
            results: results.into_iter().map(to_search_json).collect(),
        };
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    for r in results {
        println!(
            "[{:?}] id={} score={:.6} kind={} author={:?} conf={:.3}",
            r.layer,
            r.chunk.id.get(),
            r.score,
            r.chunk.kind,
            r.chunk.author,
            r.chunk.confidence
        );
        if !r.hidden_layers.is_empty() {
            println!("  hidden_layers={:?}", r.hidden_layers);
        }
        println!("  {}", one_line(&r.chunk.content));
    }
    Ok(())
}

fn cmd_diff(base: &str, delta: &str, json: bool) -> anyhow::Result<()> {
    let base_file =
        agentsdb_format::LayerFile::open(base).with_context(|| format!("open {base}"))?;
    let delta_file =
        agentsdb_format::LayerFile::open(delta).with_context(|| format!("open {delta}"))?;
    let base_chunks = agentsdb_format::read_all_chunks(&base_file)?;
    let delta_chunks = agentsdb_format::read_all_chunks(&delta_file)?;

    let base_ids: BTreeSet<u32> = base_chunks.iter().map(|c| c.id).collect();
    let mut new_ids = Vec::new();
    let mut overrides = Vec::new();
    for c in &delta_chunks {
        if base_ids.contains(&c.id) {
            overrides.push(c.id);
        } else {
            new_ids.push(c.id);
        }
    }
    new_ids.sort_unstable();
    overrides.sort_unstable();

    if json {
        #[derive(Serialize)]
        struct Out<'a> {
            base: &'a str,
            delta: &'a str,
            delta_count: usize,
            new_ids: Vec<u32>,
            overrides: Vec<u32>,
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&Out {
                base,
                delta,
                delta_count: delta_chunks.len(),
                new_ids,
                overrides,
            })?
        );
    } else {
        println!("Delta: {delta} ({} chunks)", delta_chunks.len());
        println!("Base:  {base} ({} chunks)", base_chunks.len());
        println!();
        println!("New in delta (not in base): {}", new_ids.len());
        for id in &new_ids {
            println!("  - {id}");
        }
        println!("Overrides (id exists in base): {}", overrides.len());
        for id in &overrides {
            println!("  - {id}");
        }
    }
    Ok(())
}

fn cmd_promote(from_path: &str, to_path: &str, ids: &str, json: bool) -> anyhow::Result<()> {
    let wanted = parse_ids_csv(ids)?;
    if wanted.is_empty() {
        anyhow::bail!("--ids must be non-empty");
    }

    agentsdb_format::ensure_writable_layer_path_allow_user(to_path).context("permission check")?;

    let from_file =
        agentsdb_format::LayerFile::open(from_path).with_context(|| format!("open {from_path}"))?;
    let from_schema = agentsdb_format::schema_of(&from_file);
    let from_chunks = agentsdb_format::read_all_chunks(&from_file)?;

    let mut by_id: BTreeMap<u32, agentsdb_format::ChunkInput> = BTreeMap::new();
    for c in from_chunks {
        by_id.insert(c.id, c);
    }

    let mut promote = Vec::new();
    for id in &wanted {
        let Some(c) = by_id.get(id) else {
            anyhow::bail!("id {id} not found in {from_path}");
        };
        let mut c = c.clone();
        if c.author != "human" {
            c.author = "human".to_string();
        }
        promote.push(c);
    }

    let to_p = Path::new(to_path);
    if to_p.exists() {
        let to_file =
            agentsdb_format::LayerFile::open(to_path).with_context(|| format!("open {to_path}"))?;
        let to_schema = agentsdb_format::schema_of(&to_file);
        if to_schema.dim != from_schema.dim
            || to_schema.element_type != from_schema.element_type
            || to_schema.quant_scale.to_bits() != from_schema.quant_scale.to_bits()
        {
            anyhow::bail!("schema mismatch between {from_path} and {to_path}");
        }
        let mut promote_mut = promote.clone();
        agentsdb_format::append_layer_atomic(to_path, &mut promote_mut).context("append")?;
    } else {
        agentsdb_format::write_layer_atomic(to_path, &from_schema, &promote).context("write")?;
    }

    if json {
        #[derive(Serialize)]
        struct Out<'a> {
            ok: bool,
            from: &'a str,
            to: &'a str,
            ids: Vec<u32>,
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&Out {
                ok: true,
                from: from_path,
                to: to_path,
                ids: wanted,
            })?
        );
    } else {
        println!(
            "Promoted {} chunks from {from_path} to {to_path}",
            wanted.len()
        );
    }

    Ok(())
}

fn to_search_json(r: agentsdb_core::types::SearchResult) -> SearchResultJson {
    SearchResultJson {
        layer: layer_to_str(r.layer).to_string(),
        id: r.chunk.id.get(),
        kind: r.chunk.kind,
        score: r.score,
        author: format!("{:?}", r.chunk.author),
        confidence: r.chunk.confidence,
        created_at_unix_ms: r.chunk.created_at_unix_ms,
        sources: r.chunk.sources.into_iter().map(source_to_string).collect(),
        hidden_layers: r
            .hidden_layers
            .into_iter()
            .map(|l| layer_to_str(l).to_string())
            .collect(),
        content: r.chunk.content,
    }
}

fn layer_to_str(layer: LayerId) -> &'static str {
    match layer {
        LayerId::Base => "base",
        LayerId::User => "user",
        LayerId::Delta => "delta",
        LayerId::Local => "local",
    }
}

fn source_to_string(s: agentsdb_core::types::ProvenanceRef) -> String {
    match s {
        agentsdb_core::types::ProvenanceRef::ChunkId(id) => format!("chunk:{}", id.get()),
        agentsdb_core::types::ProvenanceRef::SourceString(v) => v,
    }
}

fn parse_vec_json(s: &str) -> anyhow::Result<Vec<f32>> {
    let v: Vec<f32> =
        serde_json::from_str(s).context("parse query vector JSON (expected [f32,...])")?;
    if v.is_empty() {
        anyhow::bail!("query vector must be non-empty");
    }
    Ok(v)
}

fn parse_ids_csv(s: &str) -> anyhow::Result<Vec<u32>> {
    let mut out = Vec::new();
    for part in s.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let id: u32 = part.parse().with_context(|| format!("parse id {part:?}"))?;
        if id == 0 {
            anyhow::bail!("ids must be non-zero");
        }
        out.push(id);
    }
    out.sort_unstable();
    out.dedup();
    Ok(out)
}

fn collect_files(root: &Path, includes: &[String]) -> anyhow::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    visit_dir(root, root, includes, &mut out)?;
    out.sort();
    Ok(out)
}

fn visit_dir(
    root: &Path,
    dir: &Path,
    includes: &[String],
    out: &mut Vec<PathBuf>,
) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(dir).with_context(|| format!("read dir {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        let ty = entry.file_type()?;
        if ty.is_dir() {
            if entry.file_name() == ".git" || entry.file_name() == "target" {
                continue;
            }
            visit_dir(root, &path, includes, out)?;
        } else if ty.is_file() {
            let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if includes.iter().any(|inc| inc == name) {
                let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
                out.push(rel);
            }
        }
    }
    Ok(())
}

fn assign_stable_id(path: &Path, content: &str, used: &mut BTreeSet<u32>) -> u32 {
    let mut h = fnv1a32(path.to_string_lossy().as_bytes());
    h ^= fnv1a32(content.as_bytes());
    let mut id = if h == 0 { 1 } else { h };
    while used.contains(&id) || id == 0 {
        id = id.wrapping_add(1);
        if id == 0 {
            id = 1;
        }
    }
    used.insert(id);
    id
}

fn fnv1a32(bytes: &[u8]) -> u32 {
    const OFFSET: u32 = 0x811c9dc5;
    const PRIME: u32 = 0x0100_0193;
    let mut h = OFFSET;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(PRIME);
    }
    h
}

fn one_line(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        if ch == '\n' || ch == '\r' {
            out.push(' ');
        } else if ch.is_control() {
            continue;
        } else {
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_accepts_short_k() {
        let cli = Cli::try_parse_from(["agentsdb", "search", "--query", "append-only", "-k", "5"])
            .expect("parse should succeed");
        match cli.cmd {
            Command::Search { k, .. } => assert_eq!(k, 5),
            _ => panic!("expected search command"),
        }
    }

    #[test]
    fn search_accepts_long_k() {
        let cli = Cli::try_parse_from(["agentsdb", "search", "--query", "append-only", "--k", "7"])
            .expect("parse should succeed");
        match cli.cmd {
            Command::Search { k, .. } => assert_eq!(k, 7),
            _ => panic!("expected search command"),
        }
    }
}
