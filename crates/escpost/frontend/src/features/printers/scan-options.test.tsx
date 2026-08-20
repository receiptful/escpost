import { afterEach, describe, expect, jest, test } from "bun:test";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/preact";
import type { DiscoveryNetworksResponse } from "../../api/types";
import { ScanOptions } from "./scan-options";

// Host counts that are neither equal nor derivable from the prefix, so a
// summed footer proves the panel adds up what the server reported — the
// server has already subtracted this machine's own addresses — rather than
// recomputing 254 from `/24`.
const twoNetworks: DiscoveryNetworksResponse = {
  networks: [
    { subnet: "10.42.0.0/24", interface: "enx0", hosts: 253 },
    { subnet: "192.168.1.0/24", interface: "wlp3s0", hosts: 254 },
  ],
  skipped: [],
  default_port: 9100,
  default_timeout_ms: 1000,
};

function json(body: unknown, status = 200) {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json" },
  });
}

function renderOptions(response: Response | DiscoveryNetworksResponse) {
  const onStart = jest.fn();
  const onClose = jest.fn();
  globalThis.fetch = (() => Promise.resolve(
    response instanceof Response ? response : json(response),
  )) as unknown as typeof globalThis.fetch;
  const view = render(<ScanOptions onStart={onStart} onClose={onClose} />);
  return { view, onStart, onClose };
}

function startButton() {
  return screen.getByRole("button", { name: "Start scan" });
}

afterEach(cleanup);

