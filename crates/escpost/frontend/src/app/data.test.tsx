import { afterEach, describe, expect, jest, test } from "bun:test";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/preact";
import { AppDataProvider, useAppData } from "./data";

class FakeEventSource {
  static instances: FakeEventSource[] = [];
  closed = false;
  private readonly listeners = new Map<string, ((event: Event) => void)[]>();
  constructor(readonly url: string) { FakeEventSource.instances.push(this); }
  addEventListener(name: string, handler: (event: Event) => void) { this.listeners.set(name, [...(this.listeners.get(name) ?? []), handler]); }
  close() { this.closed = true; }
  emit(name: string, data: unknown) {
    for (const handler of this.listeners.get(name) ?? []) handler(new MessageEvent(name, { data: JSON.stringify(data) }));
  }
}

const originalEventSource = globalThis.EventSource;
const originalFetch = globalThis.fetch;
const query = { usb: true, network: true, subnets: [], port: 9100, timeoutMs: 1000 };

function Probe() {
  const { ensureProfiles, profiles, startScan, scan, markScanResultConfigured } = useAppData();
  return <>
    <button type="button" onClick={() => void ensureProfiles()}>Profiles</button>
    <button type="button" onClick={() => startScan(query)}>Scan</button>
    <button type="button" onClick={() => markScanResultConfigured("Kitchen", { type: "network", host: "10.0.0.8", port: 9100 })}>Configure network</button>
    <button type="button" onClick={() => markScanResultConfigured("USB one", { type: "usb", vendor_id: 1046, product_id: 20497, serial_number: null, interface_number: 0, out_endpoint: 1, in_endpoint: null })}>Configure USB</button>
    <p>{`${profiles.phase}:${profiles.data?.profiles.length ?? "none"}`}</p>
    <p>{`${scan.phase}:${scan.printers.length}:${scan.failures.map((failure) => failure.product_id).join(",")}`}</p>
    <p data-testid="configured">{scan.printers.map((printer) => printer.configured_names.join(",")).join(";")}</p>
  </>;
}

function renderProvider() {
  FakeEventSource.instances = [];
  globalThis.EventSource = FakeEventSource as unknown as typeof EventSource;
  return render(<AppDataProvider><Probe /></AppDataProvider>);
}

afterEach(() => { cleanup(); globalThis.EventSource = originalEventSource; globalThis.fetch = originalFetch; });

describe("AppDataProvider", () => {
  test("loads the profile catalog only when a consumer asks for it", async () => {
    const fetch = jest.fn((_input: RequestInfo | URL) => Promise.resolve(new Response(JSON.stringify({ profiles: [] }), { headers: { "content-type": "application/json" } })));
    globalThis.fetch = fetch as unknown as typeof globalThis.fetch;
    renderProvider();
    expect(fetch).not.toHaveBeenCalled();
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Profiles" }));
      for (let turn = 0; turn < 6; turn += 1) await Promise.resolve();
    });
    expect(fetch.mock.calls.map(([input]) => String(input))).toEqual(["/api/profiles/list"]);
    expect(screen.getByText("ready:0")).toBeTruthy();
  });

  test("keeps discovery stream ownership separate from inventory and preserves ordered USB failures", () => {
    const fetch = jest.fn((_input: RequestInfo | URL) => Promise.reject(new Error("unexpected request")));
    globalThis.fetch = fetch as unknown as typeof globalThis.fetch;
    renderProvider();
    act(() => { fireEvent.click(screen.getByRole("button", { name: "Scan" })); });
    const first = FakeEventSource.instances[0]!;
    act(() => first.emit("usb_failure", { vendor_id: 1046, product_id: 2, stage: "open_device", reason: "denied", permission_denied: true, can_grant_usb_permissions: true }));
    act(() => first.emit("usb_failure", { vendor_id: 1046, product_id: 3, stage: "open_device", reason: "denied", permission_denied: true, can_grant_usb_permissions: true }));
    expect(screen.getByText("running:0:2,3")).toBeTruthy();
    expect(fetch.mock.calls.map(([input]) => String(input))).not.toContain("/api/printers/list");
    act(() => { fireEvent.click(screen.getByRole("button", { name: "Scan" })); });
    expect(first.closed).toBe(true);
    expect(FakeEventSource.instances).toHaveLength(2);
  });

  test("marks only matching network and one ambiguous USB discovery result configured", () => {
    renderProvider();
    act(() => { fireEvent.click(screen.getByRole("button", { name: "Scan" })); });
    const source = FakeEventSource.instances[0]!;
    const usb = (host: string) => ({
      transport: "usb", configured_names: [], configured_profile: null,
      connection: { type: "usb", vendor_id: 1046, product_id: 20497, bus: "003", address: 7, manufacturer: null, product: host, serial_number: null, interface_number: 0, out_endpoints: [1], in_endpoints: [] },
    });
    act(() => source.emit("printer", { transport: "network", configured_names: [], configured_profile: null, connection: { type: "network", host: "10.0.0.8", port: 9100 } }));
    act(() => source.emit("printer", usb("first")));
    act(() => source.emit("printer", usb("second")));
    fireEvent.click(screen.getByRole("button", { name: "Configure network" }));
    fireEvent.click(screen.getByRole("button", { name: "Configure USB" }));
    expect(screen.getByTestId("configured").textContent).toBe("Kitchen;USB one;");
  });
});
