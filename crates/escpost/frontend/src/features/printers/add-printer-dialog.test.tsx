import { afterEach, describe, expect, jest, test } from "bun:test";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/preact";
import type { AddPrinterBody, DiscoveredPrinter, UsbConnection } from "../../api/types";
import { AppDataProvider } from "../../app/data";
import { PrinterInventoryProvider } from "../../app/printer-inventory-data";
import { ServerStatusProvider } from "../../app/server-status-data";
import { AddPrinterDialog } from "./add-printer-dialog";

const status = { virtual_printer: null, jobs_processed: 0, config_path: "/home/dev/.config/escpost/printers.toml" };
const originalEventSource = globalThis.EventSource;

class FakeEventSource {
  static instances: FakeEventSource[] = [];
  static instance: FakeEventSource | null = null;
  private readonly listeners = new Map<string, ((event: Event) => void)[]>();

  constructor(readonly url: string) {
    FakeEventSource.instance = this;
    FakeEventSource.instances.push(this);
  }

  addEventListener(name: string, handler: (event: Event) => void) {
    this.listeners.set(name, [...(this.listeners.get(name) ?? []), handler]);
  }

  close() {}

  emit(name: string, data: unknown) {
    for (const handler of this.listeners.get(name) ?? []) {
      handler(new MessageEvent(name, { data: JSON.stringify(data) }));
    }
  }

  static forUrl(url: string) { return FakeEventSource.instances.find((source) => source.url === url); }
}

function json(body: unknown, status = 200) {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json" },
  });
}

function configured(name: string) {
  return {
    name,
    transport: "network",
    availability: "connected",
    profile: null,
    connection: { type: "network", host: "10.42.0.71", port: 9100 },
  };
}

function catalogued(id: string) {
  return {
    id,
    vendor: "Epson",
    model: id,
    source: "calibrated",
    paper_width_mm: 80,
    printable_width_mm: 72,
    printable_width_dots: 576,
    dpi_x: 203,
    dpi_y: 203,
    full_cut: true,
    partial_cut: true,
    barcode_function_a: true,
    barcode_function_b: true,
    qr_code: true,
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
      serial_number: "X9",
      interface_number: 0,
      out_endpoints: [0x01],
      in_endpoints: [],
      ...overrides,
    },
  };
}

function networkPrinter(): DiscoveredPrinter {
  return {
    transport: "network",
    configured_names: [],
    configured_profile: null,
    interface: "enx0",
    connection: { type: "network", host: "10.42.0.90", port: 9100 },
  };
}

// Renders the dialog inside the application data provider it reads the
// configured names and the profile catalog from, and records what it posts.
// `add` overrides the registration response, which otherwise echoes the
// posted name back the way the endpoint does.
// Absence as a boolean. `expect(node).toBeNull()` prints the entire happy-dom
// node graph when it fails — tens of megabytes for a node still attached to
// the page — which buries every other failure in the run.
function gone(element: Element | null) {
  return element === null;
}

