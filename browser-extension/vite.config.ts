import { defineConfig } from "vite";

const browser = process.env["ESCPOST_BROWSER"];
if (browser !== "chrome" && browser !== "firefox") throw new Error("Set ESCPOST_BROWSER to chrome or firefox");

const entries = {
  background: { file: `src/${browser}/background.ts`, format: "es" },
  bridge: { file: "src/chrome/bridge.ts", format: "es" },
  relay: { file: "src/relay.ts", format: "iife" },
  popup: { file: `src/${browser}/popup.ts`, format: "es" },
} as const;

const name = process.env["ESCPOST_ENTRY"] as keyof typeof entries | undefined;
if (!name || !(name in entries)) {
  throw new Error(`Set ESCPOST_ENTRY to one of: ${Object.keys(entries).join(", ")}`);
}

const entry = entries[name];

export default defineConfig({
  build: {
    outDir: `dist/${browser}`,
    emptyOutDir: false,
    lib: {
      entry: entry.file,
      formats: [entry.format],
      name: `escpost_${name}`,
      fileName: () => `${name}.js`,
      cssFileName: "popup",
    },
    target: browser === "chrome" ? "chrome114" : "firefox121",
    minify: false,
  },
});
