import { EscpostError } from "../errors";
import { base64ToBytes, bytesToBase64, type QzPrintElement } from "./jobs";
import { horizontalStretch, qzImageOptions, qzImageToEscpos } from "./escpos-image";
import { LABEL_DPI, labelImageOptions, printerLanguage, rasterToLabel, type LabelLanguage } from "./label-image";
import { importPdfJs } from "./pdfjs";
import type { PrinterProfile } from "../types";

/**
 * Turns the QZ jobs that need rendering (images, HTML, PDF, and the `file`
 * flavor) into plain raw ESC/POS commands, so `jobToBytes` can send them.
 *
 * QZ Tray renders these in its Java app: in raw mode it converts the picture
 * to printer commands, and in pixel mode it hands it to the OS printer driver.
 * ESCPost has no driver, so both modes become an ESC/POS raster image here,
 * in the browser. That only suits ESC/POS printers, which is ESCPost's target.
 */

/**
 * The printer's real geometry, from its ESCPost profile. It decides how wide a
 * job is drawn and at what resolution. Without one, ESCPost's documented
 * default applies: a 58 mm roll at 203 dpi (docs/PROFILE_SCHEMA.md), which
 * wraps early rather than printing past a narrower head than assumed.
 */
export type PrinterGeometry = Pick<PrinterProfile, "printableWidthDots" | "dpiX" | "dpiY">;

/** Printable width of a 58 mm roll at 203 dpi (48 mm): the default without a profile or a paper size. */
const NARROW_WIDTH_DOTS = 384;
/** Printable width of an 80 mm roll at 203 dpi (72 mm). */
const WIDE_WIDTH_DOTS = 576;
/** The resolution assumed for a printer without a profile, the usual thermal head. */
const DEFAULT_DPI = 203;
/** CSS defines 96 px per inch, so a pixel job's HTML keeps its real-world size. */
const CSS_DPI = 96;
/** PDF sizes are in points, 72 per inch. */
const PDF_DPI = 72;
/** QZ Tray's density for ESC/POS when the config sets none: raw HTML is laid out for it. */
const QZ_ESCPOS_DENSITY = 180;
/** Rows per `GS v 0` command; small bands keep cheap printers' buffers safe. */
const BAND_HEIGHT = 256;
/**
 * A thermal dot spreads a little on the paper ("dot gain"), so a dithered grey
 * prints darker than it should. Greys are lightened by this gamma before
 * dithering to make up for it; black and white are left exactly as they are.
 * 1 turns the correction off. Tuned by printing a grey ramp (see the PR).
 */
const DOT_GAIN_GAMMA = 0.8;
/**
 * The browser draws text anti-aliased: at receipt resolution a letter is mostly
 * grey edge pixels, and dithering those greys scatters the letter into sparse
 * dots. A printer driver draws text solid instead. So a pixel on a sharp edge,
 * where its 3×3 neighbourhood spans at least `EDGE_CONTRAST` grey levels, is
 * printed black below `EDGE_BLACK_BELOW` and white above, with no dithering.
 * Flat areas, such as photos and shaded logos, are still dithered. Tuned on
 * QZ Tray's sample PDF at 512 dots: 200 keeps thin Courier strokes whole,
 * 220 starts to fill the counters of small serif text.
 */
const EDGE_CONTRAST = 64;
const EDGE_BLACK_BELOW = 200;

const ESC_INIT = [0x1b, 0x40];
const FEED_AND_CUT = [0x0a, 0x0a, 0x0a, 0x1d, 0x56, 0x41, 0x00];

type Options = Record<string, unknown>;

/**
 * Replace every element that needs rendering by a raw base64 element; leave the
 * rest alone. `geometry` is only asked for when something is drawn, so a job
 * of raw commands costs no extra round trip to ESCPost.
 */
export async function renderElements(
  elements: unknown[],
  configOptions: Options,
  geometry: () => Promise<PrinterGeometry | undefined> = async () => undefined,
): Promise<unknown[]> {
  return Promise.all(elements.map((element) => renderElement(element, configOptions, geometry)));
}

