import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import { fileURLToPath } from 'node:url';

const legacy = (file) => fileURLToPath(new URL(`./node_modules/pdfjs-dist/legacy/build/${file}`, import.meta.url));

// Tauri expects a fixed dev port; the production build is a static bundle in ./dist.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  resolve: {
    alias: [
      // PDF.js "legacy" build: works on older WebKit (macOS 10.15+, Ubuntu 22.04 WebKitGTK).
      { find: /^pdfjs-dist\/build\/pdf\.worker\.mjs$/, replacement: legacy('pdf.worker.mjs') },
      { find: /^pdfjs-dist$/, replacement: legacy('pdf.mjs') },
    ],
  },
  server: { port: 1420, strictPort: true, host: '127.0.0.1' },
  build: {
    target: ['es2021', 'chrome100', 'safari14'],
    chunkSizeWarningLimit: 8000,
    sourcemap: false,
  },
  worker: { format: 'es' },
});
