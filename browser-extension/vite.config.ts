import { defineConfig } from "vite";

const entries = {
  background: { file: "src/background.ts", format: "es" },
  relay: { file: "src/relay.ts", format: "iife" },
  popup: { file: "src/popup/popup.ts", format: "es" },
} as const;

const name = process.env["ESCPOST_ENTRY"] as keyof typeof entries | undefined;
if (!name || !(name in entries)) {
  throw new Error(`Set ESCPOST_ENTRY to one of: ${Object.keys(entries).join(", ")}`);
}

const entry = entries[name];

export default defineConfig({
  build: {
    outDir: "dist",
    emptyOutDir: false,
    lib: {
      entry: entry.file,
      formats: [entry.format],
      name: `escpost_${name}`,
      fileName: () => `${name}.js`,
      cssFileName: "popup",
    },
    target: "chrome114",
    minify: false,
  },
});
