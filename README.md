# agents.db (AGENTS.db)

`agents.db` is a local, layered “context store” file format (`AGENTS.db`) plus tools to **build**, **validate**, **search**, and **append** new context without rewriting history.

It’s designed for agent systems and MCP servers that need:

- A **read-only, canonical** knowledge base (the Base layer).
- **Append-only layers** for new notes, derived summaries, and proposals.
- Clear **provenance** (who/what wrote a chunk, and what sources it came from).
- Fast local search (v0.1 uses a simple brute-force baseline).

This repo is currently targeting **v0.1**; the spec is in `docs/RFC.md`.

## The Big Idea (for humans)

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

### 1) Collect canonical sources into a manifest

This scans your repo for source files (by default `AGENTS.md`) and produces a JSON manifest that `compile` understands.

```sh
agentsdb collect \
  --root . \
  --include AGENTS.md \
  --out build/agents.sources.json \
  --dim 128 \
  --element-type f32
```

### 2) Compile an immutable base layer (`AGENTS.db`)

```sh
agentsdb compile \
  --in build/agents.sources.json \
  --out AGENTS.db
```

Notes:
- If the manifest doesn’t include embeddings, `compile` uses a deterministic built-in hash embedder (handy for local/dev).

### 3) Validate and inspect a layer file

```sh
agentsdb validate AGENTS.db
agentsdb inspect AGENTS.db
```

### 4) Search

Search just the base layer:

```sh
agentsdb search --base AGENTS.db --query "append-only" -k 5
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

## MCP server

`agentsdb serve` starts an MCP server over stdio (intended to be launched by an MCP-capable host).

```sh
agentsdb serve --base AGENTS.db --local AGENTS.local.db
```

The v0.1 target API surface is described in `docs/RFC.md` (e.g. `agents_search`, `agents_context_write`).

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
- `crates/agentsdb-cli/`: `agentsdb` CLI binary.
- `docs/`: spec and implementation plan (`docs/RFC.md`, `docs/Reference Implementation v0.1.md`).

## Development

Common commands:

```sh
cargo test
cargo fmt --all
cargo clippy --all-targets --all-features
```

## What about my AGENTS.md file?

Agents.db is supposed to complement the human readable text contexts, not replace it.
Keep a high level overview of the project and add these lines:

```
## Agent-Specific Notes

Treat `AGENTS.db` layers as immutable; avoid in-place mutation utilities unless required by the design.
This repository includes a compiled documentation database at `agents.db`.
Use MCP `agents_search` for architectural, API, and historical context.
```

## Learn more

- Spec and semantics: `docs/RFC.md`
- Planned scope for v0.1: `docs/Reference Implementation v0.1.md`

## License

MIT. See `LICENSE`.
