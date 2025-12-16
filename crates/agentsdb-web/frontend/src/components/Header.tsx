import { useEffect, useState } from 'preact/hooks';
import { api } from '../api';
import type { ListedLayer } from '../types';

interface HeaderProps {
  onRefresh: () => void;
  layers: ListedLayer[];
  selectedLayer: string;
  onLayerChange: (path: string) => void;
  onShowMetadata: () => void;
}

export function Header({
  onRefresh,
  layers,
  selectedLayer,
  onLayerChange,
  onShowMetadata
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
