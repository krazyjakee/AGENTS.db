import type { LayerMeta } from '../types';

interface LayerMetadataModalProps {
  layerMeta: LayerMeta | null;
  onClose: () => void;
}

export function LayerMetadataModal({ layerMeta, onClose }: LayerMetadataModalProps) {
  if (!layerMeta) return null;

  return (
    <div class="modal modal-open">
      <div class="modal-box max-w-3xl">
        <button
          onClick={onClose}
          class="btn btn-sm btn-circle btn-ghost absolute right-2 top-2"
          aria-label="Close"
        >
          ✕
        </button>

        <h3 class="font-bold text-2xl mb-4">Layer Metadata</h3>
        <div class="text-sm mb-2 mono opacity-70">{layerMeta.path}</div>

        <div class="space-y-4">
          <div class="grid grid-cols-1 sm:grid-cols-2 gap-3">
            <div class="stat bg-base-200 rounded-lg p-4">
              <div class="stat-title text-xs">Chunks</div>
              <div class="stat-value text-3xl">{layerMeta.chunk_count}</div>
              <div class="stat-desc">
                <span class="badge badge-error badge-sm">{layerMeta.removed_count} removed</span>
              </div>
            </div>
            <div class="stat bg-base-200 rounded-lg p-4">
              <div class="stat-title text-xs">File Size</div>
              <div class="stat-value text-2xl">
                {Math.round(layerMeta.file_length_bytes / 1024)} KiB
              </div>
              <div class="stat-desc">{layerMeta.file_length_bytes} bytes</div>
            </div>
          </div>

          <div class="stat bg-base-200 rounded-lg p-4">
            <div class="stat-title text-xs">Embedding</div>
            <div class="stat-value text-lg mono">dim={layerMeta.embedding_dim}</div>
            <div class="stat-desc">{layerMeta.embedding_element_type}</div>
          </div>

          <div class="stat bg-base-200 rounded-lg p-4">
            <div class="stat-title text-xs">Confidence Range</div>
            <div class="flex gap-2 items-center mt-2">
              <span class="badge badge-info">min: {layerMeta.confidence_min.toFixed(2)}</span>
              <span class="badge badge-success">avg: {layerMeta.confidence_avg.toFixed(2)}</span>
              <span class="badge badge-warning">max: {layerMeta.confidence_max.toFixed(2)}</span>
            </div>
          </div>

          <div class="bg-base-200 rounded-lg p-4">
            <div class="text-sm font-semibold mb-3">Chunk Kinds</div>
            <div class="flex flex-wrap gap-2">
              {Object.entries(layerMeta.kinds).map(([kind, count]) => (
                <span key={kind} class="badge badge-outline badge-lg">
                  {kind}: <span class="font-bold ml-1">{count}</span>
                </span>
              ))}
            </div>
          </div>
        </div>

        <div class="modal-action">
          <button onClick={onClose} class="btn btn-primary">
            Close
          </button>
        </div>
      </div>
      <div class="modal-backdrop" onClick={onClose}></div>
    </div>
  );
}
