import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

/**
 * Caught in a real browser, not by these unit tests: the welcome tab hardcoded
 * its own palette, so in dark mode it rendered as a white page while the popup
 * and the settings page both went dark. happy-dom does not evaluate
 * prefers-color-scheme, so no DOM test could see it. What is checkable, and what
 * actually went wrong, is the page not using the shared token sheet at all.
 */
const PAGES = [
  { name: "popup", html: "extension/src/popup/popup.html", css: "extension/src/popup/popup.css" },
  { name: "settings", html: "extension/src/settings/settings.html", css: "extension/src/settings/settings.css" },
  { name: "welcome", html: "extension/src/welcome/welcome.html", css: null },
];

/** Pure white and pure black are legitimate on an accent fill; anything else is a
 *  palette decision that belongs in theme.css so it can change with the scheme. */
const ALLOWED = new Set(["#fff", "#ffffff", "#000", "#000000"]);

function hexColours(source: string): string[] {
  return (source.match(/#[0-9a-fA-F]{3,8}\b/g) ?? []).filter((hex) => !ALLOWED.has(hex.toLowerCase()));
}

describe("every extension page uses the shared theme tokens", () => {
  it.each(PAGES)("$name links theme.css", ({ html }) => {
    expect(readFileSync(html, "utf8")).toContain('href="theme.css"');
  });

  it.each(PAGES)("$name hardcodes no palette of its own", ({ html, css }) => {
    expect(hexColours(readFileSync(html, "utf8"))).toEqual([]);
    if (css !== null) expect(hexColours(readFileSync(css, "utf8"))).toEqual([]);
  });

  it("declares color-scheme, so native controls follow the theme too", () => {
    // Without this the settings <select> and the welcome <input> keep a light
    // chrome on a dark page.
    expect(readFileSync("extension/src/ui/theme.css", "utf8")).toMatch(/color-scheme:\s*light dark/);
  });

  it("paints the body from a token rather than leaving the canvas transparent", () => {
    // A transparent body is why the welcome tab stayed white in dark mode.
    expect(readFileSync("extension/src/ui/theme.css", "utf8")).toMatch(/body\s*\{[^}]*background:\s*var\(--ground\)/);
  });
});

describe("no font is bundled or fetched", () => {
  const theme = readFileSync("extension/src/ui/theme.css", "utf8");

  it("declares the typeface with local files", () => {
    // escpost.dev names Inter and ships no font file. Matching it costs the
    // extension nothing to download and nothing for a reviewer to vet.
    expect(theme).not.toMatch(/@font-face/);
    expect(theme).not.toMatch(/\.woff2?/);
  });

  it("fetches no remote resource from any page or stylesheet", () => {
    // A Web Store extension must not, and its CSP would block it. This is the
    // guard against someone "fixing" a missing font with a Google Fonts link.
    for (const file of [
      "extension/src/ui/theme.css",
      "extension/src/popup/popup.css",
      "extension/src/settings/settings.css",
      "extension/src/popup/popup.html",
      "extension/src/settings/settings.html",
      "extension/src/welcome/welcome.html",
    ]) {
      const source = readFileSync(file, "utf8");
      expect(source, file).not.toMatch(/https?:\/\/[^"')\s]*\.(css|woff2?|ttf|otf)/);
      expect(source, file).not.toMatch(/fonts\.(googleapis|gstatic)\.com/);
      expect(source, file).not.toMatch(/@import\s+url\(/);
    }
  });

  it("sets the whole UI in the one typeface, per the design system", () => {
    // "The whole site reads as printed output" -- one face for display and body.
    expect(theme).toMatch(/--sans:\s*Inter/);
    expect(theme).toMatch(/body\s*\{[^}]*font-family:\s*var\(--sans\)/);
  });

  it("gives the status glyphs a stack that actually has them", () => {
    // The mono stack cannot be relied on to carry the status glyphs.
    const glyph = /--glyph:([^;]+);/.exec(theme)?.[1] ?? "";
    expect(glyph).not.toContain("SFMono-Regular");
    expect(glyph).toContain("monospace");
  });
});
