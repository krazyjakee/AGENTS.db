use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "agentsdb",
    version,
    about = "AGENTS.db tooling (v0.1.1)",
    long_about = "Tools for creating, inspecting, and querying AGENTS.db layers.\n\nNotes:\n  - Layers are treated as append-only. Writes append new chunks.\n  - `search --query` uses a deterministic hash embedding (not a semantic model)."
)]
pub(crate) struct Cli {
    /// Emit machine-readable JSON instead of human output.
    #[arg(long)]
    pub(crate) json: bool,

    #[command(subcommand)]
    pub(crate) cmd: Command,
}

#[derive(Subcommand)]
pub(crate) enum Command {
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
        /// Embedding dimension for the emitted schema.
        #[arg(long, default_value_t = 128)]
        dim: u32,
        /// Embedding element type: `f32` or `i8`.
        #[arg(long, default_value = "f32")]
        element_type: String,
        /// Quantization scale (only used when `--element-type i8`).
        #[arg(long)]
        quant_scale: Option<f32>,
    },
    /// Collect files and emit a compile JSON payload.
    Collect {
        /// Root directory to search for files.
        #[arg(long, default_value = ".")]
        root: String,
        /// File names to include (repeatable).
        #[arg(long = "include", default_value = "AGENTS.md")]
        includes: Vec<String>,
        /// Output path for the generated JSON.
        #[arg(long)]
        out: String,
        /// Chunk kind to assign to collected files.
        #[arg(long, default_value = "canonical")]
        kind: String,
        /// Embedding dimension for the emitted schema.
        #[arg(long, default_value_t = 128)]
        dim: u32,
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
        /// Path to a base layer (usually `AGENTS.base.db`).
        #[arg(long)]
        base: Option<String>,
        /// Path to a user layer (usually `AGENTS.user.db`).
        #[arg(long)]
        user: Option<String>,
        /// Path to a delta layer (usually `AGENTS.delta.db`).
        #[arg(long)]
        delta: Option<String>,
        /// Path to a local layer (usually `AGENTS.local.db`).
        #[arg(long)]
        local: Option<String>,
    },
    /// Compile a JSON payload into an on-disk layer file.
    Compile {
        /// Input JSON path (from `collect` or manually authored).
        #[arg(long = "in")]
        input: String,
        /// Output layer path to write.
        #[arg(long)]
        out: String,
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
        after_help = "Examples:\n  agentsdb search --base AGENTS.base.db --query \"how do I run tests?\"\n  agentsdb search --user AGENTS.user.db --query-vec-file query.json -k 5\n  agentsdb search --base AGENTS.base.db --delta AGENTS.delta.db --query \"rustfmt\" --kind canonical --kind note\n\nQuery modes:\n  - --query: text hashed into a deterministic embedding (fast, but not semantic).\n  - --query-vec/--query-vec-file: provide an explicit embedding as a JSON array of numbers."
    )]
    Search {
        /// Path to a base layer (usually `AGENTS.base.db`).
        #[arg(long)]
        base: Option<String>,
        /// Path to a user layer (usually `AGENTS.user.db`).
        #[arg(long)]
        user: Option<String>,
        /// Path to a delta layer (usually `AGENTS.delta.db`).
        #[arg(long)]
        delta: Option<String>,
        /// Path to a local layer (usually `AGENTS.local.db`).
        #[arg(long)]
        local: Option<String>,

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
        #[arg(short, long, default_value_t = 10)]
        k: usize,

        /// Filter results by chunk kind (repeatable).
        #[arg(long = "kind")]
        kinds: Vec<String>,
    },
    /// Compare a base layer to a delta layer by id.
    Diff {
        /// Path to the base layer.
        #[arg(long)]
        base: String,
        /// Path to the delta layer.
        #[arg(long)]
        delta: String,
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
    },
    /// (Not implemented) Rewrite and deduplicate layers.
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
    },
    /// Delete AGENTS*.db files under a root directory.
    Clean {
        /// Root directory to scan.
        #[arg(long, default_value = ".")]
        root: String,
        /// Print what would be removed without deleting files.
        #[arg(long)]
        dry_run: bool,
    },
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
                assert_eq!(dim, 128);
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
    fn clean_parses_defaults() {
        let cli = Cli::try_parse_from(["agentsdb", "clean"]).expect("parse should succeed");
        match cli.cmd {
            Command::Clean { root, dry_run } => {
                assert_eq!(root, ".");
                assert!(!dry_run);
            }
            _ => panic!("expected clean command"),
        }
    }
}
