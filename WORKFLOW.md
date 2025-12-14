# The Mental Model

### **AGENTS.db = A layered context store**
Think of it as:

> **A compiled, versioned context artifact optimized for tools and agents.**

Key ideas:

- **Code is behavior**
- **AGENTS.db layers store intent, constraints, and history**
- **Layers are append-only (no in-place mutation)**

Instead of:
- hoping reviewers remember invariants  
- expecting new hires to read stale docs  
- re‑explaining architecture to every LLM session  

You maintain a **machine-readable context artifact** that:
- Tools can validate (`agentsdb validate`)
- Agents/LLMs can query (`agentsdb search` or MCP via `agentsdb serve`)
- Humans can curate via append-only layers

---

# The Workflow (End‑to‑End)

At a high level, the workflow is:

```
Canonical sources → Base compiled → Local notes → Delta proposals → User acceptance
```

Broken down:

---

## 1. Compile a Base Layer (Canonical, Immutable)

The “compiler” is driven by canonical sources you choose (by default `AGENTS.md` files).

Shortcut (wide net docs + compile, no manifest left behind):

```sh
agentsdb init
```

Compile canonical sources into the base layer (no manifest left behind):

```sh
agentsdb compile --root . --include AGENTS.md --out AGENTS.db --dim 128 --element-type f32
```

✅ Produces: `AGENTS.db` (Base layer)  
✅ Commit it  
✅ `agentsdb validate AGENTS.db` confirms it’s readable/well-formed

Note: `agentsdb compile` appends by default; use `--replace` when rebuilding a base layer from scratch.

---

## 2. Query the Context (CLI or MCP)

Engineers keep working as usual:

```text
“Can I refactor the auth token flow?”
“What breaks if I change this cache?”
```

Behind the scenes:
- Tools search one or more layers
- Higher-precedence layers override lower ones

Search just the base layer:

```sh
agentsdb search --base AGENTS.db --query "token lifecycle" -k 5
```

Or start an MCP server (stdio) for an MCP-capable host:

```sh
agentsdb serve --base AGENTS.db --local AGENTS.local.db --delta AGENTS.delta.db --user AGENTS.user.db
```

---

## 3. Capture Working Notes Locally (Zero Risk)

During a session, you may want to store useful notes/summaries that are:

- Not committed
- Easy to discard
- Still searchable

Append a chunk to the Local layer:

```sh
agentsdb write AGENTS.local.db \
  --scope local \
  --kind note \
  --content "The cache key must include tenant_id; see src/cache.rs:42." \
  --confidence 0.7 \
  --dim 128 \
  --source "src/cache.rs:42"
```

✅ Writes to: `AGENTS.local.db`

Rules:
- ❌ Not committed
- ❌ Not trusted
- ✅ Safe to ignore

---

## 4. Propose Changes via a Delta Layer (Light Review)

When something *real* is discovered:

- “Oh wow, this invariant isn’t documented anywhere”
- “This ordering constraint matters”

You can write directly to the Delta layer:

```sh
agentsdb write AGENTS.delta.db \
  --scope delta \
  --kind invariant \
  --content "Auth tokens must be globally unique across regions." \
  --confidence 0.9 \
  --dim 128 \
  --source "docs/RFC.md:1"
```

Or (common) promote selected chunks from Local → Delta after you’ve iterated:

```sh
agentsdb promote --from AGENTS.local.db --to AGENTS.delta.db --ids 3,4,9
```

To review what’s in a layer:

```sh
agentsdb inspect --layer AGENTS.delta.db
agentsdb inspect --layer AGENTS.delta.db --id 3
```

To compare Base vs Delta by chunk id:

```sh
agentsdb diff --base AGENTS.db --delta AGENTS.delta.db
```

---

## 5. Accept Proposals (Append-Only)

Acceptance is append-only: you copy selected Delta chunks into the User layer.

```sh
agentsdb promote --from AGENTS.delta.db --to AGENTS.user.db --ids 3,4
```

✅ `AGENTS.user.db` is durable and append-only  
✅ Base changes still happen by editing canonical sources (e.g. `AGENTS.md`) and rebuilding `AGENTS.db`

---

## 6. Validate in CI (Minimal, Deterministic)

At minimum, CI can validate that layer files are readable and well-formed:

```sh
agentsdb validate AGENTS.db
agentsdb validate AGENTS.user.db   # if present
agentsdb validate AGENTS.delta.db  # if present
```

---

# The Everyday Engineer Experience

### Before
- “Does anyone know if this is safe?”
- “Why is this weird check here?”
- “Ask Alice, she knows”

### After
- “What context do we have on this?”
- “Is there a relevant invariant/decision chunk?”
- “The LLM already explained why”

---

# The Core Loop (One Diagram)

```
[ Canonical sources (e.g. AGENTS.md) ]
              ↓
[ agentsdb compile ]
              ↓
[ Base / Local / Delta / User layers ]
              ↓
[ agentsdb search / agentsdb serve / CI validate ]
```

---

# One‑Sentence Workflow Summary

> **AGENTS.db turns project context into append-only layers that you can compile, search, validate, and selectively promote.**
