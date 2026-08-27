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
        { byte_start: 0, byte_end: 1, name: "Text", detail: "H", code_bytes: "", capped_parameter_bytes: "48", total_parameter_bytes: 1, fixed_parameters: false, paint_lifecycle: "committed", effects: [{ type: "paint", bounds: { x: 0, y: 0, width: 12, height: 24 } }] },
        { byte_start: 1, byte_end: 2, name: "Text", detail: "i", code_bytes: "", capped_parameter_bytes: "69", total_parameter_bytes: 1, fixed_parameters: false, paint_lifecycle: "committed", effects: [{ type: "paint", bounds: { x: 12, y: 0, width: 12, height: 24 } }] },
        { byte_start: 2, byte_end: 3, name: "LF", detail: "Print and line feed", code_bytes: "0A", capped_parameter_bytes: "", total_parameter_bytes: 0, fixed_parameters: true, effects: [{ type: "motion", before: { x: 24, y: 12 }, after: { x: 0, y: 30 } }] },
        { byte_start: 3, byte_end: 3011, name: "GS ( k", detail: "QR Code: Print the symbol data in the symbol storage area · Function 181", code_bytes: "1D 28 6B", capped_parameter_bytes: "03 00 31 51 30", total_parameter_bytes: 3005, fixed_parameters: false, paint_lifecycle: "committed", annotation: { label: "example.test", content: "https://example.test" }, effects: [{ type: "paint", bounds: { x: 0, y: 32, width: 42, height: 42 } }] },
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

    const textButton = screen.getByRole("button", { name: "Text 0..1: Hi" });
    const lineFeedButton = screen.getByRole("button", { name: "LF 2..2: Print and line feed" });
    const textOverlay = screen.getByRole("button", { name: "Highlight Text group at bytes 0 to 1" });
    const lineFeedOverlay = screen.getByRole("button", { name: "Highlight LF group at bytes 2 to 2" });
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

    const command = await screen.findByRole("button", { name: "GS ( k 3..3010: QR Code: Print the symbol data in the symbol storage area · Function 181" });
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
    const command = await screen.findByRole("button", { name: "Text 0..1: Hi" });
    const annotation = screen.getByRole("button", { name: "Highlight Text group at bytes 0 to 1" });
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

describe("command bytes", () => {
  test("shows the command bytes apart from the parameters they carry", async () => {
    globalThis.fetch = jest.fn(() => Promise.resolve(json(currentJob))) as unknown as typeof fetch;
    render(<JobsPage />);

    const lineFeed = await screen.findByRole("button", { name: "LF 2..2: Print and line feed" });

    expect(within(lineFeed).getByLabelText("Command bytes").textContent).toBe("0A");
    expect(within(lineFeed).queryByLabelText("Parameter bytes")).toBeNull();
  });

  test("joins the bytes of a grouped text run into one parameter box", async () => {
    globalThis.fetch = jest.fn(() => Promise.resolve(json(currentJob))) as unknown as typeof fetch;
    render(<JobsPage />);

    const text = await screen.findByRole("button", { name: "Text 0..1: Hi" });

    expect(within(text).queryByLabelText("Command bytes")).toBeNull();
    expect(within(text).getByLabelText("Parameter bytes").textContent).toBe("48 69");
  });

  test("counts the parameter bytes it cannot show", async () => {
    globalThis.fetch = jest.fn(() => Promise.resolve(json(currentJob))) as unknown as typeof fetch;
    render(<JobsPage />);

    const qr = await screen.findByRole("button", {
      name: /^GS \( k 3\.\.3010:/,
    });

    expect(within(qr).getByLabelText("Command bytes").textContent).toBe("1D 28 6B");
    expect(within(qr).getByLabelText("Parameter bytes").textContent).toBe(
      "03 00 31 51 30 … (3005 bytes)",
    );
  });
});

describe("command byte layout", () => {
  test("keeps fixed-size parameters beside the command name", async () => {
    globalThis.fetch = jest.fn(() => Promise.resolve(json(currentJob))) as unknown as typeof fetch;
    render(<JobsPage />);

    const lineFeed = await screen.findByRole("button", { name: "LF 2..2: Print and line feed" });
    const header = within(lineFeed).getByText("LF").parentElement;
    if (!header) throw new Error("expected a header row");

    expect(within(header).getByLabelText("Command bytes").textContent).toBe("0A");
  });

  test("keeps parameters of unknown size on a line of their own", async () => {
    globalThis.fetch = jest.fn(() => Promise.resolve(json(currentJob))) as unknown as typeof fetch;
    render(<JobsPage />);

    const qr = await screen.findByRole("button", { name: /^GS \( k 3\.\.3010:/ });
    const header = within(qr).getByText("GS ( k").parentElement;
    if (!header) throw new Error("expected a header row");

    expect(within(header).getByLabelText("Command bytes").textContent).toBe("1D 28 6B");
    expect(within(header).queryByLabelText("Parameter bytes")).toBeNull();
    expect(within(qr).getByLabelText("Parameter bytes").textContent).toBe(
      "03 00 31 51 30 … (3005 bytes)",
    );
  });
});
