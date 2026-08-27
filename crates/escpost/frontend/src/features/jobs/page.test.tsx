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

function byteSpans(box: HTMLElement): string[] {
  return [...box.querySelectorAll("[data-hex]")].map((span) => span.textContent ?? "");
}

function characterSpans(box: HTMLElement): string[] {
  return [...box.querySelectorAll("[data-character]")].map((span) => span.textContent ?? "");
}

afterEach(() => {
  cleanup();
  localStorage.clear();
  jest.restoreAllMocks();
});

describe("JobsPage", () => {
  test("renders current-job parity facts and keeps pin and hover highlights independent", async () => {
    globalThis.fetch = jest.fn(() => Promise.resolve(json(currentJob))) as unknown as typeof fetch;
    render(<JobsPage />);

    expect(await screen.findByRole("button", { name: "Text 1..2: Hi" })).toBeTruthy();
    expect(screen.getByText("idle-timeout")).toBeTruthy();
    expect(screen.getByText("Unknown command was ignored.")).toBeTruthy();
    expect(screen.getByRole("link", { name: "Download" }).getAttribute("href")).toBe("/api/jobs/7/input");
    expect(screen.getByAltText("Rendered receipt sheet 1 of 1").getAttribute("src")).toBe("/api/jobs/7/sheets/1");

    const textButton = screen.getByRole("button", { name: "Text 1..2: Hi" });
    const lineFeedButton = screen.getByRole("button", { name: "LF 3..3: Print and line feed" });
    const textOverlay = screen.getByRole("button", { name: "Highlight Text group at bytes 1 to 2" });
    const lineFeedOverlay = screen.getByRole("button", { name: "Highlight LF group at bytes 3 to 3" });
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

    const command = await screen.findByRole("button", { name: "GS ( k 4..3011: QR Code: Print the symbol data in the symbol storage area · Function 181" });
    fireEvent.pointerEnter(command);
    const annotation = screen.getByRole("link", { name: "Copy and open QR content: example.test" });
    fireEvent.keyDown(annotation, { key: "Enter" });

    expect(writeText).toHaveBeenCalledWith("https://example.test");
    expect(open).toHaveBeenCalledWith("https://example.test/", "_blank", "noopener,noreferrer");
  });

  test("defaults paper margin on and persists changes", async () => {
    globalThis.fetch = jest.fn(() => Promise.resolve(json(currentJob))) as unknown as typeof fetch;
    render(<JobsPage />);
    await screen.findByRole("button", { name: "Text 1..2: Hi" });

    const toggle = screen.getByRole("checkbox", { name: "Paper margin" });
    const status = screen.getByRole("group", { name: "Current job status" });
    expect(within(status).getByRole("checkbox", { name: "Paper margin" })).toBe(toggle);
    expect(within(status).queryByRole("link", { name: /Download/ })).toBeNull();
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
    const command = await screen.findByRole("button", { name: "Text 1..2: Hi" });
    const annotation = screen.getByRole("button", { name: "Highlight Text group at bytes 1 to 2" });
    const workspace = screen.getByRole("region", { name: "Rendered receipt sheets" });
    const panel = screen.getByRole("complementary", { name: "ESC/POS bytes in the current print job" });
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

    const lineFeed = await screen.findByRole("button", { name: "LF 3..3: Print and line feed" });

    expect(within(lineFeed).getByLabelText("Command bytes").textContent).toBe("0A");
    expect(within(lineFeed).queryByLabelText("Parameter bytes")).toBeNull();
  });

  test("joins the bytes of a grouped text run into one parameter box", async () => {
    globalThis.fetch = jest.fn(() => Promise.resolve(json(currentJob))) as unknown as typeof fetch;
    render(<JobsPage />);

    const text = await screen.findByRole("button", { name: "Text 1..2: Hi" });

    expect(within(text).queryByLabelText("Command bytes")).toBeNull();
    expect(byteSpans(within(text).getByLabelText("Parameter bytes"))).toEqual(["48", "69"]);
  });

  test("counts the parameter bytes it cannot show", async () => {
    globalThis.fetch = jest.fn(() => Promise.resolve(json(currentJob))) as unknown as typeof fetch;
    render(<JobsPage />);

    const qr = await screen.findByRole("button", {
      name: /^GS \( k 4\.\.3011:/,
    });

    expect(within(qr).getByLabelText("Command bytes").textContent).toBe("1D 28 6B");
    const parameters = within(qr).getByLabelText("Parameter bytes");
    expect(byteSpans(parameters)).toEqual(["03", "00", "31", "51", "30"]);
    expect(parameters.textContent).toContain("… (3005 bytes)");
  });
});

describe("command byte layout", () => {
  test("keeps fixed-size parameters beside the command name", async () => {
    globalThis.fetch = jest.fn(() => Promise.resolve(json(currentJob))) as unknown as typeof fetch;
    render(<JobsPage />);

    const lineFeed = await screen.findByRole("button", { name: "LF 3..3: Print and line feed" });
    const header = within(lineFeed).getByText("LF").parentElement;
    if (!header) throw new Error("expected a header row");

    expect(within(header).getByLabelText("Command bytes").textContent).toBe("0A");
  });

  test("keeps parameters of unknown size on a line of their own", async () => {
    globalThis.fetch = jest.fn(() => Promise.resolve(json(currentJob))) as unknown as typeof fetch;
    render(<JobsPage />);

    const qr = await screen.findByRole("button", { name: /^GS \( k 4\.\.3011:/ });
    const header = within(qr).getByText("GS ( k").parentElement;
    if (!header) throw new Error("expected a header row");

    expect(within(header).getByLabelText("Command bytes").textContent).toBe("1D 28 6B");
    expect(within(header).queryByLabelText("Parameter bytes")).toBeNull();
    const parameters = within(qr).getByLabelText("Parameter bytes");
    expect(byteSpans(parameters)).toEqual(["03", "00", "31", "51", "30"]);
    expect(parameters.textContent).toContain("… (3005 bytes)");
  });
});

describe("command rows", () => {
  test("alternates the row background so neighbouring commands differ", async () => {
    globalThis.fetch = jest.fn(() => Promise.resolve(json(currentJob))) as unknown as typeof fetch;
    render(<JobsPage />);

    const panel = await screen.findByRole("complementary", {
      name: "ESC/POS bytes in the current print job",
    });
    const rows = within(panel).getAllByRole("listitem");
    const buttons = within(panel).getAllByRole("button");

    expect(rows).toHaveLength(3);
    expect(rows[0].className).toBe(rows[2].className);
    expect(rows[1].className).not.toBe(rows[0].className);
    expect(buttons[0].className).toBe(buttons[1].className);
  });
});

describe("command panel header", () => {
  test("keeps the header on its own surface above the rows", async () => {
    globalThis.fetch = jest.fn(() => Promise.resolve(json(currentJob))) as unknown as typeof fetch;
    render(<JobsPage />);

    const panel = await screen.findByRole("complementary", {
      name: "ESC/POS bytes in the current print job",
    });
    const header = within(panel).getByRole("heading", { name: /3011 bytes/ })
      .closest("[data-sticky-header]");
    if (!header) throw new Error("expected a header");
    const rows = within(panel).getAllByRole("listitem");

    expect(header.className).toContain("bg-base-300");
    expect(header.className).toContain("shadow");
    expect(rows[0].className).not.toContain("bg-base-300");
    // Scrolling a row into view measures this header, thus the panel has to
    // find it by the marker the reveal helper looks for.
    expect(panel.querySelector("[data-sticky-header]")).toBe(header);
  });
});

describe("character highlighting", () => {
  async function textRun() {
    globalThis.fetch = jest.fn(() => Promise.resolve(json(currentJob))) as unknown as typeof fetch;
    const view = render(<JobsPage />);
    const row = await screen.findByRole("button", { name: "Text 1..2: Hi" });
    const bytes = [...within(row).getByLabelText("Parameter bytes").querySelectorAll("[data-byte]")];

    const regions = [...view.container.querySelectorAll(".trace-character")];
    return { row, bytes, regions, view };
  }

  test("hovering a character marks its byte and only that byte", async () => {
    const { bytes, regions } = await textRun();

    fireEvent.pointerEnter(regions[1]);

    expect(bytes[1].className).toContain("font-bold");
    expect(bytes[0].className).not.toContain("font-bold");
  });

  test("hovering a byte marks its character and only that character", async () => {
    const { bytes, regions } = await textRun();

    fireEvent.pointerEnter(bytes[0]);

    expect(regions[0].getAttribute("class")).toContain("trace-character-active");
    expect(regions[1].getAttribute("class")).not.toContain("trace-character-active");
  });

  test("draws one box for the run and none for a single character", async () => {
    const { row, view } = await textRun();
    const boxes = [...view.container.querySelectorAll(".trace-region")];

    // Two characters and the QR code, thus one box for the run and one for QR.
    expect(boxes).toHaveLength(2);
    expect(within(row).getByLabelText("Parameter bytes")).toBeTruthy();
  });

  test("names the byte of the character on the sheet", async () => {
    const { regions, view } = await textRun();

    fireEvent.pointerEnter(regions[1]);
    const label = view.container.querySelector(".trace-character-label");

    expect(label?.textContent).toContain("69");
    // The label lies over the characters below it, thus it must not take the
    // pointer away from them.
    expect(label?.getAttribute("pointer-events")).toBe("none");
  });

  test("takes the byte off the sheet once the pointer leaves", async () => {
    const { regions, view } = await textRun();

    fireEvent.pointerEnter(regions[1]);
    fireEvent.pointerLeave(regions[1]);

    expect(view.container.querySelector(".trace-character-label")).toBeNull();
  });

  test("forgets the character once the pointer leaves", async () => {
    const { bytes, regions, view } = await textRun();

    fireEvent.pointerEnter(regions[0]);
    fireEvent.pointerLeave(regions[0]);

    expect(bytes[0].className).not.toContain("font-bold");
    expect(regions[0].getAttribute("class")).not.toContain("trace-character-active");
  });
});

describe("characters beside their bytes", () => {
  test("shows each character over the byte that printed it", async () => {
    globalThis.fetch = jest.fn(() => Promise.resolve(json(currentJob))) as unknown as typeof fetch;
    render(<JobsPage />);

    const row = await screen.findByRole("button", { name: "Text 1..2: Hi" });
    const box = within(row).getByLabelText("Parameter bytes");

    expect(characterSpans(box)).toEqual(["H", "i"]);
    expect(byteSpans(box)).toEqual(["48", "69"]);
  });

  test("drops the separate text line, because the bytes now carry it", async () => {
    globalThis.fetch = jest.fn(() => Promise.resolve(json(currentJob))) as unknown as typeof fetch;
    render(<JobsPage />);

    const row = await screen.findByRole("button", { name: "Text 1..2: Hi" });

    expect(within(row).queryByText("Hi")).toBeNull();
  });

  test("keeps the description of a command that prints no text", async () => {
    globalThis.fetch = jest.fn(() => Promise.resolve(json(currentJob))) as unknown as typeof fetch;
    render(<JobsPage />);

    const row = await screen.findByRole("button", { name: "LF 3..3: Print and line feed" });

    expect(within(row).getByText("Print and line feed")).toBeTruthy();
  });
});

describe("byte cells of any command", () => {
  test("shades every second byte of a command that prints no text", async () => {
    globalThis.fetch = jest.fn(() => Promise.resolve(json(currentJob))) as unknown as typeof fetch;
    render(<JobsPage />);

    const qr = await screen.findByRole("button", { name: /^GS \( k 4\.\.3011:/ });
    const cells = [...within(qr).getByLabelText("Parameter bytes").querySelectorAll("[data-byte]")];

    expect(cells).toHaveLength(5);
    expect(cells[0].className).toBe(cells[2].className);
    expect(cells[1].className).not.toBe(cells[0].className);
  });
});

describe("what the panel holds", () => {
  test("names the bytes it explains, and how many the job holds", async () => {
    globalThis.fetch = jest.fn(() => Promise.resolve(json(currentJob))) as unknown as typeof fetch;
    render(<JobsPage />);

    const panel = await screen.findByRole("complementary", {
      name: "ESC/POS bytes in the current print job",
    });

    expect(within(panel).getByRole("heading", { name: /3011 bytes/ }).textContent)
      .toContain("ESC/POS");
    expect(within(panel).getByText("Command")).toBeTruthy();
    expect(within(panel).getByText("Index")).toBeTruthy();
  });
});

describe("where the job furniture sits", () => {
  test("keeps the download beside the bytes it downloads", async () => {
    globalThis.fetch = jest.fn(() => Promise.resolve(json(currentJob))) as unknown as typeof fetch;
    render(<JobsPage />);

    const panel = await screen.findByRole("complementary", {
      name: "ESC/POS bytes in the current print job",
    });
    const download = within(panel).getByRole("link", { name: "Download" });

    expect(download.getAttribute("href")).toBe("/api/jobs/7/input");
    expect(download.getAttribute("download")).not.toBeNull();
  });

  test("keeps the job status over the sheets, not over the bytes", async () => {
    globalThis.fetch = jest.fn(() => Promise.resolve(json(currentJob))) as unknown as typeof fetch;
    render(<JobsPage />);

    // The status also stands alone while a job is awaited, thus the sheets are
    // what says the job arrived.
    const sheets = await screen.findByRole("region", { name: "Rendered receipt sheets" });
    const status = screen.getByRole("group", { name: "Current job status" });
    const panel = screen.getByRole("complementary", {
      name: "ESC/POS bytes in the current print job",
    });

    // The status and the sheets share a column, thus the bytes start at the
    // top of the page beside them.
    const column = status.closest("[data-sheet-column]");
    expect(column).not.toBeNull();
    expect(column?.contains(sheets)).toBe(true);
    expect(column?.contains(panel)).toBe(false);
    // The column and the panel are cells of one row of the grid, which is what
    // gives them the same top and the same height.
    expect(panel.parentElement).toBe(column?.parentElement ?? null);
    expect(panel.parentElement?.getAttribute("class")).toContain("grid");
  });
});
