# RFC: AGENTS Context Store and MCP Read/Write Semantics

**Status:** Draft  
**Version:** 0.1  
**Authors:** Jacob Cattrall (sub7@duck.com)  
**Last Updated:** 2025-12-12

---

## 1. Abstract

This document specifies an immutable context store format (`AGENTS.db`) and a layered append-only write model for Model Context Protocol (MCP) implementations. Implementations can read, search, and append derived context without mutating canonical data. The design prioritizes determinism, reproducibility, provenance, and performance.

---

## 2. Requirements Language

The key words "MUST", "MUST NOT", "REQUIRED", "SHALL", "SHALL NOT", "SHOULD", "SHOULD NOT", "RECOMMENDED", "NOT RECOMMENDED", "MAY", and "OPTIONAL" in this document are to be interpreted as described in RFC 2119 and RFC 8174 when, and only when, they appear in all capitals.

---

## 3. Motivation (Informative)

Agent systems commonly need to persist knowledge across sessions, distinguish source-of-truth from derived context, and search large local knowledge bases efficiently. This document defines a compiled immutable base artifact and append-only writable layers with explicit promotion and trust semantics.

---

## 4. Terminology

| Term | Definition |
|---|---|
| Chunk | Atomic unit of contextual information. |
| Layer | A standalone context store with a defined trust level. |
| Base Layer | Canonical, human-authored context stored in `AGENTS.db`. |
| User Layer | Durable, human-authored append-only additions stored in `AGENTS.user.db`. |
| Delta Layer | Reviewable, proposed additions stored in `AGENTS.delta.db`. |
| Local Layer | Ephemeral, agent-generated additions stored in `AGENTS.local.db`. |
| MCP | Model Context Protocol implementation. |
| Promotion | The act of moving or copying context between layers. |

---

## 5. Goals and Non-goals (Informative)

### 5.1 Goals

- The format SHOULD be deterministic and reproducible across machines.
- Implementations SHOULD support fast local vector search.
- Implementations SHOULD support zero-copy startup via memory mapping.
- All chunks SHOULD include explicit provenance and trust metadata.
- Durability of canonical knowledge SHOULD be human-governed.
- The storage format SHOULD be language-agnostic.

### 5.2 Non-goals

- This specification does not define multi-writer concurrency.
- This specification does not define online re-embedding.
- This specification does not define automatic truth adjudication.

---

## 6. Context Layers

### 6.1 Standard Layers and Files

An implementation that claims conformance to this document MUST support the following layer identifiers and filenames:

| Layer | File | Mutability | Primary Author |
|---|---|---|---|
| base | `AGENTS.db` | Immutable | Humans / CI |
| user | `AGENTS.user.db` | Append-only | Humans |
| delta | `AGENTS.delta.db` | Append-only | MCP (proposed) |
| local | `AGENTS.local.db` | Append-only | MCP (ephemeral) |

An implementation MUST NOT modify `AGENTS.db` in place.

### 6.2 Layer Precedence

When the same query is executed across multiple layers, results MUST be interpreted with the following precedence order:

```
local > user > delta > base
```

Higher-precedence layers MAY override or annotate lower-precedence layers. An implementation MUST NOT delete or mutate lower-precedence data as a mechanism for override.

---

## 7. Chunk Model

### 7.1 Required Fields

Each chunk record MUST include the following fields:

```json
{
  "id": "uint32",
  "content": "string",
  "kind": "enum",
  "embedding": "vector",
  "sources": ["chunk_id | file:line"],
  "author": "human | mcp",
  "confidence": "float (0.0–1.0)",
  "created_at": "timestamp"
}
```

The `confidence` field MUST be present for all chunks. For `author: "mcp"`, `confidence` MUST represent the MCP's confidence in the chunk's correctness. For `author: "human"`, `confidence` MAY be set to `1.0`.

### 7.2 Provenance Requirements

- `sources` MUST be present and MUST contain zero or more provenance references.
- A provenance reference MUST be either a chunk identifier or a `file:line` reference.

### 7.3 Binary Encoding

