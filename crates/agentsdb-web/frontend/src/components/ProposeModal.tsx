import { useState, useEffect } from 'preact/hooks';
import type { ChunkFull, ProposeRequest } from '../types';

interface ProposeModalProps {
  chunk: ChunkFull | null;
  selectedLayer: string;
  onPropose: (request: ProposeRequest) => Promise<void>;
  onClose: () => void;
}

export function ProposeModal({
  chunk,
  selectedLayer,
  onPropose,
  onClose,
}: ProposeModalProps) {
  const [title, setTitle] = useState('');
  const [why, setWhy] = useState('');
  const [what, setWhat] = useState('');
  const [fromPath, setFromPath] = useState('');
  const [toPath, setToPath] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [isMaximized, setIsMaximized] = useState(false);

  useEffect(() => {
    if (chunk && selectedLayer) {
      // Set defaults
      setFromPath(selectedLayer);
      const defaultToPath = selectedLayer.replace('.delta.db', '.user.db')
        .replace('.local.db', '.delta.db');
      setToPath(defaultToPath);

      // Auto-fill what field with chunk kind and preview
      const preview = chunk.content.length > 100
        ? chunk.content.substring(0, 100) + '...'
        : chunk.content;
      setWhat(`${chunk.kind}: ${preview}`);

      setError(null);
    }
  }, [chunk, selectedLayer]);

  if (!chunk) return null;

  const handleSubmit = async (e: Event) => {
    e.preventDefault();
    setError(null);

    if (!title.trim()) {
      setError('Title is required');
      return;
    }

    const request: ProposeRequest = {
      context_id: chunk.id,
      from_path: fromPath,
      to_path: toPath,
      title: title.trim(),
      why: why.trim() || undefined,
      what: what.trim() || undefined,
      where: `${selectedLayer} chunk ${chunk.id}`,
    };

    try {
      setSubmitting(true);
      await onPropose(request);
      onClose();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <dialog class="modal modal-open">
      <div class={`modal-box ${isMaximized ? 'w-screen h-screen max-w-none max-h-none m-0 rounded-none' : 'max-w-3xl'}`}>
        <div class="flex justify-between items-center mb-4">
          <h3 class="font-bold text-lg flex items-center gap-2">
            <svg class="h-5 w-5 text-secondary" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M12 19l7-7 3 3-7 7-3-3z" />
              <path d="M18 13l-1.5-7.5L2 2l3.5 14.5L13 18l5-5z" />
              <path d="M2 2l7.586 7.586" />
              <circle cx="11" cy="11" r="2" />
            </svg>
            Create Promotion Proposal
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
            <p class="font-semibold mb-1">Proposals allow team review before promotion.</p>
            <p>Create a proposal to suggest promoting chunk <span class="mono">id={chunk.id}</span> for review and approval.</p>
          </div>
        </div>

        <form onSubmit={handleSubmit}>
          <div class="form-control">
            <label class="label">
              <span class="label-text font-semibold">Title *</span>
            </label>
            <input
              type="text"
              class="input input-bordered"
              value={title}
              onInput={(e) => setTitle((e.target as HTMLInputElement).value)}
              placeholder="Brief description of this proposal"
              disabled={submitting}
              required
              autoFocus
            />
            <label class="label">
              <span class="label-text-alt text-base-content/70">
                A short, descriptive title for the proposal
              </span>
            </label>
          </div>

          <div class="grid grid-cols-1 md:grid-cols-2 gap-4 mt-4">
            <div class="form-control">
              <label class="label">
                <span class="label-text">From Layer</span>
              </label>
              <input
                type="text"
                class="input input-bordered bg-base-200"
                value={fromPath}
                disabled
                readOnly
              />
            </div>

            <div class="form-control">
              <label class="label">
                <span class="label-text">To Layer</span>
              </label>
              <input
                type="text"
                class="input input-bordered"
                value={toPath}
                onInput={(e) => setToPath((e.target as HTMLInputElement).value)}
                disabled={submitting}
              />
            </div>
          </div>

          <div class="form-control mt-4">
            <label class="label">
              <span class="label-text font-semibold">Why promote this?</span>
            </label>
            <textarea
              class="textarea textarea-bordered h-24"
              value={why}
              onInput={(e) => setWhy((e.target as HTMLTextAreaElement).value)}
              placeholder="Explain the reasoning and benefits of promoting this chunk..."
              disabled={submitting}
            />
            <label class="label">
              <span class="label-text-alt text-base-content/70">
                Justification for why this chunk should be promoted
              </span>
            </label>
          </div>

          <div class="form-control mt-4">
            <label class="label">
              <span class="label-text font-semibold">What does this contain?</span>
            </label>
            <textarea
              class="textarea textarea-bordered h-24"
              value={what}
              onInput={(e) => setWhat((e.target as HTMLTextAreaElement).value)}
              placeholder="Describe the content and purpose of this chunk..."
              disabled={submitting}
            />
            <label class="label">
              <span class="label-text-alt text-base-content/70">
                Summary of what this chunk contains
              </span>
            </label>
          </div>

          <div class="divider"></div>

          <div class="bg-base-200 rounded-lg p-4">
            <h4 class="font-semibold text-sm mb-2 flex items-center gap-2">
              <svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
                <polyline points="14 2 14 8 20 8" />
              </svg>
              Chunk Preview
            </h4>
            <div class="text-xs space-y-1">
              <div class="flex gap-2">
                <span class="text-base-content/70">ID:</span>
                <span class="mono font-semibold">{chunk.id}</span>
              </div>
              <div class="flex gap-2">
                <span class="text-base-content/70">Kind:</span>
                <span class="badge badge-sm">{chunk.kind}</span>
              </div>
              <div class="flex gap-2">
                <span class="text-base-content/70">Author:</span>
                <span class="mono">{chunk.author}</span>
              </div>
              <div class="flex gap-2">
                <span class="text-base-content/70">Confidence:</span>
                <span class="mono">{chunk.confidence.toFixed(2)}</span>
              </div>
            </div>
          </div>

          <div class="modal-action mt-6">
            <button type="submit" class="btn btn-secondary" disabled={submitting}>
              {submitting ? (
                <>
                  <span class="loading loading-spinner"></span>
                  Creating Proposal...
                </>
              ) : (
                <>
                  <svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                    <path d="M12 19l7-7 3 3-7 7-3-3z" />
                    <path d="M18 13l-1.5-7.5L2 2l3.5 14.5L13 18l5-5z" />
                    <path d="M2 2l7.586 7.586" />
                    <circle cx="11" cy="11" r="2" />
                  </svg>
                  Create Proposal
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
