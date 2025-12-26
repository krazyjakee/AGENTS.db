import { useState, useEffect, useCallback } from 'preact/hooks';
import { Header } from './components/Header';
import { LayerMetadataModal } from './components/LayerMetadataModal';
import { ChunkList } from './components/ChunkList';
import { ChunkViewer } from './components/ChunkViewer';
import { AddChunkPanel } from './components/AddChunkPanel';
import { EditChunkModal } from './components/EditChunkModal';
import { ProposalsPanel } from './components/ProposalsPanel';
import { ProposalDetailsModal } from './components/ProposalDetailsModal';
import { ExportImportPanel } from './components/ExportImportPanel';
import { PromoteModal } from './components/PromoteModal';
import { ProposeModal } from './components/ProposeModal';
import type {
  ListedLayer,
  LayerMeta,
  ChunkSummary,
  ProposalRow,
  ChunkFull,
  AddChunkRequest,
  ImportRequest,
  ProposeRequest,
} from './types';
import { api } from './api';
import { writeScopeForPath } from './utils/helpers';

// Custom hook for error handling in async operations
function useErrorHandler(setError: (error: string | null) => void) {
  return useCallback(
    async <T,>(fn: () => Promise<T>): Promise<T | undefined> => {
      try {
        const result = await fn();
        setError(null);
        return result;
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err));
        return undefined;
      }
    },
    [setError]
  );
}

