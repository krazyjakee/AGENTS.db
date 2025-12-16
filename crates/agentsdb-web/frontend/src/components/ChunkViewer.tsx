import { useState } from 'preact/hooks';
import type { ChunkFull } from '../types';
import { renderMarkdown } from '../utils/markdown';

interface ChunkViewerProps {
  chunk: ChunkFull | null;
  onClose: () => void;
  onPropose?: (chunk: ChunkFull) => void;
  onPromote?: (chunk: ChunkFull) => void;
  onEdit?: (chunk: ChunkFull) => void;
}

export function ChunkViewer({ chunk, onClose, onPropose, onEdit, onPromote }: ChunkViewerProps) {
  const [showRaw, setShowRaw] = useState(false);

  if (!chunk) return null;

  const renderedContent = showRaw ? chunk.content : renderMarkdown(chunk.content);
  const createdDate = chunk.created_at_unix_ms
    ? new Date(chunk.created_at_unix_ms).toLocaleString()
    : 'Unknown';

  return (
    <dialog class="modal modal-open">
      <div class="modal-box max-w-5xl max-h-[90vh]">
        <div class="flex justify-between items-start mb-4">
          <div class="flex-1">
            <h3 class="font-bold text-xl mb-2 flex items-center gap-2">
              <svg
                class="h-6 w-6 text-primary"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="2"
                stroke-linecap="round"
                stroke-linejoin="round"
              >
                <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
                <polyline points="14 2 14 8 20 8" />
                <line x1="16" y1="13" x2="8" y2="13" />
                <line x1="16" y1="17" x2="8" y2="17" />
                <line x1="10" y1="9" x2="8" y2="9" />
              </svg>
              Chunk Details
            </h3>
            <div class="flex flex-wrap gap-2 items-center">
              <span class="badge badge-lg badge-primary mono">ID: {chunk.id}</span>
              <span class="badge badge-lg badge-secondary">{chunk.kind}</span>
              {chunk.removed && <span class="badge badge-lg badge-error">removed</span>}
              <span class="badge badge-lg badge-info mono">conf: {chunk.confidence.toFixed(2)}</span>
            </div>
            <div class="text-sm text-base-content/70 mt-2 flex flex-wrap gap-x-3 gap-y-1">
              <span class="flex items-center gap-1">
                <svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                  <path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2" />
                  <circle cx="12" cy="7" r="4" />
                </svg>
                <span class="mono">{chunk.author}</span>
              </span>
              <span class="flex items-center gap-1">
                <svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                  <circle cx="12" cy="12" r="10" />
                  <polyline points="12 6 12 12 16 14" />
                </svg>
                {createdDate}
              </span>
            </div>
          </div>
          <button class="btn btn-sm btn-circle btn-ghost" onClick={onClose}>
            ✕
          </button>
        </div>

        {chunk.sources && chunk.sources.length > 0 && (
          <div class="alert alert-info mb-4">
            <div class="w-full">
              <div class="flex items-center gap-2 mb-2">
                <svg class="h-5 w-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                  <path d="M4 19.5A2.5 2.5 0 0 1 6.5 17H20" />
                  <path d="M6.5 2H20v20H6.5A2.5 2.5 0 0 1 4 19.5v-15A2.5 2.5 0 0 1 6.5 2z" />
                </svg>
                <span class="font-semibold">Sources</span>
              </div>
              <div class="flex flex-wrap gap-2">
                {chunk.sources.map((source, idx) => (
                  <span key={idx} class="badge badge-outline mono text-xs">
                    {source}
                  </span>
                ))}
              </div>
            </div>
          </div>
        )}

        <div class="divider my-2"></div>

        <div class="flex justify-between items-center mb-3">
          <h4 class="font-semibold text-lg flex items-center gap-2">
            <svg class="h-5 w-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M12 20h9" />
              <path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4Z" />
            </svg>
            Content
          </h4>
          <div class="join">
            <button
              class={`join-item btn btn-xs ${!showRaw ? 'btn-active' : ''}`}
              onClick={() => setShowRaw(false)}
              title="Show rendered"
            >
              Rendered
            </button>
            <button
              class={`join-item btn btn-xs ${showRaw ? 'btn-active' : ''}`}
              onClick={() => setShowRaw(true)}
              title="Show raw"
            >
              Raw
            </button>
          </div>
        </div>

        <div class="py-4 overflow-auto max-h-[50vh] bg-base-200 rounded-lg p-4">
          {showRaw ? (
            <pre class="whitespace-pre-wrap font-mono text-sm">
              {chunk.content}
            </pre>
          ) : (
            <div
              class="prose prose-sm max-w-none"
              dangerouslySetInnerHTML={{ __html: renderedContent }}
            />
          )}
        </div>

        <div class="modal-action mt-4">
          <div class="flex gap-2 flex-wrap w-full justify-end">
            {onEdit && !chunk.removed && (
              <button class="btn btn-sm btn-primary gap-1" onClick={() => onEdit(chunk)}>
                <svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                  <path d="M12 20h9" />
                  <path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4Z" />
                </svg>
                Edit
              </button>
            )}
            {onPropose && !chunk.removed && (
              <button
                class="btn btn-sm btn-secondary gap-1"
                onClick={() => onPropose(chunk)}
                title="Propose this chunk for promotion"
              >
                <svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                  <path d="M12 19l7-7 3 3-7 7-3-3z" />
                  <path d="M18 13l-1.5-7.5L2 2l3.5 14.5L13 18l5-5z" />
                  <path d="M2 2l7.586 7.586" />
                  <circle cx="11" cy="11" r="2" />
                </svg>
                Propose
              </button>
            )}
            {onPromote && !chunk.removed && (
              <button
                class="btn btn-sm btn-accent gap-1"
                onClick={() => onPromote(chunk)}
                title="Promote this chunk directly"
              >
                <svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                  <polyline points="18 15 12 9 6 15" />
                </svg>
                Promote
              </button>
            )}
            <button class="btn btn-sm btn-ghost" onClick={onClose}>
              Close
            </button>
          </div>
        </div>
      </div>
      <form method="dialog" class="modal-backdrop" onClick={onClose}>
        <button>close</button>
      </form>
    </dialog>
  );
}