async function renderElement(
  element: unknown,
  configOptions: Options,
  geometry: () => Promise<PrinterGeometry | undefined>,
): Promise<unknown> {
  if (element === null || typeof element !== "object") return element;
  const job = element as QzPrintElement;
  const type = String(job.type ?? "raw").toLowerCase();
  const format = String(job.format ?? "command").toLowerCase();
  const flavor = String(job.flavor ?? "").toLowerCase();

  if (type === "raw" && format === "command") {
    // QZ Tray downloads a `file` flavor itself; we download it in the page.
    if (flavor !== "file") return element;
    return rawElement(await fetchBytes(job.data));
  }

  if (format !== "image" && format !== "html" && format !== "pdf") return element;
  if (type !== "raw" && type !== "pixel") return element;

  const options = (job.options ?? {}) as Options;
  // Pixel jobs have no printer language; a raw one is drawn for the language it names.
  const language = type === "raw" ? printerLanguage(options["language"]) : "escpos";
  if (language !== "escpos") return rawElement(await renderLabel(job, format, language, options, configOptions));
  const profile = await geometry();
  const scaleContent = configOptions["scaleContent"] !== false;
  // A raw job that sets QZ's image options gets QZ's own encoding (escpos-image.ts).
  const qzImage = type === "raw" ? qzImageOptions(options) : undefined;
  // `ESC *` single density prints every column two dots wide, so the picture
  // is drawn half as wide: once stretched, it still fits the printable width.
  const stretch = qzImage === undefined ? 1 : horizontalStretch(qzImage);
  const width = Math.floor(targetWidth(type, options, configOptions, profile) / stretch);
  const drawn =
    format === "image"
      ? [await imageToCanvas(job, width, type === "pixel" && scaleContent, qzImage !== undefined)]
      : format === "html"
        ? [await htmlToCanvas(job, width, type === "pixel" ? pixelDotsPerCssPixel(profile) : rawDotsPerPixel(configOptions), scaleContent)]
        : await pdfToCanvases(job, width, type === "raw" || scaleContent, profile);
  // A pixel job keeps its real proportions on a head whose dots are not square.
  // A raw job is one image pixel per dot, as in QZ Tray.
  const canvases = type === "pixel" ? drawn.map((canvas) => toPrinterRows(canvas, profile)) : drawn;

  const commands = canvases.flatMap((canvas) => {
    const { data } = canvas.getContext("2d", { willReadFrequently: true })!.getImageData(0, 0, canvas.width, canvas.height);
    const bytes =
      qzImage === undefined
        ? rasterToEscpos(data, canvas.width, canvas.height)
        : qzImageToEscpos(data, canvas.width, canvas.height, qzImage, ditherToBlack);
    return Array.from(bytes);
  });
  // A pixel job is a whole document, which a driver would start and cut.
  const bytes = type === "pixel" ? [...ESC_INIT, ...commands, ...FEED_AND_CUT] : commands;
  return rawElement(Uint8Array.from(bytes));
}

/** A label is 4 inches wide at 203 dpi when a raw HTML job gives no `pageWidth`. */
const DEFAULT_LABEL_WIDTH_DOTS = 4 * LABEL_DPI;

/**
 * A raw image, HTML or PDF job for a label printer (ZPL, EPL, CPCL or SBPL),
 * drawn as QZ Tray draws it: one pixel per dot, black or white by QZ's
 * `quantization` and `threshold`, at 203 dpi. ESCPost's printer profiles
 * describe receipt printers, so the size comes from the job: a `pageWidth` in
 * dots if it gives one; otherwise an image keeps its own size, a PDF its real
 * size, and HTML is laid out on a 4 inch label.
 */
