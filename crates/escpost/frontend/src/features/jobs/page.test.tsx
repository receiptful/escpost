import { afterEach, describe, expect, jest, test } from "bun:test";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/preact";
import { JobsPage } from "./page";

function json(body: unknown) {
  return new Response(JSON.stringify(body), {
    headers: { "content-type": "application/json" },
  });
}

const currentJob = {
  receiving: false,
  profile: "REFERENCE",
  error: null,
  job: {
    id: "7",
    completed_at_unix_ms: 1_787_041_234_567,
    completion: "timeout",
    antialias: false,
    warnings: ["Unknown command was ignored."],
    input_url: "/api/jobs/7/input",
    sheets: [{
      number: 1,
      name: "sheet-001.png",
      width_dots: 384,
      height_dots: 80,
      image_url: "/api/jobs/7/sheets/1",
      commands: [
        { byte_start: 0, byte_end: 1, name: "Text", detail: "H", paint_lifecycle: "committed", effects: [{ type: "paint", bounds: { x: 0, y: 0, width: 12, height: 24 } }] },
        { byte_start: 1, byte_end: 2, name: "Text", detail: "i", paint_lifecycle: "committed", effects: [{ type: "paint", bounds: { x: 12, y: 0, width: 12, height: 24 } }] },
        { byte_start: 2, byte_end: 3, name: "LF", detail: "Print and line feed", effects: [{ type: "motion", before: { x: 24, y: 12 }, after: { x: 0, y: 30 } }] },
        { byte_start: 3, byte_end: 8, name: "GS ( k", detail: "Print QR code · Function 181", paint_lifecycle: "committed", annotation: { label: "example.test", content: "https://example.test" }, effects: [{ type: "paint", bounds: { x: 0, y: 32, width: 42, height: 42 } }] },
      ],
    }],
  },
};

afterEach(() => {
  cleanup();
  localStorage.clear();
  jest.restoreAllMocks();
});

