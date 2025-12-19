use clap::{Args, Parser, Subcommand};

#[derive(Args, Clone, Debug, Default)]
/// Arguments for specifying various AGENTS.db layers.
pub(crate) struct LayerArgs {
    #[arg(long)]
    pub(crate) base: Option<String>,
    #[arg(long)]
    pub(crate) user: Option<String>,
    #[arg(long)]
    pub(crate) delta: Option<String>,
    #[arg(long)]
    pub(crate) local: Option<String>,
}

#[derive(Parser)]
#[command(
    name = "agentsdb",
    version,
    long_about = "Tools for creating, inspecting, and querying AGENTS.db layers.\n\nNotes:\n  - Layers are treated as append-only. Writes append new chunks.\n  - Embedding backends are configured via rolled-up options (default: deterministic hash)."
)]
/// Main command-line interface structure for the agentsdb tool.
///
/// This struct uses `clap` to parse command-line arguments and subcommands.
pub(crate) struct Cli {
    /// Emit machine-readable JSON instead of human output.
    #[arg(long)]
    pub(crate) json: bool,

    #[command(subcommand)]
    pub(crate) cmd: Command,
}

#[derive(Subcommand)]
pub(crate) enum Command {
    /// List readable `.db` layer files in a directory.
    List {
        /// Root directory to scan for `.db` files.
        #[arg(long, default_value = ".")]
        root: String,
    },
    /// Collect common documentation sources and compile an AGENTS.db layer (no manifest left behind).
    Init {
        /// Root directory to scan for documentation.
        #[arg(long, default_value = ".")]
        root: String,
        /// Output layer path to write.
        #[arg(long, default_value = "AGENTS.db")]
        out: String,
        /// Chunk kind to assign to collected files.
        #[arg(long, default_value = "canonical")]
        kind: String,
        /// Embedding dimension for the emitted schema (defaults to configured options if present, else 128).
        #[arg(long)]
        dim: Option<u32>,
        /// Embedding element type: `f32` or `i8`.
        #[arg(long, default_value = "f32")]
        element_type: String,
        /// Quantization scale (only used when `--element-type i8`).
        #[arg(long)]
        quant_scale: Option<f32>,
    },
    /// Validate that a layer file is readable and well-formed.
    Validate {
        /// Layer path (e.g. `AGENTS.base.db`).
        path: String,
    },
    /// Inspect a layer file header/sections, or print a chunk by id.
    Inspect {
        /// Layer path to inspect (alternative to providing PATH).
        #[arg(long)]
        layer: Option<String>,
        /// Chunk id to print (prints layer metadata if omitted).
        #[arg(long)]
        id: Option<u32>,
        /// Layer path to inspect (positional alternative to `--layer`).
        #[arg(value_name = "PATH")]
        path: Option<String>,
    },
    /// Run the MCP server over stdio.
    Serve {
        #[command(flatten)]
        layers: LayerArgs,
    },
    /// Compile text and/or files into an on-disk layer file.
    Compile {
        /// Optional input JSON path (legacy; previously produced by `collect`).
        #[arg(long = "in")]
        input: Option<String>,
        /// Output layer path to write.
        #[arg(long)]
        out: String,
        /// Replace the output file instead of appending.
        #[arg(long)]
        replace: bool,
        /// Root directory to search for files when no PATHs are provided.
        #[arg(long, default_value = ".")]
        root: String,
        /// File names to include (repeatable) when no PATHs are provided.
        #[arg(long = "include", default_value = "AGENTS.md")]
        includes: Vec<String>,
        /// File paths to include (repeatable positional args).
        #[arg(value_name = "PATH")]
        paths: Vec<String>,
        /// Inline text chunks to include (repeatable).
        #[arg(long = "text")]
        texts: Vec<String>,
        /// Chunk kind to assign to generated chunks.
        #[arg(long, default_value = "canonical")]
        kind: String,
        /// Embedding dimension for the emitted schema (defaults to configured options if present, else 128).
        #[arg(long)]
        dim: Option<u32>,
        /// Embedding element type: `f32` or `i8`.
        #[arg(long, default_value = "f32")]
        element_type: String,
        /// Quantization scale (only used when `--element-type i8`).
        #[arg(long)]
        quant_scale: Option<f32>,
    },
    /// Append a chunk to a writable layer file.
    Write {
        /// Destination layer path (must be `AGENTS.local.db` or `AGENTS.delta.db`).
        path: String,
        /// Target scope for permission checks: `local` or `delta`.
        #[arg(long)]
        scope: String, // local | delta
        /// Chunk id to write (if omitted, an id is assigned).
        #[arg(long)]
        id: Option<u32>,
        /// Chunk kind (e.g. `canonical`, `note`, etc).
        #[arg(long)]
        kind: String,
        /// Chunk content (the text to store).
        #[arg(long)]
        content: String,
        /// Confidence score in [0, 1].
        #[arg(long)]
        confidence: f32,
        /// Embedding JSON array (e.g. `[0.1, 0.2, ...]`); if omitted, uses hash embedding.
        #[arg(long)]
        embedding: Option<String>, // JSON array; if omitted, uses hash embed
        /// Embedding dimension (required when creating a new layer and `--embedding` is omitted).
        #[arg(long)]
        dim: Option<u32>, // required when creating a new layer and embedding omitted
        /// Source references like `path/to/file:line` (repeatable).
        #[arg(long = "source")]
        sources: Vec<String>, // file:line-style strings
        /// Source chunk ids (repeatable).
        #[arg(long = "source-chunk")]
        source_chunks: Vec<u32>,
    },
    /// Search one or more layers using vector similarity.
    #[command(
        after_help = "Examples:\n  agentsdb search --base AGENTS.base.db --query \"how do I run tests?\"\n  agentsdb search --user AGENTS.user.db --query-vec '[0.1, 0.2, 0.3]' -k 5\n  agentsdb search --base AGENTS.base.db --delta AGENTS.delta.db --query \"rustfmt\" --kind canonical --kind note\n\nQuery modes:\n  - --query: text hashed into a deterministic embedding (fast, but not semantic).\n  - --query-vec/--query-vec-file: provide an explicit embedding as a JSON array of numbers."
    )]
    Search {
        #[command(flatten)]
        layers: LayerArgs,

        /// Text query (hashed into an embedding).
        #[arg(long)]
        query: Option<String>,
        /// Explicit embedding as a JSON array (e.g. `[0.1, 0.2, ...]`).
        #[arg(long)]
        query_vec: Option<String>,
        /// Path to a file containing a JSON array embedding.
        #[arg(long)]
        query_vec_file: Option<String>,

        /// Number of nearest neighbors to return.
        #[arg(short, long, default_value_t = 5)]
        k: usize,

        /// Filter results by chunk kind (repeatable).
        #[arg(long = "kind")]
        kinds: Vec<String>,

        /// Use a rebuildable sidecar index (if present) to accelerate exact search.
        #[arg(long)]
        use_index: bool,

        /// Search mode: hybrid (lexical + semantic) or semantic-only.
        #[arg(long, default_value = "hybrid")]
        mode: String,
    },
    /// Build a rebuildable sidecar index for one or more layers.
    Index {
        #[command(flatten)]
        layers: LayerArgs,

        /// Optional directory to write index artifacts into (defaults to next to each layer).
        #[arg(long)]
        out_dir: Option<String>,

        /// Store decoded f32 embeddings even when the layer already stores f32 embeddings.
        #[arg(long)]
        store_embeddings_f32: bool,
    },
    /// Export one or more layers to a stable JSON/NDJSON format.
    Export {
        /// Directory to resolve standard layer paths from.
        #[arg(long, default_value = ".")]
        dir: String,
        /// Output format: `json` or `ndjson`.
        #[arg(long, default_value = "json", value_parser = ["json", "ndjson"])]
        format: String,
        /// Comma-separated logical layers: `base,user,delta,local`.
        #[arg(long, default_value = "base,user,delta,local")]
        layers: String,
        /// Optional output file path (defaults to stdout).
        #[arg(long)]
        out: Option<String>,
        /// Redaction mode: `none`, `content`, `embeddings`, or `all`.
        #[arg(long, default_value = "none", value_parser = ["none", "content", "embeddings", "all"])]
        redact: String,
    },
    /// Import a JSON/NDJSON export and append it to a writable layer.
    Import {
        /// Directory to resolve the target layer path from.
        #[arg(long, default_value = ".")]
        dir: String,
        /// Input file path (JSON or NDJSON).
        #[arg(long = "in")]
        input: String,
        /// Optional target logical layer: `local`, `delta`, `user`, or `base`.
        ///
        /// If omitted, the import reads the export bundle and writes each exported layer into the
        /// corresponding standard file under `--dir` (e.g. `AGENTS.delta.db`, `AGENTS.local.db`).
        #[arg(long, value_parser = ["local", "delta", "user", "base"])]
        target: Option<String>,
        /// Optional explicit target path (only valid when `--target` is provided).
        #[arg(long)]
        out: Option<String>,
        /// Dry-run (parse/validate only; no writes).
        #[arg(long)]
        dry_run: bool,
        /// Dedupe by content hash (skips chunks whose `sha256(content)` already exists in target).
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        dedupe: bool,
        /// Preserve input chunk ids when creating a new layer (errors on conflicts when appending).
        #[arg(long)]
        preserve_ids: bool,
        /// Allow writing to `AGENTS.db` (dangerous; bypasses immutability).
        #[arg(long)]
        allow_base: bool,
        /// Embedding dimension when creating a new layer and embeddings are missing.
        #[arg(long)]
        dim: Option<u32>,
    },
    /// Compare a base layer to a delta layer by id.
    Diff {
        /// Path to the base layer.
        #[arg(long)]
        base: String,
        /// Path to the delta layer.
        #[arg(long)]
        delta: String,
        /// Optional target layer to compare against (e.g. to preview promotion conflicts).
        #[arg(long, value_parser = ["user"])]
        target: Option<String>,
        /// Path to the user layer (required when `--target user`).
        #[arg(long)]
        user: Option<String>,
    },
    /// Copy selected chunks from one layer into another.
    Promote {
        /// Source layer path.
        #[arg(long = "from")]
        from_path: String,
        /// Destination layer path (must be writable).
        #[arg(long = "to")]
        to_path: String,
        /// Comma-separated chunk ids to promote (e.g. `1,2,3`).
        #[arg(long)]
        ids: String, // comma-separated
        /// Skip ids already present in the destination layer instead of erroring.
        #[arg(long)]
        skip_existing: bool,
        /// Tombstone promoted chunks in the source layer after promotion (default: true).
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        tombstone_source: bool,
        /// Assume \"yes\" for interactive confirmation prompts.
        #[arg(long)]
        yes: bool,
    },
    /// Rewrite and deduplicate layer files.
    Compact {
        /// Path to a base layer.
        #[arg(long)]
        base: Option<String>,
        /// Path to a user layer.
        #[arg(long)]
        user: Option<String>,
        /// Output path for the compacted layer.
        #[arg(long)]
        out: Option<String>,
        /// Remove tombstone chunks during compaction.
        #[arg(long)]
        remove_tombstones: bool,
        /// Remove proposal event chunks during compaction.
        #[arg(long)]
        remove_proposals: bool,
    },
    /// Re-embed content from all layers using the embedding options configured in AGENTS.db.
    Reembed {
        /// Directory containing `AGENTS*.db` standard layer files.
        #[arg(long, default_value = ".")]
        dir: String,
        /// Comma-separated logical layers to re-embed: `base,user,delta,local`.
        #[arg(long, default_value = "user,delta,local")]
        layers: String,
        /// Allow re-embedding the base layer (AGENTS.db). Required to include `base` in --layers.
        #[arg(long)]
        allow_base: bool,
    },
    /// Delete AGENTS*.db files under a root directory.
    Destroy {
        /// Root directory to scan.
        #[arg(long, default_value = ".")]
        root: String,
        /// Print what would be removed without deleting files.
        #[arg(long)]
        dry_run: bool,
    },
    /// Launch a local Web UI for browsing and editing writable layers.
    Web {
        /// Root directory to scan for `.db` files.
        #[arg(long, default_value = ".")]
        root: String,
        /// Bind address, e.g. `127.0.0.1:3030`.
        #[arg(long, default_value = "127.0.0.1:3030")]
        bind: String,
    },
    /// Show or update embedding-related options stored in standard layer files.
    Options {
        /// Directory containing `AGENTS*.db` standard layer files.
        #[arg(long, default_value = ".")]
        dir: String,
        #[command(subcommand)]
        cmd: OptionsCommand,
    },
    /// Review and manage MCP promotion proposals.
    Proposals {
        /// Directory containing `AGENTS*.db` standard layer files.
        #[arg(long, default_value = ".")]
        dir: String,
        /// Override the delta layer path (default: DIR/AGENTS.delta.db).
        #[arg(long)]
        delta: Option<String>,
        /// Override the user layer path (default: DIR/AGENTS.user.db).
        #[arg(long)]
        user: Option<String>,
        /// Override the proposal-events layer path (default: the delta layer path).
        #[arg(long)]
        proposals: Option<String>,
        #[command(subcommand)]
        cmd: ProposalsCommand,
    },
}