describe("ScanOptions", () => {
  test("sends no subnets when every known network is checked, so the scan runs in automatic mode", async () => {
    const { onStart } = renderOptions(twoNetworks);
    await screen.findByLabelText("10.42.0.0/24");
    expect(screen.getByText("507 probes")).toBeTruthy();

    fireEvent.click(startButton());
    expect(onStart).toHaveBeenCalledWith({ usb: true, network: true, subnets: [], port: 9100, timeoutMs: 1000 });
  });

  test("names the checked networks once the selection is partial, and Reset restores automatic mode", async () => {
    const { onStart } = renderOptions(twoNetworks);
    fireEvent.click(await screen.findByLabelText("192.168.1.0/24"));
    fireEvent.input(screen.getByLabelText("RAW TCP port"), { target: { value: "9101" } });
    fireEvent.input(screen.getByLabelText("Timeout per host"), { target: { value: "500" } });
    expect(screen.getByText("253 probes")).toBeTruthy();

    fireEvent.click(startButton());
    expect(onStart).toHaveBeenCalledWith({
      usb: true,
      network: true,
      subnets: ["10.42.0.0/24"],
      port: 9101,
      timeoutMs: 500,
    });

    fireEvent.click(screen.getByRole("button", { name: "Reset" }));
    expect(screen.getByText("507 probes")).toBeTruthy();
    fireEvent.click(startButton());
    expect(onStart).toHaveBeenLastCalledWith({ usb: true, network: true, subnets: [], port: 9100, timeoutMs: 1000 });
  });

  test("start is disabled when the network transport is checked with nothing selected", async () => {
    renderOptions(twoNetworks);
    fireEvent.click(await screen.findByLabelText("10.42.0.0/24"));
    fireEvent.click(screen.getByLabelText("192.168.1.0/24"));

    expect(screen.getByText("Select a known network or enter a custom one.")).toBeTruthy();
    expect(screen.getByText("No networks selected")).toBeTruthy();
    expect(startButton().hasAttribute("disabled")).toBe(true);
  });

  test("an unchecked network transport scans USB alone, and unchecking both leaves nothing to do", async () => {
    const { onStart } = renderOptions(twoNetworks);
    await screen.findByLabelText("10.42.0.0/24");
    fireEvent.click(screen.getByLabelText("Network"));

    expect(screen.getByText("USB only · no network probes")).toBeTruthy();
    expect((screen.getByLabelText("Custom network") as HTMLInputElement).disabled).toBe(true);
    fireEvent.click(startButton());
    expect(onStart).toHaveBeenCalledWith({ usb: true, network: false, subnets: [], port: 9100, timeoutMs: 1000 });

    fireEvent.click(screen.getByLabelText("USB"));
    expect(screen.getByText("Nothing to scan")).toBeTruthy();
    expect(startButton().hasAttribute("disabled")).toBe(true);
  });

  test("a custom network disables the known networks without removing them", async () => {
    const { onStart } = renderOptions(twoNetworks);
    fireEvent.input(await screen.findByLabelText("Custom network"), { target: { value: "10.0.5.0/24, 10.0.6.0/24" } });

    const known = screen.getByLabelText("10.42.0.0/24") as HTMLInputElement;
    expect(known.disabled).toBe(true);
    expect(known.isConnected).toBe(true);
    expect(known.checked).toBe(true);
    expect(screen.getByText("508 probes")).toBeTruthy();

    fireEvent.click(startButton());
    expect(onStart).toHaveBeenCalledWith({
      usb: true,
      network: true,
      subnets: ["10.0.5.0/24", "10.0.6.0/24"],
      port: 9100,
      timeoutMs: 1000,
    });
  });

  test("refuses a custom network the shared layer would refuse, in the shared layer's words", async () => {
    renderOptions(twoNetworks);
    const custom = await screen.findByLabelText("Custom network");

    fireEvent.input(custom, { target: { value: "enp5s0" } });
    expect(screen.getByText("Expected CIDR notation such as 10.42.0.0/24, found `enp5s0`.")).toBeTruthy();
    expect(startButton().hasAttribute("disabled")).toBe(true);

    fireEvent.input(custom, { target: { value: "10.0.0.0/8" } });
    expect(screen.getByText("Subnet 10.0.0.0/8 is too large to scan (at most /16).")).toBeTruthy();
    expect(startButton().hasAttribute("disabled")).toBe(true);

    fireEvent.input(custom, { target: { value: "10.0.0.0/16" } });
    expect(screen.getByText("65,534 probes")).toBeTruthy();
    expect(startButton().hasAttribute("disabled")).toBe(false);
  });

  test("a skipped adapter is listed with its reason and cannot be selected", async () => {
    renderOptions({
      networks: [],
      skipped: [{
        interface: "enp5s0",
        subnet: "10.0.0.0/16",
        reason: "too_large",
        description: "enp5s0 (10.0.0.0/16): larger than /24, scan it with --subnet 10.0.0.0/16",
      }],
      default_port: 9100,
      default_timeout_ms: 1000,
    });

    const skipped = await screen.findByLabelText("10.0.0.0/16");
    expect((skipped as HTMLInputElement).disabled).toBe(true);
    expect((skipped as HTMLInputElement).checked).toBe(false);
    expect(screen.getByText(/larger than \/24/)).toBeTruthy();
    expect(startButton().hasAttribute("disabled")).toBe(true);
  });

  test("keeps a skeleton network list until the networks response arrives", async () => {
    let resolveNetworks!: (response: Response) => void;
    globalThis.fetch = (() => new Promise<Response>((resolve) => { resolveNetworks = resolve; })) as unknown as typeof globalThis.fetch;
    const view = render(<ScanOptions onStart={jest.fn()} onClose={jest.fn()} />);

    expect(screen.getByLabelText("Detecting networks")).toBeTruthy();
    expect(view.container.querySelectorAll(".skeleton").length).toBeGreaterThan(0);
    expect(screen.getByText("Counting…")).toBeTruthy();
    expect(startButton().hasAttribute("disabled")).toBe(true);

    await act(async () => { resolveNetworks(json(twoNetworks)); });
    expect(await screen.findByLabelText("10.42.0.0/24")).toBeTruthy();
    expect(screen.queryByLabelText("Detecting networks")).toBeNull();
    expect(startButton().hasAttribute("disabled")).toBe(false);
  });

  test("reports a failed networks request and offers a retry that recovers", async () => {
    const responses = [
      json({ error: { code: "network_detection_failed", message: "Unable to detect this machine's networks." } }, 500),
      json(twoNetworks),
    ];
    globalThis.fetch = (() => Promise.resolve(responses.shift()!)) as unknown as typeof globalThis.fetch;
    render(<ScanOptions onStart={jest.fn()} onClose={jest.fn()} />);

    expect(await screen.findByText("Unable to detect this machine's networks.")).toBeTruthy();
    expect(screen.getByText("Networks unavailable")).toBeTruthy();
    expect(startButton().hasAttribute("disabled")).toBe(true);

    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(await screen.findByLabelText("10.42.0.0/24")).toBeTruthy();
    expect(startButton().hasAttribute("disabled")).toBe(false);
  });

  test("closes from the close button and from Escape", async () => {
    const { onClose } = renderOptions(twoNetworks);
    await screen.findByLabelText("10.42.0.0/24");

    fireEvent.click(screen.getByRole("button", { name: "Close scan options" }));
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(2);
  });
});
