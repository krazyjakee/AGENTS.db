# The Mental Model

### **AGENTS.db = A living memory for your codebase**

Think of it as your project's institutional knowledge, made machine-readable:

> **Context that persists across sessions, team members, and AI conversations.**

## The Core Philosophy

**Code tells you *what* happens. AGENTS.db tells you *why*.**

Traditional documentation fails because:
- It lives separately from the code
- It becomes stale the moment it's written
- Nobody knows if it's still accurate
- Each new team member or AI session starts from zero

AGENTS.db is different. It's:
- **Version-controlled** alongside your code
- **Queryable** by both humans and AI
- **Layered** so experiments don't pollute truth
- **Append-only** so history is never lost

---

# The Workflow Mindset

Think in terms of knowledge evolution, not file management:

```
Truth → Experimentation → Discovery → Validation → Shared Knowledge
```

## 1. Establish Your Foundation

**The Goal:** Create a canonical source of truth from what you already know.

**Real Example:**
Your team has been building a multi-tenant SaaS platform. You've learned hard lessons about data isolation, but they're scattered across Slack threads, old PRs, and tribal knowledge.

You start by documenting these lessons in markdown files—architecture decisions, security boundaries, known gotchas. These become your canonical sources that get compiled into a base layer.

**Why This Matters:**
New engineers or AI assistants can now query "What are our tenant isolation rules?" and get accurate, versioned answers instead of outdated wiki pages or nothing at all.

---

## 2. Work Naturally, Capture Context

**The Goal:** Let knowledge emerge from actual work, not forced documentation.

**Real Example:**
You're debugging a caching issue. After three hours, you discover that cache keys must include the tenant_id or you'll leak data between customers. This is critical but undocumented.

Instead of hoping someone writes this in a doc someday (they won't), you capture it immediately as a working note. It's searchable within minutes, but not yet "official" knowledge.

**Why This Matters:**
Knowledge capture becomes a natural part of problem-solving, not a chore you do later (and forget). You're building a safety net in real-time.

---

## 3. Discover Through Questions

**The Goal:** Let curiosity drive knowledge discovery, not grep skills.

**Real Example:**
A new developer asks: "Can I refactor the auth token flow to share tokens across regions?"

Instead of interrupting senior engineers, they query the knowledge base. They discover that tokens must be globally unique across regions (a hard-learned lesson from a production incident six months ago). The decision record includes the incident link and reasoning.

**Why This Matters:**
Questions that used to require human interruption now get answered immediately with full context. Senior engineers' time is freed for new problems, not re-explaining old decisions.

---

## 4. Validate Discoveries as a Team

**The Goal:** Turn individual insights into shared truth through lightweight review.

**Real Example:**
During a refactoring, you realize that a particular API endpoint must never be called concurrently because it modifies shared state. This constraint isn't obvious from the code.

You propose this as a constraint in a delta layer—a staging area for discovered knowledge. In code review, the team confirms it's accurate and promotes it to the shared knowledge base.

**Why This Matters:**
Important discoveries don't get lost in PR comments. They become queryable facts that protect future changes. The review burden is minimal because the context is right there.

---

## 5. Build Confidence Through Layers

**The Goal:** Separate "I think" from "we know" without losing either.

**The Layers:**

- **Base**: The committed, canonical truth (built from your official docs)
- **User**: Team-validated knowledge that's proven over time
- **Delta**: Proposed knowledge under review
- **Local**: Your personal working notes and hunches

**Real Example:**
You're exploring whether to adopt GraphQL. You capture research notes locally—pros, cons, migration complexity. These are ephemeral "thinking out loud" notes.

As your understanding solidifies, you move key findings to delta for team review. After the team validates them, they become user-layer knowledge that informs future decisions.

**Why This Matters:**
You can be messy and experimental without polluting shared knowledge. But when you discover something real, there's a clear path to make it official. No knowledge is lost; confidence levels are explicit.

---

## 6. Protect Knowledge in Production

**The Goal:** Make knowledge infrastructure as reliable as code infrastructure.

**Real Example:**
Your CI pipeline validates that all knowledge layers are well-formed and accessible. A corrupted knowledge base fails the build just like broken tests would.

When deploying, your production systems can query the same knowledge base that developers use. If an automated system needs to know "Is this operation safe during peak hours?", it gets the same answer a human would.

**Why This Matters:**
Knowledge becomes infrastructure. It's tested, versioned, and deployed alongside code. Systems can make informed decisions without hardcoded rules or human judgment calls.

---

# Real-World Transformations

### Before AGENTS.db

**Scenario:** A security researcher joins your team.

*Day 1:* "What are the authentication boundaries?"
*Response:* "Let me find Alice... she worked on that last year..."
*Reality:* Alice is on vacation. The new hire reads code for three days and still isn't sure.

**Scenario:** Your LLM assistant tries to help with a refactoring.

*AI:* "I can optimize this by caching globally!"
*You:* "No wait, that breaks multi-tenancy..."
*Reality:* You explain the same constraint in every AI session because it has no memory.

---

### After AGENTS.db

**Scenario:** A security researcher joins your team.

*Day 1:* "What are the authentication boundaries?"
*Response:* They query the knowledge base and get a detailed explanation with links to implementation, decision records, and related constraints.
*Reality:* They're productive immediately. Alice can stay on vacation.

**Scenario:** Your LLM assistant tries to help with a refactoring.

*AI:* "I see there's a multi-tenancy constraint about global caching. Let me suggest an alternative..."
*Reality:* The AI has context from previous sessions and team knowledge. It suggests safe solutions from the start.

---

# The Everyday Experience

### Old Way
- "Does anyone know if this is safe?" *(Slack message, waiting...)*
- "Why is this weird check here?" *(Git archaeology, reading old PRs...)*
- "Ask Alice, she knows" *(Alice becomes a bottleneck)*

### New Way
- "Let me check what we know about this..." *(Query returns relevant constraints)*
- "Ah, there's a decision record from the 2024 incident" *(Full context in seconds)*
- "I'll propose this new finding for the team to review" *(Knowledge contribution is easy)*

---

# The Core Promise

**AGENTS.db doesn't replace documentation. It makes documentation queryable, versioned, and living.**

You're not maintaining yet another system—you're giving your existing knowledge a memory and a voice. The codebase can finally explain itself, not just to humans, but to the AI tools that help you build it.

---

# When It Clicks

You'll know AGENTS.db is working when:

1. **New team members stop asking the same questions** because they can find answers themselves
2. **Code reviews reference knowledge base entries** instead of re-explaining architectural decisions
3. **AI assistants suggest solutions that respect your constraints** because they have access to your institutional knowledge
4. **Production incidents lead to knowledge captures** that prevent the same mistake twice
5. **You trust your knowledge base** the way you trust your test suite

---

# One‑Sentence Summary

> **AGENTS.db transforms scattered tribal knowledge into queryable, versioned, machine-readable context that persists across people, tools, and time.**
