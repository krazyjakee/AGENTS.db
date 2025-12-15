const $ = (id) => document.getElementById(id);
const state = { layers: [], selected: "" };

async function api(path, opts) {
  const res = await fetch(path, opts);
  if (!res.ok) throw new Error(await res.text());
  return res.headers.get("content-type")?.includes("application/json") ? res.json() : res.text();
}

function setStatus(msg) { $("status").textContent = msg; }

async function refreshVersion() {
  const el = $("webVersion");
  if (!el) return;
  try {
    const out = await api("/api/version");
    if (out && out.version) el.textContent = `v${out.version}`;
  } catch {
    el.textContent = "v?";
  }
}

function escapeHtml(s) {
  return String(s).replaceAll("&","&amp;").replaceAll("<","&lt;").replaceAll(">","&gt;");
}

function escapeAttr(s) {
  return String(s).replaceAll("&","&amp;").replaceAll("\"","&quot;").replaceAll("<","&lt;").replaceAll(">","&gt;");
}

function safeHref(raw) {
  const href = String(raw || "").trim();
  const lower = href.toLowerCase();
  if (!href) return "#";
  if (lower.startsWith("http://") || lower.startsWith("https://") || lower.startsWith("mailto:")) return href;
  if (href.startsWith("#") || href.startsWith("/") || href.startsWith("./") || href.startsWith("../")) return href;
  return "#";
}

function renderEmphasis(escaped) {
  return String(escaped)
    .replaceAll(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>")
    .replaceAll(/\*([^*]+)\*/g, "<em>$1</em>");
}

function renderInline(raw) {
  const text = String(raw ?? "");
  let out = "";
  let i = 0;
  while (i < text.length) {
    const tick = text.indexOf("`", i);
    if (tick === -1) {
      out += renderInlineNoCode(text.slice(i));
      break;
    }
    const end = text.indexOf("`", tick + 1);
    if (end === -1) {
      out += renderInlineNoCode(text.slice(i));
      break;
    }
    out += renderInlineNoCode(text.slice(i, tick));
    out += `<code>${escapeHtml(text.slice(tick + 1, end))}</code>`;
    i = end + 1;
  }
  return out;
}

function renderInlineNoCode(raw) {
  const s = String(raw ?? "");
  let out = "";
  let idx = 0;
  const linkRe = /\[([^\]]+)\]\(([^)]+)\)/g;
  for (;;) {
    const m = linkRe.exec(s);
    if (!m) break;
    out += renderEmphasis(escapeHtml(s.slice(idx, m.index)));
    const label = renderEmphasis(escapeHtml(m[1]));
    const href = escapeAttr(safeHref(m[2]));
    out += `<a href="${href}" target="_blank" rel="noreferrer noopener">${label}</a>`;
    idx = m.index + m[0].length;
  }
  out += renderEmphasis(escapeHtml(s.slice(idx)));
  return out;
}

