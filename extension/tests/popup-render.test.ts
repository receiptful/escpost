// @vitest-environment happy-dom
import { beforeEach, describe, expect, it } from "vitest";
import { describePopup } from "../src/popup/state";
import { renderPopup } from "../src/popup/popup";
import { ALL_KINDS, INPUTS } from "./fixtures/popup-fixtures";

let root: HTMLElement;

beforeEach(() => {
  document.body.innerHTML = "";
  root = document.createElement("main");
  document.body.append(root);
});

function render(kind: (typeof ALL_KINDS)[number]): HTMLElement {
  renderPopup(root, describePopup(INPUTS[kind]));
  return root;
}

describe("renderPopup", () => {
  it("renders every one of the five states and labels the root with which it is (T9)", () => {
    for (const kind of ALL_KINDS) {
      render(kind);
      expect(root.dataset["state"], kind).toBe(kind);
      expect(root.querySelectorAll(".pop")).toHaveLength(1);
    }
  });

  it("gives the status pill a glyph as well as a colour, in every state (U6)", () => {
    for (const kind of ALL_KINDS) {
      render(kind);
      const mark = root.querySelector(".pop-top .pill .pill-mark");
      expect(mark?.textContent ?? "", kind).not.toBe("");
    }
  });

  it("shows the install command in the first-run state", () => {
    render("no-daemon");
    expect(root.querySelector(".pop-cmd")?.textContent).toContain("brew install escpost");
    expect(root.querySelector('[data-action="check-again"]')).not.toBeNull();
  });

  it("offers the paid plan in the exhausted state and in no other (U3)", () => {
    for (const kind of ALL_KINDS) {
      render(kind);
      const offered = root.querySelector('[data-action="open-plans"]') !== null;
      expect(offered, kind).toBe(kind === "exhausted");
    }
  });

  it("renders the meter with the tone class its state asked for", () => {
    render("signed-in");
    expect(root.querySelector(".meter")?.className).toBe("meter ok");
    render("exhausted");
    expect(root.querySelector(".meter")?.className).toBe("meter out");
  });

  it("emphasises the part of a strip that says what still works", () => {
    render("offline");
    const strip = root.querySelector(".strip");
    expect(strip?.textContent).toContain("Raw ESC/POS printing is unaffected");
    expect(strip?.querySelector("strong")?.textContent).toContain("Raw ESC/POS printing is unaffected");
  });

  it("replaces the previous render instead of appending to it", () => {
    render("signed-in");
    render("no-daemon");
    expect(root.querySelectorAll(".pop")).toHaveLength(1);
    expect(root.textContent).not.toContain("sam@bluebirdcafe.co");
  });

  it("puts every footer item behind a real button, so it is reachable by keyboard", () => {
    render("signed-in");
    const footer = root.querySelectorAll(".pop-foot button");
    expect(footer).toHaveLength(2);
    expect(footer[0]?.getAttribute("data-action")).toBe("open-settings");
  });
});

describe("renderPopup — the grant control", () => {
  const site = { origin: "pos.thornbury.app", pattern: "https://pos.thornbury.app/*", denied: false };

  it("renders the grant as a real button carrying the match pattern", () => {
    renderPopup(root, describePopup({ ...INPUTS["signed-out"], pendingSite: site }));
    const button = root.querySelector<HTMLElement>('[data-action="grant-site"]');
    expect(button?.tagName).toBe("BUTTON");
    // The handler must read the pattern off the element: it cannot look it up
    // asynchronously without losing the user gesture.
    expect(button?.getAttribute("data-value")).toBe("https://pos.thornbury.app/*");
  });

  it("names the site in the text, so nobody grants a site they cannot see", () => {
    renderPopup(root, describePopup({ ...INPUTS["signed-out"], pendingSite: site }));
    expect(root.textContent).toContain("pos.thornbury.app");
  });

  it("renders no grant control when there is nothing to grant", () => {
    renderPopup(root, describePopup(INPUTS["signed-out"]));
    expect(root.querySelector('[data-action="grant-site"]')).toBeNull();
  });
});
