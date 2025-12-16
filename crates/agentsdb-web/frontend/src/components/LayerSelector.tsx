import type { LayerMeta } from '../types';

interface LayerSelectorProps {
  layerMeta: LayerMeta | null;
  kindFilter: string;
  includeRemoved: boolean;
  onKindFilterChange: (kind: string) => void;
  onIncludeRemovedChange: (include: boolean) => void;
  onLoad: () => void;
  onAdd: () => void;
}

export function LayerSelector({
  layerMeta,
  kindFilter,
  includeRemoved,
  onKindFilterChange,
  onIncludeRemovedChange,
  onLoad,
  onAdd,
}: LayerSelectorProps) {
  return (
    <div class="card bg-base-200 shadow-xl mb-4">
      <div class="card-body">
        <h2 class="card-title text-2xl mb-2">
          <svg
            class="h-6 w-6"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            stroke-linecap="round"
            stroke-linejoin="round"
          >
            <path d="M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71" />
            <path d="M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71" />
          </svg>
          Filters &amp; Actions
        </h2>

        <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
          <div class="form-control">
            <label class="label">
              <span class="label-text font-semibold">Filter by Kind</span>
            </label>
            <select
              class="select select-bordered w-full"
              value={kindFilter}
              onChange={(e) => onKindFilterChange((e.target as HTMLSelectElement).value)}
            >
              <option value="">(all kinds)</option>
              {layerMeta &&
                Object.keys(layerMeta.kinds).map((kind) => (
                  <option key={kind} value={kind}>
                    {kind}
                  </option>
                ))}
            </select>
          </div>

          <div class="form-control">
            <label class="label">
              <span class="label-text font-semibold">Include Removed</span>
            </label>
            <div class="flex gap-2">
              <label class="label cursor-pointer">
                <input
                  type="radio"
                  name="includeRemoved"
                  class="radio radio-primary"
                  checked={!includeRemoved}
                  onChange={() => onIncludeRemovedChange(false)}
                />
                <span class="label-text ml-2">No</span>
              </label>
              <label class="label cursor-pointer ml-4">
                <input
                  type="radio"
                  name="includeRemoved"
                  class="radio radio-primary"
                  checked={includeRemoved}
                  onChange={() => onIncludeRemovedChange(true)}
                />
                <span class="label-text ml-2">Yes</span>
              </label>
            </div>
          </div>
        </div>

        <div class="flex gap-2 mt-4">
          <button onClick={onLoad} class="btn btn-primary btn-md flex-1">
            <svg
              class="h-5 w-5"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="2"
              stroke-linecap="round"
              stroke-linejoin="round"
            >
              <path d="M21 12a9 9 0 1 1-2.64-6.36" />
              <path d="M21 3v6h-6" />
            </svg>
            Load Chunks
          </button>
          <button onClick={onAdd} class="btn btn-secondary btn-md flex-1">
            <svg
              class="h-5 w-5"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="2"
              stroke-linecap="round"
              stroke-linejoin="round"
            >
              <line x1="12" y1="5" x2="12" y2="19" />
              <line x1="5" y1="12" x2="19" y2="12" />
            </svg>
            Add Chunk
          </button>
        </div>
      </div>
    </div>
  );
}
