/// <reference types="vitest/config" />
import { createHash } from 'node:crypto';
import react from '@vitejs/plugin-react';
import { defineConfig, type Plugin } from 'vite';

// The API comes from the Rust simulator (`cargo run -p openkwl-sim`, port 8080), which runs the
// firmware's own application code, or from a real unit: OPENKWL_DEVICE=http://192.168.1.50.
const device = process.env.OPENKWL_DEVICE ?? 'http://localhost:8080';

// The SD card's FAT driver only knows 8.3 names, so the build emits exactly
// index.html, app.js and app.css plus the icons from public/; scripts/package-sd.mjs gzips
// them under 8.3 names into WWW/ (INDEX.HTM, APP.JS, APP.CSS, FAVICON.ICO, …).
// The file names cannot change with the content, so index.html asks for `/app.js?v=<hash>`
// instead: a new build is a new URL, and the firmware serves versioned files as immutable.
function versionedAssets(): Plugin {
  return {
    name: 'versioned-assets',
    apply: 'build',
    enforce: 'post',
    generateBundle(_, bundle) {
      const html = bundle['index.html'];
      if (html?.type !== 'asset') return;
      let source = String(html.source);
      for (const name of ['app.js', 'app.css']) {
        const file = bundle[name];
        if (!file) continue;
        const content = file.type === 'chunk' ? file.code : file.source;
        const v = createHash('sha256').update(content).digest('hex').slice(0, 10);
        source = source.replaceAll(`"/${name}"`, `"/${name}?v=${v}"`);
      }
      html.source = source;
    },
  };
}

export default defineConfig({
  plugins: [react(), versionedAssets()],
  test: { setupFiles: ['src/test-setup.ts'] },
  server: {
    proxy: { '/api': device },
  },
  build: {
    target: 'es2022',
    cssCodeSplit: false,
    assetsInlineLimit: 100_000,
    modulePreload: false,
    // One bundle on purpose (fixed 8.3 file names on the SD card).
    chunkSizeWarningLimit: 900,
    rolldownOptions: {
      output: {
        entryFileNames: 'app.js',
        assetFileNames: (asset) => (asset.names?.[0]?.endsWith('.css') ? 'app.css' : '[name][extname]'),
        // No split chunks: dynamic imports are inlined into app.js.
        codeSplitting: false,
      },
    },
  },
});
