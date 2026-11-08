const routes = new Set(['health', 'setup', 'auth', 'extensions', 'repositories', 'sources', 'invoke', 'resolve', 'settings', 'devices', 'diagnostics', 'export', 'import', 'native', 'cache', 'media']);
export function logRoute(pathname, prefix) {
  const parts = pathname.slice(prefix.length).split('/').filter(Boolean);
  if (!routes.has(parts[0])) return '/unknown';
  return '/' + parts[0] + (parts[0] === 'auth' && ['login', 'logout', 'password', 'pair', 'pairing'].includes(parts[1]) ? '/' + parts[1] : '');
}
export async function appendServerLog(store, entry) {
  await store.mutate(state => {
    if (!state) return { state, value: undefined };
    const row = { id: (state.logSequence || 0) + 1, at: new Date().toISOString(),
      level: entry.status >= 400 || entry.event === 'source_failed' ? 'error' : 'info',
      event: entry.event, route: entry.route, method: entry.method, status: entry.status,
      durationMs: Math.max(0, Math.round(entry.durationMs || 0)), count: entry.count,
      name: typeof entry.name === 'string' ? entry.name.slice(0, 100) : undefined };
    state.logSequence = row.id; state.logs = [...(state.logs || []), row].slice(-200);
    return { state, value: row };
  }).catch(() => {});
}
