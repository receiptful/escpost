// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from "vitest";
import { inlineCssUrls } from "../src/qz/render";

/** A fake network: each URL maps to a file's text, anything else fails like a CORS refusal. */
function serve(files: Record<string, string>) {
  const requested: string[] = [];
  vi.stubGlobal("fetch", async (url: URL) => {
    requested.push(url.href);
    const body = files[url.href];
    if (body === undefined) throw new TypeError("Failed to fetch");
    return {
      ok: true,
      text: async () => body,
      blob: async () => new Blob([body], { type: "font/woff2" }),
    };
  });
  return requested;
}

const dataUrlOf = (text: string) => `data:font/woff2;base64,${btoa(text)}`;

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("inlineCssUrls", () => {
  it("embeds a font, resolving its url against the sheet, not the page", async () => {
    serve({ "https://shop.test/css/fonts/receipt.woff2": "FONT" });
    const css = await inlineCssUrls(
      "@font-face { font-family: Receipt; src: url('fonts/receipt.woff2') format('woff2'); }",
      new URL("https://shop.test/css/receipt.css"),
    );
    expect(css).toContain(`url("${dataUrlOf("FONT")}")`);
    expect(css).toContain("format('woff2')");
  });

  it("pastes an @import in place, with its own urls relative to the imported sheet", async () => {
    const requested = serve({
      "https://shop.test/css/base.css": "body { background: url(img/paper.png) }",
      "https://shop.test/css/img/paper.png": "PNG",
    });
    const css = await inlineCssUrls('@import url("css/base.css");\nh1 { margin: 0 }', "https://shop.test/");
    expect(css).not.toContain("@import");
    expect(css).toContain(`body { background: url("${dataUrlOf("PNG")}") }`);
    expect(css).toContain("h1 { margin: 0 }");
    expect(requested).toContain("https://shop.test/css/img/paper.png");
  });

  it("leaves a file it may not download as it was, so the receipt still prints", async () => {
    serve({});
    const css = "@font-face { src: url(https://other.test/font.woff2) }";
    await expect(inlineCssUrls(css, "https://shop.test/")).resolves.toBe(css);
  });

  it("does not download data: urls or references into the page", async () => {
    const requested = serve({});
    const css = "a { background: url(data:image/png;base64,AAAA); filter: url(#blur) }";
    await expect(inlineCssUrls(css, "https://shop.test/")).resolves.toBe(css);
    expect(requested).toHaveLength(0);
  });

  it("stops following @import chains after a few levels", async () => {
    const requested = serve({ "https://shop.test/loop.css": '@import "loop.css";' });
    await inlineCssUrls('@import "loop.css";', "https://shop.test/");
    expect(requested.length).toBeLessThanOrEqual(3);
  });
});
