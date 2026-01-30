use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::util::one_line;

const PROPOSAL_EVENT_KIND: &str = "meta.proposal_event";

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[derive(Debug, Clone)]
/// Represents the resolved paths for delta, user, and proposals layers.
struct ResolvedPaths {
    delta: PathBuf,
    user: PathBuf,
    proposals_layer: PathBuf,
}

fn resolve_paths(
    dir: &Path,
    delta: Option<&str>,
    user: Option<&str>,
    proposals_layer: Option<&str>,
) -> ResolvedPaths {
    let standard = agentsdb_embeddings::config::standard_layer_paths_for_dir(dir);
    let delta = delta.map(PathBuf::from).unwrap_or(standard.delta);
    let user = user.map(PathBuf::from).unwrap_or(standard.user);
    let proposals_layer = proposals_layer
        .map(PathBuf::from)
        .unwrap_or_else(|| delta.clone());
    ResolvedPaths {
        delta,
        user,
        proposals_layer,
    }
}

fn resolve_under_dir(dir: &Path, s: &str) -> PathBuf {
    let p = Path::new(s);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        dir.join(p)
    }
}

fn resolve_layer_label(dir: &Path, paths: &ResolvedPaths, label: &str) -> PathBuf {
    match label {
        "AGENTS.delta.db" => paths.delta.clone(),
        "AGENTS.user.db" => paths.user.clone(),
        _ => resolve_under_dir(dir, label),
    }
}

#[derive(Debug, Clone, Deserialize)]
/// Represents a proposal event, such as a proposal, acceptance, or rejection.
///
/// This struct is deserialized from the `meta.proposal_event` chunk content.
struct ProposalEvent {
    #[serde(default)]
    action: Option<String>, // propose | accept | reject
    #[serde(default)]
    proposal_id: Option<u32>, // for accept/reject
    context_id: u32,
    #[serde(default)]
    from_path: Option<String>,
    #[serde(default)]
    to_path: Option<String>,
    #[serde(default)]
    created_at_unix_ms: Option<u64>,

    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    why: Option<String>,
    #[serde(default)]
    what: Option<String>,
    #[serde(default, rename = "where")]
    where_: Option<String>,

    #[serde(default)]
    actor: Option<String>,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    outcome: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
/// Represents the current status of a proposal.
enum ProposalStatus {
    Pending,
    Accepted,
    Rejected,
}

#[derive(Debug, Clone)]
/// Represents the accumulated state of a proposal, derived from a series of `ProposalEvent`s.
struct ProposalState {
    proposal_id: u32,
    context_id: u32,
    from_path: String,
    to_path: String,
    status: ProposalStatus,
    created_at_unix_ms: Option<u64>,
    title: Option<String>,
    why: Option<String>,
    what: Option<String>,
    where_: Option<String>,
    decided_at_unix_ms: Option<u64>,
    decided_by: Option<String>,
    decision_reason: Option<String>,
    decision_outcome: Option<String>,
}

fn read_proposal_events(path: &Path) -> anyhow::Result<Vec<(u32, ProposalEvent)>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let f = agentsdb_format::LayerFile::open(path)
        .with_context(|| format!("open {}", path.display()))?;
    let mut out = Vec::new();
    for chunk in f.chunks() {
        let chunk = chunk?;
        if chunk.kind != PROPOSAL_EVENT_KIND {
            continue;
        }
        let ev: ProposalEvent = serde_json::from_str(chunk.content)
            .with_context(|| format!("parse proposal event chunk {}", chunk.id))?;
        out.push((chunk.id, ev));
    }
    Ok(out)
}