function renderDialog(printer: DiscoveredPrinter | null, options: {
  printers?: string[];
  profiles?: string[];
  add?: (body: AddPrinterBody) => Response;
  configPath?: string;
} = {}) {
  const posted: AddPrinterBody[] = [];
  const onClose = jest.fn();
  const onAdded = jest.fn();
  FakeEventSource.instance = null;
  FakeEventSource.instances = [];
  globalThis.EventSource = FakeEventSource as unknown as typeof EventSource;
  globalThis.fetch = ((input: RequestInfo | URL, init?: RequestInit) => {
    const path = String(input);
    if (path === "/api/profiles/list") {
      return Promise.resolve(json({ profiles: (options.profiles ?? []).map(catalogued) }));
    }
    if (path === "/api/printers/add") {
      const body = JSON.parse(String(init?.body)) as AddPrinterBody;
      posted.push(body);
      return Promise.resolve(options.add
        ? options.add(body)
        : json({ name: body.name, transport: body.connection.type, profile: body.profile, warnings: [] }, 201));
    }
    return Promise.reject(new Error(`unexpected request to ${path}`));
  }) as typeof globalThis.fetch;
  const view = render(
    <ServerStatusProvider>
      <PrinterInventoryProvider><AppDataProvider>
        <AddPrinterDialog printer={printer} onClose={onClose} onAdded={onAdded} />
      </AppDataProvider></PrinterInventoryProvider>
    </ServerStatusProvider>,
  );
  act(() => FakeEventSource.forUrl("/api/status/events")?.emit("message", {
    ...status,
    config_path: options.configPath ?? status.config_path,
  }));
  act(() => FakeEventSource.forUrl("/api/printers/list/events")?.emit("message", {
    updated_at: "2026-08-26T14:32:10Z", warning: null, printers: (options.printers ?? []).map(configured),
  }));
  // Hands the open dialog another device, which is the usage the owner is not
  // supposed to have but which must not silently register the wrong route.
  const hand = (next: DiscoveredPrinter | null) => view.rerender(
    <ServerStatusProvider>
      <PrinterInventoryProvider><AppDataProvider>
        <AddPrinterDialog printer={next} onClose={onClose} onAdded={onAdded} />
      </AppDataProvider></PrinterInventoryProvider>
    </ServerStatusProvider>,
  );
  return { view, hand, posted, onClose, onAdded };
}

function addButton() {
  return screen.getByRole("button", { name: "Add printer" });
}

afterEach(() => {
  cleanup();
  globalThis.EventSource = originalEventSource;
});

