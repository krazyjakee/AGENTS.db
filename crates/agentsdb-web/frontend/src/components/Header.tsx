import { useEffect, useState } from 'preact/hooks';
import { api } from '../api';
import type { ListedLayer } from '../types';

interface HeaderProps {
  onRefresh: () => void;
  layers: ListedLayer[];
  selectedLayer: string;
  onLayerChange: (path: string) => void;
  onShowMetadata: () => void;
  onShowExportImport: () => void;
}

export function Header({
  onRefresh,
  layers,
  selectedLayer,
  onLayerChange,
  onShowMetadata,
  onShowExportImport
}: HeaderProps) {
  const [version, setVersion] = useState<string>('…');
  const [theme, setTheme] = useState<'light' | 'dark'>('light');

  useEffect(() => {
    api
      .getVersion()
      .then((res) => setVersion(`v${res.version}`))
      .catch(() => setVersion('v?'));

    // Load theme from localStorage
    const savedTheme = localStorage.getItem('theme') as 'light' | 'dark' | null;
    if (savedTheme) {
      setTheme(savedTheme);
      document.documentElement.setAttribute('data-theme', savedTheme);
    } else {
      // Check system preference
      const prefersDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
      const initialTheme = prefersDark ? 'dark' : 'light';
      setTheme(initialTheme);
      document.documentElement.setAttribute('data-theme', initialTheme);
    }
  }, []);

  const toggleTheme = () => {
    const newTheme = theme === 'light' ? 'dark' : 'light';
    setTheme(newTheme);
    document.documentElement.setAttribute('data-theme', newTheme);
    localStorage.setItem('theme', newTheme);
  };

  return (
    <header class="navbar bg-base-200 shadow-lg">
      <div class="flex-1">
        <div class="flex items-center gap-4">
          <img src="/logo.png" alt="AGENTS.db logo" class="h-10 w-10" />
          <div>
            <div class="font-mono text-lg font-bold">AGENTS.db</div>
            <div class="text-sm">
              Web interface{' '}
              <span class="badge badge-sm mono" title="Web UI version">
                {version}
              </span>
            </div>
          </div>
        </div>
      </div>
      <div class="flex-none gap-2">
        <div class="flex items-center gap-2">
          <select
            class="select select-bordered select-sm max-w-xs"
            value={selectedLayer}
            onChange={(e) => onLayerChange((e.target as HTMLSelectElement).value)}
            title="Select layer"
          >
            {layers.map((layer) => (
              <option key={layer.path} value={layer.path}>
                {layer.path}
              </option>
            ))}
          </select>
          <button
            onClick={onShowMetadata}
            class="btn btn-ghost btn-sm btn-circle"
            title="Show layer metadata"
            aria-label="Show metadata"
            disabled={!selectedLayer}
          >
            <svg
              class="h-5 w-5"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="2"
              stroke-linecap="round"
              stroke-linejoin="round"
            >
              <circle cx="12" cy="12" r="10" />
              <path d="M12 16v-4" />
              <path d="M12 8h.01" />
            </svg>
          </button>
          <button
            onClick={onShowExportImport}
            class="btn btn-ghost btn-sm btn-circle"
            title="Export / Import"
            aria-label="Export / Import"
            disabled={!selectedLayer}
          >
            <svg
              class="h-5 w-5"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="2"
              stroke-linecap="round"
              stroke-linejoin="round"
            >
              <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
              <polyline points="7 10 12 15 17 10" />
              <line x1="12" y1="15" x2="12" y2="3" />
            </svg>
          </button>
          <a
            href="https://github.com/krazyjakee/AGENTS.db"
            target="_blank"
            rel="noopener noreferrer"
            class="btn btn-ghost btn-sm btn-circle"
            title="View on GitHub"
            aria-label="View on GitHub"
          >
            <svg
              class="h-5 w-5"
              viewBox="0 0 24 24"
              fill="currentColor"
            >
              <path d="M12 0c-6.626 0-12 5.373-12 12 0 5.302 3.438 9.8 8.207 11.387.599.111.793-.261.793-.577v-2.234c-3.338.726-4.033-1.416-4.033-1.416-.546-1.387-1.333-1.756-1.333-1.756-1.089-.745.083-.729.083-.729 1.205.084 1.839 1.237 1.839 1.237 1.07 1.834 2.807 1.304 3.492.997.107-.775.418-1.305.762-1.604-2.665-.305-5.467-1.334-5.467-5.931 0-1.311.469-2.381 1.236-3.221-.124-.303-.535-1.524.117-3.176 0 0 1.008-.322 3.301 1.23.957-.266 1.983-.399 3.003-.404 1.02.005 2.047.138 3.006.404 2.291-1.552 3.297-1.23 3.297-1.23.653 1.653.242 2.874.118 3.176.77.84 1.235 1.911 1.235 3.221 0 4.609-2.807 5.624-5.479 5.921.43.372.823 1.102.823 2.222v3.293c0 .319.192.694.801.576 4.765-1.589 8.199-6.086 8.199-11.386 0-6.627-5.373-12-12-12z" />
            </svg>
          </a>
        </div>
        <button
          onClick={toggleTheme}
          class="btn btn-ghost btn-sm btn-circle"
          title={`Switch to ${theme === 'light' ? 'dark' : 'light'} mode`}
          aria-label="Toggle theme"
        >
          {theme === 'light' ? (
            <svg
              class="h-5 w-5"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="2"
              stroke-linecap="round"
              stroke-linejoin="round"
            >
              <path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z" />
            </svg>
          ) : (
            <svg
              class="h-5 w-5"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="2"
              stroke-linecap="round"
              stroke-linejoin="round"
            >
              <circle cx="12" cy="12" r="5" />
              <path d="M12 1v2m0 18v2M4.22 4.22l1.42 1.42m12.72 12.72 1.42 1.42M1 12h2m18 0h2M4.22 19.78l1.42-1.42M18.36 5.64l1.42-1.42" />
            </svg>
          )}
        </button>
        <button
          onClick={onRefresh}
          class="btn btn-ghost btn-sm btn-circle"
          title="Refresh layer list"
          aria-label="Refresh"
        >
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
        </button>
      </div>
    </header>
  );
}
