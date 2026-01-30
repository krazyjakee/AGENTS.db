import type {
  ListedLayer,
  LayerMeta,
  ChunksResponse,
  ChunkFull,
  VersionResponse,
  ProposalRow,
  PromoteResponse,
  AddChunkRequest,
  ProposeRequest,
  ImportRequest,
  ImportResponse,
  SearchRequest,
  SearchResponse,
} from './types';

class ApiError extends Error {
  constructor(message: string, public status?: number) {
    super(message);
    this.name = 'ApiError';
  }
}

async function request<T>(path: string, options?: RequestInit): Promise<T> {
  const response = await fetch(path, options);

  if (!response.ok) {
    const text = await response.text();
    throw new ApiError(text || response.statusText, response.status);
  }

  const contentType = response.headers.get('content-type');
  if (contentType?.includes('application/json')) {
    return response.json();
  }

  return response.text() as T;
}

export const api = {
  async getVersion(): Promise<VersionResponse> {
    return request<VersionResponse>('/api/version');
  },

  async getLayers(): Promise<ListedLayer[]> {
    return request<ListedLayer[]>('/api/layers');
  },

  async getLayerMeta(path: string): Promise<LayerMeta> {
    return request<LayerMeta>(`/api/layer/meta?path=${encodeURIComponent(path)}`);
  },

  async getChunks(
    path: string,
    offset: number,
    limit: number,
    includeRemoved: boolean,
    kind: string
  ): Promise<ChunksResponse> {
    const params = new URLSearchParams({
      path,
      offset: offset.toString(),
      limit: limit.toString(),
      include_removed: includeRemoved ? '1' : '0',
      kind,
    });
    return request<ChunksResponse>(`/api/layer/chunks?${params}`);
  },

  async getChunk(path: string, id: number): Promise<ChunkFull> {
    return request<ChunkFull>(`/api/layer/chunk?path=${encodeURIComponent(path)}&id=${id}`);
  },

  async addChunk(data: AddChunkRequest): Promise<{ ok: boolean; path: string; id: number }> {
    return request('/api/layer/add', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(data),
    });
  },

  async removeChunk(path: string, id: number): Promise<{ ok: boolean; removed: boolean }> {
    return request('/api/layer/remove', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ path, id }),
    });
  },

  async exportLayer(path: string, format: string, redact: string): Promise<Blob> {
    const params = new URLSearchParams({ path, format, redact });
    const response = await fetch(`/api/export?${params}`);
    if (!response.ok) {
      throw new ApiError(await response.text(), response.status);
    }
    return response.blob();
  },

  async importLayer(data: ImportRequest): Promise<ImportResponse> {
    return request('/api/import', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(data),
    });
  },

  async getProposals(includeAll: boolean = false): Promise<ProposalRow[]> {
    const params = new URLSearchParams();
    if (includeAll) {
      params.set('all', '1');
    }
    return request<ProposalRow[]>(`/api/proposals${params.toString() ? '?' + params : ''}`);
  },

  async propose(data: ProposeRequest): Promise<{ ok: boolean; proposal_id: number }> {
    return request('/api/proposals/propose', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(data),
    });
  },

  async acceptProposals(
    ids: number[],
    skipExisting: boolean
  ): Promise<PromoteResponse> {
    return request('/api/proposals/accept', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ ids, skip_existing: skipExisting }),
    });
  },

  async rejectProposals(
    ids: number[],
    reason?: string
  ): Promise<{ ok: boolean }> {
    return request('/api/proposals/reject', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ ids, reason }),
    });
  },

  async promoteBatch(
    fromPath: string,
    toPath: string,
    ids: number[],
    skipExisting: boolean
  ): Promise<PromoteResponse> {
    return request('/api/promote/batch', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ from_path: fromPath, to_path: toPath, ids, skip_existing: skipExisting }),
    });
  },

  async searchChunks(data: SearchRequest): Promise<SearchResponse> {
    return request('/api/search', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(data),
    });
  },
};

export { ApiError };