describe("JobsPage", () => {
  test("renders current-job parity facts and keeps pin and hover highlights independent", async () => {
    globalThis.fetch = jest.fn(() => Promise.resolve(json(currentJob))) as unknown as typeof fetch;
    render(<JobsPage />);

    expect(await screen.findByText("Hi")).toBeTruthy();
    expect(screen.getByText("idle-timeout")).toBeTruthy();
    expect(screen.getByText("Unknown command was ignored.")).toBeTruthy();
    expect(screen.getByRole("link", { name: "Download raw input" }).getAttribute("href")).toBe("/api/jobs/7/input");
    expect(screen.getByAltText("Rendered receipt sheet 1 of 1").getAttribute("src")).toBe("/api/jobs/7/sheets/1");

    const textButton = screen.getByRole("button", { name: "Text 0..2: Hi" });
    const lineFeedButton = screen.getByRole("button", { name: "LF 2..3: Print and line feed" });
    const textOverlay = screen.getByRole("button", { name: "Highlight Text group at bytes 0 to 2" });
    const lineFeedOverlay = screen.getByRole("button", { name: "Highlight LF group at bytes 2 to 3" });
    expect(lineFeedOverlay.querySelector("text")?.getAttribute("text-anchor")).toBe("middle");

    await act(async () => { await Promise.resolve(); });
    fireEvent.click(lineFeedButton);
    await waitFor(() => expect(lineFeedButton.getAttribute("aria-pressed")).toBe("true"));
    expect(lineFeedOverlay.classList.contains("trace-pinned")).toBe(true);

    fireEvent.pointerEnter(textButton);
    expect(textOverlay.classList.contains("trace-previewed")).toBe(true);
    expect(lineFeedOverlay.classList.contains("trace-pinned")).toBe(true);
    fireEvent.pointerLeave(textButton);
    expect(textOverlay.classList.contains("trace-previewed")).toBe(false);
    expect(lineFeedOverlay.classList.contains("trace-pinned")).toBe(true);

    fireEvent.click(lineFeedButton);
    expect(lineFeedButton.getAttribute("aria-pressed")).toBe("true");
    fireEvent.click(lineFeedOverlay);
    expect(lineFeedButton.getAttribute("aria-pressed")).toBe("true");

    fireEvent.click(screen.getByAltText("Rendered receipt sheet 1 of 1"));
    expect(lineFeedButton.getAttribute("aria-pressed")).toBe("false");

    fireEvent.click(lineFeedButton);
    fireEvent.keyDown(window, { key: "Escape" });
    expect(lineFeedButton.getAttribute("aria-pressed")).toBe("false");
  });

  test("copies QR annotations and opens validated web URLs", async () => {
    globalThis.fetch = jest.fn(() => Promise.resolve(json(currentJob))) as unknown as typeof fetch;
    const writeText = jest.fn(() => Promise.resolve());
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
    const open = jest.spyOn(window, "open").mockImplementation(() => null);
    render(<JobsPage />);

    const command = await screen.findByRole("button", { name: "GS ( k 3..8: Print QR code · Function 181" });
    fireEvent.pointerEnter(command);
    const annotation = screen.getByRole("link", { name: "Copy and open QR content: example.test" });
    fireEvent.keyDown(annotation, { key: "Enter" });

    expect(writeText).toHaveBeenCalledWith("https://example.test");
    expect(open).toHaveBeenCalledWith("https://example.test/", "_blank", "noopener,noreferrer");
  });

  test("defaults paper margin on and persists changes", async () => {
    globalThis.fetch = jest.fn(() => Promise.resolve(json(currentJob))) as unknown as typeof fetch;
    render(<JobsPage />);
    await screen.findByText("Hi");

    const toggle = screen.getByRole("checkbox", { name: "Paper margin" });
    const status = screen.getByRole("group", { name: "Current job status" });
    expect(within(status).getByRole("checkbox", { name: "Paper margin" })).toBe(toggle);
    expect(within(status).getByRole("link", { name: "Download raw input" })).toBeTruthy();
    expect((toggle as HTMLInputElement).checked).toBe(true);
    fireEvent.click(toggle);
    expect(localStorage.getItem("escpost.paper_margin")).toBe("false");
    expect(document.querySelector(".receipt-paper-margin")).toBeNull();
  });

  test("lays multiple sheets left-to-right and wraps only when space runs out", async () => {
    const secondSheet = {
      ...currentJob.job.sheets[0],
      number: 2,
      name: "sheet-002.png",
      image_url: "/api/jobs/7/sheets/2",
      commands: [],
    };
    globalThis.fetch = jest.fn(() => Promise.resolve(json({
      ...currentJob,
      job: { ...currentJob.job, sheets: [...currentJob.job.sheets, secondSheet] },
    }))) as unknown as typeof fetch;
    render(<JobsPage />);

    await screen.findByAltText("Rendered receipt sheet 1 of 2");
    expect(screen.getByAltText("Rendered receipt sheet 2 of 2")).toBeTruthy();
    const sheetFlow = screen.getByRole("region", { name: "Rendered receipt sheets" }).firstElementChild;
    expect(sheetFlow?.getAttribute("class")).toContain("flex-wrap");
    expect(sheetFlow?.getAttribute("class")).toContain("items-start");
    expect(sheetFlow?.getAttribute("class")).not.toContain("flex-col");
    expect(sheetFlow?.getAttribute("class")).not.toContain("items-center");
  });

  test("reveals annotations from commands and commands from annotations", async () => {
    globalThis.fetch = jest.fn(() => Promise.resolve(json(currentJob))) as unknown as typeof fetch;
    render(<JobsPage />);
    const command = await screen.findByRole("button", { name: "Text 0..2: Hi" });
    const annotation = screen.getByRole("button", { name: "Highlight Text group at bytes 0 to 2" });
    const workspace = screen.getByRole("region", { name: "Rendered receipt sheets" });
    const panel = screen.getByRole("complementary", { name: "Commands in the current print job" });
    const sheetScroll = jest.fn();
    const commandScroll = jest.fn();
    workspace.scrollBy = sheetScroll;
    panel.scrollBy = commandScroll;
    Object.defineProperties(workspace, { clientWidth: { value: 10 }, clientHeight: { value: 10 } });
    Object.defineProperties(panel, { clientWidth: { value: 10 }, clientHeight: { value: 10 } });
    workspace.getBoundingClientRect = () => ({ top: 0, left: 0, right: 10, bottom: 10, width: 10, height: 10, x: 0, y: 0, toJSON() {} });
    panel.getBoundingClientRect = workspace.getBoundingClientRect;
    annotation.getBoundingClientRect = () => ({ top: 20, left: 20, right: 30, bottom: 30, width: 10, height: 10, x: 20, y: 20, toJSON() {} });
    command.getBoundingClientRect = annotation.getBoundingClientRect;

    fireEvent.pointerEnter(command);
    expect(sheetScroll).toHaveBeenCalledWith({ top: 20, left: 20 });
    fireEvent.pointerEnter(annotation);
    expect(commandScroll).toHaveBeenCalledWith({ top: 20, left: 0 });
  });

  test("shows waiting guidance when the server has no current job", async () => {
    globalThis.fetch = jest.fn(() => Promise.resolve(json({
      receiving: false,
      profile: "REFERENCE",
      error: null,
      hint: "Send an ESC/POS job to 127.0.0.1:9100",
      job: null,
    }))) as unknown as typeof fetch;
    render(<JobsPage />);

    expect(await screen.findByText("Waiting for first job")).toBeTruthy();
    expect(screen.getByText("Send an ESC/POS job to 127.0.0.1:9100")).toBeTruthy();
  });
});