In binary encodings, string values SHOULD be represented via dictionary identifiers. Encodings MUST preserve the semantics of the fields defined in Section 7.1.

When using the binary structures defined in Section 8.4, implementations MUST represent:

- `content` as `content_str_id`.
- `kind` as `kind_str_id`.
- `author` as `author_str_id`.
- `sources` as relationship records referenced by `rel_start` and `rel_count`.

---

## 8. Storage Format

### 8.1 File Properties

Each layer file MUST be:

- A single binary file per layer.
- Memory-mappable.
- Little-endian.
- Offset-addressed (no pointer addresses).
- Append-only with no in-place mutation.

### 8.2 Required Sections

Each layer file MUST include, at minimum:

1. Header
2. String Dictionary
3. Chunk Table
4. Embedding Matrix

A Relationship Table MAY be included.

A Layer Metadata section MAY be included.

### 8.3 Schema Compatibility

All layers used together in a single query MUST share identical schemas. If schemas differ, the implementation MUST return an error and MUST NOT silently merge incompatible layers.

### 8.4 Binary Structure Definitions

This section defines a minimal, interoperable on-disk layout. All multi-byte fields MUST be little-endian. All offsets MUST be absolute file offsets in bytes.

#### 8.4.1 Primitive Types

- `u8`, `u16`, `u32`, `u64`: Unsigned integers.
- `i8`, `i16`, `i32`, `i64`: Signed integers.
- `f32`, `f64`: IEEE-754 floating point.

#### 8.4.2 Common Conventions

- All structs MUST be tightly packed with no implicit padding.
- All variable-length data MUST be referenced by `(offset, length)` pairs.
- All IDs in this format MUST be stable within a file and MUST NOT change after being written.
- A value of `0` for an ID field MUST mean "unset" unless explicitly specified otherwise.

#### 8.4.3 File Header and Section Table

Each layer file MUST begin with a header followed by a section table.

```c
// Magic bytes: 'A' 'G' 'D' 'B'
struct FileHeaderV1 {
  u32 magic;              // 0x42444741
  u16 version_major;      // MUST be 1
  u16 version_minor;      // MAY be incremented for backward-compatible changes
  u64 file_length_bytes;  // MUST equal the file length
  u64 section_count;      // Number of SectionEntry records
  u64 sections_offset;    // Offset to SectionEntry[section_count]
  u64 flags;              // Reserved; MUST be 0 for v1
};

enum SectionKind : u32 {
  SECTION_STRING_DICTIONARY = 1,
  SECTION_CHUNK_TABLE       = 2,
  SECTION_EMBEDDING_MATRIX  = 3,
  SECTION_RELATIONSHIPS     = 4,
  SECTION_LAYER_METADATA    = 5
};

struct SectionEntry {
  u32 kind;    // SectionKind
  u32 reserved;// MUST be 0
  u64 offset;  // Section start
  u64 length;  // Section length in bytes
};
```

The file MUST contain exactly one section each of `SECTION_STRING_DICTIONARY`, `SECTION_CHUNK_TABLE`, and `SECTION_EMBEDDING_MATRIX`. The file MAY contain `SECTION_RELATIONSHIPS` and/or `SECTION_LAYER_METADATA`.

#### 8.4.4 String Dictionary Section

The String Dictionary section MUST contain all string values referenced by other sections.

```c
struct StringDictionaryHeaderV1 {
  u64 string_count;
  u64 entries_offset;  // Offset (from file start) to StringEntry[string_count]
  u64 bytes_offset;    // Offset to the start of the string byte blob
  u64 bytes_length;    // Length of the byte blob
};

struct StringEntry {
  u64 byte_offset;     // Offset from bytes_offset
  u64 byte_length;     // Length in bytes (UTF-8)
};
```

- String bytes MUST be UTF-8.
- String IDs MUST be 1-based indices into `StringEntry` (i.e., valid IDs are `1..string_count`).

#### 8.4.5 Chunk Table Section

The Chunk Table section MUST contain fixed-size chunk records.

