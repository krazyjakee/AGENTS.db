# AGENTS.db

<p align="center">
  <img src="https://raw.githubusercontent.com/krazyjakee/AGENTS.db/main/crates/agentsdb-web/assets/logo.png" alt="logo" />
</p>

AGENTS.db is a file format and toolkit for creating, inspecting, and querying immutable, layered documentation databases—built for deterministic context storage.

[![GitHub Sponsors](https://img.shields.io/github/sponsors/krazyjakee?label=sponsors&style=for-the-badge)](https://github.com/sponsors/krazyjakee) [![GitHub Stars](https://img.shields.io/github/stars/krazyjakee/AGENTS.db?style=for-the-badge&color=yellow)](https://github.com/krazyjakee/AGENTS.db)

![Alt](https://repobeats.axiom.co/api/embed/754b9c5db54aa484d2f93d9d3c943766b33ac869.svg "Repobeats analytics image")

It’s designed for agent systems and MCP servers that need:

- A **read-only, canonical** knowledge base (the Base layer).
- **Append-only layers** for new notes, derived summaries, and proposals.
- Clear **provenance** (who/what wrote a chunk, and what sources it came from).
- Fast local search.

This repo is currently targeting the spec is in `docs/RFC.md`.

## The Big Idea

Think of your project knowledge as “chunks” stored in layer files:

- **Base**: `AGENTS.db` (immutable; built by a compiler).
- **User**: `AGENTS.user.db` (append-only; durable human additions).
- **Delta**: `AGENTS.delta.db` (append-only; reviewable proposed additions).
- **Local**: `AGENTS.local.db` (append-only; ephemeral/session notes).

When searching across layers, higher-precedence layers win:

`local > user > delta > base`

The key safety rule: tooling **must not modify `AGENTS.db` in place**.

## Quickstart (CLI)

The CLI binary is named `agentsdb` and lives in `crates/agentsdb-cli/`.

Install a prebuilt release (macOS/Linux/Windows via Git Bash) into `~/.local/bin`:

```sh
curl -fsSL https://raw.githubusercontent.com/krazyjakee/AGENTS.db/main/scripts/install.sh | bash
agentsdb --help
```

Install it locally (macOS/Linux/Windows via Git Bash or WSL):

```sh
bash scripts/install-cli.sh
agentsdb --help
```

Install to a specific prefix (builds then copies the binary into `PREFIX/bin`):

```sh
bash scripts/install-cli.sh --prefix "$HOME/.local" --force
```

Build it:

```sh
cargo build -p agentsdb-cli
```

See available commands:

```sh
agentsdb --help
```

### 1) Init (wide collect + compile)

This scans your repo for common documentation files (wide net) and directly writes `AGENTS.db`.

```sh
agentsdb init
```

#### Or manually collect and compile canonical sources

Compile directly from file paths and/or inline text (no intermediate JSON manifest).

```sh
agentsdb compile --out AGENTS.db --dim 128 --element-type f32 AGENTS.md docs/RFC.md
```

```sh
agentsdb compile --out AGENTS.db --text "Project note: layers are append-only."
```

Notes:
- If embeddings aren’t provided, `compile` uses a deterministic built-in hash embedder (handy for local/dev).
- `compile` appends to an existing `--out` file by default; use `--replace` to overwrite.

### 3) Validate and inspect a layer file

```sh
agentsdb validate AGENTS.db
agentsdb inspect AGENTS.db
```

### 4) Search

Search just the base layer:

```sh
agentsdb search --base AGENTS.db --query "something awesome"
```

Search across multiple layers:

```sh
agentsdb search \
  --base AGENTS.db \
  --user AGENTS.user.db \
  --delta AGENTS.delta.db \
  --local AGENTS.local.db \
  --query "what is precedence?" -k 5
```

### 5) Append a chunk to a writable layer

Write to the Local or Delta layer (the CLI enforces the expected filenames).

Create/append to `AGENTS.local.db`:

```sh
agentsdb write AGENTS.local.db \
  --scope local \
  --kind derived-summary \
  --content "This repo treats AGENTS.db as immutable; writes go to local/delta." \
  --confidence 0.7 \
  --dim 128 \
  --source "docs/RFC.md:1"
```

Then search including local results:

```sh
agentsdb search --base AGENTS.db --local AGENTS.local.db --query "immutable" -k 5
```

## Web UI

`agentsdb web` launches a local Web UI for browsing layers under a root directory and appending/removing chunks in writable layers (`AGENTS.local.db` / `AGENTS.delta.db`).

```sh
agentsdb web --root . --bind 127.0.0.1:3030
```

<p align="center">
  <img src="https://raw.githubusercontent.com/krazyjakee/AGENTS.db/main/screenshot.png" alt="web-ui" />
</p>

## MCP server

`agentsdb serve` starts an MCP server over stdio (intended to be launched by an MCP-capable host).

```sh
agentsdb serve --base "$PWD/AGENTS.db" --local "$PWD/AGENTS.local.db"
```

The target API surface is described in `docs/RFC.md` (e.g. `agents_search`, `agents_context_write`).

## MCP setup (Codex CLI / Claude Code / Gemini CLI)

`agentsdb` exposes an MCP **stdio** server via `agentsdb serve`. To hook it up, install `agentsdb`, make sure you have a base layer (`AGENTS.db`) plus at least one writable layer (`AGENTS.local.db` and/or `AGENTS.delta.db`), then register a server that runs `agentsdb serve` with **absolute paths**.

Example server command:

```sh
agentsdb serve \
  --base "$PWD/AGENTS.db" \
  --local "$PWD/AGENTS.local.db" \
  --delta "$PWD/AGENTS.delta.db"
```

### OpenAI Codex CLI

Add a global MCP server entry:

```sh
codex mcp add agentsdb -- agentsdb serve --base "$PWD/AGENTS.db" --local "$PWD/AGENTS.local.db" --delta "$PWD/AGENTS.delta.db"
```

### Claude Code

Add an MCP server (pick `--scope project` or `--scope user` as desired):

```sh
claude mcp add --transport stdio --scope project agentsdb -- agentsdb serve --base "$PWD/AGENTS.db" --local "$PWD/AGENTS.local.db" --delta "$PWD/AGENTS.delta.db"
```

### Gemini CLI

Add an MCP server (defaults to `--scope project`):

```sh
gemini mcp add --transport stdio --scope project agentsdb agentsdb serve --base "$PWD/AGENTS.db" --local "$PWD/AGENTS.local.db" --delta "$PWD/AGENTS.delta.db"
```

## Repository layout

- `crates/agentsdb-core/`: shared types, errors, embedding utilities.
- `crates/agentsdb-format/`: `AGENTS.db` file reader/writer.
- `crates/agentsdb-query/`: query engine across one or more layers.
- `crates/agentsdb-mcp/`: MCP server library.
- `crates/agentsdb-web/`: Web UI server + embedded assets.
- `crates/agentsdb-cli/`: `agentsdb` CLI binary.
- `docs/`: spec and implementation plan (`docs/RFC.md`, `docs/Reference Implementation.md`).

## Development

Common commands:

```sh
cargo test
cargo fmt --all
cargo clippy --all-targets --all-features
```

## Learn more

- Spec and semantics: `docs/RFC.md`
- Planned scope: `docs/Reference Implementation.md`
- Looking for a workflow/mental model of how to lean into this approach? See `WORKFLOW.md`

## License

MIT. See `LICENSE`.
