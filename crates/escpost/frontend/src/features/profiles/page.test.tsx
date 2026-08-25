import { afterEach, describe, expect, test } from "bun:test";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/preact";
import { useState } from "preact/hooks";
import { AppDataProvider } from "../../app/data";
import { ProfilesPage } from "./page";

const profiles = [
  {
    id: "CALIBRATED", vendor: "Acme", model: "Pro 80", source: "calibrated",
    paper_width_mm: 80, printable_width_mm: 72.25, printable_width_dots: 576,
    dpi_x: 203, dpi_y: 203, full_cut: true, partial_cut: false,
    barcode_function_a: true, barcode_function_b: true, qr_code: true,
  },
  {
    id: "SYNTHESIZED", vendor: "Acme", model: "Lite 58", source: "synthesized",
    paper_width_mm: 58, printable_width_mm: 48, printable_width_dots: 384,
    dpi_x: 203, dpi_y: 203, full_cut: false, partial_cut: true,
    barcode_function_a: true, barcode_function_b: false, qr_code: false,
  },
  {
    id: "REFERENCE", vendor: "ESCPost", model: "Reference", source: "virtual",
    paper_width_mm: 80, printable_width_mm: 72, printable_width_dots: 576,
    dpi_x: 203, dpi_y: 203, full_cut: false, partial_cut: false,
    barcode_function_a: false, barcode_function_b: true, qr_code: false,
  },
  {
    id: "NONE", vendor: "Acme", model: "No codes", source: "virtual",
    paper_width_mm: 80, printable_width_mm: 64, printable_width_dots: 512,
    dpi_x: 180, dpi_y: 180, full_cut: false, partial_cut: false,
    barcode_function_a: false, barcode_function_b: false, qr_code: false,
  },
];

function json(body: unknown, status = 200) {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json" },
  });
}

function renderPage(fetch: typeof globalThis.fetch) {
  globalThis.fetch = fetch;
  return render(<AppDataProvider><ProfilesPage /></AppDataProvider>);
}

function ProfileToggle() {
  const [visible, setVisible] = useState(true);
  return <>
    <button type="button" onClick={() => setVisible((current) => !current)}>Toggle profiles</button>
    {visible && <ProfilesPage />}
  </>;
}

function DeferredProfiles() {
  const [visible, setVisible] = useState(false);
  return <>
    <button type="button" onClick={() => setVisible(true)}>Visit profiles</button>
    {visible && <ProfilesPage />}
  </>;
}

afterEach(cleanup);

