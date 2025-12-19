use crate::types::DestroyJson;
use anyhow::Context;
use std::path::{Path, PathBuf};

pub(crate) fn cmd_destroy(root: &str, dry_run: bool, json: bool) -> anyhow::Result<()> {
    let root_path = Path::new(root);
    let mut matches = Vec::new();
    visit_dir(root_path, root_path, &mut matches)?;
    matches.sort();
    matches.dedup();

    let mut rendered = Vec::with_capacity(matches.len());
    for rel in &matches {
        rendered.push(rel.to_string_lossy().to_string());
    }

    if json {
        if !dry_run {
            for rel in &matches {
                let abs = root_path.join(rel);
                std::fs::remove_file(&abs)
                    .with_context(|| format!("remove file {}", abs.display()))?;
            }
        }
        let out = DestroyJson {
            root,
            dry_run,
            paths: rendered,
        };
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    if matches.is_empty() {
        println!("No AGENTS*.db files found under {}", root_path.display());
        return Ok(());
    }

    if dry_run {
        for rel in matches {
            println!("Would remove: {}", root_path.join(rel).display());
        }
        return Ok(());
    }

    for rel in &matches {
        let abs = root_path.join(rel);
        std::fs::remove_file(&abs).with_context(|| format!("remove file {}", abs.display()))?;
        println!("Removed: {}", abs.display());
    }
    println!("Removed {} file(s).", matches.len());
    Ok(())
}

fn visit_dir(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(dir).with_context(|| format!("read dir {}", dir.display()))? {
        let entry = entry?;
        let ty = entry.file_type().context("read file type")?;
        if ty.is_symlink() {
            continue;
        }

        let path = entry.path();
        if ty.is_dir() {
            if entry.file_name() == ".git" || entry.file_name() == "target" {
                continue;
            }
            visit_dir(root, &path, out)?;
            continue;
        }
        if !ty.is_file() {
            continue;
        }

        if !is_agents_db_file(&path) {
            continue;
        }

        let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
        out.push(rel);
    }
    Ok(())
}

fn is_agents_db_file(path: &Path) -> bool {
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    if name == "AGENTS.db" {
        return true;
    }
    name.starts_with("AGENTS.") && name.ends_with(".db")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn destroy_removes_agents_db_files_recursively() {
        let root = crate::util::make_temp_dir();
        std::fs::create_dir_all(root.join("nested")).expect("create nested");

        std::fs::write(root.join("AGENTS.db"), "x").expect("write AGENTS.db");
        std::fs::write(root.join("AGENTS.base.db"), "x").expect("write AGENTS.base.db");
        std::fs::write(root.join("nested").join("AGENTS.local.db"), "x")
            .expect("write AGENTS.local.db");
        std::fs::write(root.join("nested").join("AGENTS.db.sig"), "x").expect("write sig");
        std::fs::write(root.join("nested").join("notes.txt"), "x").expect("write notes");

        cmd_destroy(root.to_str().unwrap(), false, false).expect("destroy should succeed");

        assert!(!root.join("AGENTS.db").exists());
        assert!(!root.join("AGENTS.base.db").exists());
        assert!(!root.join("nested").join("AGENTS.local.db").exists());
        assert!(root.join("nested").join("AGENTS.db.sig").exists());
        assert!(root.join("nested").join("notes.txt").exists());

        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn destroy_dry_run_does_not_delete() {
        let root = crate::util::make_temp_dir();
        std::fs::write(root.join("AGENTS.db"), "x").expect("write AGENTS.db");

        cmd_destroy(root.to_str().unwrap(), true, false).expect("dry-run should succeed");
        assert!(root.join("AGENTS.db").exists());

        std::fs::remove_dir_all(&root).expect("cleanup");
    }
}
