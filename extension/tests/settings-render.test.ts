// @vitest-environment happy-dom
import { beforeEach, describe, expect, it } from "vitest";
import { describeSettings } from "../src/settings/model";
import { renderSettings } from "../src/settings/settings";
import { account } from "./fixtures/popup-fixtures";
import { settingsInput } from "./fixtures/settings-fixtures";

let root: HTMLElement;

beforeEach(() => {
  document.body.innerHTML = "";
  root = document.createElement("main");
  document.body.append(root);
});

function render(overrides = {}): HTMLElement {
  renderSettings(root, describeSettings(settingsInput(overrides)));
  return root;
}

function headings(): string[] {
  return [...root.querySelectorAll(".set-h")].map((node) => node.textContent ?? "");
}

describe("renderSettings", () => {
  it("renders the sections in the order the mockup puts them in", () => {
    // The fixture account is on the signup grant, which has no monthly window,
    // so the usage heading must not claim one.
    render({ account: account() });
    expect(headings()).toEqual(["Account", "Sites that can print", "Printer names", "Usage", "About"]);
  });

  it("heads usage with the month once the allowance is a monthly one", () => {
    render({
      account: account({
        allowance: { known: true, kind: "monthly", remaining: 14, total: 20, resetsAt: Date.UTC(2026, 8, 1) },
      }),
    });
    expect(headings()).toContain("Usage this month");
  });

  it("omits the account and usage sections when nobody is signed in", () => {
    render();
    expect(headings()).toEqual(["Sites that can print", "Printer names", "About"]);
  });

  it("gives every site a revoke action carrying the pattern revoking needs (P4)", () => {
    render();
    const revokes = [...root.querySelectorAll('[data-action="revoke"]')];
    expect(revokes).toHaveLength(2);
    expect(revokes[0]?.getAttribute("data-pattern")).toBe("https://bluebirdcafe.co/*");
  });

  it("shows the unmatched name exactly as the page asked for it, with a one-click fix (U5)", () => {
    render();
    const create = root.querySelector('[data-action="create-alias"]');
    expect(create?.getAttribute("data-requested")).toBe("Star TSP100");
    expect(create?.getAttribute("data-origin")).toBe("https://pos.thornbury.app");
    expect(root.textContent).toContain("“Star TSP100”");
    expect(root.textContent).toContain("that print failed");
  });

  it("marks an unmatched name in form as well as in colour (U6)", () => {
    render();
    const mark = root.querySelector(".set-row .pill.warn .pill-mark");
    expect(mark?.textContent ?? "").not.toBe("");
  });

  it("offers every configured printer as an alias target", () => {
    render();
    const options = [...root.querySelectorAll(".set-pick option")].map((node) => node.textContent);
    expect(options).toEqual(["TM-T20", "Kitchen"]);
  });

  it("says so plainly when there are no printers to map a name onto", () => {
    render({ printers: [] });
    expect(root.querySelector('[data-action="create-alias"]')).toBeNull();
    expect(root.textContent).toContain("no printers to map to");
  });

  it("lets a name that will never be aliased be dismissed, so the list stays trustworthy", () => {
    render();
    const dismiss = root.querySelector('[data-action="dismiss-unmatched"]');
    expect(dismiss?.getAttribute("data-requested")).toBe("Star TSP100");
    expect(dismiss?.getAttribute("data-origin")).toBe("https://pos.thornbury.app");
  });

  it("replaces the previous render instead of appending to it", () => {
    render({ account: account() });
    render();
    expect(root.querySelectorAll(".set")).toHaveLength(1);
    expect(root.textContent).not.toContain("sam@bluebirdcafe.co");
  });
});