#[derive(Subcommand)]
/// Subcommands for managing embedding-related options.
pub(crate) enum OptionsCommand {
    /// Print the rolled-up embedding options and where they came from.
    Show {
        #[command(flatten)]
        layers: LayerArgs,
    },
    /// Append a new options record to a writable standard layer file.
    Set {
        /// Destination scope to write to: `base` (required for consistency).
        #[arg(long, default_value = "base", value_parser = ["base"])]
        scope: String,
        /// Embedder backend (e.g. `hash`, `candle`, `ort`, `openai`, `voyage`, `cohere`).
        #[arg(long)]
        backend: Option<String>,
        /// Embedding model identifier (provider-specific; currently unused for `hash`).
        #[arg(long)]
        model: Option<String>,
        /// Embedding model revision/version (provider-specific).
        #[arg(long)]
        revision: Option<String>,
        /// Local model path (dir or file) for offline/local backends (e.g. `ort`).
        #[arg(long)]
        model_path: Option<String>,
        /// Optional expected SHA-256 (lowercase hex) for local downloaded model bytes (e.g. ONNX).
        #[arg(long)]
        model_sha256: Option<String>,
        /// Embedding dimension (>0; must match existing layer schemas).
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
        dim: Option<u32>,
        /// API base URL for remote providers (e.g. OpenAI-compatible servers).
        #[arg(long)]
        api_base: Option<String>,
        /// Environment variable name holding the provider API key.
        #[arg(long)]
        api_key_env: Option<String>,
        /// Enable or disable the embedding cache.
        #[arg(long, value_enum)]
        cache: Option<Toggle>,
        /// Override the embedding cache directory.
        #[arg(long)]
        cache_dir: Option<String>,
    },
    /// Interactive prompt for configuring embedding options.
    Wizard {
        /// Destination scope to write to: `base` (required for consistency).
        #[arg(long, default_value = "base", value_parser = ["base"])]
        scope: String,
    },
    /// Manage a known-good SHA-256 allowlist for local models (per model+revision).
    Allowlist {
        #[command(subcommand)]
        cmd: AllowlistCommand,
    },
}

