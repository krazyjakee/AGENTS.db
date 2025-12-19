use crate::cli::{AllowlistCommand, Cli, Command, LayerArgs, OptionsCommand, ProposalsCommand};

/// Runs the main application logic based on the provided CLI arguments.
///
/// This function dispatches to the appropriate command handler based on the `cli.cmd` value.
pub(crate) fn run(cli: Cli) -> anyhow::Result<()> {
    let json = cli.json;
    match cli.cmd {
        Command::List { root } => crate::commands::list::cmd_list(&root, json),
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
            json,
        ),
        Command::Validate { path } => crate::commands::validate::cmd_validate(&path, json),
        Command::Inspect { layer, id, path } => {
            crate::commands::inspect::cmd_inspect(layer.as_deref(), path.as_deref(), id, json)
        }
        Command::Serve { layers } => {
            if json {
                anyhow::bail!("--json is not supported for serve");
            }
            agentsdb_mcp::serve_stdio(agentsdb_mcp::ServerConfig {
                base: layers.base,
                user: layers.user,
                delta: layers.delta,
                local: layers.local,
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
            json,
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
            json,
        ),
        Command::Search {
            layers,
            query,
            query_vec,
            query_vec_file,
            k,
            kinds,
            use_index,
            mode,
        } => crate::commands::search::cmd_search(
            layerset(layers),
            query,
            query_vec,
            query_vec_file,
            k,
            kinds,
            use_index,
            mode,
            json,
        ),
        Command::Index {
            layers,
            out_dir,
            store_embeddings_f32,
        } => crate::commands::index::cmd_index(
            layerset(layers),
            out_dir.as_deref(),
            store_embeddings_f32,
            json,
        ),
        Command::Export {
            dir,
            format,
            layers,
            out,
            redact,
        } => crate::commands::export::cmd_export(
            &dir,
            &format,
            &layers,
            out.as_deref(),
            &redact,
            json,
        ),
        Command::Import {
            dir,
            input,
            target,
            out,
            dry_run,
            dedupe,
            preserve_ids,
            allow_base,
            dim,
        } => crate::commands::import::cmd_import(
            &dir,
            &input,
            target.as_deref(),
            out.as_deref(),
            dry_run,
            dedupe,
            preserve_ids,
            allow_base,
            dim,
            json,
        ),
        Command::Diff {
            base,
            delta,
            target,
            user,
        } => {
            crate::commands::diff::cmd_diff(&base, &delta, target.as_deref(), user.as_deref(), json)
        }
        Command::Promote {
            from_path,
            to_path,
            ids,
            skip_existing,
            tombstone_source,
            yes,
        } => crate::commands::promote::cmd_promote(
            &from_path,
            &to_path,
            &ids,
            skip_existing,
            tombstone_source,
            yes,
            json,
        ),
        Command::Compact {
            base,
            user,
            out,
            remove_tombstones,
            remove_proposals,
        } => crate::commands::compact::cmd_compact(
            base.as_deref(),
            user.as_deref(),
            out.as_deref(),
            remove_tombstones,
            remove_proposals,
            json,
        ),
        Command::Reembed {
            dir,
            layers,
            allow_base,
        } => crate::commands::reembed::cmd_reembed(&dir, &layers, allow_base, json),
        Command::Destroy { root, dry_run } => crate::commands::destroy::cmd_destroy(&root, dry_run, json),
        Command::Web { root, bind } => {
            if json {
                anyhow::bail!("--json is not supported for web");
            }
            crate::commands::web::cmd_web(&root, &bind)
        }
        Command::Options { dir, cmd } => match cmd {
            OptionsCommand::Show { layers } => crate::commands::options::cmd_options_show(
                &dir,
                layers.base.as_deref(),
                layers.user.as_deref(),
                layers.delta.as_deref(),
                layers.local.as_deref(),
                json,
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
                json,
            ),
            OptionsCommand::Wizard { scope: _ } => {
                // Wizard always writes to base layer (AGENTS.db) only
                crate::commands::options::cmd_options_wizard(&dir, json)
            }
            OptionsCommand::Allowlist { cmd } => match cmd {
                AllowlistCommand::List { layers } => {
                    crate::commands::options::cmd_options_allowlist_list(
                        &dir,
                        layers.base.as_deref(),
                        layers.user.as_deref(),
                        layers.delta.as_deref(),
                        layers.local.as_deref(),
                        json,
                    )
                }
                AllowlistCommand::Add {
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
                    json,
                ),
                AllowlistCommand::Remove {
                    scope,
                    model,
                    revision,
                } => crate::commands::options::cmd_options_allowlist_remove(
                    &dir,
                    &scope,
                    &model,
                    revision.as_deref(),
                    json,
                ),
                AllowlistCommand::Clear { scope } => {
                    crate::commands::options::cmd_options_allowlist_clear(&dir, &scope, json)
                }
            },
        },
        Command::Proposals {
            dir,
            delta,
            user,
            proposals,
            cmd,
        } => match cmd {
            ProposalsCommand::List { all } => crate::commands::proposals::cmd_proposals_list(
                &dir,
                delta.as_deref(),
                user.as_deref(),
                proposals.as_deref(),
                all,
                json,
            ),
            ProposalsCommand::Show { id } => crate::commands::proposals::cmd_proposals_show(
                &dir,
                delta.as_deref(),
                user.as_deref(),
                proposals.as_deref(),
                id,
                json,
            ),
            ProposalsCommand::Accept {
                ids,
                skip_existing,
                yes,
            } => crate::commands::proposals::cmd_proposals_accept(
                &dir,
                delta.as_deref(),
                user.as_deref(),
                proposals.as_deref(),
                &ids,
                skip_existing,
                yes,
                json,
            ),
            ProposalsCommand::Reject { ids, reason } => {
                crate::commands::proposals::cmd_proposals_reject(
                    &dir,
                    delta.as_deref(),
                    user.as_deref(),
                    proposals.as_deref(),
                    &ids,
                    reason.as_deref(),
                    json,
                )
            }
        },
    }
}

fn layerset(layers: LayerArgs) -> agentsdb_query::LayerSet {
    // If all layers are None, auto-discover standard layer files in the current directory
    if layers.base.is_none()
        && layers.user.is_none()
        && layers.delta.is_none()
        && layers.local.is_none()
    {
        discover_standard_layers()
    } else {
        // Auto-discover AGENTS.db if base is not explicitly provided
        // This ensures embedding options are always available from the base layer
        let base = layers.base.or_else(|| {
            let path = "AGENTS.db";
            if std::path::Path::new(path).exists() {
                Some(path.to_string())
            } else {
                None
            }
        });

        agentsdb_query::LayerSet {
            base,
            user: layers.user,
            delta: layers.delta,
            local: layers.local,
        }
    }
}

fn discover_standard_layers() -> agentsdb_query::LayerSet {
    // Standard layer filenames in the current directory
    let standard_paths = [
        ("AGENTS.db", "base"),
        ("AGENTS.user.db", "user"),
        ("AGENTS.delta.db", "delta"),
        ("AGENTS.local.db", "local"),
    ];

    let mut base = None;
    let mut user = None;
    let mut delta = None;
    let mut local = None;

    for (filename, layer_type) in standard_paths {
        if std::path::Path::new(filename).exists() {
            let path_str = filename.to_string();
            match layer_type {
                "base" => base = Some(path_str),
                "user" => user = Some(path_str),
                "delta" => delta = Some(path_str),
                "local" => local = Some(path_str),
                _ => {}
            }
        }
    }

    agentsdb_query::LayerSet {
        base,
        user,
        delta,
        local,
    }
}
