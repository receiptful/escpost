import { defineConfig } from "vite";
import { resolve } from "node:path";

/**
 * One entry per build pass, deliberately.
 *
 * A multi-entry Rollup build extracts code shared between entries into chunks and
 * emits `import` statements to reach them. That is fatal here: Chrome loads content
 * scripts as CLASSIC scripts, where an `import` statement is a syntax error, so
 * ws-patch.js would never execute and window.WebSocket would never be patched (W1) —
 * silently, with nothing in the console of the page that broke.
 *
 * So each entry is built alone and self-contained. Content scripts are emitted as
 * IIFEs (no import, no export); the service worker and the popup are ES modules,
 * because the manifest declares the worker `"type": "module"` and popup.html loads
 * popup.js with `<script type="module">`.
 */
const ENTRIES = {
  background: { file: "src/background.ts", format: "es" },
  relay: { file: "src/relay.ts", format: "iife" },
  "auth-bridge": { file: "src/auth-bridge.ts", format: "iife" },
  "ws-patch": { file: "src/ws-patch.ts", format: "iife" },
  "qz-shim": { file: "src/qz-shim.ts", format: "iife" },
  popup: { file: "src/popup/popup.ts", format: "es" },
  settings: { file: "src/settings/settings.ts", format: "es" },
  welcome: { file: "src/welcome/welcome.ts", format: "es" },
} as const;

const name = process.env["ESCPOST_ENTRY"] as keyof typeof ENTRIES | undefined;
if (!name || !(name in ENTRIES)) {
  throw new Error(`Set ESCPOST_ENTRY to one of: ${Object.keys(ENTRIES).join(", ")}`);
}
const entry = ENTRIES[name];

/**
 * The one place a build learns which Receiptful it talks to. Defaults to
 * production, so a plain `bun run build` is always a shippable build.
 * scripts/build-manifest.mjs reads the same variable and keeps
 * manifest.json's host_permissions and auth-bridge match in step.
 */
const API_BASE = process.env["ESCPOST_API_BASE"] ?? "https://api.receiptful.io";

export default defineConfig({
  define: {
    __ESCPOST_API_BASE__: JSON.stringify(API_BASE),
  },
  build: {
    outDir: "dist",
    // Each pass writes one file; the build script clears dist once, up front.
    emptyOutDir: false,
    lib: {
      entry: resolve(__dirname, entry.file),
      formats: [entry.format],
      name: `escpost_${name.replace(/-/g, "_")}`,
      fileName: () => `${name}.js`,
    },
    target: "chrome111",
    minify: false, // Web Store review reads this code; so will you, in six months.
  },
});