```c
struct ChunkTableHeaderV1 {
  u64 chunk_count;
  u64 records_offset;  // Offset to ChunkRecord[chunk_count]
};

// Note: string IDs refer to the String Dictionary.
struct ChunkRecord {
  u32 id;              // Chunk identifier; MUST be unique within the file
  u32 kind_str_id;     // String ID; MUST be non-zero
  u32 content_str_id;  // String ID; MUST be non-zero
  u32 author_str_id;   // String ID; MUST refer to "human" or "mcp"
  f32 confidence;      // 0.0..1.0
  u64 created_at_unix_ms;
  u32 embedding_row;   // Row index into Embedding Matrix; MUST be non-zero
  u32 reserved0;       // MUST be 0
  u64 rel_start;       // First relationship index (see Section 8.4.7); MAY be 0
  u32 rel_count;       // Relationship count; MAY be 0
  u32 reserved1;       // MUST be 0
};
```

The `id` field MUST be stable and MUST NOT be reused for a different chunk within the same file.

#### 8.4.6 Embedding Matrix Section

The Embedding Matrix section MUST contain a row-major matrix of embedding vectors.

```c
enum EmbeddingElementType : u32 {
  EMBED_F32 = 1,
  EMBED_I8  = 2
};

struct EmbeddingMatrixHeaderV1 {
  u64 row_count;       // Number of embedding rows
  u32 dim;             // Embedding dimension
  u32 element_type;    // EmbeddingElementType
  u64 data_offset;     // Offset to matrix data
  u64 data_length;     // Length in bytes
  f32 quant_scale;     // MUST be 1.0 for EMBED_F32; otherwise quantization scale
  f32 reserved0;       // MUST be 0
};
```

- If `element_type` is `EMBED_F32`, the data MUST be `row_count * dim` contiguous `f32` values.
- If `element_type` is `EMBED_I8`, the data MUST be `row_count * dim` contiguous `i8` values and `quant_scale` MUST be non-zero.
- For every chunk, `embedding_row` MUST be in the inclusive range `1..row_count`.

#### 8.4.7 Relationships Section (Optional)

If present, the Relationships section SHOULD encode chunk provenance and other edges.

```c
enum RelationshipKind : u32 {
  REL_SOURCE_CHUNK_ID = 1, // value_u32 is a chunk id
  REL_SOURCE_STRING   = 2  // value_u32 is a string id (e.g., "file:line")
};

struct RelationshipsHeaderV1 {
  u64 relationship_count;
  u64 records_offset; // Offset to RelationshipRecord[relationship_count]
};

struct RelationshipRecord {
  u32 kind;      // RelationshipKind
  u32 value_u32; // Chunk id or string id
};
```

If `SECTION_RELATIONSHIPS` is absent, `rel_start` and `rel_count` in `ChunkRecord` MUST be `0`.

#### 8.4.8 Layer Metadata Section (Optional)

If present, the Layer Metadata section stores a versioned metadata blob describing how embeddings were produced for this layer.

```c
enum LayerMetadataFormat : u32 {
  LAYER_METADATA_JSON = 1
};

struct LayerMetadataHeaderV1 {
  u32 version;      // MUST be 1
  u32 format;       // MUST be LAYER_METADATA_JSON for v1
  u64 blob_offset;  // MUST equal (section.offset + 24)
  u64 blob_length;  // MUST equal (section.length - 24)
};
```

- For `format == LAYER_METADATA_JSON`, the blob MUST be UTF-8 JSON.
- Implementations MUST NOT include timestamps in this metadata blob if they claim deterministic/reproducible builds.

The JSON blob SHOULD include at least:

- `v` (u32): metadata schema version (currently `1`).
- `embedding_profile`: `{ backend, model, revision, dim, output_norm }`.
  - `output_norm` MUST be explicit (e.g., `"none"` or `"l2"`), because normalization affects determinism and cache keys.
- `cache_key_alg`: a string or enum identifying the cache key algorithm.
- `embedder_metadata` (optional): provider/runtime details that help audit and reproduce embeddings, e.g.:
  - provider name + API base (for remote providers)
  - runtime name/version (for local runtimes)
  - model file hashes (e.g., SHA-256) and relevant runtime knobs (e.g., quantization mode)

