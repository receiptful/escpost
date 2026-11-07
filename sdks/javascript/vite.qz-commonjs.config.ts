import { defineConfig } from "vite";

// The ES build copies these files into dist/. Keep them external here so the
// CommonJS entry preserves the same lazy PDF loading and package layout.
const PDFJS_FILES: Record<string, string> = {
  "pdfjs-dist/build/pdf.min.mjs": "pdfjs/pdf.min.js",
  "pdfjs-dist/build/pdf.worker.min.mjs": "pdfjs/pdf.worker.min.js",
};

export default defineConfig({
  build: {
    lib: {
      // A separate default-only entry lets require(".../qz") return qz itself
      // rather than Rollup's { default, createQzShim, ... } namespace object.
      entry: "src/qz/commonjs.ts",
      formats: ["cjs"],
      fileName: () => "qz.cjs",
    },
    rollupOptions: {
      external: Object.keys(PDFJS_FILES),
      output: {
        exports: "default",
        paths: (id) => `./${PDFJS_FILES[id] ?? id}`,
      },
    },
    emptyOutDir: false,
    minify: false,
  },
});