describe("AddPrinterDialog", () => {
  test("refuses a name the configuration already holds, in the CLI's own words", async () => {
    const { posted } = renderDialog(null, { printers: ["kitchen"] });
    fireEvent.input(screen.getByLabelText("Name"), { target: { value: "kitchen" } });
    fireEvent.input(screen.getByLabelText("Host"), { target: { value: "10.0.5.20" } });

    expect(await screen.findByText('printer "kitchen" is already configured')).toBeTruthy();
    expect(addButton().hasAttribute("disabled")).toBe(true);
    expect(posted).toHaveLength(0);
  });

  // `configuration::add_network_printer` refuses a colliding name with
  // `document.contains_key(name)` — an exact match on the TOML key — so
  // `escpost printers add Kitchen` succeeds on a machine that already has
  // `kitchen`. Folding case here would refuse a name the terminal accepts,
  // which is the one thing the two interfaces may never do.
  test("accepts a name that differs only in case, because the configuration does", async () => {
    const { posted, onAdded } = renderDialog(null, { printers: ["kitchen"] });
    fireEvent.input(screen.getByLabelText("Name"), { target: { value: "kitchen" } });
    fireEvent.input(screen.getByLabelText("Host"), { target: { value: "10.0.5.20" } });
    // Waiting on the refusal is also what proves the inventory has arrived,
    // so the acceptance below is a decision rather than an unloaded list.
    await screen.findByText('printer "kitchen" is already configured');

    fireEvent.input(screen.getByLabelText("Name"), { target: { value: "Kitchen" } });
    expect(gone(screen.queryByText(/already configured/))).toBe(true);
    fireEvent.click(addButton());

    // The connection travels with the name because the owner cannot
    // reconstruct it: for a manual registration this host and port were typed
    // here, and a scan still listing that endpoint as new has to be told.
    await waitFor(() => expect(onAdded).toHaveBeenCalledWith("Kitchen", { type: "network", host: "10.0.5.20", port: 9100 }));
    expect(posted[0]).toEqual({
      name: "Kitchen",
      profile: null,
      connection: { type: "network", host: "10.0.5.20", port: 9100 },
    });
  });

  // Why adding exists at all. Printing goes through the configured list, and
  // nothing else in this dialog says so — a reader who has just found a
  // printer on the network could otherwise reasonably expect to print to it.
  // The wording never mentions discovery, so the manual dialog says the same
  // thing for the same reason.
  test("both dialogs explain that printing needs a configured printer, and name the file", async () => {
    renderDialog(usbPrinter());
    // Awaited through the path itself: the sentence renders before the status
    // response lands, so finding the sentence proves nothing about the path.
    await screen.findByText("/home/dev/.config/escpost/printers.toml");
    const explanation = screen.getByText(/^You can only print to printers/);
    expect(explanation.textContent).toBe(
      "You can only print to printers you have added to your list of configured printers, stored in /home/dev/.config/escpost/printers.toml.",
    );
    // The path in the same monospace the Overview page gives it.
    expect(explanation.querySelector(".font-mono")?.textContent).toBe("/home/dev/.config/escpost/printers.toml");

    cleanup();
    renderDialog(null);
    await screen.findByText("/home/dev/.config/escpost/printers.toml");
    expect(screen.getByText(/^You can only print to printers/).textContent).toBe(
      "You can only print to printers you have added to your list of configured printers, stored in /home/dev/.config/escpost/printers.toml.",
    );
  });

  // `config_path` is empty when the configuration cannot be resolved, which
  // is deliberate — a config problem must not present as "server down" — so
  // the clause that would name it goes rather than dangling.
  test("an unresolvable configuration drops the clause naming the file", async () => {
    const { view } = renderDialog(null, { configPath: "" });

    const explanation = screen.getByText(/^You can only print to printers/);
    expect(explanation.textContent).toBe(
      "You can only print to printers you have added to your list of configured printers.",
    );
    expect(gone(explanation.querySelector(".font-mono"))).toBe(true);
    expect(view.container.textContent).not.toContain("stored in");
  });

  // The command reference is gone: nothing else in the browser teaches CLI
  // usage, and the line above already says what a name is for.
  test("the name hint carries the constraint and nothing else", async () => {
    const { view } = renderDialog(null);
    await screen.findByText("/home/dev/.config/escpost/printers.toml");

    expect(screen.getByText("(must be unique)")).toBeTruthy();
    expect(view.container.textContent).not.toContain("escpost print");
  });

  test("the manual dialog is IP-only, defaults to port 9100, and refuses a port the shared layer cannot take", async () => {
    const { view, posted, onClose } = renderDialog(null, { printers: [] });

    // `Add IP printer`, matching the button that opens it. The transport it
    // registers is still `network` on the wire and in the inventory column —
    // only what the reader is told to call it changed.
    expect(screen.getByRole("heading", { name: "Add IP printer" })).toBeTruthy();
    expect(gone(screen.queryByLabelText("OUT endpoint"))).toBe(true);
    expect(view.container.textContent).not.toContain("USB");
    expect((screen.getByLabelText("Port") as HTMLInputElement).value).toBe("9100");

    fireEvent.input(screen.getByLabelText("Name"), { target: { value: "warehouse" } });
    fireEvent.input(screen.getByLabelText("Host"), { target: { value: "10.0.5.20" } });
    fireEvent.input(screen.getByLabelText("Port"), { target: { value: "0" } });
    expect(screen.getByText("Enter a port between 1 and 65535.")).toBeTruthy();
    expect(addButton().hasAttribute("disabled")).toBe(true);

    // `max` binds the spinner, not typed or pasted text, so the upper bound
    // has to be the dialog's own — otherwise the value reaches serde, which
    // answers with a generic body rejection instead of this sentence.
    fireEvent.input(screen.getByLabelText("Port"), { target: { value: "70000" } });
    expect(screen.getByText("Enter a port between 1 and 65535.")).toBeTruthy();
    expect(addButton().hasAttribute("disabled")).toBe(true);

    fireEvent.input(screen.getByLabelText("Port"), { target: { value: "9101" } });
    fireEvent.click(addButton());
    await waitFor(() => expect(posted).toHaveLength(1));
    expect(posted[0]).toEqual({
      name: "warehouse",
      profile: null,
      connection: { type: "network", host: "10.0.5.20", port: 9101 },
    });

    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onClose).toHaveBeenCalled();
  });

  test("a USB device offering one route shows both endpoint selects disabled rather than hiding them", async () => {
    renderDialog(usbPrinter({ out_endpoints: [0x01], in_endpoints: [] }), { printers: [] });

    const out = screen.getByLabelText("OUT endpoint") as HTMLSelectElement;
    const inbound = screen.getByLabelText("IN endpoint") as HTMLSelectElement;
    expect(out.disabled).toBe(true);
    expect(out.value).toBe("0x01");
    expect(inbound.disabled).toBe(true);
    expect(inbound.value).toBe("");
  });

  // The endpoint defaults are the ones `usb_add_targets` resolves for the
  // terminal: every bulk OUT endpoint is an explicit choice, and an IN
  // endpoint is only assumed when the device exposes exactly one.
  test("a USB device offering several routes lets the browser choose the one the terminal would ask for", async () => {
    const { posted } = renderDialog(
      usbPrinter({ out_endpoints: [0x01, 0x02], in_endpoints: [0x81] }),
      { printers: [], profiles: ["TM-T88V"] },
    );

    const out = screen.getByLabelText("OUT endpoint") as HTMLSelectElement;
    const inbound = screen.getByLabelText("IN endpoint") as HTMLSelectElement;
    expect(out.disabled).toBe(false);
    expect(inbound.disabled).toBe(false);
    expect(inbound.value).toBe("0x81");

    fireEvent.input(screen.getByLabelText("Name"), { target: { value: "counter" } });
    fireEvent.change(out, { target: { value: "0x02" } });
    fireEvent.change(inbound, { target: { value: "" } });
    await screen.findByRole("option", { name: "TM-T88V" });
    fireEvent.change(screen.getByLabelText("Profile"), { target: { value: "TM-T88V" } });
    fireEvent.click(addButton());

    await waitFor(() => expect(posted).toHaveLength(1));
    expect(posted[0]).toEqual({
      name: "counter",
      profile: "TM-T88V",
      connection: {
        type: "usb",
        vendor_id: 0x0416,
        product_id: 0x5011,
        serial_number: "X9",
        interface_number: 0,
        out_endpoint: 0x02,
        in_endpoint: null,
      },
    });
  });

  // `usb_add_targets` assumes an IN endpoint only when the device exposes
  // exactly one, because "several IN endpoints must not be reduced to an
  // arbitrary guess". Pre-selecting one here would make the browser guess
  // where the terminal offers None.
  test("leaves the IN endpoint unchosen when the device exposes several", async () => {
    renderDialog(usbPrinter({ out_endpoints: [0x01], in_endpoints: [0x81, 0x82] }), { printers: [] });

    const inbound = screen.getByLabelText("IN endpoint") as HTMLSelectElement;
    expect(inbound.value).toBe("");
    expect(inbound.disabled).toBe(false);
  });

  // The connection facts come from the prop and the route from state, so a
  // dialog handed another device must re-seed the route rather than submit
  // the previous device's — an endpoint inside `0x01..=0x0f` that the new
  // device does not expose is saved by every layer without complaint and
  // simply never prints.
  test("re-seeds the route when it is handed a different device", async () => {
    const { hand, posted } = renderDialog(null, { printers: [] });
    fireEvent.input(screen.getByLabelText("Name"), { target: { value: "counter" } });
    fireEvent.input(screen.getByLabelText("Host"), { target: { value: "10.0.5.20" } });

    // Manual to USB: without a re-seed there is no route at all, and Add
    // stays disabled with nothing on screen to explain it.
    hand(usbPrinter({ out_endpoints: [0x01, 0x02], in_endpoints: [0x81] }));
    const out = screen.getByLabelText("OUT endpoint") as HTMLSelectElement;
    expect(out.value).toBe("0x01");
    expect(addButton().hasAttribute("disabled")).toBe(false);
    fireEvent.change(out, { target: { value: "0x02" } });

    // USB to a USB device that does not expose the chosen route.
    hand(usbPrinter({ serial_number: "Z1", out_endpoints: [0x01], in_endpoints: [] }));
    expect((screen.getByLabelText("OUT endpoint") as HTMLSelectElement).value).toBe("0x01");
    expect((screen.getByLabelText("IN endpoint") as HTMLSelectElement).value).toBe("");

    fireEvent.click(addButton());
    await waitFor(() => expect(posted).toHaveLength(1));
    expect(posted[0]?.connection).toEqual({
      type: "usb",
      vendor_id: 0x0416,
      product_id: 0x5011,
      serial_number: "Z1",
      interface_number: 0,
      out_endpoint: 0x01,
      in_endpoint: null,
    });
  });

  // `Request::new` refuses only a name that is nothing but whitespace, and
  // `contains_key` looks the raw string up, so `printers add "kitchen "` is a
  // distinct printer. Trimming here would register a different one than the
  // terminal does.
  test("posts the name exactly as typed and refuses one that is only whitespace", async () => {
    const { posted } = renderDialog(null, { printers: [] });
    fireEvent.input(screen.getByLabelText("Name"), { target: { value: "   " } });
    fireEvent.input(screen.getByLabelText("Host"), { target: { value: "10.0.5.20" } });
    expect(addButton().hasAttribute("disabled")).toBe(true);

    fireEvent.input(screen.getByLabelText("Name"), { target: { value: "kitchen " } });
    expect(addButton().hasAttribute("disabled")).toBe(false);
    fireEvent.input(screen.getByLabelText("Host"), { target: { value: "  " } });
    expect(addButton().hasAttribute("disabled")).toBe(true);

    fireEvent.input(screen.getByLabelText("Host"), { target: { value: "10.0.5.20" } });
    fireEvent.click(addButton());
    await waitFor(() => expect(posted).toHaveLength(1));
    expect(posted[0]?.name).toBe("kitchen ");
  });

  // Advisory, not a refusal: the CLI registers the same device and prints
  // the same sentence afterwards.
  test("a USB device without a serial number carries the shared ambiguity warning", async () => {
    renderDialog(usbPrinter({ serial_number: null }), { printers: [] });

    expect(screen.getByText(
      "This printer reports no serial number. Printing will be ambiguous while another device with the same USB identity is connected.",
    )).toBeTruthy();
    fireEvent.input(screen.getByLabelText("Name"), { target: { value: "pos-58" } });
    expect(addButton().hasAttribute("disabled")).toBe(false);
  });

  test("a discovered network printer registers the endpoint it answered on, read-only", async () => {
    const { posted } = renderDialog(networkPrinter(), { printers: [] });

    expect(gone(screen.queryByLabelText("Host"))).toBe(true);
    expect(screen.getByText(/10\.42\.0\.90:9100/)).toBeTruthy();

    fireEvent.input(screen.getByLabelText("Name"), { target: { value: "back-office" } });
    fireEvent.click(addButton());
    await waitFor(() => expect(posted).toHaveLength(1));
    expect(posted[0]).toEqual({
      name: "back-office",
      profile: null,
      connection: { type: "network", host: "10.42.0.90", port: 9100 },
    });
  });

  // The inline check is a convenience, never the authority: another tab can
  // register the name between the keystroke and the click, and the server
  // still answers 409. The dialog has to survive that with the name still on
  // screen to edit.
  test("keeps the dialog open on a collision the inventory did not know about", async () => {
    const { onAdded, onClose } = renderDialog(null, {
      printers: [],
      add: () => json(
        { error: { code: "printer_already_configured", message: 'printer "kitchen" is already configured' } },
        409,
      ),
    });
    fireEvent.input(screen.getByLabelText("Name"), { target: { value: "kitchen" } });
    fireEvent.input(screen.getByLabelText("Host"), { target: { value: "10.0.5.20" } });
    fireEvent.click(addButton());

    expect((await screen.findByRole("alert")).textContent).toBe('printer "kitchen" is already configured');
    expect((screen.getByLabelText("Name") as HTMLInputElement).value).toBe("kitchen");
    expect(addButton().hasAttribute("disabled")).toBe(false);
    expect(onAdded).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
  });
});
