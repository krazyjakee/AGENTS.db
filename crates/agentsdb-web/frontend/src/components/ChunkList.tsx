import { useState, useMemo, useEffect } from 'preact/hooks';
import type { ChunkSummary, SearchResultJson } from '../types';
import { api } from '../api';

interface ChunkListProps {
  chunks: ChunkSummary[];
  total: number;
  offset: number;
  limit: number;
  loading?: boolean;
  selectedLayer: string;
  kindFilter?: string;
  onViewChunk: (chunk: ChunkSummary) => void;
  onEditChunk: (chunk: ChunkSummary) => void;
  onRemoveChunk: (chunk: ChunkSummary) => void;
  onPageChange: (newOffset: number) => void;
}

export function ChunkList({
  chunks,
  total,
  offset,
  limit,
  loading = false,
  selectedLayer,
  kindFilter = '',
  onViewChunk,
  onEditChunk,
  onRemoveChunk,
  onPageChange,
}: ChunkListProps) {
  const [searchQuery, setSearchQuery] = useState('');
  const [searchMode, setSearchMode] = useState<'filter' | 'search'>('search');
  const [searchResults, setSearchResults] = useState<SearchResultJson[]>([]);
  const [searchLoading, setSearchLoading] = useState(false);
  const [searchError, setSearchError] = useState<string | null>(null);

  // Client-side filtering for filter mode
  const filteredChunks = useMemo(() => {
    if (!searchQuery.trim() || searchMode === 'search') {
      return chunks;
    }

    const query = searchQuery.toLowerCase();
    return chunks.filter((chunk) => {
      return (
        chunk.id.toString().includes(query) ||
        chunk.kind.toLowerCase().includes(query) ||
        chunk.content_preview.toLowerCase().includes(query) ||
        chunk.confidence.toString().includes(query)
      );
    });
  }, [chunks, searchQuery, searchMode]);

  // Semantic search for search mode
  const handleSearch = async () => {
    if (searchMode !== 'search' || !searchQuery.trim()) {
      return;
    }

    try {
      setSearchLoading(true);
      setSearchError(null);
      const results = await api.searchChunks({
        query: searchQuery,
        layers: [selectedLayer],
        k: 50,
        kinds: kindFilter ? [kindFilter] : undefined,
      });
      setSearchResults(results.results);
    } catch (err) {
      setSearchError(err instanceof Error ? err.message : String(err));
      setSearchResults([]);
    } finally {
      setSearchLoading(false);
    }
  };

  // Trigger search when mode changes to search or query changes in search mode
  useEffect(() => {
    if (searchMode === 'search' && searchQuery.trim()) {
      handleSearch();
    }
  }, [searchMode, searchQuery, selectedLayer, kindFilter]);

  // Clear search results when switching to filter mode
  useEffect(() => {
    if (searchMode === 'filter') {
      setSearchResults([]);
      setSearchError(null);
    }
  }, [searchMode]);

  // Convert search results to ChunkSummary format for display
  const searchResultChunks: ChunkSummary[] = useMemo(() => {
    return searchResults.map((r) => ({
      id: r.id,
      kind: r.kind,
      author: r.author,
      confidence: r.confidence,
      created_at_unix_ms: r.created_at_unix_ms,
      source_count: r.sources.length,
      removed: false,
      content_preview: r.content_preview,
    }));
  }, [searchResults]);

  const displayChunks = searchMode === 'search' && searchQuery.trim()
    ? searchResultChunks
    : filteredChunks;

  const isSearchActive = searchMode === 'search' && searchQuery.trim();
  const isLoading = loading || (searchLoading && searchMode === 'search');

  return (
    <div class="card bg-base-200 shadow-xl mb-4">
      <div class="card-body">
        <div class="flex flex-col gap-4 mb-4">
          {/* Mode Toggle */}
          <div class="flex justify-between items-center">
            <div class="join">
              <button
                class={`join-item btn btn-sm ${searchMode === 'search' ? 'btn-active' : ''}`}
                onClick={() => setSearchMode('search')}
              >
                Search (semantic)
              </button>
              <button
                class={`join-item btn btn-sm ${searchMode === 'filter' ? 'btn-active' : ''}`}
                onClick={() => setSearchMode('filter')}
              >
                Filter (current page)
              </button>
            </div>
            {searchMode === 'search' && (
              <div class="text-xs text-base-content/60">
                Searches entire layer using vector similarity
              </div>
            )}
          </div>

          {/* Search Input */}
          <div class="form-control w-full">
            <div class="input-group">
              <span class="bg-base-300">
                <svg
                  class="h-5 w-5"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  stroke-width="2"
                  stroke-linecap="round"
                  stroke-linejoin="round"
                >
                  <circle cx="11" cy="11" r="8" />
                  <path d="m21 21-4.35-4.35" />
                </svg>
              </span>
              <input
                type="text"
                placeholder={
                  searchMode === 'filter'
                    ? 'Search chunks by ID, kind, content, or confidence...'
                    : 'Semantic search across entire layer...'
                }
                class="input input-bordered w-full"
                value={searchQuery}
                onInput={(e) => setSearchQuery((e.target as HTMLInputElement).value)}
                onKeyPress={(e) => {
                  if (e.key === 'Enter' && searchMode === 'search') {
                    handleSearch();
                  }
                }}
              />
              {searchQuery && (
                <button
                  class="btn btn-square"
                  onClick={() => {
                    setSearchQuery('');
                    setSearchResults([]);
                    setSearchError(null);
                  }}
                  title="Clear search"
                >
                  ✕
                </button>
              )}
            </div>
            {searchQuery && searchMode === 'filter' && (
              <label class="label">
                <span class="label-text-alt">
                  Found {filteredChunks.length} of {chunks.length} chunks
                </span>
              </label>
            )}
            {searchMode === 'search' && isSearchActive && !searchLoading && (
              <label class="label">
                <span class="label-text-alt">
                  Found {searchResults.length} results
                </span>
              </label>
            )}
            {searchError && (
              <label class="label">
                <span class="label-text-alt text-error select-text">{searchError}</span>
              </label>
            )}
          </div>

          {/* Pagination (only for filter mode) */}
          {searchMode === 'filter' && (
            <div class="flex justify-between items-center">
              <div class="text-sm">
                Showing {chunks.length} of {total} (offset={offset}, limit={limit})
              </div>
              <div class="join">
                <button
                  class="join-item btn btn-sm"
                  onClick={() => onPageChange(Math.max(0, offset - limit))}
                  disabled={offset === 0 || loading}
                >
                  «
                </button>
                <button class="join-item btn btn-sm">
                  Page {Math.floor(offset / limit) + 1}
                </button>
                <button
                  class="join-item btn btn-sm"
                  onClick={() => onPageChange(offset + limit)}
                  disabled={offset + limit >= total || loading}
                >
                  »
                </button>
              </div>
            </div>
          )}
        </div>

        {isLoading ? (
          <div class="flex justify-center py-8">
            <span class="loading loading-spinner loading-lg"></span>
          </div>
        ) : (
          <div class="overflow-x-auto">
            <table class="table table-zebra">
              <thead>
                <tr>
                  <th>ID</th>
                  <th>Kind</th>
                  <th>Conf</th>
                  {searchMode === 'search' && isSearchActive && <th>Score</th>}
                  <th>Preview</th>
                  <th>Actions</th>
                </tr>
              </thead>
              <tbody>
                {displayChunks.length === 0 ? (
                  <tr>
                    <td colSpan={searchMode === 'search' && isSearchActive ? 6 : 5} class="text-center py-8">
                      {searchQuery
                        ? searchMode === 'search'
                          ? 'No chunks match your semantic search'
                          : 'No chunks match your filter'
                        : 'No chunks to display'}
                    </td>
                  </tr>
                ) : (
                  displayChunks.map((chunk, idx) => {
                    const searchResult = searchMode === 'search' && isSearchActive ? searchResults[idx] : null;
                    return (
                      <tr key={chunk.id}>
                        <td class="mono">
                          {chunk.id}
                          {chunk.removed && <span class="badge badge-error ml-2">removed</span>}
                        </td>
                        <td>
                          <span class="badge">{chunk.kind}</span>
                        </td>
                        <td class="mono">{chunk.confidence.toFixed(2)}</td>
                        {searchMode === 'search' && isSearchActive && searchResult && (
                          <td class="mono">
                            <span class="badge badge-accent">{searchResult.score.toFixed(4)}</span>
                          </td>
                        )}
                        <td class="mono text-sm">{chunk.content_preview}</td>
                        <td>
                          <div class="flex gap-1">
                            <button
                              class="btn btn-ghost btn-xs"
                              onClick={() => onViewChunk(chunk)}
                              title="View chunk details"
                            >
                              View
                            </button>
                            {!chunk.removed && (
                              <>
                                <button
                                  class="btn btn-ghost btn-xs"
                                  onClick={() => onEditChunk(chunk)}
                                  title="Edit chunk"
                                >
                                  Edit
                                </button>
                                <button
                                  class="btn btn-ghost btn-xs text-error"
                                  onClick={() => onRemoveChunk(chunk)}
                                  title="Remove chunk (tombstone)"
                                >
                                  Remove
                                </button>
                              </>
                            )}
                          </div>
                        </td>
                      </tr>
                    );
                  })
                )}
              </tbody>
            </table>
          </div>
        )}
      </div>
    </div>
  );
}
