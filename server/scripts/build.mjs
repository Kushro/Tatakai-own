import { build } from 'esbuild';
import { build as viteBuild } from 'vite';
import { mkdir, copyFile, cp } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
const root = fileURLToPath(new URL('../', import.meta.url));
const require = createRequire(import.meta.url);
await mkdir(path.join(root, 'dist'), { recursive: true });
const shared = path.join(root, 'shared/tatakai');
await cp(path.join(shared, 'desktop/runtime/compatibility'), path.join(root, 'dist/compatibility'), { recursive: true });
const compiled = JSON.parse(await (await import('node:fs/promises')).readFile(path.join(shared, 'desktop/runtime/compatibility/aniyomi-compiled.json'), 'utf8'));
for (const name of Object.keys(compiled.sources)) {
  if (path.isAbsolute(name) || name.split(/[\\/]/).includes('..')) throw Error('Invalid native source path');
  const target = path.join(root, 'dist/compatibility/source', name);
  await mkdir(path.dirname(target), { recursive: true }); await copyFile(path.join(shared, name), target);
}
await build({ entryPoints: [path.join(root, 'src/node-extension-worker.cjs')], outfile: path.join(root, 'dist/node-extension-worker.cjs'), bundle: true, format: 'cjs', platform: 'node', nodePaths: [path.join(root, 'node_modules')] });
await build({ entryPoints: [path.join(root, 'src/interpreter.ts')], outfile: path.join(root, 'dist/interpreter.mjs'), bundle: true, format: 'esm', platform: 'node',
  external: ['quickjs-emscripten', 'linkedom/worker'], banner: { js: "import { createRequire as __createRequire } from 'node:module'; const require = __createRequire(import.meta.url);" }, nodePaths: [path.join(root, 'node_modules')] });
await copyFile(require.resolve('@jitl/quickjs-wasmfile-release-sync/wasm'), path.join(root, 'dist/quickjs.wasm'));
await build({ entryPoints: [path.join(root, 'src/interpreter-worker.ts')], outfile: path.join(root, 'dist/interpreter-worker.mjs'), bundle: true, format: 'esm', platform: 'browser', conditions: ['workerd'],
  nodePaths: [path.join(root, 'node_modules')],
  external: ['./quickjs.wasm'], plugins: [{ name: 'wasm-import', setup(build) { build.onResolve({ filter: /quickjs\.wasm$/ }, () => ({ path: './quickjs.wasm', external: true })); } }],
});
await viteBuild({ configFile: path.join(root, 'ui/vite.config.ts') });