async function renderLabel(
  job: QzPrintElement,
  format: string,
  language: LabelLanguage,
  options: Options,
  configOptions: Options,
): Promise<Uint8Array> {
  const pageWidth = positive(options["pageWidth"]);
  const drawn =
    format === "image"
      ? [await imageToCanvas(job, pageWidth ?? Number.POSITIVE_INFINITY, false, true)]
      : format === "html"
        ? [await htmlToCanvas(job, pageWidth ?? DEFAULT_LABEL_WIDTH_DOTS, rawDotsPerPixel(configOptions, LABEL_DPI), configOptions["scaleContent"] !== false)]
        : await pdfToCanvases(job, pageWidth ?? Number.POSITIVE_INFINITY, pageWidth !== undefined, {
            printableWidthDots: Number.POSITIVE_INFINITY,
            dpiX: LABEL_DPI,
            dpiY: LABEL_DPI,
          });
  const imageOptions = labelImageOptions(options);
  const commands = drawn.flatMap((canvas) => {
    const { data } = canvas.getContext("2d", { willReadFrequently: true })!.getImageData(0, 0, canvas.width, canvas.height);
    return Array.from(rasterToLabel(data, canvas.width, canvas.height, language, imageOptions, ditherToBlack));
  });
  return Uint8Array.from(commands);
}

function rawElement(bytes: Uint8Array): QzPrintElement {
  return { type: "raw", format: "command", flavor: "base64", data: bytesToBase64(bytes) };
}

/**
 * The width to draw at, never wider than the printable width (see
 * `printableWidth`). Raw jobs may give `pageWidth` in dots; pixel jobs may give
 * it in the config's units, converted at the printer's resolution. Without
 * one, the job fills the printable width.
 */
export function targetWidth(type: string, options: Options, configOptions: Options, profile?: PrinterGeometry): number {
  const printable = printableWidth(configOptions, profile);
  const pageWidth = positive(options["pageWidth"]);
  if (pageWidth === undefined) return printable;
  const dpi = profile?.dpiX ?? DEFAULT_DPI;
  const dots = type === "raw" ? pageWidth : Math.round(toMillimetres(pageWidth, configOptions) * (dpi / 25.4));
  return Math.min(printable, dots);
}

/**
 * The width the print head can actually print, in dots. The printer's profile
 * knows it, and it is not the paper width times the resolution: heads leave
 * margins, so 80 mm paper prints 512 dots on a 180 dpi TM-T88III and 576 on a
 * 203 dpi head.
 *
 * Without a profile, a page that says its paper size (QZ's `size`) gets the
 * usual width at 203 dpi: 48 mm (384 dots) for a 58 mm roll, 72 mm (576 dots)
 * for an 80 mm one, and 8 mm less for any other paper. With neither, the
 * narrow roll is assumed, so nothing prints past a narrower head than assumed.
 */
export function printableWidth(configOptions: Options, profile?: PrinterGeometry): number {
  if (profile !== undefined) return profile.printableWidthDots;

  const size = configOptions["size"] as { width?: unknown } | null | undefined;
  const paper = positive(size?.width);
  if (paper === undefined) return NARROW_WIDTH_DOTS;

  const millimetres = toMillimetres(paper, configOptions);
  if (millimetres <= 60) return NARROW_WIDTH_DOTS;
  if (millimetres <= 82) return WIDE_WIDTH_DOTS;
  return Math.round(((millimetres - 8) * DEFAULT_DPI) / 25.4);
}

/**
 * Dots per CSS pixel for pixel HTML: CSS has 96 px per inch, so a document
 * keeps its real size at the printer's resolution (203 / 96 on a 203 dpi head).
 */
export function pixelDotsPerCssPixel(profile?: PrinterGeometry): number {
  return (profile?.dpiX ?? DEFAULT_DPI) / CSS_DPI;
}

/**
 * How many dots wide a picture of `sourceWidth` pixels is drawn. `fill` is
 * QZ's `scaleContent` for a pixel image: QZ Tray stretches it, up or down, to
 * the page width. Otherwise one image pixel is one dot, as QZ Tray prints at the
 * printer's density, and only an image wider than the paper is shrunk to fit.
 */
