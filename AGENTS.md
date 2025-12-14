# Repository Guidelines

## Project Structure

- `crates/` contains the Rust workspace crates:
  - `crates/agentsdb-core/`: shared types, errors, embedding utilities.
  - `crates/agentsdb-format/`: `AGENTS.db` file reader/writer.
  - `crates/agentsdb-query/`: query engine across one or more layers.
  - `crates/agentsdb-mcp/`: MCP-facing library for search/write semantics.
  - `crates/agentsdb-cli/`: `agentsdb` CLI binary (`src/main.rs`).
- `docs/`: design docs, including `docs/RFC.md`.
- `target/`: Cargo build output (ignored).

## Build, Test, and Development Commands

- `cargo build`: build the full workspace.
- `cargo test`: run all unit tests (inline `#[cfg(test)]` modules).
- `cargo test -p agentsdb-format`: run tests for a single crate.
- `cargo run -p agentsdb-cli -- --help`: run the CLI help locally.
- `cargo fmt --all`: format code with rustfmt.
- `cargo clippy --all-targets --all-features`: lint with Clippy (aim for clean output).

## Coding Style & Naming Conventions

- Rust edition: 2021 (see crate `Cargo.toml` files).
- Formatting: rustfmt defaults; keep diffs minimal and idiomatic.
- Naming: crates/modules/functions in `snake_case`, types/traits in `PascalCase`, constants in `SCREAMING_SNAKE_CASE`.
- Prefer explicit error context (`anyhow::Context`) at binary boundaries (CLI).

## Testing Guidelines

- Tests live next to code in `#[cfg(test)] mod tests` blocks.
- Prefer small unit tests per feature (reader/writer/query invariants).
- Use deterministic inputs (avoid timestamps/randomness unless explicitly tested).

## Commit & Pull Request Guidelines

- Git history is not established yet; use Conventional Commits:
  - `feat: ...`, `fix: ...`, `docs: ...`, `chore: ...`
- PRs should include: a short rationale, key commands run (e.g., `cargo test`), and any relevant CLI examples (e.g., `agentsdb compile`, `agentsdb search`).

## Agent-Specific Notes

This repository includes a compiled documentation database/knowledgebase at `AGENTS.db`.
For context for any task, you MUST use MCP `agents_search` to look up context including architectural, API, and historical changes.
Treat `AGENTS.db` layers as immutable; avoid in-place mutation utilities unless required by the design.
