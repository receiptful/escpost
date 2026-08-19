import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { cleanup, render, screen, within } from "@testing-library/preact";
import { locationStub } from "preact-iso/prerender";
import { App } from "../app";

const originalFetch = globalThis.fetch;

beforeEach(() => {
  globalThis.fetch = ((input: RequestInfo | URL) => Promise.resolve(new Response(JSON.stringify(
    String(input) === "/api/status"
      ? { virtual_printer: null, jobs_processed: 0 }
      : String(input) === "/api/jobs/current"
        ? { receiving: false, profile: "REFERENCE", error: null, job: null }
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
  test("shows the current job workbench from the Print jobs route", async () => {
    renderAt("/app/jobs");

    expect(screen.getByRole("heading", { name: "Print jobs" }).getAttribute("class")).toContain("sr-only");
    expect(await screen.findByText("Waiting for first job")).toBeTruthy();
    expect(screen.queryByRole("link", { name: "Open current job viewer" })).toBeNull();
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

  test("exposes polite live server status semantics for both responsive variants", async () => {
    renderAt("/app/jobs");

    await screen.findAllByText("Ready");
    const statuses = screen.getAllByRole("status", { name: "Server status" });
    expect(statuses).toHaveLength(2);
    for (const status of statuses) {
      expect(status.getAttribute("aria-live")).toBe("polite");
      expect(status.getAttribute("aria-atomic")).toBe("true");
      expect(status.textContent).toContain("Ready");
    }
    const desktopStatus = statuses.find((status) => status.closest("aside"));
    expect(desktopStatus?.closest("aside")?.getAttribute("class")).toContain("hidden");
    expect(desktopStatus?.closest("aside")?.getAttribute("class")).toContain("lg:flex");
  });

  test("keeps the mobile server status in normal flow above content while only navigation is fixed", () => {
    const view = renderAt("/app/printers");

    const statuses = screen.getAllByRole("status", { name: "Server status" });
    const mobileStatus = statuses.find((status) => status.closest("header"));
    expect(mobileStatus?.closest("header")?.getAttribute("class")).toContain("lg:hidden");
    expect(mobileStatus?.closest("header")?.nextElementSibling?.tagName).toBe("MAIN");
    const fixedMobileBar = view.container.querySelector("div.fixed");
    expect(fixedMobileBar?.querySelector("header")).toBeNull();
    expect(
      fixedMobileBar?.querySelector('[aria-label="Mobile workbench navigation"]'),
    ).toBeTruthy();
  });

  test("selects Overview at the normalized workbench root path", () => {
    renderAt("/app/");

    expect(
      within(screen.getByRole("navigation", { name: "Workbench navigation" }))
        .getByRole("link", { name: "Overview" })
        .getAttribute("aria-current"),
    ).toBe("page");
  });

  test("visually marks the current destination in desktop and mobile navigation", () => {
    renderAt("/app/jobs");

    const desktop = within(screen.getByRole("navigation", { name: "Workbench navigation" }))
      .getByRole("link", { name: "Print jobs" });
    const mobile = within(screen.getByRole("navigation", { name: "Mobile workbench navigation" }))
      .getByRole("link", { name: "Print jobs" });
    expect(desktop.getAttribute("class")).toContain("menu-active");
    expect(mobile.getAttribute("class")).toContain("bg-primary");
  });

  test("lets pages own their width instead of centering the entire application", () => {
    const view = renderAt("/app/jobs");
    const pageContainer = view.container.querySelector("main > div");

    expect(pageContainer?.getAttribute("class")).toBe("flex w-full flex-col");
  });

  test("keeps semantic page headings while hiding every repeated visual title", () => {
    const pages = [
      ["/app/", "Overview"],
      ["/app/jobs", "Print jobs"],
      ["/app/printers", "Printers"],
      ["/app/profiles", "Profiles"],
      ["/app/calibration", "Calibration"],
    ];

    for (const [path, name] of pages) {
      const view = renderAt(path);
      expect(screen.getByRole("heading", { name }).getAttribute("class")).toContain("sr-only");
      view.unmount();
    }
  });

  test("replaces the old construction screen", () => {
    renderAt("/app/");

    expect(
      screen.queryByText("The new web workbench is under construction."),
    ).toBeNull();
  });
});
