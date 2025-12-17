import { useState, useEffect } from 'preact/hooks';
import type { ChunkFull, AddChunkRequest } from '../types';

interface EditChunkModalProps {
  chunk: ChunkFull | null;
  selectedLayer: string;
  embeddingDim?: number;
  onSubmit: (data: AddChunkRequest) => Promise<void>;
  onClose: () => void;
}

export function EditChunkModal({
  chunk,
  selectedLayer,
  embeddingDim = 128,
  onSubmit,
  onClose,
}: EditChunkModalProps) {
  const [scope, setScope] = useState<'local' | 'delta'>('delta');
  const [kind, setKind] = useState('');
  const [content, setContent] = useState('');
  const [confidence, setConfidence] = useState(0.8);
  const [sources, setSources] = useState('');
  const [tombstoneOld, setTombstoneOld] = useState(true);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [isMaximized, setIsMaximized] = useState(false);

  useEffect(() => {
    if (chunk) {
      setKind(chunk.kind);
      setContent(chunk.content);
      setConfidence(chunk.confidence);
      setSources((chunk.sources || []).join(', '));
      setError(null);
    }
  }, [chunk]);

  if (!chunk) return null;

  const handleSubmit = async (e: Event) => {
    e.preventDefault();
    setError(null);

    if (!content.trim()) {
      setError('Content is required');
      return;
    }

    const sourcesArray = sources
      .split(',')
      .map((s) => s.trim())
      .filter(Boolean);

    const data: AddChunkRequest = {
      path: selectedLayer,
      scope,
      id: chunk.id,
      kind,
      content: content.trim(),
      confidence,
      dim: embeddingDim,
      sources: sourcesArray.length > 0 ? sourcesArray : undefined,
      tombstone_old: tombstoneOld,
    };

    try {
      setSubmitting(true);
      await onSubmit(data);
      onClose();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <dialog class="modal modal-open">
      <div class={`modal-box ${isMaximized ? 'w-screen h-screen max-w-none max-h-none m-0 rounded-none' : 'max-w-4xl'}`}>
        <div class="flex justify-between items-center mb-4">
          <h3 class="font-bold text-lg">
            Edit Chunk <span class="mono">id={chunk.id}</span>
          </h3>
          <div class="flex gap-1">
            <button
              class="btn btn-sm btn-circle btn-ghost"
              onClick={() => setIsMaximized(!isMaximized)}
              disabled={submitting}
              title={isMaximized ? "Restore" : "Maximize"}
            >
              {isMaximized ? (
                <svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                  <path d="M8 3v3a2 2 0 0 1-2 2H3m18 0h-3a2 2 0 0 1-2-2V3m0 18v-3a2 2 0 0 1 2-2h3M3 16h3a2 2 0 0 1 2 2v3" />
                </svg>
              ) : (
                <svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                  <path d="M8 3H5a2 2 0 0 0-2 2v3m18 0V5a2 2 0 0 0-2-2h-3m0 18h3a2 2 0 0 0 2-2v-3M3 16v3a2 2 0 0 0 2 2h3" />
                </svg>
              )}
            </button>
            <button class="btn btn-sm btn-circle btn-ghost" onClick={onClose} disabled={submitting}>
              ✕
            </button>
          </div>
        </div>

        {error && (
          <div class="alert alert-error mb-4">
            <span>{error}</span>
          </div>
        )}

        <form onSubmit={handleSubmit}>
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
            <span>
              Editing creates a new chunk with the same ID. Enable "Tombstone old" to mark the
              original as removed.
            </span>
          </div>

          <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
            <div class="form-control">
              <label class="label">
                <span class="label-text">Scope *</span>
              </label>
              <select
                class="select select-bordered"
                value={scope}
                onChange={(e) => setScope((e.target as HTMLSelectElement).value as 'local' | 'delta')}
                disabled={submitting}
              >
                <option value="local">Local (temporary)</option>
                <option value="delta">Delta (proposed)</option>
              </select>
            </div>

            <div class="form-control">
              <label class="label">
                <span class="label-text">Kind *</span>
              </label>
              <input
                type="text"
                class="input input-bordered"
                value={kind}
                onInput={(e) => setKind((e.target as HTMLInputElement).value)}
                disabled={submitting}
                required
              />
            </div>
          </div>

          <div class="form-control mt-4">
            <label class="label">
              <span class="label-text">Content *</span>
            </label>
            <textarea
              class="textarea textarea-bordered h-40"
              value={content}
              onInput={(e) => setContent((e.target as HTMLTextAreaElement).value)}
              disabled={submitting}
              required
            />
          </div>

          <div class="form-control mt-4">
            <label class="label">
              <span class="label-text">Confidence</span>
            </label>
            <input
              type="range"
              min="0"
              max="1"
              step="0.1"
              class="range range-sm"
              value={confidence}
              onInput={(e) => setConfidence(parseFloat((e.target as HTMLInputElement).value))}
              disabled={submitting}
            />
            <div class="text-sm text-center mono">{confidence.toFixed(1)}</div>
          </div>

          <div class="form-control mt-4">
            <label class="label">
              <span class="label-text">Sources</span>
            </label>
            <input
              type="text"
              class="input input-bordered"
              value={sources}
              onInput={(e) => setSources((e.target as HTMLInputElement).value)}
              placeholder="file.rs:42, doc.md:10 (comma-separated)"
              disabled={submitting}
            />
          </div>

          <div class="form-control mt-4">
            <label class="cursor-pointer label justify-start gap-2">
              <input
                type="checkbox"
                class="checkbox"
                checked={tombstoneOld}
                onChange={(e) => setTombstoneOld((e.target as HTMLInputElement).checked)}
                disabled={submitting}
              />
              <span class="label-text">Tombstone old chunk (mark original as removed)</span>
            </label>
          </div>

          <div class="modal-action">
            <button type="submit" class="btn btn-primary" disabled={submitting}>
              {submitting ? <span class="loading loading-spinner"></span> : 'Save Changes'}
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