export function imageDrawWidth(width: number, sourceWidth: number, fill: boolean): number {
  return fill ? width : Math.min(width, sourceWidth);
}

/**
 * The scale from a PDF page, in points, to dots. `fit` (QZ's `scaleContent`,
 * and every raw PDF) fills the width. Otherwise the page keeps its real size,
 * 72 points per inch at the printer's resolution, shrunk only if wider than
 * the paper.
 */
export function pdfScale(width: number, pageWidth: number, fit: boolean, profile?: PrinterGeometry): number {
  if (fit) return width / pageWidth;
  return Math.min(width / pageWidth, (profile?.dpiX ?? DEFAULT_DPI) / PDF_DPI);
}

/**
 * Rows to print for `rows` drawn at the horizontal resolution: a head whose
 * vertical resolution differs prints `dpiY / dpiX` rows per row, so a circle
 * stays round.
 */
export function printerRows(rows: number, profile?: PrinterGeometry): number {
  if (profile === undefined || profile.dpiX === profile.dpiY) return rows;
  return Math.max(1, Math.round((rows * profile.dpiY) / profile.dpiX));
}

/** The canvas resampled to `printerRows` rows; the same canvas when no change is needed. */
function toPrinterRows(canvas: HTMLCanvasElement, profile?: PrinterGeometry): HTMLCanvasElement {
  const rows = printerRows(canvas.height, profile);
  if (rows === canvas.height) return canvas;
  const { canvas: resampled, context } = whiteCanvas(canvas.width, rows);
  context.imageSmoothingQuality = "high";
  context.drawImage(canvas, 0, 0, canvas.width, rows);
  return resampled;
}

function toMillimetres(value: number, configOptions: Options): number {
  const units = String(configOptions["units"] ?? "in").toLowerCase();
  return units === "mm" ? value : units === "cm" ? value * 10 : value * 25.4;
}

/**
 * Dots per CSS pixel for raw HTML. QZ Tray lays it out at 96 CSS px per inch of
 * the config's density, or of 180 dpi for ESC/POS when there is none, and
 * prints that at one dot per density pixel: 180 / 96 = 1.875 dots per CSS px.
 * A label printer language uses 203 dpi instead, QZ Tray's density for it.
 */
function rawDotsPerPixel(configOptions: Options, fallbackDpi = QZ_ESCPOS_DENSITY): number {
  const density = Number(configOptions["density"]);
  const units = String(configOptions["units"] ?? "in").toLowerCase();
  const perInch = units === "mm" ? 25.4 : units === "cm" ? 2.54 : 1;
  const dpi = Number.isFinite(density) && density * perInch > 1 ? density * perInch : fallbackDpi;
  return dpi / CSS_DPI;
}

function positive(value: unknown): number | undefined {
  const number = Number(value);
  return Number.isFinite(number) && number > 0 ? number : undefined;
}

/** The job's content as bytes: base64 data, or a URL for the `file` and `plain` flavors. */
async function contentBytes(job: QzPrintElement): Promise<Uint8Array> {
  if (typeof job.data !== "string") {
    throw new EscpostError("UNSUPPORTED_FORMAT", "A print element must carry its data as a string.");
  }
  const flavor = String(job.flavor ?? "file").toLowerCase();
  if (flavor === "base64") return base64ToBytes(job.data);
  return fetchBytes(job.data);
}

async function fetchBytes(url: unknown): Promise<Uint8Array> {
  if (typeof url !== "string") {
    throw new EscpostError("UNSUPPORTED_FORMAT", "The file flavor needs a URL string.");
  }
  let response: Response;
  try {
    response = await fetch(url);
  } catch {
    throw new EscpostError("PRINT_FAILED", `Could not download "${url}". A file on another site needs CORS.`);
  }
  if (!response.ok) throw new EscpostError("PRINT_FAILED", `Could not download "${url}" (HTTP ${response.status}).`);
  return new Uint8Array(await response.arrayBuffer());
}

