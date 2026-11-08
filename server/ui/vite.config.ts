import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react-swc';
import { fileURLToPath } from 'node:url';
const project = fileURLToPath(new URL('../shared/tatakai/', import.meta.url));
export default defineConfig({ root: fileURLToPath(new URL('./', import.meta.url)), plugins: [react()],
  resolve: { alias: { '@': project + 'src' }, dedupe: ['react', 'react-dom', '@radix-ui/react-slot', '@radix-ui/react-label', 'class-variance-authority', 'clsx', 'tailwind-merge'] },
  build: { outDir: '../dist/ui', emptyOutDir: false },
  server: { proxy: { '/api': process.env.TATAKAI_DEV_API_URL || `http://127.0.0.1:${process.env.PORT || 8787}` } },
});
