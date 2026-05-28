import path from 'path';
import tailwindcss from '@tailwindcss/vite';
import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';
import { viteSingleFile } from 'vite-plugin-singlefile';
import { adminApiMockPlugin } from './dev-mocks';

export default defineConfig({
  // The admin-api mock plugin only attaches middleware during `vite dev`;
  // it never participates in the production single-file build, so the
  // bundle shipped through the gateway stays unchanged.
  plugins: [react(), tailwindcss(), viteSingleFile(), adminApiMockPlugin()],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  build: {
    assetsInlineLimit: Number.MAX_SAFE_INTEGER,
    cssCodeSplit: false,
    emptyOutDir: true,
    outDir: '../crates/dcc-mcp-gateway/src/gateway/admin/generated',
  },
});