function whiteCanvas(
  width: number,
  height: number,
  transparent = false,
): { canvas: HTMLCanvasElement; context: CanvasRenderingContext2D } {
  const canvas = document.createElement("canvas");
  canvas.width = width;
  canvas.height = Math.max(1, height);
  const context = canvas.getContext("2d", { willReadFrequently: true });
  if (context === null) throw new EscpostError("PRINT_FAILED", "This browser cannot draw on a canvas.");
  // Transparent pixels must print as paper, not ink. QZ's own image rules read
  // the transparency themselves, so they get the pixels as they are.
  if (!transparent) {
    context.fillStyle = "#fff";
    context.fillRect(0, 0, canvas.width, canvas.height);
  }
  return { canvas, context };
}

/** Draw the image keeping its proportions, `imageDrawWidth` dots wide. */
async function imageToCanvas(job: QzPrintElement, width: number, fill: boolean, transparent: boolean): Promise<HTMLCanvasElement> {
  const bytes = await contentBytes(job);
  let bitmap: ImageBitmap;
  try {
    bitmap = await createImageBitmap(new Blob([bytes as BlobPart]));
  } catch {
    throw new EscpostError("UNSUPPORTED_FORMAT", "The image could not be decoded (PNG, JPEG, GIF, WebP or BMP expected).");
  }
  const drawWidth = imageDrawWidth(width, bitmap.width, fill);
  const height = Math.round((bitmap.height * drawWidth) / bitmap.width);
  const { canvas, context } = whiteCanvas(drawWidth, height, transparent);
  // Resized images keep thin lines and text edges instead of dropping pixels.
  context.imageSmoothingQuality = "high";
  context.drawImage(bitmap, 0, 0, drawWidth, height);
  bitmap.close();
  return canvas;
}

/**
 * Render HTML through an SVG `foreignObject`, the browser's own layout engine.
 *
 * `dotsPerPixel` sets the size. A pixel job is a document, so its CSS sizes
 * are honoured: it is laid out at the paper width in CSS pixels (96 per inch),
 * then drawn enlarged to the printer's resolution, so 12 px text prints about
 * 3 mm tall, as with QZ Tray. A raw job uses QZ Tray's raw density instead
 * (see `rawDotsPerPixel`).
 *
 * The HTML is first laid out in a hidden sandboxed iframe (no scripts) at the
 * target width, to learn its height and to get it as XHTML, which SVG needs.
 * An SVG drawn as an image may not download anything, so `<img>` files, style
 * sheets and the fonts and images they use are embedded first (see
 * `inlineImages` and `inlineStyles`).
 *
 * With QZ's `scaleContent` option (on by default), content wider than the
 * paper, such as a large image, is shrunk to fit instead of being cut off.
 */
async function htmlToCanvas(
  job: QzPrintElement,
  width: number,
  dotsPerPixel: number,
  scaleContent: boolean,
): Promise<HTMLCanvasElement> {
  const flavor = String(job.flavor ?? "plain").toLowerCase();
  const html =
    flavor === "plain" && typeof job.data === "string"
      ? job.data
      : new TextDecoder().decode(await contentBytes(job));

  const layoutWidth = Math.round(width / dotsPerPixel);
  const frame = document.createElement("iframe");
  frame.setAttribute("sandbox", "allow-same-origin");
  frame.style.cssText = `position:fixed; left:-10000px; top:0; width:${layoutWidth}px; height:10px; border:0; visibility:hidden;`;
  frame.srcdoc = html;
  const loaded = new Promise<void>((resolve) => frame.addEventListener("load", () => resolve(), { once: true }));
  document.body.append(frame);

  try {
    await loaded;
    const frameDocument = frame.contentDocument;
    if (frameDocument === null) throw new EscpostError("PRINT_FAILED", "The HTML could not be laid out.");
    await Promise.all([inlineImages(frameDocument), inlineStyles(frameDocument)]);
    // A font changes the text's size, so measure only once the fonts are in.
    await frameDocument.fonts.ready;
    const root = frameDocument.documentElement;
    const contentWidth = scaleContent ? Math.max(layoutWidth, Math.ceil(root.scrollWidth)) : layoutWidth;
    const layoutHeight = Math.ceil(root.scrollHeight);
    const height = Math.round((layoutHeight * width) / contentWidth);
    const xhtml = new XMLSerializer().serializeToString(root);
    // The viewBox scales the whole content to the paper width as vectors, so
    // text stays sharp at the printer's resolution and nothing wider than the paper is cut off.
    const svg =
      `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" ` +
      `viewBox="0 0 ${contentWidth} ${layoutHeight}">` +
      `<foreignObject width="${contentWidth}" height="${layoutHeight}">${xhtml}</foreignObject></svg>`;

    const image = new Image();
    image.src = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
    await image.decode();

    const { canvas, context } = whiteCanvas(width, height);
    context.drawImage(image, 0, 0, width, height);
    return canvas;
  } finally {
    frame.remove();
  }
}

