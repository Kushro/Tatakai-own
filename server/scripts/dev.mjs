import { createServer as createViteServer } from 'vite';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { startNode } from '../src/node.mjs';

process.chdir(fileURLToPath(new URL('../', import.meta.url)));
try { process.loadEnvFile?.(); } catch (error) { if (error.code !== 'ENOENT') throw error; }
const uiPort = Number(process.env.DEV_UI_PORT || 5173);
const apiPort = Number(process.env.PORT || 8787);
process.env.TATAKAI_DEV_API_URL = `http://127.0.0.1:${apiPort}`;
await new Promise((resolve, reject) => {
  const child = spawn(process.execPath, ['scripts/build.mjs'], { stdio: 'inherit', windowsHide: true });
  child.once('error', reject);
  child.once('exit', code => code === 0 ? resolve() : reject(Error('Server runtime build failed')));
});
let runtime, vite;
try {
  // Browser requests and minted media URLs use the dashboard's proxy origin.
  runtime = await startNode({ port: apiPort, publicUrl: `http://localhost:${uiPort}`, bind: '127.0.0.1' });
  vite = await createViteServer({ configFile: 'ui/vite.config.ts', server: { port: uiPort, strictPort: true, host: 'localhost' } });
  await vite.listen();
  console.log(`Tatakai API: http://127.0.0.1:${apiPort}`);
  vite.printUrls();
} catch (error) {
  await vite?.close(); await runtime?.close(); throw error;
}
let closing = false;
for (const signal of ['SIGINT', 'SIGTERM']) process.once(signal, async () => {
  if (closing) return; closing = true;
  await vite.close(); await runtime.close(); process.exit(0);
});