#[derive(Subcommand)]
/// Subcommands for managing the SHA-256 allowlist for local models.
pub(crate) enum AllowlistCommand {
    /// Print the rolled-up allowlist.
    List {
        #[command(flatten)]
        layers: LayerArgs,
    },
    /// Add or update an allowlist entry.
    Add {
        /// Destination scope to write to: `base` (required for consistency).
        #[arg(long, default_value = "base", value_parser = ["base"])]
        scope: String,
        /// Model identifier (e.g. `all-minilm-l6-v2`).
        #[arg(long)]
        model: String,
        /// Model revision/version (default: `main`).
        #[arg(long)]
        revision: Option<String>,
        /// Expected SHA-256 (lowercase hex) for the downloaded model bytes.
        #[arg(long)]
        sha256: String,
    },
    /// Remove an allowlist entry.
    Remove {
        /// Destination scope to write to: `base` (required for consistency).
        #[arg(long, default_value = "base", value_parser = ["base"])]
        scope: String,
        /// Model identifier (e.g. `all-minilm-l6-v2`).
        #[arg(long)]
        model: String,
        /// Model revision/version (default: `main`).
        #[arg(long)]
        revision: Option<String>,
    },
    /// Clear the allowlist in the target layer (higher layers still apply).
    Clear {
        /// Destination scope to write to: `base` (required for consistency).
        #[arg(long, default_value = "base", value_parser = ["base"])]
        scope: String,
    },
}