fn apply_event(map: &mut BTreeMap<u32, ProposalState>, event_id: u32, ev: ProposalEvent) {
    let action = ev.action.as_deref().unwrap_or("propose");
    match action {
        "propose" => {
            let from_path = ev
                .from_path
                .unwrap_or_else(|| "AGENTS.delta.db".to_string());
            let to_path = ev.to_path.unwrap_or_else(|| "AGENTS.user.db".to_string());
            map.insert(
                event_id,
                ProposalState {
                    proposal_id: event_id,
                    context_id: ev.context_id,
                    from_path,
                    to_path,
                    status: ProposalStatus::Pending,
                    created_at_unix_ms: ev.created_at_unix_ms,
                    title: ev.title,
                    why: ev.why,
                    what: ev.what,
                    where_: ev.where_,
                    decided_at_unix_ms: None,
                    decided_by: None,
                    decision_reason: None,
                    decision_outcome: None,
                },
            );
        }
        "accept" | "reject" => {
            let Some(proposal_id) = ev.proposal_id else {
                return;
            };
            if let Some(state) = map.get_mut(&proposal_id) {
                state.status = if action == "accept" {
                    ProposalStatus::Accepted
                } else {
                    ProposalStatus::Rejected
                };
                state.decided_at_unix_ms = ev.created_at_unix_ms;
                state.decided_by = ev.actor;
                state.decision_reason = ev.reason;
                state.decision_outcome = ev.outcome;
            }
        }
        _other => {}
    }
}

fn load_states(proposals_layer_path: &Path) -> anyhow::Result<BTreeMap<u32, ProposalState>> {
    let events = read_proposal_events(proposals_layer_path)?;
    let mut map: BTreeMap<u32, ProposalState> = BTreeMap::new();
    for (event_id, ev) in events {
        apply_event(&mut map, event_id, ev);
    }
    Ok(map)
}

fn read_layer_ids(path: &Path) -> anyhow::Result<BTreeSet<u32>> {
    if !path.exists() {
        return Ok(BTreeSet::new());
    }
    let f = agentsdb_format::LayerFile::open(path)
        .with_context(|| format!("open {}", path.display()))?;
    let chunks = agentsdb_format::read_all_chunks(&f)?;
    Ok(chunks.into_iter().map(|c| c.id).collect())
}

fn append_decision_event(
    proposals_layer_path: &Path,
    action: &str,
    proposal_id: u32,
    context_id: u32,
    outcome: Option<&str>,
    reason: Option<&str>,
) -> anyhow::Result<()> {
    let now_ms = now_unix_ms();
    let record = serde_json::json!({
        "action": action,
        "proposal_id": proposal_id,
        "context_id": context_id,
        "created_at_unix_ms": now_ms,
        "actor": "human",
        "outcome": outcome,
        "reason": reason,
    });

    let file = agentsdb_format::LayerFile::open(proposals_layer_path).with_context(|| {
        format!(
            "open proposal events layer {}",
            proposals_layer_path.display()
        )
    })?;
    let dim = file.embedding_dim();
    let mut chunk = agentsdb_format::ChunkInput {
        id: 0,
        kind: PROPOSAL_EVENT_KIND.to_string(),
        content: serde_json::to_string(&record).context("serialize decision event")?,
        author: "human".to_string(),
        confidence: 1.0,
        created_at_unix_ms: now_ms,
        embedding: vec![0.0; dim],
        sources: vec![agentsdb_format::ChunkSource::ChunkId(context_id)],
    };
    agentsdb_format::append_layer_atomic(
        proposals_layer_path,
        std::slice::from_mut(&mut chunk),
        None,
    )
    .context("append decision event")?;
    Ok(())
}

