import { useState } from 'preact/hooks';
import type { AddChunkRequest } from '../types';

interface AddChunkPanelProps {
  selectedLayer: string;
  embeddingDim?: number;
  onSubmit: (data: AddChunkRequest) => Promise<void>;
  onCancel: () => void;
}

export function AddChunkPanel({
  selectedLayer,
  embeddingDim = 128,
  onSubmit,
  onCancel,
}: AddChunkPanelProps) {
  const [scope, setScope] = useState<'local' | 'delta'>('local');
  const [kind, setKind] = useState('note');
  const [content, setContent] = useState('');
  const [confidence, setConfidence] = useState(0.8);
  const [sources, setSources] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

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
      kind,
      content: content.trim(),
      confidence,
      dim: embeddingDim,
      sources: sourcesArray.length > 0 ? sourcesArray : undefined,
    };

    try {
      setSubmitting(true);
      await onSubmit(data);
      // Reset form on success
      setContent('');
      setSources('');
      setKind('note');
      setConfidence(0.8);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div class="card bg-base-200 shadow-xl mb-4">
      <div class="card-body">
        <div class="flex justify-between items-center">
          <h2 class="card-title">Add New Chunk</h2>
          <button class="btn btn-sm btn-circle btn-ghost" onClick={onCancel}>
            ✕
          </button>
        </div>

        {error && (
          <div class="alert alert-error">
            <span>{error}</span>
          </div>
        )}

        <form onSubmit={handleSubmit}>
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
              <label class="label">
                <span class="label-text-alt">
                  Local: not committed • Delta: proposed for review
                </span>
              </label>
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
                placeholder="note, invariant, decision, etc."
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
              placeholder="Enter chunk content (Markdown supported)..."
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
              <span class="label-text">Sources (optional)</span>
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

          <div class="flex gap-2 mt-6">
            <button type="submit" class="btn btn-primary" disabled={submitting}>
              {submitting ? <span class="loading loading-spinner"></span> : 'Add Chunk'}
            </button>
            <button type="button" class="btn" onClick={onCancel} disabled={submitting}>
              Cancel
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