#[derive(Subcommand)]
/// Subcommands for reviewing and managing MCP promotion proposals.
pub(crate) enum ProposalsCommand {
    /// List proposals and their current status.
    List {
        /// Include accepted/rejected proposals (default shows only pending).
        #[arg(long)]
        all: bool,
    },
    /// Show a single proposal and its linked chunk.
    Show {
        /// Proposal id (chunk id of the `meta.proposal_event` record).
        #[arg(long)]
        id: u32,
    },
    /// Accept proposals by promoting their chunks into the user layer.
    Accept {
        /// Comma-separated proposal ids to accept.
        #[arg(long)]
        ids: String,
        /// Skip ids already present in the user layer instead of erroring.
        #[arg(long)]
        skip_existing: bool,
        /// Assume \"yes\" for interactive confirmation prompts.
        #[arg(long)]
        yes: bool,
    },
    /// Reject proposals without promoting them.
    Reject {
        /// Comma-separated proposal ids to reject.
        #[arg(long)]
        ids: String,
        /// Optional rejection reason to record.
        #[arg(long)]
        reason: Option<String>,
    },
}

#[derive(clap::ValueEnum, Debug, Clone, Copy)]
/// Represents a toggle state, either on or off.
pub(crate) enum Toggle {
    On,
    Off,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_parses_defaults() {
        let cli = Cli::try_parse_from(["agentsdb", "init"]).expect("parse should succeed");
        match cli.cmd {
            Command::Init {
                root,
                out,
                kind,
                dim,
                element_type,
                quant_scale,
            } => {
                assert_eq!(root, ".");
                assert_eq!(out, "AGENTS.db");
                assert_eq!(kind, "canonical");
                assert_eq!(dim, None);
                assert_eq!(element_type, "f32");
                assert_eq!(quant_scale, None);
            }
            _ => panic!("expected init command"),
        }
    }