pub(crate) fn cmd_proposals_list(
    dir: &str,
    delta: Option<&str>,
    user: Option<&str>,
    proposals_layer: Option<&str>,
    all: bool,
    json: bool,
) -> anyhow::Result<()> {
    let dir = Path::new(dir);
    let paths = resolve_paths(dir, delta, user, proposals_layer);
    let states = load_states(&paths.proposals_layer)?;

    let mut out = Vec::new();
    for s in states.values() {
        if !all && !matches!(s.status, ProposalStatus::Pending) {
            continue;
        }
        let from_ids = read_layer_ids(&resolve_layer_label(dir, &paths, &s.from_path))
            .with_context(|| format!("read ids from {}", s.from_path))?;
        let to_ids = read_layer_ids(&resolve_layer_label(dir, &paths, &s.to_path))
            .with_context(|| format!("read ids from {}", s.to_path))?;
        out.push((
            s.clone(),
            from_ids.contains(&s.context_id),
            to_ids.contains(&s.context_id),
        ));
    }

    if json {
        #[derive(Serialize)]
        struct Row {
            proposal_id: u32,
            context_id: u32,
            from_path: String,
            to_path: String,
            status: ProposalStatus,
            created_at_unix_ms: Option<u64>,
            title: Option<String>,
            why: Option<String>,
            what: Option<String>,
            #[serde(rename = "where")]
            where_: Option<String>,
            exists_in_source: bool,
            exists_in_target: bool,
        }
        let rows: Vec<Row> = out
            .into_iter()
            .map(|(s, exists_in_source, exists_in_target)| Row {
                proposal_id: s.proposal_id,
                context_id: s.context_id,
                from_path: s.from_path,
                to_path: s.to_path,
                status: s.status,
                created_at_unix_ms: s.created_at_unix_ms,
                title: s.title,
                why: s.why,
                what: s.what,
                where_: s.where_,
                exists_in_source,
                exists_in_target,
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }

    let pending = out
        .iter()
        .filter(|(s, _, _)| matches!(s.status, ProposalStatus::Pending))
        .count();
    if all {
        println!("Proposals: {} total", out.len());
    } else {
        println!("Proposals: {} pending", pending);
    }
    for (s, exists_in_source, exists_in_target) in out {
        let title = s
            .title
            .as_deref()
            .map(|t| format!(" - {}", one_line(t)))
            .unwrap_or_default();
        let mut flags = Vec::new();
        if !exists_in_source {
            flags.push("missing-in-source");
        }
        if exists_in_target {
            flags.push("already-in-target");
        }
        if flags.is_empty() {
            println!(
                "  - proposal {}: {} {} -> {}{}",
                s.proposal_id, s.context_id, s.from_path, s.to_path, title
            );
        } else {
            println!(
                "  - proposal {}: {} {} -> {}{} ({})",
                s.proposal_id,
                s.context_id,
                s.from_path,
                s.to_path,
                title,
                flags.join(", ")
            );
        }
    }
    Ok(())
}

pub(crate) fn cmd_proposals_show(
    dir: &str,
    delta: Option<&str>,
    user: Option<&str>,
    proposals_layer: Option<&str>,
    id: u32,
    json: bool,
) -> anyhow::Result<()> {
    let dir = Path::new(dir);
    let paths = resolve_paths(dir, delta, user, proposals_layer);
    let states = load_states(&paths.proposals_layer)?;
    let Some(state) = states.get(&id) else {
        anyhow::bail!("proposal {id} not found");
    };

    let from_abs = resolve_layer_label(dir, &paths, &state.from_path);
    let from_file = agentsdb_format::LayerFile::open(&from_abs)
        .with_context(|| format!("open {}", from_abs.display()))?;
    let chunk = agentsdb_format::read_all_chunks(&from_file)?
        .into_iter()
        .find(|c| c.id == state.context_id)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "chunk {} not found in {}",
                state.context_id,
                state.from_path
            )
        })?;

    if json {
        #[derive(Serialize)]
        struct Out {
            proposal: ProposalStateJson,
            chunk: ChunkJson,
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&Out {
                proposal: ProposalStateJson::from(state.clone()),
                chunk: ChunkJson::from(chunk),
            })?
        );
        return Ok(());
    }

    println!("Proposal: {}", state.proposal_id);
    println!("From: {}", state.from_path);
    println!("To: {}", state.to_path);
    println!("Status: {:?}", state.status);
    println!("Context id: {}", state.context_id);
    if let Some(t) = state.title.as_deref() {
        println!("Title: {}", one_line(t));
    }
    if let Some(why) = state.why.as_deref() {
        println!("Why: {}", one_line(why));
    }
    if let Some(what) = state.what.as_deref() {
        println!("What: {}", one_line(what));
    }
    if let Some(where_) = state.where_.as_deref() {
        println!("Where: {}", one_line(where_));
    }
    println!("Chunk kind: {}", chunk.kind);
    println!("Chunk content: {}", one_line(&chunk.content));
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
/// Represents a chunk source in JSON format for display.
struct ChunkSourceJson {
    kind: String,
    value: String,
}

