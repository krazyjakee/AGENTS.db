use crate::cli::{Cli, Command};

pub(crate) fn run(cli: Cli) -> anyhow::Result<()> {
    match cli.cmd {
        Command::List { root } => crate::commands::list::cmd_list(&root, cli.json),
        Command::Init {
            root,
            out,
            kind,
            dim,
            element_type,
            quant_scale,
        } => crate::commands::init::cmd_init(
            &root,
            &out,
            &kind,
            dim,
            &element_type,
            quant_scale,
            cli.json,
        ),
        Command::Validate { path } => crate::commands::validate::cmd_validate(&path, cli.json),
        Command::Inspect { layer, id, path } => {
            crate::commands::inspect::cmd_inspect(layer.as_deref(), path.as_deref(), id, cli.json)
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
        Command::Compile {
            input,
            out,
            root,
            includes,
            paths,
            texts,
            kind,
            dim,
            element_type,
            quant_scale,
        } => crate::commands::compile::cmd_compile(
            input.as_deref(),
            &out,
            &root,
            &includes,
            &paths,
            &texts,
            &kind,
            dim,
            &element_type,
            quant_scale,
            cli.json,
        ),
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
        } => crate::commands::write::cmd_write(
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
        } => crate::commands::search::cmd_search(
            agentsdb_query::LayerSet {
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
        Command::Diff { base, delta } => crate::commands::diff::cmd_diff(&base, &delta, cli.json),
        Command::Promote {
            from_path,
            to_path,
            ids,
        } => crate::commands::promote::cmd_promote(&from_path, &to_path, &ids, cli.json),
        Command::Compact { base, user, out } => crate::commands::compact::cmd_compact(
            base.as_deref(),
            user.as_deref(),
            out.as_deref(),
            cli.json,
        ),
        Command::Clean { root, dry_run } => {
            crate::commands::clean::cmd_clean(&root, dry_run, cli.json)
        }
        Command::Web { root, bind } => {
            if cli.json {
                anyhow::bail!("--json is not supported for web");
            }
            crate::commands::web::cmd_web(&root, &bind)
        }
    }
}