    #[test]
    fn search_accepts_short_k() {
        let cli = Cli::try_parse_from(["agentsdb", "search", "--query", "append-only", "-k", "5"])
            .expect("parse should succeed");
        match cli.cmd {
            Command::Search { k, .. } => assert_eq!(k, 5),
            _ => panic!("expected search command"),
        }
    }

    #[test]
    fn search_accepts_long_k() {
        let cli = Cli::try_parse_from(["agentsdb", "search", "--query", "append-only", "--k", "7"])
            .expect("parse should succeed");
        match cli.cmd {
            Command::Search { k, .. } => assert_eq!(k, 7),
            _ => panic!("expected search command"),
        }
    }

    #[test]
    fn destroy_parses_defaults() {
        let cli = Cli::try_parse_from(["agentsdb", "destroy"]).expect("parse should succeed");
        match cli.cmd {
            Command::Destroy { root, dry_run } => {
                assert_eq!(root, ".");
                assert!(!dry_run);
            }
            _ => panic!("expected destroy command"),
        }
    }

    #[test]
    fn list_parses_defaults() {
        let cli = Cli::try_parse_from(["agentsdb", "list"]).expect("parse should succeed");
        match cli.cmd {
            Command::List { root } => assert_eq!(root, "."),
            _ => panic!("expected list command"),
        }
    }

