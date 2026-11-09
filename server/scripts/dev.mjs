// Standalone development loop: builds the runtime bundles, starts the Node API
// on PORT (default 8787) and serves the dashboard through Vite with HMR at
// http://localhost:<DEV_UI_PORT>/server/ (the Vite config proxies /api to the API).
import { createServer as createViteServer } from 'vite';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import { startNode } from '../src/node.mjs';

const server = fileURLToPath(new URL('../', import.meta.url));
process.chdir(server);
try { process.loadEnvFile?.(); } catch (error) { if (error.code !== 'ENOENT') throw error; }
const uiPort = Number(process.env.DEV_UI_PORT || 5173);
const apiPort = Number(process.env.PORT || 8787);
process.env.TATAKAI_DEV_API_URL = `http://127.0.0.1:${apiPort}`;
await new Promise((resolve, reject) => {
  const child = spawn(process.execPath, [path.join(server, 'scripts/build.mjs')], { stdio: 'inherit', windowsHide: true });
  child.once('error', reject);
  child.once('exit', code => code === 0 ? resolve() : reject(Error('Server runtime build failed')));
});
let runtime, vite;
try {
  // Browser requests and minted media URLs use the dashboard's proxy origin.
  runtime = await startNode({ port: apiPort, publicUrl: `http://localhost:${uiPort}`, bind: '127.0.0.1' });
  vite = await createViteServer({ configFile: path.join(server, 'ui/vite.config.ts'), base: '/server/', server: { port: uiPort, strictPort: true, host: 'localhost' } });
  await vite.listen();
  console.log(`Tatakai API: http://127.0.0.1:${apiPort}/api/server/v1`);
  console.log(`Dashboard:   http://localhost:${uiPort}/server/`);
  vite.printUrls();
} catch (error) {
  await vite?.close(); await runtime?.close(); throw error;
}
let closing = false;
for (const signal of ['SIGINT', 'SIGTERM']) process.once(signal, async () => {
  if (closing) return; closing = true;
  await vite.close(); await runtime.close(); process.exit(0);
});