export function App() {
  // Layer state
  const [layers, setLayers] = useState<ListedLayer[]>([]);
  const [selectedLayer, setSelectedLayer] = useState<string>('');
  const [layerMeta, setLayerMeta] = useState<LayerMeta | null>(null);

  // Chunk state
  const [chunks, setChunks] = useState<ChunkSummary[]>([]);
  const [total, setTotal] = useState(0);
  const [offset, setOffset] = useState(0);
  const [limit] = useState(100);
  const [kindFilter, setKindFilter] = useState('');
  const [includeRemoved, setIncludeRemoved] = useState(false);

  // Proposals state
  const [proposals, setProposals] = useState<ProposalRow[]>([]);

  // UI state
  const [viewingChunk, setViewingChunk] = useState<ChunkFull | null>(null);
  const [editingChunk, setEditingChunk] = useState<ChunkFull | null>(null);
  const [promotingChunk, setPromotingChunk] = useState<ChunkFull | null>(null);
  const [proposingChunk, setProposingChunk] = useState<ChunkFull | null>(null);
  const [viewingProposal, setViewingProposal] = useState<ProposalRow | null>(null);
  const [showAddPanel, setShowAddPanel] = useState(false);
  const [showExportImport, setShowExportImport] = useState(false);
  const [showMetadata, setShowMetadata] = useState(false);

  // Loading and error state
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Error handler hook
  const withErrorHandling = useErrorHandler(setError);

  // Refresh functions
  const refreshLayers = async () => {
    await withErrorHandling(async () => {
      const layersList = await api.getLayers();
      setLayers(layersList);
      if (!selectedLayer && layersList.length > 0) {
        const first = layersList[0];
        if (first) {
          setSelectedLayer(first.path);
        }
      }
    });
  };

  const refreshMeta = async () => {
    if (!selectedLayer) {
      setLayerMeta(null);
      return;
    }
    await withErrorHandling(async () => {
      const meta = await api.getLayerMeta(selectedLayer);
      setLayerMeta(meta);
    });
  };

  const loadChunks = async () => {
    if (!selectedLayer) {
      setChunks([]);
      setTotal(0);
      return;
    }
    setLoading(true);
    await withErrorHandling(async () => {
      const response = await api.getChunks(
        selectedLayer,
        offset,
        limit,
        includeRemoved,
        kindFilter
      );
      setChunks(response.items);
      setTotal(response.total);
    });
    setLoading(false);
  };

  const refreshProposals = async () => {
    await withErrorHandling(async () => {
      const proposalsList = await api.getProposals(false);
      setProposals(proposalsList);
    });
  };

  // Event handlers
  const handleViewChunk = async (chunk: ChunkSummary) => {
    await withErrorHandling(async () => {
      // Use chunk's layer if available (from search results), otherwise use selected layer
      const layerToUse = chunk.layer || selectedLayer;
      const full = await api.getChunk(layerToUse, chunk.id);
      setViewingChunk(full);
    });
  };

  const handleEditChunk = async (chunk: ChunkSummary) => {
    await withErrorHandling(async () => {
      // Use chunk's layer if available (from search results), otherwise use selected layer
      const layerToUse = chunk.layer || selectedLayer;
      const full = await api.getChunk(layerToUse, chunk.id);
      setEditingChunk(full);
      setViewingChunk(null);
    });
  };

  const handleRemoveChunk = async (chunk: ChunkSummary) => {
    const confirmed = confirm(
      `Remove chunk ${chunk.id}? This will create a tombstone (soft delete).`
    );
    if (!confirmed) return;

    await withErrorHandling(async () => {
      // Use chunk's layer if available (from search results), otherwise use selected layer
      const layerToUse = chunk.layer || selectedLayer;
      await api.removeChunk({
        path: layerToUse,
        scope: writeScopeForPath(layerToUse),
        id: chunk.id,
      });
      await loadChunks();
    });
  };

  const handleAddChunk = async (data: AddChunkRequest) => {
    try {
      await api.addChunk(data);
      await loadChunks();
      await refreshLayers();
      setShowAddPanel(false);
      setError(null);
    } catch (err) {
      throw err;
    }
  };

  const handleEditSubmit = async (data: AddChunkRequest) => {
    try {
      await api.addChunk(data);
      await loadChunks();
      await refreshLayers();
      setEditingChunk(null);
      setError(null);
    } catch (err) {
      throw err;
    }
  };

  const handlePropose = (chunk: ChunkFull) => {
    setViewingChunk(null);
    setProposingChunk(chunk);
  };

  const handleProposeSubmit = async (request: ProposeRequest) => {
    try {
      await api.propose(request);
      await refreshProposals();
      setProposingChunk(null);
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      throw err;
    }
  };

  const handlePromote = (chunk: ChunkFull) => {
    setViewingChunk(null);
    setPromotingChunk(chunk);
  };

  const handlePromoteSubmit = async (toPath: string, skipExisting: boolean) => {
    if (!promotingChunk) return;

    try {
      await api.promoteBatch(selectedLayer, toPath, [promotingChunk.id], skipExisting);
      await loadChunks();
      await refreshLayers();
      setPromotingChunk(null);
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      throw err;
    }
  };

  const handleAcceptProposals = async (ids: number[], skipExisting: boolean) => {
    try {
      const result = await api.acceptProposals(ids, skipExisting);
      await refreshProposals();
      await loadChunks();
      await refreshLayers();
      setError(null);
      alert(`Promoted: ${result.promoted.length}, Skipped: ${result.skipped.length}`);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      throw err;
    }
  };

  const handleRejectProposals = async (ids: number[], reason?: string) => {
    try {
      await api.rejectProposals(ids, reason);
      await refreshProposals();
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      throw err;
    }
  };

  const handleExport = async (path: string, format: string, redact: string) => {
    try {
      const blob = await api.exportLayer(path, format, redact);
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = `${path.replace(/[/\\]/g, '_')}.${format}`;
      document.body.appendChild(a);
      a.click();
      document.body.removeChild(a);
      URL.revokeObjectURL(url);
      setError(null);
    } catch (err) {
      throw err;
    }
  };

  const handleImport = async (data: ImportRequest) => {
    try {
      const result = await api.importLayer(data);
      await loadChunks();
      await refreshLayers();
      setError(null);
      return result;
    } catch (err) {
      throw err;
    }
  };

  // Effects
  useEffect(() => {
    refreshLayers();
    refreshProposals();
  }, []);

  useEffect(() => {
    if (selectedLayer) {
      refreshMeta();
      setOffset(0);
    }
  }, [selectedLayer]);

  useEffect(() => {
    loadChunks();
  }, [selectedLayer, offset, limit, kindFilter, includeRemoved]);

  return (
    <div class="min-h-screen flex flex-col">
      <Header
        layers={layers}
        selectedLayer={selectedLayer}
        onLayerChange={setSelectedLayer}
        onShowMetadata={() => setShowMetadata(true)}
        onShowExportImport={() => setShowExportImport(true)}
      />

      <main class="container mx-auto p-4 flex-1">
        {error && (
          <div class="alert alert-error mb-4">
            <span>{error}</span>
            <button onClick={() => setError(null)} class="btn btn-sm btn-ghost">
              ✕
            </button>
          </div>
        )}

        {showAddPanel && (
          <AddChunkPanel
            selectedLayer={selectedLayer}
            embeddingDim={layerMeta?.embedding_dim}
            onSubmit={handleAddChunk}
            onCancel={() => setShowAddPanel(false)}
          />
        )}

        <ChunkList
          chunks={chunks}
          total={total}
          offset={offset}
          limit={limit}
          loading={loading}
          selectedLayer={selectedLayer}
          kindFilter={kindFilter}
          includeRemoved={includeRemoved}
          layerMeta={layerMeta}
          onViewChunk={handleViewChunk}
          onEditChunk={handleEditChunk}
          onRemoveChunk={handleRemoveChunk}
          onPageChange={setOffset}
          onKindFilterChange={setKindFilter}
          onIncludeRemovedChange={setIncludeRemoved}
          onLoad={loadChunks}
          onAdd={() => setShowAddPanel(!showAddPanel)}
        />

        {proposals.length > 0 && (
          <ProposalsPanel
            proposals={proposals}
            onAccept={handleAcceptProposals}
            onReject={handleRejectProposals}
            onViewDetails={setViewingProposal}
            onRefresh={refreshProposals}
          />
        )}
      </main>

      {viewingChunk && (
        <ChunkViewer
          chunk={viewingChunk}
          onClose={() => setViewingChunk(null)}
          onPropose={handlePropose}
          onPromote={handlePromote}
          onEdit={(chunk) => {
            setViewingChunk(null);
            setEditingChunk(chunk);
          }}
        />
      )}

      {editingChunk && (
        <EditChunkModal
          chunk={editingChunk}
          selectedLayer={selectedLayer}
          embeddingDim={layerMeta?.embedding_dim}
          onSubmit={handleEditSubmit}
          onClose={() => setEditingChunk(null)}
        />
      )}

      {promotingChunk && (
        <PromoteModal
          chunk={promotingChunk}
          selectedLayer={selectedLayer}
          onPromote={handlePromoteSubmit}
          onClose={() => setPromotingChunk(null)}
        />
      )}

      {proposingChunk && (
        <ProposeModal
          chunk={proposingChunk}
          selectedLayer={selectedLayer}
          onPropose={handleProposeSubmit}
          onClose={() => setProposingChunk(null)}
        />
      )}

      {viewingProposal && (
        <ProposalDetailsModal
          proposal={viewingProposal}
          onClose={() => setViewingProposal(null)}
        />
      )}

      {showMetadata && (
        <LayerMetadataModal
          layerMeta={layerMeta}
          onClose={() => setShowMetadata(false)}
        />
      )}

      {showExportImport && (
        <ExportImportPanel
          selectedLayer={selectedLayer}
          embeddingDim={layerMeta?.embedding_dim}
          onExport={handleExport}
          onImport={handleImport}
          onClose={() => setShowExportImport(false)}
        />
      )}
    </div>
  );
}