Implementations SHOULD treat `embedding_profile` as the canonical “compatibility contract” for merging/searching across layers. If an implementation embeds queries (as opposed to receiving an explicit query vector), it SHOULD validate that the active embedder profile matches the layer metadata profile for all layers being queried, and return a clear error if not.

---

## 9. Vector Semantics

- Embeddings MAY be quantized. If quantized, `int8` is RECOMMENDED.
- If embeddings are stored quantized, the implementation MUST apply a compatible quantization or dequantization strategy before computing similarity.
- The implementation MUST support cosine similarity or dot-product similarity.
- The implementation MUST support brute-force search.
- The implementation MAY support approximate indexes.

### 9.1 Deterministic Cache Keys (Informative)

To make embedding caches stable across runs, a cache key SHOULD be computed as:

```
key = sha256(profile_json_v1 || 0x00 || content_utf8)
```

Where:

- `content_utf8` is the raw UTF-8 bytes of the input text.
- `profile_json_v1` is a UTF-8 JSON object with keys in this exact order:
  - `v` (number, value `1`)
  - `backend` (string)
  - `model` (string or null)
  - `revision` (string or null)
  - `dim` (number)

---

## 10. MCP Read API

### 10.1 Search Method

An MCP server conforming to this document MUST implement `agents_search` with parameters equivalent to:

```json
{
  "method": "agents_search",
  "params": {
    "query": "string",
    "k": 10,
    "filters": {
      "kind": ["architecture", "invariant"]
    },
    "layers": ["base", "user", "delta", "local"]
  }
}
```

- `query` MUST be a non-empty string.
- `k` MUST be a positive integer.
- `layers` MUST be a list of zero or more layer identifiers from Section 6.1. If omitted, the default MUST be `["base", "user", "delta", "local"]`.
- If `filters.kind` is provided, the server MUST only return chunks whose `kind` matches one of the supplied values.

Note: Some clients (including OpenAI tool schemas) require tool names to match `^[a-zA-Z0-9_-]+$`. Implementations MAY also accept the legacy dot-separated alias `agents.search`.

---

## 11. MCP Write API

### 11.1 Write Context Method

An MCP server conforming to this document MUST implement `agents_context_write` with parameters equivalent to:

```json
{
  "method": "agents_context_write",
  "params": {
    "content": "string",
    "kind": "derived-summary",
    "confidence": 0.0,
    "sources": [],
    "scope": "local | delta"
  }
}
```

- Writes MUST be append-only.
- `scope` MUST be either `local` or `delta`.
- The server MUST reject writes targeting `base` or `user`.
- The server MUST set `author` to `mcp` for records written through this method.

Note: Implementations MAY also accept the legacy dot-separated alias `agents.context.write`.

### 11.2 Propose Promotion Method

An MCP server conforming to this document MUST implement `agents_context_propose` with parameters equivalent to:

```json
{
  "method": "agents_context_propose",
  "params": {
    "context_id": "uint32",
    "target": "user"
  }
}
```

- `target` MUST be `user`.
- Promotion MUST require human or CI approval.
- The server MUST record the proposal as durable, append-only state inside a layer file (RECOMMENDED: append a `meta.proposal_event` chunk to `AGENTS.delta.db` with `sources` referencing the proposed `context_id`).
- Implementations MUST NOT create or rely on standalone JSON/JSONL “sidecar” files to record proposals or other durable workflow state; any durable state worth keeping MUST live inside `.db` layer files.

Note: Implementations MAY also accept the legacy dot-separated alias `agents.context.propose`.

---

## 12. Conflict Semantics

- The base layer MUST be treated as authoritative canonical context.
- When higher-precedence layers conflict with lower-precedence layers, the implementation MUST surface the conflict to the MCP client.
- The implementation MUST NOT automatically overwrite or delete existing chunks to resolve conflicts.

---

## 13. Security and Trust Considerations

