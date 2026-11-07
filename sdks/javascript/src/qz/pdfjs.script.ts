/**
 * The `escpost-qz.js` version of pdfjs.ts. A classic script cannot be split
 * into files, so instead of bundling pdf.js (1.7 MB for every page), it loads
 * the two pdf.js files the npm build wrote, from the folder escpost-qz.js was
 * loaded from: `dist/pdfjs/` next to `dist/escpost-qz.js`.
 */

// Read while the script runs: document.currentScript is null afterwards.
const scriptUrl =
  typeof document !== "undefined" && document.currentScript instanceof HTMLScriptElement
    ? document.currentScript.src
    : "";

export function importPdfJs(): Promise<[unknown, unknown]> {
  const base = scriptUrl || location.href;
  return Promise.all([
    import(/* @vite-ignore */ new URL("pdfjs/pdf.min.js", base).href),
    import(/* @vite-ignore */ new URL("pdfjs/pdf.worker.min.js", base).href),
  ]);
}