/**
 * Replace each `<img>` source by a `data:` URL holding the image itself, so the
 * SVG needs no download. The page downloads it instead, which it may do for its
 * own files, and for other sites' files when they allow it (CORS). An image
 * that cannot be downloaded stays blank, like a broken image, and the rest of
 * the receipt still prints.
 */
async function inlineImages(frameDocument: Document): Promise<void> {
  await Promise.all(
    Array.from(frameDocument.images).map(async (image) => {
      const source = image.getAttribute("src");
      if (!source || source.startsWith("data:")) return;
      const dataUrl = await fetchDataUrl(new URL(source, frameDocument.baseURI));
      if (dataUrl === undefined) return;
      image.removeAttribute("srcset");
      image.setAttribute("src", dataUrl);
      await image.decode().catch(() => {});
    }),
  );
}

/**
 * The same for CSS: each `<link rel="stylesheet">` becomes a `<style>` holding
 * the file's text, and the files the CSS points to (fonts, background images,
 * `@import`ed sheets) become `data:` URLs. Same rule as images: a file the page
 * may not download is skipped, and the receipt prints without it.
 */
async function inlineStyles(frameDocument: Document): Promise<void> {
  const base = frameDocument.baseURI;
  const links = Array.from(frameDocument.querySelectorAll<HTMLLinkElement>('link[rel~="stylesheet" i][href]'));
  const styles = Array.from(frameDocument.querySelectorAll("style"));
  const styled = Array.from(frameDocument.querySelectorAll<HTMLElement>('[style*="url("]'));

  await Promise.all([
    ...links.map(async (link) => {
      const css = await fetchCss(new URL(link.getAttribute("href")!, base), 0);
      if (css === undefined) return;
      const style = frameDocument.createElement("style");
      const media = link.getAttribute("media");
      if (media) style.setAttribute("media", media);
      style.textContent = css;
      link.replaceWith(style);
    }),
    ...styles.map(async (style) => {
      style.textContent = await inlineCssUrls(style.textContent ?? "", base);
    }),
    ...styled.map(async (element) => {
      element.setAttribute("style", await inlineCssUrls(element.getAttribute("style")!, base));
    }),
  ]);
}

