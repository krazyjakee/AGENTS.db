use crate::types::ValidateJson;
use anyhow::Context;
use std::path::Path;

use agentsdb_embeddings::config::{
    roll_up_embedding_options_from_paths, standard_layer_paths_for_dir,
};

/// Validates a single layer file for format correctness and optionally checks embedding alignment.
fn validate_single_file(
    path: &str,
    check_options: bool,
    dir_for_options: Option<&Path>,
) -> anyhow::Result<ValidateJson> {
    let file_result = agentsdb_format::LayerFile::open(path);

    let mut warnings = Vec::new();
    let mut embedding_mismatch = None;

    match &file_result {
        Ok(file) if check_options => {
            // Check if embedding dimensions align with options
            if let Some(dir) = dir_for_options {
                let schema = agentsdb_format::schema_of(file);
                let paths = standard_layer_paths_for_dir(dir);

                match roll_up_embedding_options_from_paths(
                    Some(paths.local.as_path()),
                    Some(paths.user.as_path()),
                    Some(paths.delta.as_path()),
                    Some(paths.base.as_path()),
                ) {
                    Ok(resolved) => {
                        if let Some(options_dim) = resolved.dim {
                            if schema.dim != options_dim as u32 {
                                let msg = format!(
                                    "embedding dimension mismatch: file schema has dim={}, but resolved options specify dim={}",
                                    schema.dim, options_dim
                                );
                                warnings.push(msg.clone());
                                embedding_mismatch = Some((schema.dim, options_dim as u32));
                            }
                        }
                    }
                    Err(e) => {
                        warnings.push(format!("failed to resolve options: {}", e));
                    }
                }
            }
        }
        _ => {}
    }

    let (ok, error) = match &file_result {
        Ok(_) => (true, None),
        Err(e) => (false, Some(e.to_string())),
    };

    Ok(ValidateJson {
        ok,
        path: path.to_string(),
        error,
        warnings: if warnings.is_empty() {
            None
        } else {
            Some(warnings)
        },
        schema_dim: file_result
            .as_ref()
            .ok()
            .map(|f| agentsdb_format::schema_of(f).dim),
        options_dim: embedding_mismatch.map(|(_, opts)| opts),
    })
}

/// Validates all standard layer files in a directory and checks embedding alignment.
fn validate_directory(dir: &Path, json: bool) -> anyhow::Result<()> {
    let paths = standard_layer_paths_for_dir(dir);

    // Resolve options once for the entire directory
    let resolved_options = roll_up_embedding_options_from_paths(
        Some(paths.local.as_path()),
        Some(paths.user.as_path()),
        Some(paths.delta.as_path()),
        Some(paths.base.as_path()),
    )
    .context("roll up options")?;

    let expected_dim = resolved_options.dim;

    // Validate each layer
    let layers = [
        ("base", &paths.base),
        ("user", &paths.user),
        ("delta", &paths.delta),
        ("local", &paths.local),
    ];

    let mut results: Vec<(&str, ValidateJson)> = Vec::new();
    let mut has_error = false;
    let mut has_warning = false;

    for (layer_name, layer_path) in &layers {
        if !layer_path.exists() {
            continue;
        }

        let path_str = layer_path.display().to_string();
        let result = validate_single_file(&path_str, true, Some(dir))?;

        if !result.ok {
            has_error = true;
        }
        if result.warnings.is_some() {
            has_warning = true;
        }

        results.push((*layer_name, result));
    }

    if json {
        #[derive(serde::Serialize)]
        struct DirectoryValidateJson {
            ok: bool,
            dir: String,
            expected_dim: Option<usize>,
            layers: Vec<LayerValidation>,
        }

        #[derive(serde::Serialize)]
        struct LayerValidation {
            layer: String,
            path: String,
            ok: bool,
            #[serde(skip_serializing_if = "Option::is_none")]
            error: Option<String>,
            #[serde(skip_serializing_if = "Option::is_none")]
            warnings: Option<Vec<String>>,
            #[serde(skip_serializing_if = "Option::is_none")]
            schema_dim: Option<u32>,
            #[serde(skip_serializing_if = "Option::is_none")]
            options_dim: Option<u32>,
        }

        let layer_validations: Vec<_> = results
            .iter()
            .map(|(name, result)| LayerValidation {
                layer: name.to_string(),
                path: result.path.clone(),
                ok: result.ok,
                error: result.error.clone(),
                warnings: result.warnings.clone(),
                schema_dim: result.schema_dim,
                options_dim: result.options_dim,
            })
            .collect();

        let out = DirectoryValidateJson {
            ok: !has_error && !has_warning,
            dir: dir.display().to_string(),
            expected_dim,
            layers: layer_validations,
        };

        println!("{}", serde_json::to_string_pretty(&out)?);

        if has_error || has_warning {
            std::process::exit(1);
        }
    } else {
        println!("Validating layers in directory: {}", dir.display());
        if let Some(dim) = expected_dim {
            println!("Expected embedding dimension from options: {}", dim);
        } else {
            println!("No embedding dimension configured in options");
        }
        println!();

        for (layer_name, result) in &results {
            if result.ok {
                if let Some(warnings) = &result.warnings {
                    println!("⚠  {}: {} (with warnings)", layer_name, result.path);
                    for warning in warnings {
                        println!("   WARNING: {}", warning);
                    }
                } else {
                    println!("✓  {}: {}", layer_name, result.path);
                    if let Some(dim) = result.schema_dim {
                        println!("   schema dim={}", dim);
                    }
                }
            } else if let Some(error) = &result.error {
                println!("✗  {}: {} (ERROR)", layer_name, result.path);
                println!("   {}", error);
                has_error = true;
            }
        }

        println!();
        if has_error {
            anyhow::bail!("Validation failed with errors");
        } else if has_warning {
            anyhow::bail!("Validation completed with warnings");
        } else {
            println!("All layers valid and aligned with options");
        }
    }

    Ok(())
}

pub(crate) fn cmd_validate(path: &str, json: bool) -> anyhow::Result<()> {
    // Implements the `validate` command, which validates that a layer file is readable and well-formed.
    // If the path is a directory, validates all standard layer files and checks embedding alignment.
    // If the path is a file, validates that single file.

    let path_obj = Path::new(path);

    if path_obj.is_dir() {
        // Directory mode: validate all layers and check embedding alignment
        validate_directory(path_obj, json)
    } else {
        // Single file mode: validate the file format
        let parent_dir = path_obj.parent();
        let result = validate_single_file(path, true, parent_dir)?;

        if json {
            println!("{}", serde_json::to_string_pretty(&result)?);
            if !result.ok || result.warnings.is_some() {
                std::process::exit(1);
            }
        } else {
            if result.ok {
                if let Some(warnings) = &result.warnings {
                    println!("OK: {} (with warnings)", path);
                    for warning in warnings {
                        println!("  WARNING: {}", warning);
                    }
                    std::process::exit(1);
                } else {
                    println!("OK: {}", path);
                    if let Some(dim) = result.schema_dim {
                        println!("  schema dim={}", dim);
                    }
                }
            } else if let Some(error) = &result.error {
                anyhow::bail!("INVALID: {}: {}", path, error);
            }
        }

        Ok(())
    }
}