- Layers MAY be cryptographically signed.
- Chunks authored by MCP (`author: "mcp"`) MUST include a `confidence` value as specified in Section 7.1.
- Tooling SHOULD support inspection and diffing of layer contents.

---

## 14. Implementation Notes (Informative)

- Memory mapping (`mmap`) can enable near-instant startup.
- Bitset-based filtering can outperform SQL-style filtering for common workloads.
- Layers can be union-mapped at runtime.
- Recompaction MAY be used to merge layers.

---

## 15. Example Tooling Flows (Informative)

This section illustrates end-to-end workflows. Command names are examples only.

### 15.1 Build `AGENTS.db` in CI

1. Collect canonical sources (e.g., `AGENTS.md`, curated notes, architecture docs).
2. Chunk and normalize content into chunk records.
3. Compute embeddings and compile a single immutable file `AGENTS.db`.
4. Publish `AGENTS.db` as a build artifact and/or commit it to the repository.

Example CLI-style flow:

```sh
# Compile canonical sources directly (no manifest left behind).
agentsdb compile --root . --include 'AGENTS.md' --out AGENTS.db

# Optional signing step.
agentsdb sign --in AGENTS.db --out AGENTS.db.sig
```

### 15.2 Serve Search Over Multiple Layers

An MCP server typically loads multiple layer files and executes `agents_search` over the union.

Example:

```sh
agentsdb serve \
  --base AGENTS.db \
  --user AGENTS.user.db \
  --delta AGENTS.delta.db \
  --local AGENTS.local.db
```

### 15.3 Agent Writes Derived Context Locally

An agent SHOULD write ephemeral, session-specific notes to the Local layer via `agents_context_write` with `scope: "local"`. This flow keeps derived context reviewable and avoids mutating canonical data.

Example MCP request:

```json
{
  "method": "agents_context_write",
  "params": {
    "content": "The repo uses X for Y; entrypoint is Z.",
    "kind": "derived-summary",
    "confidence": 0.7,
    "sources": ["docs/ARCHITECTURE.md:12"],
    "scope": "local"
  }
}
```

### 15.4 Propose Promotion to the Delta Layer

When the agent believes a derived chunk is broadly useful, it MAY write it to the Delta layer (`scope: "delta"`) and then propose promotion to the User layer.

Example:

```json
{
  "method": "agents_context_write",
  "params": {
    "content": "Invariant: requests to /foo MUST include header Bar.",
    "kind": "invariant",
    "confidence": 0.8,
    "sources": ["src/server/router.ts:88"],
    "scope": "delta"
  }
}
```

Then propose promotion:

```json
{
  "method": "agents_context_propose",
  "params": {
    "context_id": 1234,
    "target": "user"
  }
}
```

### 15.5 Human Review and Accept into the User Layer

Tooling SHOULD support reviewing the Delta layer, inspecting provenance, and diffing against Base/User.

Example CLI-style flow:

```sh
agentsdb diff --base AGENTS.db --delta AGENTS.delta.db
agentsdb inspect --layer AGENTS.delta.db --id 1234

# Accept selected records into the user layer.
agentsdb promote --from AGENTS.delta.db --to AGENTS.user.db --ids 1234,1250
```

### 15.6 Periodic Recompaction (Optional)

To keep query performance stable, CI MAY periodically rebuild `AGENTS.db` by merging approved content (e.g., User) and curated canonical sources, producing a new immutable base.

Example:

```sh
agentsdb compact \
  --base AGENTS.db \
  --user AGENTS.user.db \
  --out AGENTS.db.new
mv AGENTS.db.new AGENTS.db
```

If compaction is used, tooling SHOULD define how `AGENTS.user.db` and `AGENTS.delta.db` are rotated or truncated (e.g., archive then reset).

---

## 16. Open Questions (Informative)

- Standardized chunk kinds?
- `int4` embedding support?
- Distributed promotion workflows?
- Schema evolution strategy?

---

## 17. Summary (Informative)

This document specifies a compiled, layered context system for agents with read-only canonical knowledge, append-only learning, explicit trust boundaries, and high-performance vector search, enabling agents to learn safely without rewriting history.
