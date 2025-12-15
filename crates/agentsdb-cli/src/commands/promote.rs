use serde::Serialize;
use std::io::IsTerminal;

use crate::util::parse_ids_csv;

pub(crate) fn cmd_promote(
    from_path: &str,
    to_path: &str,
    ids: &str,
    skip_existing: bool,
    tombstone_source: bool,
    yes: bool,
    json: bool,
) -> anyhow::Result<()> {
    let wanted = parse_ids_csv(ids)?;
    if wanted.is_empty() {
        anyhow::bail!("--ids must be non-empty");
    }

    // Prompt for confirmation if writing to user layer and not in non-interactive mode
    if !yes
        && !json
        && std::path::Path::new(to_path)
            .file_name()
            .and_then(|s| s.to_str())
            == Some("AGENTS.user.db")
        && std::io::stdin().is_terminal()
    {
        eprint!(
            "Promote {} chunks into {to_path}? This is a durable, append-only layer. [y/N] ",
            wanted.len()
        );
        use std::io::Write;
        std::io::stderr().flush().ok();
        let mut s = String::new();
        std::io::stdin().read_line(&mut s).ok();
        let s = s.trim().to_ascii_lowercase();
        if s != "y" && s != "yes" {
            anyhow::bail!("aborted");
        }
    }

    // Use shared promote operation
    let out = agentsdb_ops::promote::promote_chunks(
        from_path,
        to_path,
        &wanted,
        skip_existing,
        tombstone_source,
    )?;

    if json {
        #[derive(Serialize)]
        struct Out<'a> {
            ok: bool,
            from: &'a str,
            to: &'a str,
            promoted: Vec<u32>,
            #[serde(skip_serializing_if = "Vec::is_empty")]
            skipped: Vec<u32>,
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&Out {
                ok: true,
                from: from_path,
                to: to_path,
                promoted: out.promoted,
                skipped: out.skipped,
            })?
        );
    } else {
        if out.promoted.is_empty() {
            println!("No chunks to promote (all requested ids already exist in {to_path})");
        } else {
            println!(
                "Promoted {} chunks from {from_path} to {to_path}",
                out.promoted.len()
            );
        }
        if !out.skipped.is_empty() {
            println!(
                "Skipped {} ids already present in destination",
                out.skipped.len()
            );
        }
    }

    Ok(())
}