function renderMarkdown(md) {
  const lines = String(md ?? "").replaceAll("\r\n", "\n").split("\n");
  const out = [];
  let paragraph = [];
  let listType = "";
  let inCodeFence = false;
  let codeLang = "";
  let code = [];
  let inQuote = false;
  let quote = [];

  function flushParagraph() {
    if (!paragraph.length) return;
    const text = paragraph.join("\n").trim().replaceAll(/\n+/g, " ");
    out.push(`<p>${renderInline(text)}</p>`);
    paragraph = [];
  }

  function flushList() {
    if (!listType) return;
    out.push(listType === "ol" ? "</ol>" : "</ul>");
    listType = "";
  }

  function flushQuote() {
    if (!inQuote) return;
    const text = quote.join("\n").trim().replaceAll(/\n+/g, " ");
    out.push(`<blockquote>${text ? `<p>${renderInline(text)}</p>` : ""}</blockquote>`);
    inQuote = false;
    quote = [];
  }

  function closeBlocks() {
    flushParagraph();
    flushList();
    flushQuote();
  }

  for (const line of lines) {
    if (inCodeFence) {
      if (line.startsWith("```")) {
        const klass = codeLang ? ` class="language-${escapeAttr(codeLang)}"` : "";
        out.push(`<pre><code${klass}>${escapeHtml(code.join("\n"))}</code></pre>`);
        inCodeFence = false;
        codeLang = "";
        code = [];
      } else {
        code.push(line);
      }
      continue;
    }

    if (line.startsWith("```")) {
      closeBlocks();
      inCodeFence = true;
      codeLang = line.slice(3).trim();
      code = [];
      continue;
    }

    if (/^\s*$/.test(line)) {
      flushParagraph();
      flushList();
      flushQuote();
      continue;
    }

    const quoteMatch = line.match(/^\s*>\s?(.*)$/);
    if (quoteMatch) {
      flushParagraph();
      flushList();
      inQuote = true;
      quote.push(quoteMatch[1]);
      continue;
    }
    flushQuote();

    if (/^\s*((\*\s*){3,}|(-\s*){3,}|(_\s*){3,})$/.test(line)) {
      closeBlocks();
      out.push("<hr>");
      continue;
    }

    const headingMatch = line.match(/^(#{1,6})\s+(.*)$/);
    if (headingMatch) {
      closeBlocks();
      const lvl = headingMatch[1].length;
      out.push(`<h${lvl}>${renderInline(headingMatch[2].trim())}</h${lvl}>`);
      continue;
    }

    const ulMatch = line.match(/^\s*[-*+]\s+(.*)$/);
    if (ulMatch) {
      flushParagraph();
      flushQuote();
      if (listType && listType !== "ul") flushList();
      if (!listType) { listType = "ul"; out.push("<ul>"); }
      out.push(`<li>${renderInline(ulMatch[1].trim())}</li>`);
      continue;
    }

    const olMatch = line.match(/^\s*\d+\.\s+(.*)$/);
    if (olMatch) {
      flushParagraph();
      flushQuote();
      if (listType && listType !== "ol") flushList();
      if (!listType) { listType = "ol"; out.push("<ol>"); }
      out.push(`<li>${renderInline(olMatch[1].trim())}</li>`);
      continue;
    }

    paragraph.push(line);
  }

  if (inCodeFence) {
    const klass = codeLang ? ` class="language-${escapeAttr(codeLang)}"` : "";
    out.push(`<pre><code${klass}>${escapeHtml(code.join("\n"))}</code></pre>`);
  }
  closeBlocks();
  return out.join("\n");
}

async function refreshLayers() {
  const layers = await api("/api/layers");
  const prev = $("layer").value || state.selected;
  state.layers = layers;
  const sel = $("layer");
  sel.innerHTML = "";
  for (const l of layers) {
    const opt = document.createElement("option");
    opt.value = l.path;
    opt.textContent = `${l.path} (${l.chunk_count} docs, ${Math.round(l.file_length_bytes/1024)} KiB)`;
    sel.appendChild(opt);
  }
  if (prev && layers.some(l => l.path === prev)) sel.value = prev;
  state.selected = sel.value || (layers[0]?.path ?? "");
  await refreshMeta();
  if (state.selected) await loadChunks();
  await refreshProposals();
}

async function refreshMeta() {
  const path = $("layer").value;
  if (!path) { $("meta").textContent = "No layers found."; return; }
  const meta = await api(`/api/layer/meta?path=${encodeURIComponent(path)}`);
  $("meta").innerHTML = `
    <div><span class="pill">chunks</span> ${meta.chunk_count} <span class="pill">removed</span> ${meta.removed_count}</div>
    <div><span class="pill">embed</span> dim=${meta.embedding_dim} ${meta.embedding_element_type}</div>
    <div><span class="pill">conf</span> min=${meta.confidence_min.toFixed(2)} avg=${meta.confidence_avg.toFixed(2)} max=${meta.confidence_max.toFixed(2)}</div>
    <div style="margin-top:6px">${Object.entries(meta.kinds).map(([k,v]) => `<span class="pill">${k}</span> ${v}`).join(" ")}</div>
  `;
  const kindSel = $("kindFilter");
  const cur = kindSel.value;
  kindSel.innerHTML = `<option value="">(all)</option>` + Object.keys(meta.kinds).map(k => `<option value="${escapeAttr(k)}">${escapeHtml(k)}</option>`).join("");
  if (cur && Object.keys(meta.kinds).includes(cur)) kindSel.value = cur;
  setScopeSelectOptions($("addScope"), path);
  setScopeSelectOptions($("editScope"), path);
  setImportScopeOptions($("importScope"));
}

async function loadChunks() {
  const path = $("layer").value;
  if (!path) { setStatus("No layer selected."); return; }
  const offset = Number($("offset").value || 0);
  const limit = Number($("limit").value || 100);
  const includeRemoved = $("includeRemoved").value;
  const kind = $("kindFilter").value;
  setStatus("Loading…");
  const out = await api(`/api/layer/chunks?path=${encodeURIComponent(path)}&offset=${offset}&limit=${limit}&include_removed=${includeRemoved}&kind=${encodeURIComponent(kind)}`);
  setStatus(`Showing ${out.items.length} of ${out.total} (offset=${out.offset}, limit=${out.limit})`);
  const tbody = $("table").querySelector("tbody");
  tbody.innerHTML = "";
  for (const c of out.items) {
    const scope = writeScopeForPath(path);
    const tr = document.createElement("tr");
    tr.innerHTML = `
      <td class="mono">${c.id}${c.removed ? ' <span class="pill danger">removed</span>' : ''}</td>
      <td><span class="pill">${escapeHtml(c.kind)}</span></td>
      <td class="mono">${c.confidence.toFixed(2)}</td>
      <td class="mono">${escapeHtml(c.content_preview)}</td>
      <td class="actions">
        <button data-act="view" data-id="${c.id}" class="iconOnly" title="View chunk" aria-label="View">
          <svg class="iconSvg" viewBox="0 0 24 24" aria-hidden="true">
            <path d="M2 12s4-7 10-7 10 7 10 7-4 7-10 7S2 12 2 12z"/>
            <path d="M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6z"/>
          </svg>
        </button>
        <button data-act="edit" data-id="${c.id}" class="iconOnly secondary" title="Edit (append new version)" aria-label="Edit" ${scope ? "" : "disabled"}>
          <svg class="iconSvg" viewBox="0 0 24 24" aria-hidden="true">
            <path d="M12 20h9"/>
            <path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4Z"/>
          </svg>
        </button>
        <button data-act="remove" data-id="${c.id}" class="iconOnly secondary" title="Remove (soft delete)" aria-label="Remove" ${scope ? "" : "disabled"}>
          <svg class="iconSvg" viewBox="0 0 24 24" aria-hidden="true">
            <path d="M3 6h18"/>
            <path d="M8 6V4h8v2"/>
            <path d="M19 6l-1 14H6L5 6"/>
            <path d="M10 11v6"/>
            <path d="M14 11v6"/>
          </svg>
        </button>
      </td>
    `;
    tbody.appendChild(tr);
  }
}

async function viewChunk(id) {
  const path = $("layer").value;
  const c = await api(`/api/layer/chunk?path=${encodeURIComponent(path)}&id=${id}`);
  $("viewer").style.display = "block";
  $("viewTitle").textContent = `id=${c.id} kind=${c.kind}${c.removed ? " (removed)" : ""}`;
  $("viewBody").innerHTML = renderMarkdown(c.content);
  showViewActionsForChunk(c);
}

function canProposeOrPromoteCurrentView() {
  return $("layer").value === "AGENTS.local.db" || $("layer").value === "AGENTS.user.db" || $("layer").value === "AGENTS.delta.db";
}

function showViewActionsForChunk(c) {
  const enabled = canProposeOrPromoteCurrentView() && !c.removed;
  $("viewActions").style.display = enabled ? "block" : "none";
  $("viewActions").dataset.id = String(c.id);
  setTargetSelectOptions($("promoteTarget"), $("layer").value);
  if (!$("promoteTarget").value) {
    $("viewActions").style.display = "none";
  }
  $("proposePanel").style.display = "none";
  $("proposeTitle").value = "";
  $("proposeWhy").value = "";
  $("proposeWhat").value = "";
  $("proposeWhere").value = "";
}

async function refreshProposals() {
  const panel = $("proposalsPanel");
  try {
    const rows = await api("/api/proposals");
    panel.style.display = rows.length ? "block" : "none";
    if (!rows.length) return;

    const tbody = $("proposalsTable").querySelector("tbody");
    tbody.innerHTML = "";
    const pending = rows.filter(r => r.status === "pending").length;
    $("proposalsStatus").textContent = `${pending} pending (${rows.length} total shown)`;
    for (const p of rows) {
      const tr = document.createElement("tr");
      const title = (p.title || "").trim();
      const titlePreview = title ? title : "(no title)";
      const pendingRow = p.status === "pending";
      const flow = `${p.from_path} → ${p.to_path}`;
      tr.innerHTML = `
        <td><input type="checkbox" data-prop="sel" data-id="${p.proposal_id}" ${pendingRow ? "" : "disabled"}></td>
        <td class="mono">${p.proposal_id}</td>
        <td class="mono">${p.context_id}</td>
        <td class="mono">${escapeHtml(flow)}</td>
        <td><span class="pill">${escapeHtml(p.status)}</span></td>
        <td class="mono">${escapeHtml(titlePreview)}</td>
        <td class="actions">
          <button data-prop="view" data-id="${p.context_id}" data-from="${escapeAttr(p.from_path)}" class="iconOnly" title="View in source layer" aria-label="View proposal chunk">
            <svg class="iconSvg" viewBox="0 0 24 24" aria-hidden="true">
              <path d="M2 12s4-7 10-7 10 7 10 7-4 7-10 7S2 12 2 12z"/>
              <path d="M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6z"/>
            </svg>
          </button>
          <button data-prop="accept" data-id="${p.proposal_id}" class="iconOnly secondary" title="Accept (promote)" aria-label="Accept" ${pendingRow ? "" : "disabled"}>
            <svg class="iconSvg" viewBox="0 0 24 24" aria-hidden="true">
              <path d="M20 6L9 17l-5-5"/>
            </svg>
          </button>
          <button data-prop="reject" data-id="${p.proposal_id}" class="iconOnly secondary" title="Reject" aria-label="Reject" ${pendingRow ? "" : "disabled"}>
            <svg class="iconSvg" viewBox="0 0 24 24" aria-hidden="true">
              <path d="M18 6L6 18"/>
              <path d="M6 6l12 12"/>
            </svg>
          </button>
        </td>
      `;
      tbody.appendChild(tr);
    }
  } catch (err) {
    panel.style.display = "block";
    $("proposalsStatus").textContent = `Failed to load proposals: ${err}`;
  }
}

function selectedProposalIds() {
  const inputs = $("proposalsTable").querySelectorAll('input[type="checkbox"][data-prop="sel"]');
  const ids = [];
  for (const el of inputs) {
    if (el.checked) ids.push(Number(el.getAttribute("data-id")));
  }
  return ids;
}

async function acceptProposalIds(ids) {
  if (!ids.length) { alert("No proposals selected."); return; }
  const skipExisting = confirm("Skip ids already present in the destination layer?");
  const out = await api("/api/proposals/accept", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ ids, skip_existing: skipExisting }),
  });
  if (out && out.out_path) {
    alert(`Wrote ${out.out_path} (base is read-only; replace AGENTS.db manually if desired).`);
  }
  await refreshLayers();
  await refreshProposals();
}

