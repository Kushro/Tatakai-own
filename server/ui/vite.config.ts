import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react-swc';
import { fileURLToPath } from 'node:url';
// The dashboard reuses the app's own UI primitives (no copies): '@' -> <repo>/src.
const here = fileURLToPath(new URL('./', import.meta.url));
const repo = fileURLToPath(new URL('../../', import.meta.url));
// Must match the dashboard path served by desktop.mjs / node.mjs (contract 13, D3a).
export const dashboardBase = '/server/';
export default defineConfig({ root: here, base: dashboardBase, plugins: [react()],
  resolve: { alias: { '@': repo + 'src' }, dedupe: ['react', 'react-dom', '@radix-ui/react-slot', '@radix-ui/react-label', 'class-variance-authority', 'clsx', 'tailwind-merge'] },
  // Local PostCSS/Tailwind config (server/ui/postcss.config.cjs), not the app's root one.
  css: { postcss: here },
  build: { outDir: '../dist/ui', emptyOutDir: true },
  server: { fs: { allow: [repo] }, proxy: { '/api': process.env.TATAKAI_DEV_API_URL || `http://127.0.0.1:${process.env.PORT || 8787}` } },
});
