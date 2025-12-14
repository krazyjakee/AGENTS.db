use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::Value;

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(prefix: &str) -> Self {
        static CTR: AtomicUsize = AtomicUsize::new(0);
        let n = CTR.fetch_add(1, Ordering::SeqCst);
        let mut path = std::env::temp_dir();
        path.push(format!("{}_{}_{}", prefix, std::process::id(), n));
        std::fs::create_dir_all(&path).expect("create temp dir");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn agentsdb() -> Command {
    Command::new(env!("CARGO_BIN_EXE_agentsdb"))
}

fn run_ok(cwd: &Path, args: &[&str]) -> Output {
    let out = agentsdb()
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("run agentsdb");
    assert!(
        out.status.success(),
        "expected success\nargs={args:?}\nstatus={}\nstdout={}\nstderr={}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    out
}

fn run_err(cwd: &Path, args: &[&str]) -> Output {
    let out = agentsdb()
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("run agentsdb");
    assert!(
        !out.status.success(),
        "expected failure\nargs={args:?}\nstatus={}\nstdout={}\nstderr={}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    out
}

fn run_ok_json(cwd: &Path, args: &[&str]) -> Value {
    let out = run_ok(cwd, args);
    serde_json::from_slice(&out.stdout).expect("stdout is valid JSON")
}

#[test]
fn help_smoke() {
    let dir = TempDir::new("agentsdb_e2e_help");
    let out = run_ok(dir.path(), &["--help"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Tools for creating, inspecting, and querying AGENTS.db layers."));
    assert!(stdout.contains("search"));
    assert!(stdout.contains("compile"));
}

#[test]
fn compile_validate_inspect_roundtrip() {
    let dir = TempDir::new("agentsdb_e2e_compile");
    let layer = dir.path().join("AGENTS.db");
    let layer_s = layer.to_string_lossy();

    run_ok(
        dir.path(),
        &["compile", "--out", &layer_s, "--text", "hello world", "--dim", "8"],
    );

    let out = run_ok(dir.path(), &["validate", &layer_s]);
    assert!(String::from_utf8_lossy(&out.stdout).contains("OK:"));

    let out = run_ok(dir.path(), &["inspect", "--layer", &layer_s]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("ChunkTable: chunk_count="));
    assert!(stdout.contains("EmbeddingMatrix: rows="));
}

#[test]
fn write_search_inspect_flow() {
    let dir = TempDir::new("agentsdb_e2e_write");
    let layer = dir.path().join("AGENTS.local.db");
    let layer_s = layer.to_string_lossy();

    run_ok(
        dir.path(),
        &[
            "write",
            &layer_s,
            "--scope",
            "local",
            "--id",
            "42",
            "--kind",
            "note",
            "--content",
            "hello world",
            "--confidence",
            "0.9",
            "--dim",
            "8",
            "--source",
            "README.md:1",
        ],
    );

    let out = run_ok(
        dir.path(),
        &[
            "search",
            "--local",
            &layer_s,
            "--query",
            "hello world",
            "-k",
            "1",
        ],
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("id=42"), "stdout={stdout}");

    let out = run_ok(
        dir.path(),
        &["inspect", "--layer", &layer_s, "--id", "42"],
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("hello world"), "stdout={stdout}");
    assert!(stdout.contains("source: README.md:1"), "stdout={stdout}");
}

#[test]
fn validate_json_reports_missing_file() {
    let dir = TempDir::new("agentsdb_e2e_validate_json");
    let missing = dir.path().join("does_not_exist.db");
    let missing_s = missing.to_string_lossy();

    let out = run_err(dir.path(), &["--json", "validate", &missing_s]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("\"ok\": false"), "stdout={stdout}");
    assert!(stdout.contains("\"error\""), "stdout={stdout}");
}

#[test]
fn list_json_includes_only_valid_layers() {
    let dir = TempDir::new("agentsdb_e2e_list_json");

    run_ok(
        dir.path(),
        &[
            "compile",
            "--out",
            "b.db",
            "--text",
            "b",
            "--dim",
            "8",
            "--replace",
        ],
    );
    run_ok(
        dir.path(),
        &[
            "compile",
            "--out",
            "a.db",
            "--text",
            "a",
            "--dim",
            "8",
            "--replace",
        ],
    );
    std::fs::write(dir.path().join("invalid.db"), b"not a layer").expect("write invalid");

    let v = run_ok_json(dir.path(), &["--json", "list", "--root", "."]);
    let arr = v.as_array().expect("list JSON is an array");
    let names: Vec<String> = arr
        .iter()
        .map(|e| e.get("path").and_then(Value::as_str).unwrap_or_default().to_string())
        .collect();
    assert_eq!(names, vec!["a.db".to_string(), "b.db".to_string()]);
}

#[test]
fn diff_and_promote_json_flow() {
    let dir = TempDir::new("agentsdb_e2e_diff_promote");

    let base_in = dir.path().join("base_in.json");
    let delta_in = dir.path().join("delta_in.json");

    std::fs::write(
        &base_in,
        r#"
{
  "schema": { "dim": 8, "element_type": "f32", "quant_scale": null },
  "chunks": [
    { "id": 1, "kind": "canonical", "content": "base one", "author": "human", "confidence": 1.0, "created_at_unix_ms": 0, "embedding": null, "sources": [] },
    { "id": 2, "kind": "note", "content": "base two", "author": "human", "confidence": 1.0, "created_at_unix_ms": 0, "embedding": null, "sources": [] }
  ]
}
"#,
    )
    .expect("write base input");
    std::fs::write(
        &delta_in,
        r#"
{
  "schema": { "dim": 8, "element_type": "f32", "quant_scale": null },
  "chunks": [
    { "id": 2, "kind": "note", "content": "delta overrides two", "author": "mcp", "confidence": 0.5, "created_at_unix_ms": 0, "embedding": null, "sources": [] },
    { "id": 3, "kind": "note", "content": "delta new three", "author": "mcp", "confidence": 0.5, "created_at_unix_ms": 0, "embedding": null, "sources": [] }
  ]
}
"#,
    )
    .expect("write delta input");

    run_ok(
        dir.path(),
        &[
            "compile",
            "--in",
            base_in.to_str().unwrap(),
            "--out",
            "AGENTS.base.db",
            "--replace",
        ],
    );
    run_ok(
        dir.path(),
        &[
            "compile",
            "--in",
            delta_in.to_str().unwrap(),
            "--out",
            "AGENTS.delta.db",
            "--replace",
        ],
    );

    let diff = run_ok_json(
        dir.path(),
        &[
            "--json",
            "diff",
            "--base",
            "AGENTS.base.db",
            "--delta",
            "AGENTS.delta.db",
        ],
    );
    assert_eq!(diff["delta_count"].as_u64(), Some(2));
    assert_eq!(diff["new_ids"].as_array().unwrap().len(), 1);
    assert_eq!(diff["new_ids"][0].as_u64(), Some(3));
    assert_eq!(diff["overrides"][0].as_u64(), Some(2));

    run_ok(
        dir.path(),
        &[
            "promote",
            "--from",
            "AGENTS.delta.db",
            "--to",
            "AGENTS.user.db",
            "--ids",
            "2,3",
        ],
    );

    let c2 = run_ok_json(
        dir.path(),
        &["--json", "inspect", "--layer", "AGENTS.user.db", "--id", "2"],
    );
    assert_eq!(c2["id"].as_u64(), Some(2));
    assert_eq!(c2["author"].as_str(), Some("human"));

    let c3 = run_ok_json(
        dir.path(),
        &["--json", "inspect", "--layer", "AGENTS.user.db", "--id", "3"],
    );
    assert_eq!(c3["id"].as_u64(), Some(3));
    assert_eq!(c3["author"].as_str(), Some("human"));
}

#[test]
fn compact_json_writes_expected_chunk_count() {
    let dir = TempDir::new("agentsdb_e2e_compact_json");

    let base_in = dir.path().join("base_in.json");
    let user_in = dir.path().join("user_in.json");

    std::fs::write(
        &base_in,
        r#"
{
  "schema": { "dim": 8, "element_type": "f32", "quant_scale": null },
  "chunks": [
    { "id": 1, "kind": "canonical", "content": "shared", "author": "human", "confidence": 1.0, "created_at_unix_ms": 0, "embedding": null, "sources": [] }
  ]
}
"#,
    )
    .expect("write base input");
    std::fs::write(
        &user_in,
        r#"
{
  "schema": { "dim": 8, "element_type": "f32", "quant_scale": null },
  "chunks": [
    { "id": 1, "kind": "canonical", "content": "shared", "author": "human", "confidence": 1.0, "created_at_unix_ms": 0, "embedding": null, "sources": [] },
    { "id": 2, "kind": "note", "content": "user extra", "author": "human", "confidence": 1.0, "created_at_unix_ms": 0, "embedding": null, "sources": [] }
  ]
}
"#,
    )
    .expect("write user input");

    run_ok(
        dir.path(),
        &[
            "compile",
            "--in",
            base_in.to_str().unwrap(),
            "--out",
            "AGENTS.db",
            "--replace",
        ],
    );
    run_ok(
        dir.path(),
        &[
            "compile",
            "--in",
            user_in.to_str().unwrap(),
            "--out",
            "AGENTS.user.db",
            "--replace",
        ],
    );

    let out = run_ok_json(
        dir.path(),
        &[
            "--json",
            "compact",
            "--base",
            "AGENTS.db",
            "--user",
            "AGENTS.user.db",
            "--out",
            "AGENTS.compacted.db",
        ],
    );
    assert_eq!(out["chunks"].as_u64(), Some(2));

    let inspect = run_ok_json(
        dir.path(),
        &["--json", "inspect", "--layer", "AGENTS.compacted.db"],
    );
    assert_eq!(inspect["chunk_count"].as_u64(), Some(2));
}

#[test]
fn clean_json_dry_run_and_delete() {
    let dir = TempDir::new("agentsdb_e2e_clean_json");
    std::fs::create_dir_all(dir.path().join("nested")).expect("create nested");

    std::fs::write(dir.path().join("AGENTS.db"), b"x").expect("write AGENTS.db");
    std::fs::write(dir.path().join("AGENTS.base.db"), b"x").expect("write AGENTS.base.db");
    std::fs::write(dir.path().join("nested").join("AGENTS.local.db"), b"x")
        .expect("write AGENTS.local.db");
    std::fs::write(dir.path().join("nested").join("AGENTS.db.sig"), b"x").expect("write sig");

    let dry = run_ok_json(
        dir.path(),
        &[
            "--json",
            "clean",
            "--root",
            ".",
            "--dry-run",
        ],
    );
    let paths = dry["paths"].as_array().unwrap();
    assert!(
        paths.iter().any(|p| p.as_str() == Some("AGENTS.db")),
        "paths={paths:?}"
    );
    assert!(
        paths.iter().any(|p| p.as_str() == Some("AGENTS.base.db")),
        "paths={paths:?}"
    );
    assert!(
        paths
            .iter()
            .any(|p| p.as_str() == Some("nested/AGENTS.local.db")),
        "paths={paths:?}"
    );
    assert!(dir.path().join("AGENTS.db").exists());

    run_ok(dir.path(), &["clean", "--root", "."]);
    assert!(!dir.path().join("AGENTS.db").exists());
    assert!(!dir.path().join("AGENTS.base.db").exists());
    assert!(!dir.path().join("nested").join("AGENTS.local.db").exists());
    assert!(dir.path().join("nested").join("AGENTS.db.sig").exists());
}