async function rejectProposalIds(ids) {
  if (!ids.length) { alert("No proposals selected."); return; }
  const reason = prompt("Reject reason (optional):") || "";
  await api("/api/proposals/reject", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ ids, reason: reason.trim() || undefined }),
  });
  await refreshProposals();
}

async function removeChunk(id) {
  const path = $("layer").value;
  const scope = writeScopeForPath(path);
  if (!scope) { alert("Remove is only supported for AGENTS.local.db / AGENTS.delta.db"); return; }
  if (!confirm("Remove is a soft-delete (tombstone append). Continue?")) return;
  await api("/api/layer/remove", { method:"POST", headers:{ "content-type":"application/json" }, body: JSON.stringify({ path, scope, id: Number(id) }) });
  await refreshMeta();
  await loadChunks();
}

async function addChunk() {
  const path = $("layer").value;
  const scope = $("addScope").value || writeScopeForPath(path);
  if (!scope) { alert("Add is only supported for AGENTS.local.db / AGENTS.delta.db"); return; }
  const kind = $("addKind").value.trim();
  const confidence = Number($("addConfidence").value);
  const content = $("addContent").value;
  await api("/api/layer/add", { method:"POST", headers:{ "content-type":"application/json" }, body: JSON.stringify({ path, scope, kind, confidence, content }) });
  $("addPanel").style.display = "none";
  $("addContent").value = "";
  await refreshMeta();
  await loadChunks();
}

