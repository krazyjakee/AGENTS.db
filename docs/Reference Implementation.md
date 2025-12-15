# v0.1 Reference Implementation Plan (AGENTS.db + MCP)

**Status:** Draft  
**Target:** v0.1  
**Scope:** Single-host, single-writer, local-first context store and MCP server implementing the normative requirements in `docs/RFC.md`.  

## 1. Deliverables (v0.1)

- A binary reader for `AGENTS.db`-family layer files (`AGENTS.db`, `AGENTS.user.db`, `AGENTS.delta.db`, `AGENTS.local.db`).
- A compiler that produces an immutable Base layer (`AGENTS.db`) from canonical sources.
- Append-only writers for Local and Delta layers.
- A query engine that searches across multiple layers with precedence semantics.
- An MCP server implementing `agents_search`, `agents_context_write`, and `agents_context_propose`.
- CLI tooling sufficient to support the “Example Tooling Flows” in `docs/RFC.md`.

Non-deliverables for v0.1:
- Multi-writer concurrency.
- Distributed workflows.
- Online re-embedding of existing stored chunks.
- Approximate ANN indexes (brute-force is the baseline).

## 2. Compatibility Targets

- **File format:** Conform to `docs/RFC.md` Section 8.4 structures and constraints.
- **Endianness:** Little-endian only in v0.1.
- **Platform:** macOS and Linux (Windows optional).
- **Determinism:** Compiling identical inputs SHOULD produce identical `AGENTS.db` bytes (except for timestamps, if included).

## 3. Implementation Phases

### Phase 0 — Repo Skeleton and Interfaces

Goals:
- Establish core library interfaces independent of CLI and server.

Work items:
- Define data model types mirroring RFC concepts: Layer, Chunk, Provenance, Embedding, Query.
- Define a “LayerStore” interface: `open(path)`, `search(query)`, `get_chunk(id)`, `append(chunks)` (append only for non-base).
- Define error taxonomy: format errors, schema mismatch, IO, validation, and permission errors.

Acceptance criteria:
- Minimal library compiles with no IO side effects.
- API surfaces “base is immutable” and “append-only” constraints explicitly.

### Phase 1 — Binary Reader (Base Capability)

Goals:
- Read any single layer file into a memory-mapped representation.

Work items:
- Parse `FileHeaderV1`, validate `magic`, version, `file_length_bytes`, `section_count`, section boundaries.
- Locate required sections by `SectionKind`: String Dictionary, Chunk Table, Embedding Matrix.
- Parse `StringDictionaryHeaderV1` and resolve string IDs.
- Parse `ChunkTableHeaderV1` and `ChunkRecord[]`.
- Parse `EmbeddingMatrixHeaderV1` and validate `data_length` and element type.
- If Relationships section exists, parse and validate relationship bounds.

Acceptance criteria:
- Reader rejects malformed files (bad offsets/lengths, schema mismatch) deterministically.
- Reader loads via `mmap` (or equivalent) and performs zero-copy reads for strings/records.
- Unit tests cover: truncated files, invalid offsets, invalid IDs, missing required sections.

### Phase 2 — Query Engine (Single Layer)

Goals:
- Implement brute-force vector search within one layer.

Work items:
- Define embedding normalization rules for `EMBED_F32` and `EMBED_I8` per RFC.
- Implement similarity computation (cosine or dot-product) and top-k selection.
- Implement filters (at minimum: `kind` filter).
- Implement retrieval of chunk metadata including provenance sources.

Acceptance criteria:
- Given fixed input vectors and query, results are stable across runs.
- `k` and filter semantics match `docs/RFC.md` Section 10.1.

### Phase 3 — Multi-layer Union Query

Goals:
- Search across `base/user/delta/local` with precedence semantics.

Work items:
- Implement multi-layer open and validation: schemas MUST match or error.
- Implement union search: gather candidates per layer, merge by precedence.
- Define a v0.1 override/annotation rule:
  - A higher-layer chunk MAY “override” a lower-layer chunk if it shares the same `id`.
  - Otherwise, chunks are distinct.
- Expose to callers which layer each returned result originated from.

Acceptance criteria:
- Layer ordering matches RFC precedence: `local > user > delta > base`.
- The same query against the same layer set returns the same ranked results.

### Phase 4 — Append-only Writer (Local/Delta)

Goals:
- Create and append to `AGENTS.local.db` and `AGENTS.delta.db` without in-place mutation.

Work items:
- Define file creation: write header, section table, and empty sections.
- Define append strategy:
  - Append new strings to dictionary blob.
  - Append new string entries.
  - Append new chunk records.
  - Append new embedding rows.
  - Append relationship records (if used).
  - Update header and section table by rewriting a new file (or using a trailing “footer” strategy), ensuring no in-place mutation.
