// Reference implementation of the install-time compatibility analysis (globals.v1.md, section 4).
// Kotlin (core:extensions) and Rust (tk-ext) must return the same level for the same bundle text.
// No dependencies; pure function of the bundle source.

export const SUPPORTED_MODULES = ['crypto', 'buffer', 'events', 'url', 'path', 'querystring', 'util'];
export const STUB_MODULES = ['os', 'http', 'https', 'stream', 'zlib', 'string_decoder', 'assert', 'timers', 'readline', 'perf_hooks', 'async_hooks', 'module', 'tty'];
export const UNSUPPORTED_BUILTINS = ['fs', 'child_process', 'net', 'tls', 'dgram', 'cluster', 'worker_threads', 'vm', 'dns', 'http2', 'inspector', 'v8', 'repl'];

const REQUIRE_LITERAL = /\brequire\s*\(\s*(['"`])([^'"`\r\n]+)\1\s*\)/g;
const REQUIRE_ANY = /\brequire\s*\(/g;
const REQUIRE_RESOLVE = /\brequire\s*\.\s*resolve\b/;
const DYNAMIC_IMPORT = /(?<![\w.$])import\s*\(/;

/** @returns {{ level: 'full'|'partial'|'unsupported', specifiers: string[], unsupported: string[], stubs: string[], dynamic: boolean, reason: string|null }} */
export function analyzeBundle(source) {
  const specifiers = [];
  for (const m of source.matchAll(REQUIRE_LITERAL)) specifiers.push(m[2].replace(/^node:/, ''));
  const literalCount = specifiers.length;
  const anyCount = [...source.matchAll(REQUIRE_ANY)].length;
  const dynamic = anyCount > literalCount || REQUIRE_RESOLVE.test(source) || DYNAMIC_IMPORT.test(source);

  const unique = [...new Set(specifiers)];
  const unsupported = unique.filter((s) => !SUPPORTED_MODULES.includes(s) && !STUB_MODULES.includes(s));
  const stubs = unique.filter((s) => STUB_MODULES.includes(s));

  if (unsupported.length) {
    const first = unsupported[0];
    const kind = UNSUPPORTED_BUILTINS.includes(first) ? 'Node module' : 'module';
    return { level: 'unsupported', specifiers: unique, unsupported, stubs, dynamic, reason: `Uses ${kind} '${first}' which the native runtime does not provide` };
  }
  if (stubs.length || dynamic) {
    return { level: 'partial', specifiers: unique, unsupported, stubs, dynamic, reason: stubs.length ? `Uses stubbed Node module '${stubs[0]}'` : 'Uses dynamic require/import' };
  }
  return { level: 'full', specifiers: unique, unsupported, stubs, dynamic, reason: null };
}
