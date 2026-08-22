import { afterEach, describe, expect, jest, test } from "bun:test";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/preact";
import type { UsbDiscoveryFailure } from "../../api/discovery-stream";
import type { DiscoveredPrinter, UsbConnection } from "../../api/types";
import type { ScanState } from "../../app/data";
import { DiscoveryPanel } from "./discovery-panel";

const idle: ScanState = {
  phase: "idle",
  completed: 0,
  total: 0,
  printers: [],
  failures: [],
  error: null,
};

function networkPrinter(host: string, configuredNames: string[] = []): DiscoveredPrinter {
  return {
    transport: "network",
    configured_names: configuredNames,
    configured_profile: configuredNames.length > 0 ? "TM-T88V" : null,
    interface: "enx0",
    connection: { type: "network", host, port: 9100 },
  };
}

function usbPrinter(overrides: Partial<UsbConnection> = {}): DiscoveredPrinter {
  return {
    transport: "usb",
    configured_names: [],
    configured_profile: null,
    connection: {
      type: "usb",
      vendor_id: 0x0416,
      product_id: 0x5011,
      bus: "003",
      address: 7,
      manufacturer: null,
      product: "POS-58 Printer",
      serial_number: null,
      interface_number: 0,
      out_endpoints: [0x01],
      in_endpoints: [],
      ...overrides,
    },
  };
}

function usbFailure(overrides: Partial<UsbDiscoveryFailure> = {}): UsbDiscoveryFailure {
  return {
    vendor_id: 0x04b8,
    product_id: 0x0202,
    stage: "open_device",
    reason: "permission denied (errno 13)",
    permission_denied: true,
    can_grant_usb_permissions: true,
    ...overrides,
  };
}

// `usb` is the scope's, not the stream's: the stream reports progress, and
// whether USB was part of the sweep is something only the query says. Both
// transports is the CLI's no-flag default, so it is this harness's default
// too.
function renderPanel(scan: Partial<ScanState>, usb = true) {
  const onAdd = jest.fn();
  const view = render(<DiscoveryPanel scan={{ ...idle, ...scan }} usb={usb} onAdd={onAdd} />);
  const rerender = (next: Partial<ScanState>) => {
    view.rerender(<DiscoveryPanel scan={{ ...idle, ...next }} usb={usb} onAdd={onAdd} />);
  };
  return { view, onAdd, rerender };
}

// Absence as a boolean. `expect(node).toBeNull()` prints the entire happy-dom
// node graph when it fails, which buries every other failure in the run.
function gone(element: Element | null) {
  return element === null;
}

afterEach(() => {
  cleanup();
  jest.useRealTimers();
});