function writeScopeForPath(path) {
  if (path === "AGENTS.local.db") return "local";
  if (path === "AGENTS.delta.db") return "delta";
  return "";
}

function setTargetSelectOptions(selectEl, fromPath) {
  selectEl.innerHTML = "";
  // Common promotion paths:
  // local -> delta|user
  // delta -> user|base
  // user -> delta
  if (fromPath === "AGENTS.local.db") {
    selectEl.innerHTML = `
      <option value="AGENTS.user.db">AGENTS.user.db</option>
      <option value="AGENTS.delta.db">AGENTS.delta.db</option>
    `;
  } else if (fromPath === "AGENTS.user.db") {
    selectEl.innerHTML = `<option value="AGENTS.delta.db">AGENTS.delta.db</option>`;
  } else if (fromPath === "AGENTS.delta.db") {
    selectEl.innerHTML = `
      <option value="AGENTS.user.db">AGENTS.user.db</option>
      <option value="AGENTS.db">AGENTS.db</option>
    `;
  }
}

function setScopeSelectOptions(selectEl, path) {
  const scope = writeScopeForPath(path);
  selectEl.innerHTML = "";
  if (!scope) return;
  if (scope === "local") {
    selectEl.innerHTML = `<option value="local">local (AGENTS.local.db)</option>`;
  } else if (scope === "delta") {
    selectEl.innerHTML = `<option value="delta">delta (AGENTS.delta.db)</option>`;
  }
}

