import { readFileSync } from "node:fs";
import { defineConfig } from "vite";

// pdf.js's own minified files, and where they go in dist/. The QZ shim imports
// them on its first PDF (src/qz/pdfjs.ts). They are copied unchanged, license
// header included, rather than rebuilt; `.js` because some servers send `.mjs`
// files with a type browsers refuse to run.
const PDFJS_FILES: Record<string, string> = {
  "pdfjs-dist/build/pdf.min.mjs": "pdfjs/pdf.min.js",
  "pdfjs-dist/build/pdf.worker.min.mjs": "pdfjs/pdf.worker.min.js",
};

export default defineConfig({
  plugins: [
    {
      name: "copy-pdfjs",
      apply: "build",
      generateBundle() {
        for (const [id, fileName] of Object.entries(PDFJS_FILES)) {
          this.emitFile({ type: "asset", fileName, source: readFileSync(new URL(`node_modules/${id}`, import.meta.url)) });
        }
      },
    },
  ],
  build: {
    lib: {
      // Two entries, one per package export: "." and "./qz".
      entry: { index: "src/index.ts", qz: "src/qz/index.ts" },
      formats: ["es"],
      fileName: (_format, entryName) => `${entryName}.js`,
    },
    rollupOptions: {
      // Keep pdf.js out of the bundle, and point the imports at the copies.
      external: Object.keys(PDFJS_FILES),
      output: { paths: (id) => `./${PDFJS_FILES[id] ?? id}` },
    },
    minify: false,
  },
});
