import type { StatusPill } from "./status";

export function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className = "",
  text = "",
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (className !== "") node.className = className;
  if (text !== "") node.textContent = text;
  return node;
}

/** Glyph first, then the label, then colour: never colour alone. */
export function renderPill(status: StatusPill): HTMLSpanElement {
  const span = el("span", `pill ${status.tone}`);
  span.setAttribute("aria-label", `Status: ${status.label}`);
  const mark = el("span", "pill-mark", status.glyph);
  mark.setAttribute("aria-hidden", "true");
  span.append(mark, document.createTextNode(` ${status.label}`));
  return span;
}
