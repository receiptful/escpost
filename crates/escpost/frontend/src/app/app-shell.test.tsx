import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { cleanup, render, screen, within } from "@testing-library/preact";
import { locationStub } from "preact-iso/prerender";
import { App } from "../app";

const originalFetch = globalThis.fetch;

beforeEach(() => {
  globalThis.fetch = ((input: RequestInfo | URL) => Promise.resolve(new Response(JSON.stringify(
    String(input) === "/api/status"
      ? { virtual_printer: null, jobs_processed: 0 }
      : { printers: [] },
  ), { headers: { "content-type": "application/json" } }))) as unknown as typeof globalThis.fetch;
});

afterEach(() => {
  cleanup();
  globalThis.fetch = originalFetch;
});

function renderAt(path: string) {
  locationStub(path);
  return render(<App />);
}

describe("App", () => {
  test("shows the current job viewer from the Print jobs route", () => {
    renderAt("/app/jobs");

    expect(screen.getByRole("heading", { name: "Print jobs" })).toBeTruthy();
    expect(
      screen.getByRole("link", { name: "Open current job viewer" }).getAttribute("href"),
    ).toBe("/");
    expect(
      within(screen.getByRole("navigation", { name: "Workbench navigation" }))
        .getByRole("link", { name: "Print jobs" })
        .getAttribute("aria-current"),
    ).toBe("page");
  });

  test("keeps calibration honest while it is unavailable", () => {
    renderAt("/app/calibration");

    expect(screen.getByRole("heading", { name: "Calibration" })).toBeTruthy();
    expect(screen.queryByRole("button")).toBeNull();
  });

  test("shows a not found page for an unknown workbench route", () => {
    renderAt("/app/unknown");

    expect(screen.getByRole("heading", { name: "Not found" })).toBeTruthy();
  });

  test("exposes five destinations in each responsive navigation landmark", () => {
    renderAt("/app/jobs");

    expect(
      within(screen.getByRole("navigation", { name: "Workbench navigation" })).getAllByRole(
        "link",
      ),
    ).toHaveLength(5);
    expect(
      within(screen.getByRole("navigation", { name: "Mobile workbench navigation" })).getAllByRole(
        "link",
      ),
    ).toHaveLength(5);
  });

  test("selects Overview at the normalized workbench root path", () => {
    renderAt("/app/");

    expect(
      within(screen.getByRole("navigation", { name: "Workbench navigation" }))
        .getByRole("link", { name: "Overview" })
        .getAttribute("aria-current"),
    ).toBe("page");
  });

  test("replaces the old construction screen", () => {
    renderAt("/app/");

    expect(
      screen.queryByText("The new web workbench is under construction."),
    ).toBeNull();
  });
});
