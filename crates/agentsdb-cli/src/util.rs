use anyhow::Context;
use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};

use agentsdb_core::types::LayerId;

pub(crate) fn layer_to_str(layer: LayerId) -> &'static str {
    match layer {
        LayerId::Base => "base",
        LayerId::User => "user",
        LayerId::Delta => "delta",
        LayerId::Local => "local",
    }
}

pub(crate) fn source_to_string(s: agentsdb_core::types::ProvenanceRef) -> String {
    match s {
        agentsdb_core::types::ProvenanceRef::ChunkId(id) => format!("chunk:{}", id.get()),
        agentsdb_core::types::ProvenanceRef::SourceString(v) => v,
    }
}

pub(crate) fn parse_vec_json(s: &str) -> anyhow::Result<Vec<f32>> {
    let v: Vec<f32> =
        serde_json::from_str(s).context("parse query vector JSON (expected [f32,...])")?;
    if v.is_empty() {
        anyhow::bail!("query vector must be non-empty");
    }
    Ok(v)
}

pub(crate) fn parse_ids_csv(s: &str) -> anyhow::Result<Vec<u32>> {
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

pub(crate) fn collect_files(root: &Path, includes: &[String]) -> anyhow::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    visit_dir(root, root, includes, &mut out)?;
    out.sort();
    Ok(out)
}

pub(crate) fn collect_files_wide_docs(root: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    visit_dir_wide_docs(root, root, &mut out)?;
    out.sort();
    out.dedup();
    Ok(out)
}

fn visit_dir_wide_docs(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(dir).with_context(|| format!("read dir {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        let ty = entry.file_type()?;
        if ty.is_dir() {
            if should_skip_init_dir(&entry.file_name()) {
                continue;
            }
            visit_dir_wide_docs(root, &path, out)?;
        } else if ty.is_file() && is_doc_candidate(&path) {
            let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
            out.push(rel);
        }
    }
    Ok(())
}

fn should_skip_init_dir(name: &OsStr) -> bool {
    let name = name.to_string_lossy();
    matches!(
        name.as_ref(),
        ".git"
            | "target"
            | "node_modules"
            | "dist"
            | "build"
            | "vendor"
            | ".next"
            | ".turbo"
            | ".cache"
            | "coverage"
            | ".venv"
            | "venv"
    )
}

fn is_doc_candidate(path: &Path) -> bool {
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    let name_lc = name.to_ascii_lowercase();

    if matches!(
        name_lc.as_str(),
        "license" | "copying" | "copyright" | "notice" | "authors" | "maintainers" | "contributors"
    ) {
        return true;
    }

    if name_lc.starts_with("readme") {
        return true;
    }

    if matches!(
        name_lc.as_str(),
        "agents.md"
            | "contributing.md"
            | "code_of_conduct.md"
            | "security.md"
            | "changelog.md"
            | "workflow.md"
    ) {
        return true;
    }

    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(ext.as_str(), "md" | "mdx" | "rst" | "txt" | "adoc" | "org")
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

pub(crate) fn assign_stable_id(path: &Path, content: &str, used: &mut BTreeSet<u32>) -> u32 {
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

pub(crate) fn one_line(s: &str) -> String {
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

pub(crate) fn fmt_u64_commas(mut v: u64) -> String {
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

pub(crate) fn fmt_bytes_human(bytes: u64) -> String {
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
pub(crate) fn make_temp_dir() -> PathBuf {
    static CTR: AtomicUsize = AtomicUsize::new(0);
    let n = CTR.fetch_add(1, Ordering::SeqCst);
    let mut p = std::env::temp_dir();
    p.push(format!("agentsdb_cli_test_{}_{}", std::process::id(), n));
    std::fs::create_dir_all(&p).expect("create temp dir");
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_collects_common_doc_extensions() {
        let root = make_temp_dir();
        std::fs::create_dir_all(root.join("docs")).expect("create docs");
        std::fs::create_dir_all(root.join("src")).expect("create src");
        std::fs::create_dir_all(root.join("target")).expect("create target");

        std::fs::write(root.join("README.md"), "# hi\n").expect("write readme");
        std::fs::write(root.join("docs").join("design.md"), "design\n").expect("write docs md");
        std::fs::write(root.join("src").join("notes.txt"), "notes\n").expect("write txt");
        std::fs::write(root.join("target").join("ignored.md"), "nope\n").expect("write ignored");

        let files = collect_files_wide_docs(&root).expect("collect should succeed");
        let rendered: Vec<String> = files
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();

        assert!(rendered.contains(&"README.md".to_string()));
        assert!(rendered.contains(&format!("docs{}design.md", std::path::MAIN_SEPARATOR)));
        assert!(rendered.contains(&format!("src{}notes.txt", std::path::MAIN_SEPARATOR)));
        assert!(!rendered.contains(&format!("target{}ignored.md", std::path::MAIN_SEPARATOR)));

        std::fs::remove_dir_all(&root).expect("cleanup");
    }
}
