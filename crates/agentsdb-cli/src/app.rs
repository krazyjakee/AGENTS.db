use crate::cli::{Cli, Command, OptionsCommand};

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
            replace,
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
            replace,
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
        Command::Options { dir, cmd } => match cmd {
            OptionsCommand::Show {
                base,
                user,
                delta,
                local,
            } => crate::commands::options::cmd_options_show(
                &dir,
                base.as_deref(),
                user.as_deref(),
                delta.as_deref(),
                local.as_deref(),
                cli.json,
            ),
            OptionsCommand::Set {
                scope,
                backend,
                model,
                revision,
                model_path,
                model_sha256,
                dim,
                api_base,
                api_key_env,
                cache,
                cache_dir,
            } => crate::commands::options::cmd_options_set(
                &dir,
                &scope,
                backend.as_deref(),
                model.as_deref(),
                revision.as_deref(),
                model_path.as_deref(),
                model_sha256.as_deref(),
                dim,
                api_base.as_deref(),
                api_key_env.as_deref(),
                cache.map(|t| matches!(t, crate::cli::Toggle::On)),
                cache_dir.as_deref(),
                cli.json,
            ),
            OptionsCommand::Wizard { scope } => {
                crate::commands::options::cmd_options_wizard(&dir, &scope, cli.json)
            }
            OptionsCommand::Allowlist { cmd } => match cmd {
                crate::cli::AllowlistCommand::List {
                    base,
                    user,
                    delta,
                    local,
                } => crate::commands::options::cmd_options_allowlist_list(
                    &dir,
                    base.as_deref(),
                    user.as_deref(),
                    delta.as_deref(),
                    local.as_deref(),
                    cli.json,
                ),
                crate::cli::AllowlistCommand::Add {
                    scope,
                    model,
                    revision,
                    sha256,
                } => crate::commands::options::cmd_options_allowlist_add(
                    &dir,
                    &scope,
                    &model,
                    revision.as_deref(),
                    &sha256,
                    cli.json,
                ),
                crate::cli::AllowlistCommand::Remove {
                    scope,
                    model,
                    revision,
                } => crate::commands::options::cmd_options_allowlist_remove(
                    &dir,
                    &scope,
                    &model,
                    revision.as_deref(),
                    cli.json,
                ),
                crate::cli::AllowlistCommand::Clear { scope } => {
                    crate::commands::options::cmd_options_allowlist_clear(&dir, &scope, cli.json)
                }
            },
        },
    }
}