function setImportScopeOptions(selectEl) {
  const cur = selectEl.value;
  selectEl.innerHTML = `
    <option value="local">local → AGENTS.local.db</option>
    <option value="delta">delta → AGENTS.delta.db</option>
    <option value="user">user → AGENTS.user.db</option>
    <option value="base">base → AGENTS.db (danger)</option>
  `;
  if (cur) selectEl.value = cur;
}

async function downloadExport() {
  const path = $("layer").value;
  if (!path) { alert("No layer selected."); return; }
  const format = $("exportFormat").value || "json";
  const redact = $("exportRedact").value || "none";
  const res = await fetch(`/api/export?path=${encodeURIComponent(path)}&format=${encodeURIComponent(format)}&redact=${encodeURIComponent(redact)}`);
  if (!res.ok) throw new Error(await res.text());
  const blob = await res.blob();
  const ext = format === "ndjson" ? "ndjson" : "json";
  const a = document.createElement("a");
  a.href = URL.createObjectURL(blob);
  a.download = `${path}.${ext}`;
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(a.href), 1000);
}

async function importExportFile() {
  const file = $("importFile").files?.[0];
  if (!file) { alert("Choose a file to import."); return; }
  const scope = $("importScope").value;
  const path = scope === "base" ? "AGENTS.db" : (scope === "user" ? "AGENTS.user.db" : (scope === "delta" ? "AGENTS.delta.db" : "AGENTS.local.db"));
  const name = (file.name || "").toLowerCase();
  const format = name.endsWith(".ndjson") ? "ndjson" : "json";
  const data = await file.text();
  let allowBase = false;
  if (scope === "base") {
    const typed = prompt("Type AGENTS.db to confirm writing to the base layer:") || "";
    if (typed !== "AGENTS.db") { alert("Canceled."); return; }
    allowBase = true;
  } else {
    if (!confirm(`Append imported chunks into ${path}?`)) return;
  }
  await api("/api/import", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      path,
      scope,
      format,
      data,
      dedupe: true,
      preserve_ids: false,
      allow_base: allowBase,
    }),
  });
  $("importFile").value = "";
  await refreshLayers();
}