describe("ProfilesPage", () => {
  test("renders the CLI profile columns, values, markers, legend, and mobile facts", async () => {
    renderPage(((input: RequestInfo | URL) => {
      if (String(input) === "/api/profiles/list") return Promise.resolve(json({ profiles }));
      return Promise.resolve(json({ printers: [] }));
    }) as typeof globalThis.fetch);

    await screen.findAllByText("CALIBRATED");
    for (const header of ["PROFILE", "VENDOR", "MODEL", "CAL", "PAPER", "PRINT", "DOTS", "DPI", "CUT", "BC", "QR"]) {
      expect(screen.getByRole("columnheader", { name: header })).toBeTruthy();
    }
    expect(screen.getByText((_, element) => element?.textContent === "CAL: ✓ calibrated · ~ synthesized · ○ virtual   PAPER/PRINT mm, DOTS printable")).toBeTruthy();
    const expectedRows = [
      ["CALIBRATED", "Acme", "Pro 80", "✓", "80.0", "72.3", "576", "203", "✓", "A·B", "✓"],
      ["SYNTHESIZED", "Acme", "Lite 58", "~", "58.0", "48.0", "384", "203", "✓", "A", "–"],
      ["REFERENCE", "ESCPost", "Reference", "○", "80.0", "72.0", "576", "203", "–", "B", "–"],
      ["NONE", "Acme", "No codes", "○", "80.0", "64.0", "512", "180", "–", "–", "–"],
    ];
    const dataRows = screen.getAllByRole("row").slice(1);
    const cards = screen.getAllByRole("article");
    expect(dataRows).toHaveLength(expectedRows.length);
    expect(cards).toHaveLength(expectedRows.length);
    expectedRows.forEach((values, index) => {
      expect(Array.from(dataRows[index].querySelectorAll("td"), (cell) => cell.textContent)).toEqual(values);
      expect(Array.from(cards[index].querySelectorAll("dt"), (label) => label.textContent)).toEqual(["PROFILE", "VENDOR", "MODEL", "CAL", "PAPER", "PRINT", "DOTS", "DPI", "CUT", "BC", "QR"]);
      expect(Array.from(cards[index].querySelectorAll("dd"), (field) => field.textContent)).toEqual(values);
    });
  });

  test("distinguishes initial loading, empty catalog, initial API error, and successful retry", async () => {
    let resolveProfiles!: (response: Response) => void;
    renderPage(((input: RequestInfo | URL) => {
      if (String(input) === "/api/profiles/list") return new Promise<Response>((resolve) => { resolveProfiles = resolve; });
      return Promise.resolve(json({ printers: [] }));
    }) as typeof globalThis.fetch);
    expect(screen.getByText("Loading profiles…")).toBeTruthy();
    await act(async () => { resolveProfiles(json({ profiles: [] })); });
    expect(await screen.findByText("No profiles available.")).toBeTruthy();

    cleanup();
    const responses = [
      json({ error: { code: "profile_catalog_unavailable", message: "Profile catalog is unavailable." } }, 500),
      json({ profiles: [profiles[0]] }),
    ];
    renderPage(((input: RequestInfo | URL) => {
      if (String(input) === "/api/profiles/list") return Promise.resolve(responses.shift()!);
      return Promise.resolve(json({ printers: [] }));
    }) as typeof globalThis.fetch);
    expect(await screen.findByText("Profile catalog is unavailable.")).toBeTruthy();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Retry" })); });
    expect(await screen.findAllByText("CALIBRATED")).toHaveLength(2);
  });

  test("does not offer a refresh control for the compiled profile catalog", async () => {
    renderPage(((input: RequestInfo | URL) => {
      if (String(input) === "/api/profiles/list") return Promise.resolve(json({ profiles: [profiles[0]] }));
      return Promise.resolve(json({ printers: [] }));
    }) as typeof globalThis.fetch);
    expect(await screen.findAllByText("CALIBRATED")).toHaveLength(2);
    expect(screen.queryByRole("button", { name: "Refresh" })).toBeNull();
  });

  test("keeps the successful unfiltered catalog for the workbench session", async () => {
    let profileRequests = 0;
    globalThis.fetch = ((input: RequestInfo | URL) => {
      if (String(input) === "/api/profiles/list") {
        profileRequests += 1;
        return Promise.resolve(json({ profiles: [profiles[0]] }));
      }
      return Promise.resolve(json({ printers: [] }));
    }) as typeof globalThis.fetch;

    render(<AppDataProvider><ProfileToggle /></AppDataProvider>);
    expect(await screen.findAllByText("CALIBRATED")).toHaveLength(2);
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Toggle profiles" })); });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Toggle profiles" })); });
    expect(await screen.findAllByText("CALIBRATED")).toHaveLength(2);
    expect(profileRequests).toBe(1);
  });

  test("loads profiles once when the Profiles page is first visited", async () => {
    let profileRequests = 0;
    globalThis.fetch = ((input: RequestInfo | URL) => {
      if (String(input) === "/api/profiles/list") {
        profileRequests += 1;
        return Promise.resolve(json({ profiles: [profiles[0]] }));
      }
      return Promise.resolve(json({ printers: [] }));
    }) as typeof globalThis.fetch;

    render(<AppDataProvider><DeferredProfiles /></AppDataProvider>);
    await screen.findByRole("button", { name: "Visit profiles" });
    expect(profileRequests).toBe(0);
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Visit profiles" })); });
    expect(await screen.findAllByText("CALIBRATED")).toHaveLength(2);
    expect(profileRequests).toBe(1);
  });
});