const CSS_IMPORT = /@import\s+(?:url\(\s*)?(["']?)([^"')\s;]+)\1\s*\)?[^;]*;/g;
const CSS_URL = /url\(\s*(["']?)([^"')]+)\1\s*\)/g;
/** How deep `@import` chains are followed: enough for real sheets, safe from loops. */
const MAX_IMPORT_DEPTH = 3;

async function fetchCss(url: URL, depth: number): Promise<string | undefined> {
  try {
    const response = await fetch(url);
    if (!response.ok) return undefined;
    // The sheet's own urls are relative to the sheet, not to the page.
    return await inlineCssUrls(await response.text(), url, depth);
  } catch {
    return undefined;
  }
}

/** Replace a sheet's `@import`s and `url()`s by the files themselves. */
export async function inlineCssUrls(css: string, base: string | URL, depth = 0): Promise<string> {
  const imported =
    depth < MAX_IMPORT_DEPTH
      ? await replaceAsync(css, CSS_IMPORT, async (rule, _quote, href) => (await fetchCss(new URL(href!, base), depth + 1)) ?? rule)
      : css;
  return replaceAsync(imported, CSS_URL, async (match, _quote, href) => {
    // data: is already inline, and "#id" points into the page (SVG filters).
    if (href!.startsWith("data:") || href!.startsWith("#")) return match;
    const dataUrl = await fetchDataUrl(new URL(href!, base));
    return dataUrl === undefined ? match : `url("${dataUrl}")`;
  });
}

/** `String.replace` with an async replacer: the downloads run in parallel. */
async function replaceAsync(
  text: string,
  pattern: RegExp,
  replacer: (match: string, ...groups: Array<string | undefined>) => Promise<string>,
): Promise<string> {
  const matches = Array.from(text.matchAll(pattern));
  const replacements = await Promise.all(matches.map((match) => replacer(match[0], ...match.slice(1))));
  let index = 0;
  return text.replace(pattern, () => replacements[index++]!);
}

/** A file as a `data:` URL, or undefined when the page may not download it (often CORS). */
async function fetchDataUrl(url: URL): Promise<string | undefined> {
  try {
    const response = await fetch(url);
    if (!response.ok) return undefined;
    return await blobToDataUrl(await response.blob());
  } catch {
    return undefined;
  }
}

function blobToDataUrl(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result));
    reader.onerror = () => reject(reader.error);
    reader.readAsDataURL(blob);
  });
}

type PdfJs = {
  getDocument(source: { data: Uint8Array }): {
    promise: Promise<{
      numPages: number;
      getPage(number: number): Promise<{
        getViewport(options: { scale: number }): { width: number; height: number };
        render(options: { canvasContext: CanvasRenderingContext2D; viewport: unknown }): { promise: Promise<void> };
      }>;
    }>;
  };
};

let pdfJs: Promise<PdfJs> | undefined;

/** pdf.js is about 1.7 MB, so it is loaded only for the first PDF (see pdfjs.ts). */
function loadPdfJs(): Promise<PdfJs> {
  if (pdfJs === undefined) {
    pdfJs = (async () => {
      const [library, worker] = await importPdfJs();
      // pdf.js runs its worker code in the page when it finds it here, so it
      // never has to locate pdf.worker.min.js by URL, which bundlers move around.
      (globalThis as { pdfjsWorker?: unknown }).pdfjsWorker = worker;
      return library as PdfJs;
    })();
    // A failed download (missing file) must not block every later try.
    pdfJs.catch(() => {
      pdfJs = undefined;
    });
  }
  return pdfJs;
}

/**
 * One canvas per page. `fit` scales each page to the target width, as QZ's
 * `scaleContent` and raw `pageWidth` do. Without it a page keeps its real size
 * (72 points per inch at the printer's resolution, see `pdfScale`), shrunk only if wider than the paper.
 */
async function pdfToCanvases(
  job: QzPrintElement,
  width: number,
  fit: boolean,
  profile?: PrinterGeometry,
): Promise<HTMLCanvasElement[]> {
  const bytes = await contentBytes(job);
  let pdf: Awaited<ReturnType<PdfJs["getDocument"]>["promise"]>;
  try {
    pdf = await (await loadPdfJs()).getDocument({ data: bytes }).promise;
  } catch {
    throw new EscpostError("PRINT_FAILED", "The PDF could not be opened, or pdf.js could not be loaded.");
  }

  const canvases: HTMLCanvasElement[] = [];
  for (let number = 1; number <= pdf.numPages; number++) {
    const page = await pdf.getPage(number);
    const pageWidth = page.getViewport({ scale: 1 }).width;
    const scale = pdfScale(width, pageWidth, fit, profile);
    const viewport = page.getViewport({ scale });
    const { canvas, context } = whiteCanvas(Math.round(viewport.width), Math.round(viewport.height));
    await page.render({ canvasContext: context, viewport }).promise;
    canvases.push(canvas);
  }
  return canvases;
}

/**
 * RGBA pixels to ESC/POS `GS v 0` raster commands, one command per band.
 *
 * A thermal printer only prints black dots, so greys are dithered: each
 * pixel's rounding error is passed on to its neighbours (Floyd-Steinberg), and
 * a grey area becomes a pattern of dots instead of turning all white or all
 * black. This keeps grey logos and the soft edges of thin text visible.
 * Each row is packed 8 pixels per byte, most significant bit first.
 */
export function rasterToEscpos(rgba: ArrayLike<number>, width: number, height: number): Uint8Array {
  const black = ditherToBlack(rgba, width, height);
  const bytesPerRow = Math.ceil(width / 8);
  const out: number[] = [];

  for (let top = 0; top < height; top += BAND_HEIGHT) {
    const rows = Math.min(BAND_HEIGHT, height - top);
    out.push(0x1d, 0x76, 0x30, 0x00, bytesPerRow & 0xff, bytesPerRow >> 8, rows & 0xff, rows >> 8);
    for (let y = top; y < top + rows; y++) {
      for (let column = 0; column < bytesPerRow; column++) {
        let byte = 0;
        for (let bit = 0; bit < 8; bit++) {
          const x = column * 8 + bit;
          if (x < width && black[y * width + x] === 1) byte |= 0x80 >> bit;
        }
        out.push(byte);
      }
    }
  }
  return Uint8Array.from(out);
}

/**
 * One entry per pixel, 1 for a black dot. Pixels on a sharp edge, such as text,
 * are printed solid (see `EDGE_CONTRAST`); the others use Floyd-Steinberg error
 * diffusion on greys lightened for dot gain (see `DOT_GAIN_GAMMA`).
 */
function ditherToBlack(rgba: ArrayLike<number>, width: number, height: number): Uint8Array {
  const luma = new Float32Array(width * height);
  for (let index = 0; index < luma.length; index++) {
    const offset = index * 4;
    luma[index] = (299 * rgba[offset]! + 587 * rgba[offset + 1]! + 114 * rgba[offset + 2]!) / 1000;
  }

  const grey = new Float32Array(width * height);
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const index = y * width + x;
      const value = luma[index]!;
      grey[index] = onSharpEdge(luma, width, height, x, y)
        ? value < EDGE_BLACK_BELOW ? 0 : 255
        : 255 * (value / 255) ** DOT_GAIN_GAMMA;
    }
  }

  const black = new Uint8Array(width * height);
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const index = y * width + x;
      const value = grey[index]!;
      const isBlack = value < 128;
      black[index] = isBlack ? 1 : 0;
      const error = value - (isBlack ? 0 : 255);
      if (x + 1 < width) grey[index + 1]! += (error * 7) / 16;
      if (y + 1 < height) {
        if (x > 0) grey[index + width - 1]! += (error * 3) / 16;
        grey[index + width]! += (error * 5) / 16;
        if (x + 1 < width) grey[index + width + 1]! += error / 16;
      }
    }
  }
  return black;
}

/** Whether the pixel's 3×3 neighbourhood spans at least `EDGE_CONTRAST` grey levels. */
function onSharpEdge(luma: Float32Array, width: number, height: number, x: number, y: number): boolean {
  let lightest = 0;
  let darkest = 255;
  for (let row = Math.max(0, y - 1); row <= Math.min(height - 1, y + 1); row++) {
    for (let column = Math.max(0, x - 1); column <= Math.min(width - 1, x + 1); column++) {
      const value = luma[row * width + column]!;
      if (value > lightest) lightest = value;
      if (value < darkest) darkest = value;
    }
  }
  return lightest - darkest >= EDGE_CONTRAST;
}
