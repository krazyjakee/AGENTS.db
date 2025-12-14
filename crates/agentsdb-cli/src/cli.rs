use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "agentsdb",
    version,
    ${1}0.1.3${2},
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
    /// Compile text and/or files into an on-disk layer file.
    Compile {
        /// Optional input JSON path (legacy; previously produced by `collect`).
        #[arg(long = "in")]
        input: Option<String>,
        /// Output layer path to write.
        #[arg(long)]
        out: String,
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
    /// Launch a local Web UI for browsing and editing writable layers.
    Web {
        /// Root directory to scan for `.db` files.
        #[arg(long, default_value = ".")]
        root: String,
        /// Bind address, e.g. `127.0.0.1:3030`.
        #[arg(long, default_value = "127.0.0.1:3030")]
        bind: String,
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

    #[test]
    fn list_parses_defaults() {
        let cli = Cli::try_parse_from(["agentsdb", "list"]).expect("parse should succeed");
        match cli.cmd {
            Command::List { root } => assert_eq!(root, "."),
            _ => panic!("expected list command"),
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
                assert_eq!(root, ".");
                assert_eq!(includes, vec!["AGENTS.md".to_string()]);
                assert_eq!(paths, vec!["README.md".to_string()]);
                assert_eq!(texts, vec!["hello".to_string()]);
                assert_eq!(kind, "canonical");
                assert_eq!(dim, 128);
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
            Command::Compile { input, out, .. } => {
                assert_eq!(input, Some("build/input.json".to_string()));
                assert_eq!(out, "AGENTS.db");
            }
            _ => panic!("expected compile command"),
        }
    }
}
