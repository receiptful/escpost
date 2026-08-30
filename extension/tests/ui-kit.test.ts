// @vitest-environment happy-dom
import { describe, expect, it } from "vitest";
import { TONES, pill } from "../src/ui/status";
import { formatDay } from "../src/ui/format";
import { el, renderPill } from "../src/ui/dom";

describe("pill", () => {
  it("carries the label through untouched", () => {
    expect(pill("Connected", "ok")).toMatchObject({ label: "Connected", tone: "ok" });
  });

  it("gives every tone its own glyph, so status is never carried by colour alone (U6)", () => {
    const glyphs = TONES.map((tone) => pill("x", tone).glyph);
    expect(glyphs.every((glyph) => glyph.length > 0)).toBe(true);
    expect(new Set(glyphs).size).toBe(TONES.length);
  });
});

describe("formatDay", () => {
  it("formats an epoch millisecond as a day and a month", () => {
    expect(formatDay(Date.UTC(2026, 8, 1))).toBe("1 September");
  });

  it("formats the same instant the same way on any machine", () => {
    expect(formatDay(Date.UTC(2026, 0, 1, 23, 30))).toBe("1 January");
  });
});

describe("el", () => {
  it("creates a classed, texted element in one call", () => {
    const node = el("span", "pop-mark", "escpost");
    expect(node.tagName).toBe("SPAN");
    expect(node.className).toBe("pop-mark");
    expect(node.textContent).toBe("escpost");
  });
});

describe("renderPill", () => {
  it("puts the glyph in the DOM as text, not only in a colour (U6)", () => {
    const node = renderPill(pill("Not running", "warn"));
    const mark = node.querySelector(".pill-mark");
    expect(mark?.textContent).not.toBe("");
    expect(mark?.getAttribute("aria-hidden")).toBe("true");
    expect(node.textContent).toContain("Not running");
    expect(node.className).toBe("pill warn");
  });
});
