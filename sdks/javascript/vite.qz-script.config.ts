import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";

// Second build: `dist/escpost-qz.js`, a classic script that defines `window.qz`
// for pages that load QZ Tray with a `<script>` tag instead of an import.
export default defineConfig({
  resolve: {
    alias: [
      {
        // A classic script cannot load pdf.js as split files, so it uses the
        // version that loads the first build's dist/pdfjs/ files by URL.
        find: /^\.\/pdfjs$/,
        replacement: fileURLToPath(new URL("src/qz/pdfjs.script.ts", import.meta.url)),
      },
    ],
  },
  build: {
    lib: {
      entry: "src/qz/script.ts",
      formats: ["iife"],
      name: "EscpostQz",
      fileName: () => "escpost-qz.js",
    },
    // The first build already wrote dist/; keep its files.
    emptyOutDir: false,
    minify: false,
  },
});
