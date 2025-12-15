const $ = (id) => document.getElementById(id);
const state = { layers: [], selected: "" };

async function api(path, opts) {
  const res = await fetch(path, opts);
  if (!res.ok) throw new Error(await res.text());
  return res.headers.get("content-type")?.includes("application/json") ? res.json() : res.text();
}

function setStatus(msg) { $("status").textContent = msg; }

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

function escapeHtml(s) {
  return String(s).replaceAll("&","&amp;").replaceAll("<","&lt;").replaceAll(">","&gt;");
}

function escapeAttr(s) {
  return String(s).replaceAll("&","&amp;").replaceAll("\"","&quot;").replaceAll("<","&lt;").replaceAll(">","&gt;");
}

async function viewChunk(id) {
  const path = $("layer").value;
  const c = await api(`/api/layer/chunk?path=${encodeURIComponent(path)}&id=${id}`);
  $("viewer").style.display = "block";
  $("viewTitle").textContent = `id=${c.id} kind=${c.kind}${c.removed ? " (removed)" : ""}`;
  $("viewBody").textContent = c.content;
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

$("refresh").onclick = refreshLayers;
$("layer").onchange = async () => { await refreshMeta(); await loadChunks(); };
$("load").onclick = loadChunks;
$("addBtn").onclick = () => { $("addPanel").style.display = $("addPanel").style.display === "none" ? "block" : "none"; };
$("addCancel").onclick = () => { $("addPanel").style.display = "none"; };
$("addSubmit").onclick = addChunk;
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
  await api("/api/layer/add", { method:"POST", headers:{ "content-type":"application/json" }, body: JSON.stringify({ path, scope, id, kind, confidence, content }) });
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