$("refresh").onclick = refreshLayers;
$("layer").onchange = async () => { await refreshMeta(); await loadChunks(); };
$("load").onclick = loadChunks;
$("addBtn").onclick = () => { $("addPanel").style.display = $("addPanel").style.display === "none" ? "block" : "none"; };
$("addCancel").onclick = () => { $("addPanel").style.display = "none"; };
$("addSubmit").onclick = addChunk;
$("exportBtn").onclick = () => downloadExport().catch(err => alert(String(err)));
$("importBtn").onclick = () => importExportFile().catch(err => alert(String(err)));
$("closeView").onclick = () => { $("viewer").style.display = "none"; };
$("closeEdit").onclick = () => { $("editor").style.display = "none"; };
$("kindFilter").onchange = loadChunks;
$("includeRemoved").onchange = loadChunks;
$("offset").onchange = loadChunks;
$("limit").onchange = loadChunks;
$("prevPage").onclick = async () => {
  const limit = Number($("limit").value || 100);
  $("offset").value = String(Math.max(0, Number($("offset").value || 0) - limit));
  await loadChunks();
};
$("nextPage").onclick = async () => {
  const limit = Number($("limit").value || 100);
  $("offset").value = String(Math.max(0, Number($("offset").value || 0) + limit));
  await loadChunks();
};
$("addContent").addEventListener("keydown", async (e) => {
  if ((e.ctrlKey || e.metaKey) && e.key === "Enter") {
    e.preventDefault();
    await addChunk();
  }
});
$("editContent").addEventListener("keydown", async (e) => {
  if ((e.ctrlKey || e.metaKey) && e.key === "Enter") {
    e.preventDefault();
    await editChunkSubmit();
  }
});
document.addEventListener("keydown", (e) => {
  if (e.key !== "Escape") return;
  $("viewer").style.display = "none";
  $("addPanel").style.display = "none";
  $("editor").style.display = "none";
  $("proposePanel").style.display = "none";
});
$("table").onclick = async (e) => {
  const btn = e.target.closest("button");
  if (!btn) return;
  const act = btn.getAttribute("data-act");
  const id = btn.getAttribute("data-id");
  if (act === "view") await viewChunk(id);
  if (act === "remove") await removeChunk(id);
  if (act === "edit") await openEditor(id);
};

refreshLayers().catch(err => setStatus(String(err)));
refreshVersion();

$("refreshProposals").onclick = refreshProposals;
$("acceptSelected").onclick = async () => { await acceptProposalIds(selectedProposalIds()); };
$("rejectSelected").onclick = async () => { await rejectProposalIds(selectedProposalIds()); };

