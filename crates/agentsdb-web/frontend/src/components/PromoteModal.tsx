import { useState, useEffect } from 'preact/hooks';
import type { ChunkFull } from '../types';

interface PromoteModalProps {
  chunk: ChunkFull | null;
  selectedLayer: string;
  onPromote: (toPath: string, skipExisting: boolean) => Promise<void>;
  onClose: () => void;
}

export function PromoteModal({
  chunk,
  selectedLayer,
  onPromote,
  onClose,
}: PromoteModalProps) {
  const [toPath, setToPath] = useState('');
  const [skipExisting, setSkipExisting] = useState(true);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (chunk && selectedLayer) {
      // Set default target path
      const defaultPath = selectedLayer.replace('.delta.db', '.user.db')
        .replace('.local.db', '.delta.db');
      setToPath(defaultPath);
      setError(null);
    }
  }, [chunk, selectedLayer]);

  if (!chunk) return null;

  const handleSubmit = async (e: Event) => {
    e.preventDefault();
    setError(null);

    if (!toPath.trim()) {
      setError('Target path is required');
      return;
    }

    if (!toPath.endsWith('.db')) {
      setError('Target path must end with .db');
      return;
    }

    try {
      setSubmitting(true);
      await onPromote(toPath, skipExisting);
      onClose();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <dialog class="modal modal-open">
      <div class="modal-box max-w-2xl">
        <div class="flex justify-between items-center mb-4">
          <h3 class="font-bold text-lg flex items-center gap-2">
            <svg class="h-5 w-5 text-primary" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <polyline points="18 15 12 9 6 15" />
            </svg>
            Promote Chunk <span class="mono">id={chunk.id}</span>
          </h3>
          <button class="btn btn-sm btn-circle btn-ghost" onClick={onClose} disabled={submitting}>
            ✕
          </button>
        </div>

        {error && (
          <div class="alert alert-error mb-4">
            <span>{error}</span>
          </div>
        )}

        <div class="alert alert-info mb-4">
          <svg
            xmlns="http://www.w3.org/2000/svg"
            fill="none"
            viewBox="0 0 24 24"
            class="stroke-current shrink-0 w-6 h-6"
          >
            <path
              stroke-linecap="round"
              stroke-linejoin="round"
              stroke-width="2"
              d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"
            />
          </svg>
          <div class="text-sm">
            <p class="font-semibold mb-1">Promotion copies this chunk to another layer.</p>
            <p>Common paths:</p>
            <ul class="list-disc list-inside ml-2 mt-1">
              <li>AGENTS.local.db → AGENTS.delta.db (for review)</li>
              <li>AGENTS.delta.db → AGENTS.user.db (approved)</li>
            </ul>
          </div>
        </div>

        <form onSubmit={handleSubmit}>
          <div class="form-control">
            <label class="label">
              <span class="label-text">Source Layer</span>
            </label>
            <input
              type="text"
              class="input input-bordered bg-base-200"
              value={selectedLayer}
              disabled
              readOnly
            />
          </div>

          <div class="form-control mt-4">
            <label class="label">
              <span class="label-text">Target Layer *</span>
            </label>
            <input
              type="text"
              class="input input-bordered"
              value={toPath}
              onInput={(e) => setToPath((e.target as HTMLInputElement).value)}
              placeholder="AGENTS.user.db"
              disabled={submitting}
              required
            />
            <label class="label">
              <span class="label-text-alt text-base-content/70">
                The layer file where the chunk will be copied
              </span>
            </label>
          </div>

          <div class="form-control mt-4">
            <label class="cursor-pointer label justify-start gap-2">
              <input
                type="checkbox"
                class="checkbox"
                checked={skipExisting}
                onChange={(e) => setSkipExisting((e.target as HTMLInputElement).checked)}
                disabled={submitting}
              />
              <span class="label-text">Skip if chunk ID already exists in target</span>
            </label>
            <label class="label">
              <span class="label-text-alt text-base-content/70">
                If unchecked, promotion will fail if the chunk ID already exists in the target layer
              </span>
            </label>
          </div>

          <div class="divider"></div>

          <div class="alert alert-warning">
            <svg
              xmlns="http://www.w3.org/2000/svg"
              class="stroke-current shrink-0 h-6 w-6"
              fill="none"
              viewBox="0 0 24 24"
            >
              <path
                stroke-linecap="round"
                stroke-linejoin="round"
                stroke-width="2"
                d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z"
              />
            </svg>
            <div class="text-sm">
              <p class="font-semibold">Note: The source chunk will be tombstoned after promotion.</p>
              <p class="mt-1">This marks the original chunk as removed in <span class="mono text-xs">{selectedLayer}</span></p>
            </div>
          </div>

          <div class="modal-action mt-6">
            <button type="submit" class="btn btn-primary" disabled={submitting}>
              {submitting ? (
                <>
                  <span class="loading loading-spinner"></span>
                  Promoting...
                </>
              ) : (
                <>
                  <svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                    <polyline points="18 15 12 9 6 15" />
                  </svg>
                  Promote Chunk
                </>
              )}
            </button>
            <button type="button" class="btn" onClick={onClose} disabled={submitting}>
              Cancel
            </button>
          </div>
        </form>
      </div>
      <form method="dialog" class="modal-backdrop" onClick={onClose}>
        <button>close</button>
      </form>
    </dialog>
  );
}
