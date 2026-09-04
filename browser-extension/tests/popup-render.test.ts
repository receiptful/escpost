// @vitest-environment happy-dom

import { expect, test } from "vitest";
import { Window } from "happy-dom";
import { renderPopup } from "../src/ui/dom";
import type { PopupView } from "../src/popup/model";

const testWindow = new Window();
Object.assign(globalThis, { window: testWindow, document: testWindow.document });

const view: PopupView = {
  origin: "https://shop.example",
  permission: { tone: "ok", label: "Allowed" },
  daemon: { tone: "ok", label: "escpost is running" },
  primaryAction: { kind: "revoke", label: "Remove access" },
  guidance: "Raw printing is ready for this page.",
  error: null,
};

test("renders the raw popup controls without printers or unrelated actions", () => {
  // Break caught: adding printer selection or unrelated product flow to the
  // first consent popup expands the privilege surface beyond site access.
  document.body.replaceChildren();
  const main = document.createElement("main");
  document.body.append(main);

  renderPopup(main, view, () => undefined);

  expect(main.textContent).toContain("https://shop.example");
  expect(main.textContent).toContain("escpost is running");
  expect(main.querySelector<HTMLButtonElement>("#permission-action")?.textContent).toBe("Remove access");
  expect(main.querySelectorAll("button")).toHaveLength(1);
  expect(main.textContent).not.toMatch(/printer|settings|account|quota/i);
});

test("uses DOM text nodes for page-derived origin and error content", () => {
  // Break caught: assigning page-derived strings through HTML parses injected
  // markup into executable or misleading popup elements.
  document.body.replaceChildren();
  const main = document.createElement("main");
  document.body.append(main);
  const hostile: PopupView = {
    ...view,
    origin: '<img src=x onerror="window.__pwned = true">',
    error: '<button id="injected">not a real control</button>',
  };

  renderPopup(main, hostile, () => undefined);

  expect(main.querySelector("img")).toBeNull();
  expect(main.querySelector("#injected")).toBeNull();
  expect(main.querySelector("#current-origin")?.textContent).toBe(hostile.origin);
  expect(main.querySelector("#popup-error")?.textContent).toBe(hostile.error);
});