$("proposalsTable").onclick = async (e) => {
  const btn = e.target.closest("button");
  if (!btn) return;
  const act = btn.getAttribute("data-prop");
  const id = Number(btn.getAttribute("data-id") || "0");
  if (!id) return;
  if (act === "view") {
    const target = btn.getAttribute("data-from") || "AGENTS.delta.db";
    if ($("layer").value !== target) {
      $("layer").value = target;
      await refreshMeta();
      await loadChunks();
    }
    await viewChunk(id);
  }
  if (act === "accept") await acceptProposalIds([id]);
  if (act === "reject") await rejectProposalIds([id]);
};

async function openEditor(id) {
  const path = $("layer").value;
  const scope = writeScopeForPath(path);
  if (!scope) { alert("Edit is only supported for AGENTS.local.db / AGENTS.delta.db"); return; }
  const c = await api(`/api/layer/chunk?path=${encodeURIComponent(path)}&id=${id}`);
  $("viewer").style.display = "none";
  $("editor").style.display = "block";
  $("editTitle").textContent = `edit id=${c.id} kind=${c.kind}${c.removed ? " (removed)" : ""}`;
  $("editor").dataset.id = String(c.id);
  setScopeSelectOptions($("editScope"), path);
  $("editKind").value = c.kind;
  $("editConfidence").value = String(c.confidence.toFixed(2));
  $("editContent").value = c.content;
}

async function editChunkSubmit() {
  const path = $("layer").value;
  const scope = $("editScope").value;
  const id = Number($("editor").dataset.id || "0");
  const kind = $("editKind").value.trim();
  const confidence = Number($("editConfidence").value);
  const content = $("editContent").value;
  await api("/api/layer/add", { method:"POST", headers:{ "content-type":"application/json" }, body: JSON.stringify({ path, scope, id, kind, confidence, content, tombstone_old: true }) });
  $("editor").style.display = "none";
  await refreshMeta();
  await loadChunks();
}

async function editChunkTombstone() {
  const path = $("layer").value;
  const scope = $("editScope").value;
  const id = Number($("editor").dataset.id || "0");
  if (!confirm("Append tombstone for the old record id?")) return;
  await api("/api/layer/remove", { method:"POST", headers:{ "content-type":"application/json" }, body: JSON.stringify({ path, scope, id }) });
  $("editor").style.display = "none";
  await refreshMeta();
  await loadChunks();
}

$("editSubmit").onclick = editChunkSubmit;
$("editTombstone").onclick = editChunkTombstone;

$("proposeBtn").onclick = () => { $("proposePanel").style.display = "block"; };
$("proposeCancel").onclick = () => { $("proposePanel").style.display = "none"; };
$("proposeSubmit").onclick = async () => {
  const id = Number($("viewActions").dataset.id || "0");
  if (!id) return;
  const from_path = $("layer").value;
  const to_path = $("promoteTarget").value;
  await api("/api/proposals/propose", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      context_id: id,
      from_path,
      to_path,
      title: $("proposeTitle").value.trim() || undefined,
      why: $("proposeWhy").value.trim() || undefined,
      what: $("proposeWhat").value.trim() || undefined,
      where: $("proposeWhere").value.trim() || undefined,
    }),
  });
  $("proposePanel").style.display = "none";
  await refreshProposals();
};
$("promoteBtn").onclick = async () => {
  const id = Number($("viewActions").dataset.id || "0");
  if (!id) return;
  const skipExisting = confirm("Skip id if it's already present in destination?");
  const from_path = $("layer").value;
  const to_path = $("promoteTarget").value;
  const out = await api("/api/promote/batch", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ from_path, to_path, ids: [id], skip_existing: skipExisting }),
  });
  if (out && out.out_path) {
    alert(`Wrote ${out.out_path} (base is read-only; replace AGENTS.db manually if desired).`);
  }
  await refreshLayers();
  await refreshProposals();
};