- Enforce invariants:
  - Writes MUST be append-only.
  - `author` is set to `mcp` for MCP writes.
  - `confidence` is present.

Acceptance criteria:
- Appends do not corrupt existing reads.
- After each append, a fresh reader can open and query the file successfully.
- Writer rejects attempts to target base/user layers.

### Phase 5 — Compiler for Base Layer

Goals:
- Build `AGENTS.db` from canonical text sources.

Work items:
- Implement “collect” step: discover input sources (start with `AGENTS.md` and a configurable include list).
- Implement chunking: deterministic chunk boundaries and stable `id` assignment.
- Compute embeddings:
  - Implemented: pluggable embedder interface + deterministic `hash` default, with feature-gated local/remote backends (`crates/agentsdb-embeddings/`).
  - Implemented: options roll-up from `options` records in standard layers (`local > user > delta > base`) and layer-level embedding metadata (see `docs/RFC.md`).
- Produce a complete base layer file following the RFC binary structs.

Acceptance criteria:
- `agentsdb compile` produces a valid `AGENTS.db` that passes the reader’s validations.
- Compiler can run without network access (assuming embeddings are provided or local).

### Phase 6 — MCP Server (v0.1)

Goals:
- Expose read/write methods described in the RFC.

Work items:
- Implement `agents_search`:
  - Validate `query`, `k`, optional `filters`, optional `layers`.
  - Execute union query.
  - Return results including chunk content, metadata, and provenance.
- Implement `agents_context_write`:
  - Validate payload, enforce `scope` in `{local,delta}`.
  - Append to the corresponding layer.
  - Return assigned `context_id` (chunk id).
- Implement `agents_context_propose`:
  - Record a promotion request by appending a `meta.proposal_event` chunk to `AGENTS.delta.db` (no sidecar files).
  - Enforce `target: user`.

Acceptance criteria:
- Methods conform to the RFC input constraints and error on invalid requests.
- Writes are durable and immediately readable via search.

### Phase 7 — CLI Tooling (v0.1)

Goals:
- Support the example flows in `docs/RFC.md` with concrete commands.

Commands (suggested):
- `agentsdb compile`
- `agentsdb init`
- `agentsdb serve`
- `agentsdb diff`
- `agentsdb inspect`
- `agentsdb promote`
- `agentsdb compact`
- `agentsdb validate` (format validator; RECOMMENDED)

Acceptance criteria:
- Each command has `--help`, deterministic exit codes, and machine-readable output mode (JSON) where appropriate.

## 4. Testing Strategy

- **Unit tests:** binary parsing, bounds checks, string ID resolution, embedding shape validation.
- **Golden tests:** small fixture databases checked into `docs/fixtures/` (or similar) with known query outputs.
- **Property tests (optional):** fuzz invalid offsets/lengths and ensure parser fails safely.
- **Integration tests:** compile → serve → write local → search → write delta → propose → promote to user (as file ops).

## 5. Performance and Limits (v0.1)

- Reader SHOULD open a multi-MB file in <100ms on typical developer machines (target; not a hard requirement).
- Search MUST support brute-force; performance MAY be improved later with indexes.
- Define practical limits (initial defaults):
  - Max `k` (e.g., 1000).
  - Max chunk content size for writes (e.g., 64KB).
  - Max sources per chunk (e.g., 64).

## 6. Security and Trust (v0.1)

- The server MUST treat `AGENTS.db` as immutable and MUST NOT accept write operations to it.
- File paths for layers SHOULD be explicitly configured (no implicit discovery outside a configured root).
- Optional signing:
  - Implement `agentsdb sign` and `agentsdb verify` later; for v0.1, verify MAY be stubbed behind a feature flag.

## 7. Risks and Open Decisions

- **ID assignment:** choose stable `id` semantics for compiler and writers (monotonic vs content-hash-derived).
- **Dictionary growth:** append-only dictionary may fragment; compaction strategy needs definition.
- **Override semantics:** using `id` collisions for overrides requires governance; consider explicit “supersedes” relationships in a future version.
- **Embedding backend:** if external embeddings are required, CI/network restrictions complicate v0.1.

## 8. Definition of Done (v0.1)

- A user can build `AGENTS.db`, start the MCP server, search across layers, write to `local`/`delta`, propose promotion, and promote into `user` using the CLI.
- All required RFC invariants are enforced (immutability, append-only semantics, schema compatibility).
- Tests cover critical parsing and write-path correctness, including corruption resistance.