describe("DiscoveryPanel", () => {
  // Already-configured hits are deliberately not rows: the scan reports them
  // by flashing the row they already occupy in the inventory below, so the
  // panel only owes the reader a count.
  test("already-configured printers are counted, not listed", () => {
    const { view } = renderPanel({
      phase: "done",
      completed: 508,
      total: 508,
      printers: [networkPrinter("10.42.0.83"), networkPrinter("10.42.0.71", ["kitchen"])],
    });

    expect(screen.getByText("10.42.0.83:9100")).toBeTruthy();
    expect(gone(screen.queryByText("kitchen"))).toBe(true);
    expect(view.container.textContent).not.toContain("10.42.0.71");
    // Both printers answered, so both are counted; only the unconfigured one
    // is a row.
    expect(screen.getByText("2 printers found (1 new)")).toBeTruthy();
    // A full bar says nothing a finished scan has not already said, and it
    // competes with the results for the eye.
    expect(gone(screen.queryByRole("progressbar"))).toBe(true);
    // No heading and no scan button of its own: the section this renders into
    // is titled `Discovery` and carries the one button that starts, repeats
    // and cancels a scan. A second title here would name the same block
    // twice.
    expect(gone(screen.queryByRole("heading", { level: 2 }))).toBe(true);
    expect(gone(screen.queryByRole("button", { name: "Cancel" }))).toBe(true);
  });

  // USB enumeration finishes long before the sweep does, so the panel has to
  // be right in the middle of a scan, not only at the end of one.
  test("lists USB results while the network sweep is still running", () => {
    renderPanel({
      phase: "running",
      completed: 312,
      total: 508,
      printers: [networkPrinter("10.42.0.83"), usbPrinter()],
    });

    const progress = screen.getByRole("progressbar") as HTMLProgressElement;
    expect(progress.value).toBe(312);
    expect(progress.max).toBe(508);
    expect(screen.getByText("Checking USB · scanning 312 / 508 IP addresses")).toBeTruthy();
    expect(screen.getByText("2 printers found (2 new)")).toBeTruthy();

    // USB before network regardless of arrival order: an enumerated device is
    // a fact about this machine, a swept host an observation about the world.
    const titles = screen.getAllByRole("heading", { level: 3 }).map((row) => row.textContent);
    expect(titles).toEqual(["POS-58 Printer", "10.42.0.83:9100"]);
    expect(screen.getByText("USB 0416:5011 · bus 003 addr 007 · no serial · interface 0 · out 0x01")).toBeTruthy();
    expect(screen.getByText("Network · reachable via enx0")).toBeTruthy();
  });

  // Both transports selected and no USB printer attached used to read exactly
  // like a scan where USB never ran. The scan line says which halves ran, and
  // it may not claim the USB half finished while the sweep is still going —
  // USB results arrive first, but nothing on the stream says enumeration is
  // done, so `Checking` stands until the whole scan is.
  test("the scan line states which halves of the scan ran", () => {
    const { rerender } = renderPanel({ phase: "running", completed: 129, total: 253 });
    expect(screen.getByText("Checking USB · scanning 129 / 253 IP addresses")).toBeTruthy();

    rerender({ phase: "done", completed: 253, total: 253 });
    expect(screen.getByText("Checked USB · scanned 253 IP addresses")).toBeTruthy();

    cleanup();
    // A USB-only scan has no addresses to count. It used to fall back to
    // wording that said nothing; now it says what it did.
    const usbOnly = renderPanel({ phase: "running" });
    expect(screen.getByText("Checking USB")).toBeTruthy();
    usbOnly.rerender({ phase: "done" });
    expect(screen.getByText("Checked USB")).toBeTruthy();

    cleanup();
    // Network only: no USB half to mention, and the address count leads.
    const networkOnly = renderPanel({ phase: "running", completed: 129, total: 253 }, false);
    expect(screen.getByText("Scanning 129 / 253 IP addresses")).toBeTruthy();
    networkOnly.rerender({ phase: "done", completed: 253, total: 253 });
    expect(screen.getByText("Scanned 253 IP addresses")).toBeTruthy();
  });

  // A stopped scan keeps the shape of the line the reader was already
  // watching — only the verb settles from present to past — so the number
  // they were tracking stays where it was. That it was cut short is legible
  // from the count falling short of the total, the bar being gone and the
  // button reading `Rescan`; the words do not have to carry it too.
  test("a stopped scan keeps its results and the count it reached", () => {
    renderPanel({
      phase: "stopped",
      completed: 257,
      total: 508,
      printers: [networkPrinter("10.42.0.83")],
    });

    expect(screen.getByText("Checked USB · scanned 257 / 508 IP addresses")).toBeTruthy();
    expect(screen.getByText("1 printer found (1 new)")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Add 10.42.0.83:9100" })).toBeTruthy();
    // The bar belongs to a scan in progress, and this one is over.
    expect(gone(screen.queryByRole("progressbar"))).toBe(true);
  });

  // `1 printers found` shipped on this branch once already, in the empty
  // state, so every arity both numbers can reach is pinned here. The new
  // count comes from the same list as the total, so the pair cannot disagree
  // — and the mixed case is the one a wrong derivation would still look
  // plausible in.
  test("counts every printer that answered and how many of them are new", () => {
    const { rerender } = renderPanel({ phase: "running", printers: [] });
    // No parenthetical at zero: `(0 new)` is noise when nothing answered.
    expect(screen.getByText("0 printers found")).toBeTruthy();

    rerender({ phase: "running", printers: [networkPrinter("10.42.0.83")] });
    expect(screen.getByText("1 printer found (1 new)")).toBeTruthy();

    // The second printer is already configured and still answered. The total
    // is how many printers the scan reached, not how many are new — a reader
    // watching a sweep wants to know the network replied at all.
    rerender({ phase: "running", printers: [networkPrinter("10.42.0.83"), networkPrinter("10.42.0.71", ["kitchen"])] });
    expect(screen.getByText("2 printers found (1 new)")).toBeTruthy();

    rerender({ phase: "running", printers: [networkPrinter("10.42.0.83"), networkPrinter("10.42.0.84")] });
    expect(screen.getByText("2 printers found (2 new)")).toBeTruthy();

    // The case that earns the parenthetical: something answered, none of it
    // is new, and the total alone would read as a find.
    rerender({ phase: "done", printers: [networkPrinter("10.42.0.71", ["kitchen"])] });
    expect(screen.getByText("1 printer found (0 new)")).toBeTruthy();
  });

  // Fixing USB permissions genuinely requires a terminal, so naming the
  // command is the only useful thing the browser can say — and it says it in
  // the words `escpost printers discover` uses.
  test("a permission-denied USB failure names the fix command", () => {
    const { view, rerender } = renderPanel({
      phase: "done",
      printers: [],
      failures: [usbFailure(), usbFailure({ product_id: 0x0203, stage: "inspect_configuration", permission_denied: false, reason: "device disconnected" })],
    });

    expect(screen.getByText("Could not open USB device 04b8:0202: permission denied (errno 13).")).toBeTruthy();
    expect(screen.getByText("Could not inspect the active configuration of USB device 04b8:0203: device disconnected.")).toBeTruthy();
    // One remedy for the whole banner, however many devices were refused.
    expect(screen.getAllByText(/sudo escpost printers grant-usb-permissions/)).toHaveLength(1);
    // The server may be a machine across the room, so "run this" has to say
    // where — a distinction the terminal never has to make.
    expect(screen.getByRole("alert").textContent).toContain("on the machine running");

    // A failure that is not a permission problem gets no command to run.
    rerender({
      phase: "done",
      printers: [],
      failures: [usbFailure({ stage: "inspect_configuration", permission_denied: false, reason: "device disconnected" })],
    });
    expect(view.container.textContent).not.toContain("grant-usb-permissions");
  });

  // `printers grant-usb-permissions` is a Linux-only subcommand: on a macOS
  // server it is unrecognized, and the CLI on that same host stays silent
  // about it. Only the server knows what it runs on, so it says.
  test("stays silent about the fix command on a host that does not have it", () => {
    const { view } = renderPanel({
      phase: "done",
      printers: [],
      failures: [usbFailure({ can_grant_usb_permissions: false })],
    });

    expect(screen.getByText("Could not open USB device 04b8:0202: permission denied (errno 13).")).toBeTruthy();
    expect(view.container.textContent).not.toContain("grant-usb-permissions");
  });

  // A USB failure is tolerated, not fatal: the sweep underneath keeps going,
  // and a panel that drops its progress bar the moment a device is refused
  // would look finished while probing 500 more hosts.
  test("keeps a running scan running through USB failures", () => {
    renderPanel({
      phase: "running",
      completed: 100,
      total: 508,
      printers: [usbPrinter()],
      failures: [usbFailure(), usbFailure({ product_id: 0x0203 })],
    });

    expect(screen.getByText("Could not open USB device 04b8:0202: permission denied (errno 13).")).toBeTruthy();
    expect(screen.getByText("Could not open USB device 04b8:0203: permission denied (errno 13).")).toBeTruthy();
    expect((screen.getByRole("progressbar") as HTMLProgressElement).value).toBe(100);
  });

  // Two of the same model refused at two addresses report identical facts,
  // since the failure carries no bus or address. Both still have to be
  // listed, and listing them must not collide on a key.
  test("lists both refusals when the same model fails twice", () => {
    const { rerender } = renderPanel({
      phase: "done",
      printers: [],
      failures: [usbFailure({ vendor_id: 0x0416, product_id: 0x5011 }), usbFailure({ vendor_id: 0x0416, product_id: 0x5011 })],
    });

    expect(screen.getAllByText("Could not open USB device 0416:5011: permission denied (errno 13).")).toHaveLength(2);

    // A third refusal of the same model arrives mid-scan: the list grows by
    // one rather than the diff reusing a node it cannot tell apart.
    rerender({
      phase: "done",
      printers: [],
      failures: [
        usbFailure({ vendor_id: 0x0416, product_id: 0x5011 }),
        usbFailure({ vendor_id: 0x0416, product_id: 0x5011 }),
        usbFailure({ vendor_id: 0x0416, product_id: 0x5011 }),
      ],
    });
    expect(screen.getAllByText("Could not open USB device 0416:5011: permission denied (errno 13).")).toHaveLength(3);
  });

  // A scan tolerates USB failures rather than ending on them, so a completed
  // scan can carry both a banner and results.
  test("a tolerated failure does not suppress the results", () => {
    renderPanel({
      phase: "done",
      completed: 508,
      total: 508,
      printers: [networkPrinter("10.42.0.83")],
      failures: [usbFailure()],
    });

    expect(screen.getByText(/sudo escpost printers grant-usb-permissions/)).toBeTruthy();
    expect(screen.getByRole("button", { name: "Add 10.42.0.83:9100" })).toBeTruthy();
  });

  // "Nothing is out there" and "everything out there is already registered"
  // are different answers, and only the second one points at the inventory.
  test("distinguishes nothing discovered from everything already configured", () => {
    const { rerender } = renderPanel({
      phase: "done",
      completed: 508,
      total: 508,
      printers: [],
    });

    expect(screen.getByText("No printers discovered")).toBeTruthy();
    expect(screen.getByText("Checked USB · scanned 508 IP addresses")).toBeTruthy();

    rerender({
      phase: "done",
      completed: 508,
      total: 508,
      printers: [networkPrinter("10.42.0.71", ["kitchen"]), networkPrinter("10.42.0.72", ["bar", "counter"])],
    });

    expect(screen.getByText("No new printers")).toBeTruthy();
    expect(screen.getByText("All 2 discovered printers are already configured. They are listed below with live status.")).toBeTruthy();

    // One printer is not "all 1 printers are".
    rerender({
      phase: "done",
      completed: 508,
      total: 508,
      printers: [networkPrinter("10.42.0.71", ["kitchen"])],
    });

    expect(screen.getByText("The one printer discovered is already configured. It is listed below with live status.")).toBeTruthy();
  });

  test("hands the Add button the discovered printer its row was built from", () => {
    const printer = usbPrinter({ serial_number: "X9", out_endpoints: [0x01, 0x02] });
    const { onAdd } = renderPanel({ phase: "done", printers: [printer] });

    expect(screen.getByText("USB 0416:5011 · bus 003 addr 007 · serial X9 · interface 0 · out 0x01, 0x02")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Add POS-58 Printer" }));
    expect(onAdd).toHaveBeenCalledWith(printer);
  });

  // The flash marks an arrival, so it fires for a printer that streams in
  // while the panel is watching — and not for the ones that were already
  // there when it mounted, which is what returning to the page looks like.
  test("flashes an arriving printer once, and nothing on mount", async () => {
    jest.useFakeTimers();
    const first = networkPrinter("10.42.0.83");
    const { view, rerender } = renderPanel({ phase: "running", completed: 100, total: 508, printers: [first] });

    const flashed = () => Array.from(view.container.querySelectorAll(".printer-row-found"))
      .map((row) => row.textContent);
    expect(flashed()).toHaveLength(0);

    rerender({ phase: "running", completed: 200, total: 508, printers: [first, networkPrinter("10.42.0.84")] });
    expect(flashed()).toHaveLength(1);
    expect(flashed()[0]).toContain("10.42.0.84:9100");

    // A progress event alone must not re-flash a row that has settled.
    await act(async () => { jest.advanceTimersByTime(2_000); });
    expect(flashed()).toHaveLength(0);
    rerender({ phase: "running", completed: 300, total: 508, printers: [first, networkPrinter("10.42.0.84")] });
    expect(flashed()).toHaveLength(0);
  });

  // A dropped stream is indistinguishable from a cancel, so a failed scan
  // reports what it knows instead of claiming the world holds no printers.
  test("a failed scan reports the error rather than an empty result", () => {
    renderPanel({
      phase: "error",
      completed: 42,
      total: 508,
      printers: [],
      error: "The discovery stream ended unexpectedly.",
    });

    expect(screen.getByRole("alert").textContent).toContain("The discovery stream ended unexpectedly.");
    expect(gone(screen.queryByText("No printers discovered"))).toBe(true);
  });

  // The page decides when to show the panel, but an idle scan has nothing to
  // say in any case.
  test("renders nothing before a scan has started", () => {
    const { view } = renderPanel({});
    expect(view.container.textContent).toBe("");
  });
});
