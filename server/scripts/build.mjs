// Builds the integrated extension server into server/dist:
//   interpreter.mjs            QuickJS interpreter adapters (bundles the app's src/core/extensions/{sora,mangayomi,compatibility})
//   node-extension-worker.cjs  worker bootstrap for trusted Toko/Tatakai Node bundles
//   quickjs.wasm               QuickJS release-sync wasm (shipped next to the bundle; asarUnpack'ed by electron-builder)
//   ui/                        dashboard (Vite, base '/server/')
// Nothing from the app is copied: esbuild/Vite read the app sources in place and
// resolve packages from the single root node_modules. The Java/compatibility
// runtime is NOT copied either: node.mjs requires <repo>/desktop/runtime/compatibility
// directly, and aniyomi.cjs verifies its bootstrap sources at <repo>/<name>
// (desktop/runtime/compatibility/aniyomi-java/** and shared/aniyomi/**).
import { build } from 'esbuild';
import { build as viteBuild } from 'vite';
import { mkdir, copyFile, rm } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const server = fileURLToPath(new URL('../', import.meta.url));
const repo = path.resolve(server, '..');
const dist = path.join(server, 'dist');
const require = createRequire(path.join(repo, 'package.json'));
const nodePaths = [path.join(repo, 'node_modules')];
const alias = { '@': path.join(repo, 'src') };

await rm(dist, { recursive: true, force: true });
await mkdir(dist, { recursive: true });
await build({ entryPoints: [path.join(server, 'src/node-extension-worker.cjs')], outfile: path.join(dist, 'node-extension-worker.cjs'),
  bundle: true, format: 'cjs', platform: 'node', target: 'node22', nodePaths, logLevel: 'warning' });
await build({ entryPoints: [path.join(server, 'src/interpreter.ts')], outfile: path.join(dist, 'interpreter.mjs'),
  bundle: true, format: 'esm', platform: 'node', target: 'node22', alias, nodePaths, logLevel: 'warning',
  // Resolved at runtime from the root node_modules (quickjs loads its wasm variant itself).
  external: ['quickjs-emscripten', 'linkedom/worker'],
  banner: { js: "import { createRequire as __createRequire } from 'node:module'; const require = __createRequire(import.meta.url);" } });
await copyFile(require.resolve('@jitl/quickjs-wasmfile-release-sync/wasm'), path.join(dist, 'quickjs.wasm'));
// server/ui/vite.config.ts owns the dashboard config (alias '@' -> <repo>/src, Tailwind);
// base and outDir are pinned here because the HTTP routes depend on them (contract §2, D3a).
await viteBuild({ configFile: path.join(server, 'ui/vite.config.ts'), base: '/server/',
  build: { outDir: path.join(dist, 'ui'), emptyOutDir: true }, logLevel: 'warn' });
console.log(`Tatakai extension server built in ${path.relative(repo, dist) || dist}`);
