use anyhow::Context;
use serde::Serialize;

use crate::types::{EmbeddingJson, HeaderJson, InspectJson, SectionJson};

pub(crate) fn cmd_inspect(
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