#[derive(Debug, Clone, Serialize)]
/// Represents a chunk's data in JSON format for display.
struct ChunkJson {
    id: u32,
    kind: String,
    content: String,
    author: String,
    confidence: f32,
    created_at_unix_ms: u64,
    sources: Vec<ChunkSourceJson>,
}

impl From<agentsdb_format::ChunkInput> for ChunkJson {
    fn from(c: agentsdb_format::ChunkInput) -> Self {
        let sources = c
            .sources
            .into_iter()
            .map(|s| match s {
                agentsdb_format::ChunkSource::ChunkId(id) => ChunkSourceJson {
                    kind: "chunk_id".to_string(),
                    value: id.to_string(),
                },
                agentsdb_format::ChunkSource::SourceString(v) => ChunkSourceJson {
                    kind: "string".to_string(),
                    value: v,
                },
            })
            .collect();
        ChunkJson {
            id: c.id,
            kind: c.kind,
            content: c.content,
            author: c.author,
            confidence: c.confidence,
            created_at_unix_ms: c.created_at_unix_ms,
            sources,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
/// Represents the JSON output structure for a proposal's state.
struct ProposalStateJson {
    proposal_id: u32,
    context_id: u32,
    from_path: String,
    to_path: String,
    status: ProposalStatus,
    created_at_unix_ms: Option<u64>,
    title: Option<String>,
    why: Option<String>,
    what: Option<String>,
    #[serde(rename = "where")]
    where_: Option<String>,
    decided_at_unix_ms: Option<u64>,
    decided_by: Option<String>,
    decision_reason: Option<String>,
    decision_outcome: Option<String>,
}

impl From<ProposalState> for ProposalStateJson {
    fn from(s: ProposalState) -> Self {
        ProposalStateJson {
            proposal_id: s.proposal_id,
            context_id: s.context_id,
            from_path: s.from_path,
            to_path: s.to_path,
            status: s.status,
            created_at_unix_ms: s.created_at_unix_ms,
            title: s.title,
            why: s.why,
            what: s.what,
            where_: s.where_,
            decided_at_unix_ms: s.decided_at_unix_ms,
            decided_by: s.decided_by,
            decision_reason: s.decision_reason,
            decision_outcome: s.decision_outcome,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
/// Represents the output of a promotion operation in JSON format.
struct PromoteOut {
    ok: bool,
    from: String,
    to: String,
    promoted: Vec<u32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    skipped: Vec<u32>,
}

pub(crate) fn cmd_proposals_accept(
    dir: &str,
    delta: Option<&str>,
    user: Option<&str>,
    proposals_layer: Option<&str>,
    ids: &str,
    skip_existing: bool,
    _yes: bool,
    json: bool,
) -> anyhow::Result<()> {
    // Implements the `proposals accept` command, which accepts proposals by promoting
    // their chunks into the user layer.
    //
    // This function handles validating proposals, performing the promotion, and recording
    // the acceptance event.
    let dir = Path::new(dir);
    let paths = resolve_paths(dir, delta, user, proposals_layer);
    let states = load_states(&paths.proposals_layer)?;

    let wanted = crate::util::parse_ids_csv(ids)?;
    if wanted.is_empty() {
        anyhow::bail!("--ids must be non-empty");
    }

    for id in &wanted {
        let Some(s) = states.get(id) else {
            anyhow::bail!("proposal {id} not found");
        };
        if !matches!(s.status, ProposalStatus::Pending) {
            anyhow::bail!("proposal {id} is not pending");
        }
        if s.to_path == "AGENTS.db" {
            anyhow::bail!("proposal {id} targets base; use `agentsdb compact` to rebuild base");
        }
    }

    let mut by_pair: BTreeMap<(String, String), Vec<(u32, u32)>> = BTreeMap::new();
    for pid in &wanted {
        let s = states.get(pid).context("proposal missing")?;
        by_pair
            .entry((s.from_path.clone(), s.to_path.clone()))
            .or_default()
            .push((*pid, s.context_id));
    }

    let mut promoted = Vec::new();
    let mut skipped = Vec::new();

    for ((from_rel, to_rel), refs) in by_pair {
        let from_abs = resolve_layer_label(dir, &paths, &from_rel);
        let to_abs = resolve_layer_label(dir, &paths, &to_rel);
        let ids: Vec<u32> = refs.iter().map(|(_, cid)| *cid).collect();
        let out = agentsdb_ops::promote::promote_chunks(
            &from_abs.to_string_lossy(),
            &to_abs.to_string_lossy(),
            &ids,
            skip_existing,
        )?;
        promoted.extend(out.promoted);
        skipped.extend(out.skipped);

        for (proposal_id, context_id) in refs {
            let outcome = if promoted.contains(&context_id) {
                Some("promoted")
            } else if skipped.contains(&context_id) {
                Some("skipped_existing")
            } else {
                None
            };
            append_decision_event(
                &paths.proposals_layer,
                "accept",
                proposal_id,
                context_id,
                outcome,
                None,
            )?;
        }
    }

    promoted.sort_unstable();
    promoted.dedup();
    skipped.sort_unstable();
    skipped.dedup();

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&PromoteOut {
                ok: true,
                from: "varies".to_string(),
                to: "varies".to_string(),
                promoted,
                skipped,
            })?
        );
        return Ok(());
    }
    if promoted.is_empty() {
        println!("No chunks promoted");
    } else {
        println!("Promoted {} chunks", promoted.len());
    }
    if !skipped.is_empty() {
        println!(
            "Skipped {} ids already present in destination",
            skipped.len()
        );
    }
    println!("Recorded {} proposal acceptances", wanted.len());
    Ok(())
}

pub(crate) fn cmd_proposals_reject(
    dir: &str,
    delta: Option<&str>,
    user: Option<&str>,
    proposals_layer: Option<&str>,
    ids: &str,
    reason: Option<&str>,
    json: bool,
) -> anyhow::Result<()> {
    // Implements the `proposals reject` command, which rejects proposals without promoting them.
    //
    // This function handles validating proposals and recording the rejection event with an optional reason.
    let dir = Path::new(dir);
    let paths = resolve_paths(dir, delta, user, proposals_layer);
    let states = load_states(&paths.proposals_layer)?;

    let wanted = crate::util::parse_ids_csv(ids)?;
    if wanted.is_empty() {
        anyhow::bail!("--ids must be non-empty");
    }
    for id in &wanted {
        let Some(s) = states.get(id) else {
            anyhow::bail!("proposal {id} not found");
        };
        if !matches!(s.status, ProposalStatus::Pending) {
            anyhow::bail!("proposal {id} is not pending");
        }
    }
    for id in &wanted {
        let s = states.get(id).context("proposal missing")?;
        append_decision_event(
            &paths.proposals_layer,
            "reject",
            *id,
            s.context_id,
            Some("rejected"),
            reason,
        )?;
    }

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({ "ok": true, "rejected": wanted }))?
        );
        return Ok(());
    }
    println!("Rejected {} proposals", wanted.len());
    Ok(())
}
