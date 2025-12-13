use anyhow::Context;
use std::path::Path;

use crate::types::ListEntryJson;

#[derive(Debug, Clone, PartialEq, Eq)]
struct ListedLayer {
    file_name: String,
    chunk_count: u64,
    file_length_bytes: u64,
}

pub(crate) fn cmd_list(root: &str, json: bool) -> anyhow::Result<()> {
    let layers = list_layers_in_dir(Path::new(root))?;
    if json {
        let out: Vec<ListEntryJson> = layers
            .into_iter()
            .map(|l| ListEntryJson {
                path: l.file_name,
                chunk_count: l.chunk_count,
                file_length_bytes: l.file_length_bytes,
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    if layers.is_empty() {
        println!("No valid .db files found.");
        return Ok(());
    }

    print_table(&layers);
    Ok(())
}

fn list_layers_in_dir(dir: &Path) -> anyhow::Result<Vec<ListedLayer>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).with_context(|| format!("read dir {}", dir.display()))? {
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
                file_name,
                chunk_count: f.chunk_count,
                file_length_bytes: f.header.file_length_bytes,
            }),
            Err(_) => continue,
        }
    }
    out.sort_by(|a, b| a.file_name.cmp(&b.file_name));
    Ok(out)
}

fn print_table(layers: &[ListedLayer]) {
    let file_header = "File";
    let docs_header = "Docs";
    let size_header = "Size";

    let mut file_w = file_header.len();
    let mut docs_w = docs_header.len();
    let mut size_w = size_header.len();

    let docs_fmt: Vec<String> = layers
        .iter()
        .map(|l| fmt_u64_commas(l.chunk_count))
        .collect();
    let size_fmt: Vec<String> = layers
        .iter()
        .map(|l| fmt_bytes_human(l.file_length_bytes))
        .collect();

    for (idx, l) in layers.iter().enumerate() {
        file_w = file_w.max(l.file_name.len());
        docs_w = docs_w.max(docs_fmt[idx].len());
        size_w = size_w.max(size_fmt[idx].len());
    }

    println!(
        "{file:<file_w$}  {docs:>docs_w$}  {size:>size_w$}",
        file = file_header,
        docs = docs_header,
        size = size_header
    );
    println!("{:-<file_w$}  {:-<docs_w$}  {:-<size_w$}", "", "", "");

    for (idx, l) in layers.iter().enumerate() {
        println!(
            "{file:<file_w$}  {docs:>docs_w$}  {size:>size_w$}",
            file = l.file_name,
            docs = docs_fmt[idx],
            size = size_fmt[idx]
        );
    }
}

fn fmt_u64_commas(mut v: u64) -> String {
    if v == 0 {
        return "0".to_string();
    }
    let mut parts = Vec::new();
    while v > 0 {
        parts.push((v % 1000) as u16);
        v /= 1000;
    }
    let mut out = String::new();
    for (i, part) in parts.iter().rev().enumerate() {
        if i == 0 {
            out.push_str(&part.to_string());
        } else {
            out.push_str(&format!(",{:03}", part));
        }
    }
    out
}

fn fmt_bytes_human(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        return format!("{bytes} B");
    }
    if value >= 10.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentsdb_format::{ChunkInput, EmbeddingElementType};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn make_temp_dir() -> PathBuf {
        static CTR: AtomicUsize = AtomicUsize::new(0);
        let n = CTR.fetch_add(1, Ordering::SeqCst);
        let mut p = std::env::temp_dir();
        p.push(format!(
            "agentsdb_cli_list_test_{}_{}",
            std::process::id(),
            n
        ));
        std::fs::create_dir_all(&p).expect("create temp dir");
        p
    }

    fn write_layer(path: &Path, chunk_count: u32) {
        let schema = agentsdb_format::LayerSchema {
            dim: 4,
            element_type: EmbeddingElementType::F32,
            quant_scale: 0.0,
        };
        let chunks: Vec<ChunkInput> = (0..chunk_count)
            .map(|i| ChunkInput {
                id: i + 1,
                kind: "canonical".to_string(),
                content: format!("doc {i}"),
                author: "human".to_string(),
                confidence: 1.0,
                created_at_unix_ms: 0,
                embedding: vec![0.0, 0.0, 0.0, 0.0],
                sources: Vec::new(),
            })
            .collect();
        agentsdb_format::write_layer_atomic(path, &schema, &chunks).expect("write layer");
    }

    #[test]
    fn list_layers_filters_and_sorts() {
        let root = make_temp_dir();
        write_layer(&root.join("b.db"), 2);
        write_layer(&root.join("a.db"), 1);
        std::fs::write(root.join("invalid.db"), b"not a layer").expect("write invalid");
        std::fs::write(root.join("notes.txt"), b"ignore").expect("write txt");

        let got = list_layers_in_dir(&root).expect("list should succeed");
        let names: Vec<String> = got.iter().map(|l| l.file_name.clone()).collect();
        assert_eq!(names, vec!["a.db".to_string(), "b.db".to_string()]);
        assert_eq!(got[0].chunk_count, 1);
        assert_eq!(got[1].chunk_count, 2);

        std::fs::remove_dir_all(&root).expect("cleanup");
    }
}
