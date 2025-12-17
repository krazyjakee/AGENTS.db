import { useState } from 'preact/hooks';
import type { ImportRequest, ImportResponse } from '../types';

interface ExportImportPanelProps {
  selectedLayer: string;
  embeddingDim?: number;
  onExport: (path: string, format: string, redact: string) => Promise<void>;
  onImport: (data: ImportRequest) => Promise<ImportResponse>;
  onClose: () => void;
}

export function ExportImportPanel({
  selectedLayer,
  embeddingDim = 128,
  onExport,
  onImport,
  onClose,
}: ExportImportPanelProps) {
  const [activeTab, setActiveTab] = useState<'export' | 'import'>('export');

  // Export state
  const [exportFormat, setExportFormat] = useState('json');
  const [exportRedact, setExportRedact] = useState('none');
  const [exporting, setExporting] = useState(false);

  // Import state
  const [importScope, setImportScope] = useState<'local' | 'delta'>('local');
  const [importFormat, setImportFormat] = useState('json');
  const [importData, setImportData] = useState('');
  const [importDryRun, setImportDryRun] = useState(true);
  const [importDedupe, setImportDedupe] = useState(true);
  const [importPreserveIds, setImportPreserveIds] = useState(false);
  const [importing, setImporting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [importResult, setImportResult] = useState<string | null>(null);

  const handleExport = async () => {
    setError(null);
    try {
      setExporting(true);
      await onExport(selectedLayer, exportFormat, exportRedact);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setExporting(false);
    }
  };

  const handleImport = async (e: Event) => {
    e.preventDefault();
    setError(null);
    setImportResult(null);

    if (!importData.trim()) {
      setError('Import data is required');
      return;
    }

    const request: ImportRequest = {
      path: selectedLayer,
      scope: importScope,
      format: importFormat,
      data: importData,
      dry_run: importDryRun,
      dedupe: importDedupe,
      preserve_ids: importPreserveIds,
      dim: embeddingDim,
    };

    try {
      setImporting(true);
      const result = await onImport(request);
      setImportResult(
        `${result.dry_run ? 'Dry run: ' : ''}Imported ${result.imported} chunks, skipped ${result.skipped}`
      );
      if (!result.dry_run) {
        setImportData('');
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setImporting(false);
    }
  };

  return (
    <dialog class="modal modal-open">
      <div class="modal-box max-w-3xl">
        <div class="flex justify-between items-center mb-4">
          <h2 class="font-bold text-lg">Export / Import</h2>
          <button class="btn btn-sm btn-circle btn-ghost" onClick={onClose}>
            ✕
          </button>
        </div>

        {error && (
          <div class="alert alert-error mb-4">
            <span>{error}</span>
          </div>
        )}

        {importResult && (
          <div class="alert alert-success mb-4">
            <span>{importResult}</span>
          </div>
        )}

        <div class="tabs tabs-boxed mb-4">
          <button
            class={`tab ${activeTab === 'export' ? 'tab-active' : ''}`}
            onClick={() => setActiveTab('export')}
          >
            Export
          </button>
          <button
            class={`tab ${activeTab === 'import' ? 'tab-active' : ''}`}
            onClick={() => setActiveTab('import')}
          >
            Import
          </button>
        </div>

        {activeTab === 'export' ? (
          <div class="space-y-4">
            <div class="form-control">
              <label class="label">
                <span class="label-text">Format</span>
              </label>
              <select
                class="select select-bordered"
                value={exportFormat}
                onChange={(e) => setExportFormat((e.target as HTMLSelectElement).value)}
                disabled={exporting}
              >
                <option value="json">JSON (pretty)</option>
                <option value="ndjson">NDJSON (newline-delimited)</option>
              </select>
            </div>

            <div class="form-control">
              <label class="label">
                <span class="label-text">Redaction</span>
              </label>
              <select
                class="select select-bordered"
                value={exportRedact}
                onChange={(e) => setExportRedact((e.target as HTMLSelectElement).value)}
                disabled={exporting}
              >
                <option value="none">None (full export)</option>
                <option value="embeddings">Redact embeddings</option>
                <option value="content">Redact content</option>
                <option value="both">Redact both</option>
              </select>
            </div>

            <div class="alert alert-info">
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
              <span class="text-sm">
                Exports will download as a file. Use redaction to exclude sensitive data.
              </span>
            </div>

            <button class="btn btn-primary" onClick={handleExport} disabled={exporting}>
              {exporting ? <span class="loading loading-spinner"></span> : 'Export Layer'}
            </button>
          </div>
        ) : (
          <form onSubmit={handleImport} class="space-y-4">
            <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
              <div class="form-control">
                <label class="label">
                  <span class="label-text">Scope *</span>
                </label>
                <select
                  class="select select-bordered"
                  value={importScope}
                  onChange={(e) =>
                    setImportScope((e.target as HTMLSelectElement).value as 'local' | 'delta')
                  }
                  disabled={importing}
                >
                  <option value="local">Local (temporary)</option>
                  <option value="delta">Delta (proposed)</option>
                </select>
              </div>

              <div class="form-control">
                <label class="label">
                  <span class="label-text">Format</span>
                </label>
                <select
                  class="select select-bordered"
                  value={importFormat}
                  onChange={(e) => setImportFormat((e.target as HTMLSelectElement).value)}
                  disabled={importing}
                >
                  <option value="json">JSON</option>
                  <option value="ndjson">NDJSON</option>
                </select>
              </div>
            </div>

            <div class="form-control">
              <label class="label">
                <span class="label-text">Import Data *</span>
              </label>
              <textarea
                class="textarea textarea-bordered h-40 font-mono text-xs"
                value={importData}
                onInput={(e) => setImportData((e.target as HTMLTextAreaElement).value)}
                placeholder='Paste JSON or NDJSON data here...\nExample: {"kind":"note","content":"...","confidence":0.8}'
                disabled={importing}
                required
              />
            </div>

            <div class="flex flex-wrap gap-4">
              <label class="label cursor-pointer gap-2">
                <input
                  type="checkbox"
                  class="checkbox checkbox-sm"
                  checked={importDryRun}
                  onChange={(e) => setImportDryRun((e.target as HTMLInputElement).checked)}
                  disabled={importing}
                />
                <span class="label-text">Dry run (test only)</span>
              </label>

              <label class="label cursor-pointer gap-2">
                <input
                  type="checkbox"
                  class="checkbox checkbox-sm"
                  checked={importDedupe}
                  onChange={(e) => setImportDedupe((e.target as HTMLInputElement).checked)}
                  disabled={importing}
                />
                <span class="label-text">Deduplicate</span>
              </label>

              <label class="label cursor-pointer gap-2">
                <input
                  type="checkbox"
                  class="checkbox checkbox-sm"
                  checked={importPreserveIds}
                  onChange={(e) => setImportPreserveIds((e.target as HTMLInputElement).checked)}
                  disabled={importing}
                />
                <span class="label-text">Preserve IDs</span>
              </label>
            </div>

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
              <span class="text-sm">
                Always use dry run first to preview changes. Enable "Preserve IDs" to maintain chunk
                IDs from the import data.
              </span>
            </div>

            <button type="submit" class="btn btn-primary" disabled={importing}>
              {importing ? <span class="loading loading-spinner"></span> : 'Import Chunks'}
            </button>
          </form>
        )}
      </div>
      <form method="dialog" class="modal-backdrop" onClick={onClose}>
        <button>close</button>
      </form>
    </dialog>
  );
}
