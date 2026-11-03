/**
 * A drop-in replacement for `qz-tray.js`.
 *
 * Swap the script tag, or the import specifier, and a page keeps its existing
 * `qz.*` call sites. Printing goes through the ESCPost extension rather than a
 * QZ Tray install, so there is no certificate and no dialog per print.
 *
 * Raw jobs are sent as they are. Image, HTML and PDF jobs are drawn in the
 * page and sent as an ESC/POS raster image (see render.ts).
 *
 * Both ways of arriving here work, because both are how qz-tray is used:
 *
 *     import qz from "@receiptful.io/escpost/qz";   // the default export
 *     <script src="escpost-qz.js"></script>         // installs window.qz (script.ts)
 *
 * The npm package publishes a UMD file with no `module`, `exports` or `types`
 * field, so its consumers write `require("qz-tray")` or `import qz from
 * "qz-tray"` and expect an object back. A named export alone would leave them
 * with `undefined`.
 */
import { createQzShim, installQzShim } from "./surface";

export { createQzShim, installQzShim } from "./surface";
export type { QzConnectOptions, ShimConfig } from "./surface";

/** The type of the `qz` object, for a TypeScript page that passes it around. */
export type QzShim = ReturnType<typeof createQzShim>;

/** The same object `window.qz` is given, for a page that imports rather than
 *  reads a global. */
const qz = createQzShim();

if (typeof window !== "undefined") {
  const target = window as Window & { qz?: unknown };
  if (target.qz === undefined) target.qz = qz;
}

export default qz;
