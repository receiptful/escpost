/// <reference path="./pdfjs-modules.d.ts" />

/**
 * pdf.js comes from this package, not from a CDN, so PDF jobs print offline and
 * on sites whose CSP blocks other domains.
 *
 * The build copies pdf.js's two files to `dist/pdfjs/pdf.min.js` and
 * `dist/pdfjs/pdf.worker.min.js`, and points these imports at them (see
 * vite.config.ts). A page downloads them only when it prints its first PDF, and
 * a site's bundler copies them with the rest of the package. The script build
 * loads the same two files another way (see pdfjs.script.ts).
 */
export function importPdfJs(): Promise<[unknown, unknown]> {
  return Promise.all([import("pdfjs-dist/build/pdf.min.mjs"), import("pdfjs-dist/build/pdf.worker.min.mjs")]);
}
