import { resolve } from "node:path";
import { defineConfig } from "vite";

/**
 * Two entries, built one at a time.
 *
 * `qz` installs `window.qz` on import, which is what makes it a drop-in for
 * qz-tray.js. Keeping it out of `index` means importing the SDK never touches
 * a page's globals.
 */
const ENTRIES = { index: "src/index.ts", qz: "src/qz/index.ts" } as const;
const name = (process.env["ESCPOST_ENTRY"] ?? "index") as keyof typeof ENTRIES;

export default defineConfig({
  build: {
    emptyOutDir: name === "index",
    lib: {
      entry: resolve(__dirname, ENTRIES[name]),
      formats: ["es"],
      fileName: () => `${name}.js`,
    },
    outDir: "dist",
    target: "es2022",
    // A page-side library with no dependencies has nothing to externalise, and
    // readable output is worth more here than bytes — people audit this file.
    minify: false,
  },
});