    #[test]
    fn index_parses_defaults() {
        let cli = Cli::try_parse_from(["agentsdb", "index", "--base", "AGENTS.db"])
            .expect("parse should succeed");
        match cli.cmd {
            Command::Index {
                layers,
                out_dir,
                store_embeddings_f32,
            } => {
                assert_eq!(layers.base, Some("AGENTS.db".to_string()));
                assert_eq!(layers.user, None);
                assert_eq!(layers.delta, None);
                assert_eq!(layers.local, None);
                assert_eq!(out_dir, None);
                assert!(!store_embeddings_f32);
            }
            _ => panic!("expected index command"),
        }
    }

    #[test]
    fn web_parses_defaults() {
        let cli = Cli::try_parse_from(["agentsdb", "web"]).expect("parse should succeed");
        match cli.cmd {
            Command::Web { root, bind } => {
                assert_eq!(root, ".");
                assert_eq!(bind, "127.0.0.1:3030");
            }
            _ => panic!("expected web command"),
        }
    }

    #[test]
    fn options_parses_defaults() {
        let cli =
            Cli::try_parse_from(["agentsdb", "options", "show"]).expect("parse should succeed");
        match cli.cmd {
            Command::Options { dir, cmd } => {
                assert_eq!(dir, ".");
                match cmd {
                    OptionsCommand::Show { layers } => {
                        assert_eq!(layers.base, None);
                        assert_eq!(layers.user, None);
                        assert_eq!(layers.delta, None);
                        assert_eq!(layers.local, None);
                    }
                    _ => panic!("expected show subcommand"),
                }
            }
            _ => panic!("expected options command"),
        }
    }

    #[test]
    fn options_wizard_parses_defaults() {
        let cli =
            Cli::try_parse_from(["agentsdb", "options", "wizard"]).expect("parse should succeed");
        match cli.cmd {
            Command::Options { dir, cmd } => {
                assert_eq!(dir, ".");
                match cmd {
                    OptionsCommand::Wizard { scope } => assert_eq!(scope, "base"),
                    _ => panic!("expected wizard subcommand"),
                }
            }
            _ => panic!("expected options command"),
        }
    }

    #[test]
    fn compile_accepts_paths_and_text() {
        let cli = Cli::try_parse_from([
            "agentsdb",
            "compile",
            "--out",
            "AGENTS.db",
            "--text",
            "hello",
            "README.md",
        ])
        .expect("parse should succeed");
        match cli.cmd {
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
            } => {
                assert_eq!(input, None);
                assert_eq!(out, "AGENTS.db");
                assert!(!replace);
                assert_eq!(root, ".");
                assert_eq!(includes, vec!["AGENTS.md".to_string()]);
                assert_eq!(paths, vec!["README.md".to_string()]);
                assert_eq!(texts, vec!["hello".to_string()]);
                assert_eq!(kind, "canonical");
                assert_eq!(dim, None);
                assert_eq!(element_type, "f32");
                assert_eq!(quant_scale, None);
            }
            _ => panic!("expected compile command"),
        }
    }

    #[test]
    fn compile_accepts_legacy_in() {
        let cli = Cli::try_parse_from([
            "agentsdb",
            "compile",
            "--in",
            "build/input.json",
            "--out",
            "AGENTS.db",
        ])
        .expect("parse should succeed");
        match cli.cmd {
            Command::Compile {
                input,
                out,
                replace,
                ..
            } => {
                assert_eq!(input, Some("build/input.json".to_string()));
                assert_eq!(out, "AGENTS.db");
                assert!(!replace);
            }
            _ => panic!("expected compile command"),
        }
    }
}
