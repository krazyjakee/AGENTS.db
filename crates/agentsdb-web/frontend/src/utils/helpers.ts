export function writeScopeForPath(path: string): string {
  if (path === 'AGENTS.local.db') return 'local';
  if (path === 'AGENTS.delta.db') return 'delta';
  return '';
}

export function getTargetOptions(fromPath: string): Array<{ value: string; label: string }> {
  const options: Array<{ value: string; label: string }> = [];

  if (fromPath === 'AGENTS.local.db') {
    options.push(
      { value: 'AGENTS.user.db', label: 'AGENTS.user.db' },
      { value: 'AGENTS.delta.db', label: 'AGENTS.delta.db' }
    );
  } else if (fromPath === 'AGENTS.user.db') {
    options.push({ value: 'AGENTS.delta.db', label: 'AGENTS.delta.db' });
  } else if (fromPath === 'AGENTS.delta.db') {
    options.push(
      { value: 'AGENTS.user.db', label: 'AGENTS.user.db' },
      { value: 'AGENTS.db', label: 'AGENTS.db' }
    );
  }

  return options;
}

export function getScopeOptions(path: string): Array<{ value: string; label: string }> {
  const scope = writeScopeForPath(path);
  if (!scope) return [];

  if (scope === 'local') {
    return [{ value: 'local', label: 'local (AGENTS.local.db)' }];
  } else if (scope === 'delta') {
    return [{ value: 'delta', label: 'delta (AGENTS.delta.db)' }];
  }

  return [];
}

export function getImportScopeOptions(): Array<{ value: string; label: string }> {
  return [
    { value: 'local', label: 'local → AGENTS.local.db' },
    { value: 'delta', label: 'delta → AGENTS.delta.db' },
    { value: 'user', label: 'user → AGENTS.user.db' },
    { value: 'base', label: 'base → AGENTS.db (danger)' },
  ];
}

export function downloadBlob(blob: Blob, filename: string) {
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}
