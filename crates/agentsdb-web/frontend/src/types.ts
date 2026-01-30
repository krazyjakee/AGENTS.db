export interface ListedLayer {
  path: string;
  chunk_count: number;
  file_length_bytes: number;
}

export interface LayerMeta {
  path: string;
  chunk_count: number;
  file_length_bytes: number;
  embedding_dim: number;
  embedding_element_type: string;
  embedding_backend: string | null;
  relationship_count: number | null;
  kinds: Record<string, number>;
  removed_count: number;
  confidence_min: number;
  confidence_max: number;
  confidence_avg: number;
}

export interface ChunkSummary {
  id: number;
  kind: string;
  author: string;
  confidence: number;
  created_at_unix_ms: number;
  source_count: number;
  removed: boolean;
  content_preview: string;
  layer?: string; // Optional: set when chunk comes from search results across layers
}

export interface ChunkFull {
  id: number;
  kind: string;
  author: string;
  confidence: number;
  created_at_unix_ms: number;
  sources: string[];
  content: string;
  removed: boolean;
}

export interface ChunksResponse {
  total: number;
  offset: number;
  limit: number;
  items: ChunkSummary[];
}

export interface VersionResponse {
  version: string;
}

export type ProposalStatus = 'pending' | 'accepted' | 'rejected';

export interface ProposalRow {
  proposal_id: number;
  context_id: number;
  from_path: string;
  to_path: string;
  status: ProposalStatus;
  created_at_unix_ms: number | null;
  title: string | null;
  why: string | null;
  what: string | null;
  where: string | null;
  exists_in_delta: boolean;
  exists_in_user: boolean;
  exists_in_source: boolean;
  exists_in_target: boolean;
  decided_at_unix_ms: number | null;
  decided_by: string | null;
  decision_reason: string | null;
  decision_outcome: string | null;
}

export interface PromoteResponse {
  ok: boolean;
  promoted: number[];
  skipped: number[];
  out_path?: string;
}

export interface AddChunkRequest {
  scope: string;
  id?: number;
  kind: string;
  content: string;
  confidence: number;
  dim?: number;
  sources?: string[];
  source_chunks?: number[];
}

export interface ProposeRequest {
  context_id: number;
  from_path?: string;
  to_path?: string;
  title?: string;
  why?: string;
  what?: string;
  where?: string;
}

export interface ImportRequest {
  path: string;
  scope: string;
  format?: string;
  data: string;
  dry_run?: boolean;
  dedupe?: boolean;
  preserve_ids?: boolean;
  allow_base?: boolean;
  dim?: number;
}

export interface ImportResponse {
  ok: boolean;
  path: string;
  imported: number;
  skipped: number;
  dry_run: boolean;
}

export interface SearchRequest {
  query: string;
  layers: string[];
  k?: number;
  kinds?: string[];
}

export interface SearchResultJson {
  layer: string;
  id: number;
  kind: string;
  score: number;
  author: string;
  confidence: number;
  created_at_unix_ms: number;
  content: string;
  content_preview: string;
  sources: string[];
}

export interface SearchResponse {
  results: SearchResultJson[];
  query_embedding_dim: number;
}
