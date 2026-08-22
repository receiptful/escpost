import { afterEach, describe, expect, jest, test } from "bun:test";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/preact";
import type { DiscoveryQuery } from "../../api/discovery-stream";
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

// The scope of a session where no scan has been configured yet, which is what
// the provider hands the panel until one has: the CLI's no-flag behaviour.
const noScanYet: DiscoveryQuery = { usb: true, network: true, subnets: [] };

// Collapsed is how the panel sits on an idle page, so every test about the
// form itself opens the disclosure first and only the ones about the
// disclosure render it shut.
function renderOptions(response: Response | DiscoveryNetworksResponse, query: DiscoveryQuery = noScanYet, expanded = true) {
  const onStart = jest.fn();
  const onScopeChange = jest.fn();
  globalThis.fetch = (() => Promise.resolve(
    response instanceof Response ? response : json(response),
  )) as unknown as typeof globalThis.fetch;
  const view = render(<ScanOptions query={query} onStart={onStart} onScopeChange={onScopeChange} />);
  if (expanded) {
    fireEvent.click(disclosure());
  }
  return { view, onStart, onScopeChange };
}

function disclosure() {
  return screen.getByRole("button", { name: "Scan options" });
}

// The collapsed line, read through the element the disclosure describes
// itself with. By id rather than by text, because the footer states some of
// the same phrases and this is about the one that survives the form being
// shut.
function statedScope() {
  return document.getElementById("scan-options-scope")?.textContent;
}

function startButton() {
  return screen.getByRole("button", { name: "Start scan" });
}

// Absence as a boolean. `expect(node).toBeNull()` prints the entire happy-dom
// node graph when it fails — tens of megabytes for a node still attached to
// the page — which buries every other failure in the run.
function gone(element: Element | null) {
  return element === null;
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

    // Reset re-detects rather than restoring a remembered list: an adapter
    // appears or vanishes with a cable, and this is the only way back from a
    // failed detection. Reopened first, since starting a scan shuts the form.
    fireEvent.click(disclosure());
    fireEvent.click(screen.getByRole("button", { name: "Reset" }));
    expect(screen.getByLabelText("Detecting networks")).toBeTruthy();
    expect(await screen.findByText("507 probes")).toBeTruthy();
    fireEvent.click(startButton());
    expect(onStart).toHaveBeenLastCalledWith({ usb: true, network: true, subnets: [], port: 9100, timeoutMs: 1000 });
  });

  // The panel opens on the scope the last scan ran with, so that what it
  // shows and what `Discover printers` would send are the same thing.
  test("opens on the recorded scope, port and timeout included", async () => {
    const { onStart } = renderOptions(twoNetworks, {
      usb: false,
      network: true,
      subnets: ["192.168.1.0/24"],
      port: 9101,
      timeoutMs: 500,
    });
    await screen.findByLabelText("10.42.0.0/24");

    expect((screen.getByLabelText("USB") as HTMLInputElement).checked).toBe(false);
    expect((screen.getByLabelText("10.42.0.0/24") as HTMLInputElement).checked).toBe(false);
    expect((screen.getByLabelText("192.168.1.0/24") as HTMLInputElement).checked).toBe(true);
    expect((screen.getByLabelText("RAW TCP port") as HTMLInputElement).value).toBe("9101");
    expect((screen.getByLabelText("Timeout per host") as HTMLInputElement).value).toBe("500");
    expect(screen.getByText("254 probes")).toBeTruthy();

    // Reopened and started untouched, it repeats the scan it was showing.
    fireEvent.click(startButton());
    expect(onStart).toHaveBeenCalledWith({
      usb: false,
      network: true,
      subnets: ["192.168.1.0/24"],
      port: 9101,
      timeoutMs: 500,
    });
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

    // Reopened, since starting a scan shuts the form.
    fireEvent.click(disclosure());
    fireEvent.click(screen.getByLabelText("USB"));
    expect(statedScope()).toBe("Nothing to scan");
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

  // A field holding only separators is content, not emptiness. Reading it as
  // empty would hand the query back to the known networks — including ones
  // the user unchecked — under a footer promising nothing.
  test("a custom field holding only separators is invalid, never a silent automatic scan", async () => {
    const { onStart } = renderOptions(twoNetworks);
    fireEvent.input(await screen.findByLabelText("Custom network"), { target: { value: ", ," } });

    expect(screen.getByText("Expected CIDR notation such as 10.42.0.0/24, found `, ,`.")).toBeTruthy();
    expect((screen.getByLabelText("10.42.0.0/24") as HTMLInputElement).disabled).toBe(true);
    expect(screen.getByText("No networks selected")).toBeTruthy();
    expect(startButton().hasAttribute("disabled")).toBe(true);
    expect(onStart).not.toHaveBeenCalled();
  });

  // One refused entry invalidates the whole field, so the footer states no
  // count at all rather than a total for the entries that survived. A number
  // beside a refusal would promise work the panel is refusing to do.
  test("states no probe count while any custom entry is refused", async () => {
    const { view } = renderOptions(twoNetworks);
    fireEvent.input(await screen.findByLabelText("Custom network"), { target: { value: "10.0.0.0/8, 10.0.5.0/24" } });

    expect(screen.getByText("Subnet 10.0.0.0/8 is too large to scan (at most /16).")).toBeTruthy();
    expect(screen.getByText("No networks selected")).toBeTruthy();
    expect(view.container.textContent).not.toContain("probes");
    expect(startButton().hasAttribute("disabled")).toBe(true);
  });

  // The port is a `NonZeroU16` and the timeout a `u64` of milliseconds in the
  // shared layer, so the panel accepts exactly that — `--timeout 0` included,
  // since inventing a stricter rule here is its own kind of divergence.
  test("refuses a port or timeout the shared layer cannot take, and accepts the whole range it can", async () => {
    const { onStart } = renderOptions(twoNetworks);
    const port = await screen.findByLabelText("RAW TCP port");
    const timeout = screen.getByLabelText("Timeout per host");

    fireEvent.input(port, { target: { value: "0" } });
    expect(screen.getByText("Enter a port between 1 and 65535.")).toBeTruthy();
    expect(startButton().hasAttribute("disabled")).toBe(true);

    fireEvent.input(port, { target: { value: "65536" } });
    expect(screen.getByText("Enter a port between 1 and 65535.")).toBeTruthy();
    expect(startButton().hasAttribute("disabled")).toBe(true);

    fireEvent.input(port, { target: { value: "65535" } });
    expect(screen.queryByText("Enter a port between 1 and 65535.")).toBeNull();
    expect(startButton().hasAttribute("disabled")).toBe(false);

    // Past `MAX_SAFE_INTEGER` a JavaScript number stringifies as `1e+21`,
    // which no `u64` parses — a wire limit, not a product rule.
    fireEvent.input(timeout, { target: { value: "999999999999999999999" } });
    expect(screen.getByText("Enter a timeout as a whole number of milliseconds.")).toBeTruthy();
    expect(startButton().hasAttribute("disabled")).toBe(true);

    fireEvent.input(timeout, { target: { value: "0" } });
    expect(screen.queryByText("Enter a timeout as a whole number of milliseconds.")).toBeNull();
    // The field's own constraint has to agree, or the browser paints it
    // out of range while the panel accepts it.
    expect(timeout.getAttribute("min")).toBe("0");
    fireEvent.click(startButton());
    expect(onStart).toHaveBeenCalledWith({ usb: true, network: true, subnets: [], port: 65535, timeoutMs: 0 });
  });

  // Start only vets the port and timeout while Network is checked, so an
  // unchecked Network can reach `onStart` with whatever text the disabled
  // fields hold. `DiscoveryQuery` says those are numbers, and that has to be
  // true where the query is built — not because the reader discards them.
  test("substitutes the server's defaults rather than emitting a port or timeout that is not a number", async () => {
    const { onStart } = renderOptions(twoNetworks);
    fireEvent.input(await screen.findByLabelText("RAW TCP port"), { target: { value: "" } });
    fireEvent.input(screen.getByLabelText("Timeout per host"), { target: { value: "" } });
    expect(startButton().hasAttribute("disabled")).toBe(true);

    fireEvent.click(screen.getByLabelText("Network"));
    fireEvent.click(startButton());
    expect(onStart).toHaveBeenCalledWith({ usb: true, network: false, subnets: [], port: 9100, timeoutMs: 1000 });
  });

  // The reason is the shared layer's; the remedy is this panel's. The
  // terminal answers the same omission with `--subnet`, which is useless
  // advice in a browser with a custom-network field two rows below.
  test("a skipped adapter is listed with the server's reason, this panel's remedy, and no CLI flag", async () => {
    const { view } = renderOptions({
      networks: [],
      skipped: [
        { interface: "enp5s0", subnet: "10.0.0.0/16", reason: "too_large", description: "enp5s0 (10.0.0.0/16): larger than /24" },
        // `detect_networks` reports one entry per address, so a second
        // too-large address on the same adapter arrives as a second row.
        { interface: "enp5s0", subnet: "172.16.0.0/12", reason: "too_large", description: "enp5s0 (172.16.0.0/12): larger than /24" },
        { interface: "weird0", subnet: null, reason: "unusable_netmask", description: "weird0: its netmask does not name a scannable subnet" },
      ],
      default_port: 9100,
      default_timeout_ms: 1000,
    });

    const skipped = await screen.findByLabelText("10.0.0.0/16") as HTMLInputElement;
    expect(skipped.disabled).toBe(true);
    expect(skipped.checked).toBe(false);
    // Two rows from one adapter must not share a DOM id, or the second
    // row's label points at the first row's checkbox.
    const sameAdapter = screen.getByLabelText("172.16.0.0/12") as HTMLInputElement;
    expect(sameAdapter).not.toBe(skipped);
    expect(sameAdapter.id).not.toBe(skipped.id);
    expect(screen.getByText("enp5s0 (10.0.0.0/16): larger than /24, add it as a custom network")).toBeTruthy();
    expect(view.container.textContent).not.toContain("--subnet");
    // No subnet to retype, so no remedy is offered.
    expect(screen.getByText("weird0: its netmask does not name a scannable subnet")).toBeTruthy();
    expect((screen.getByLabelText("weird0") as HTMLInputElement).disabled).toBe(true);
    expect(startButton().hasAttribute("disabled")).toBe(true);
  });

  test("keeps a skeleton network list until the networks response arrives", async () => {
    let resolveNetworks!: (response: Response) => void;
    globalThis.fetch = (() => new Promise<Response>((resolve) => { resolveNetworks = resolve; })) as unknown as typeof globalThis.fetch;
    const view = render(<ScanOptions query={noScanYet} onStart={jest.fn()} onScopeChange={jest.fn()} />);
    fireEvent.click(disclosure());

    // The collapsed line says as much as is known, which until the response
    // lands is that the networks are still being counted.
    expect(screen.getByText("USB · counting networks…")).toBeTruthy();
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
    render(<ScanOptions query={noScanYet} onStart={jest.fn()} onScopeChange={jest.fn()} />);
    fireEvent.click(disclosure());

    expect(await screen.findByText("Unable to detect this machine's networks.")).toBeTruthy();
    expect(screen.getByText("Networks unavailable")).toBeTruthy();
    // A panel that cannot name a scope states that where the scope goes,
    // rather than leaving the last thing it knew standing.
    expect(screen.getByText("USB · networks unavailable")).toBeTruthy();
    expect(startButton().hasAttribute("disabled")).toBe(true);

    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(await screen.findByLabelText("10.42.0.0/24")).toBeTruthy();
    expect(startButton().hasAttribute("disabled")).toBe(false);
  });

  // The panel lives in the page now rather than over it, so an idle page is
  // one row: the form is behind a disclosure and nothing about it overlays
  // the results below.
  test("renders collapsed, and the disclosure it announces is the form", async () => {
    renderOptions(twoNetworks, noScanYet, false);
    const button = disclosure();

    expect(button.getAttribute("aria-expanded")).toBe("false");
    expect(gone(screen.queryByLabelText("Custom network"))).toBe(true);
    expect(gone(screen.queryByRole("button", { name: "Start scan" }))).toBe(true);

    fireEvent.click(button);
    expect(button.getAttribute("aria-expanded")).toBe("true");
    const custom = await screen.findByLabelText("Custom network");
    // `aria-controls` has to name the region that actually appeared, or the
    // announcement is about something else.
    expect(document.getElementById(button.getAttribute("aria-controls") ?? "")?.contains(custom)).toBe(true);

    fireEvent.click(button);
    expect(button.getAttribute("aria-expanded")).toBe("false");
    expect(gone(screen.queryByLabelText("Custom network"))).toBe(true);
  });

  // The collapsed line is the only place an idle page states what a scan
  // would cover, so it has to open on the recorded scope rather than on the
  // default one.
  test("states the recorded scope while collapsed", async () => {
    renderOptions(twoNetworks, {
      usb: true,
      network: true,
      subnets: ["192.168.1.0/24"],
      port: 9101,
      timeoutMs: 500,
    }, false);

    await screen.findByText("USB · 1 of 2 networks · port 9101");
    expect(statedScope()).toBe("USB · 1 of 2 networks · port 9101");
  });

  test("the stated scope follows every control that changes it", async () => {
    renderOptions(twoNetworks);
    await screen.findByLabelText("10.42.0.0/24");
    expect(statedScope()).toBe("USB · 2 networks · port 9100");

    fireEvent.click(screen.getByLabelText("192.168.1.0/24"));
    expect(statedScope()).toBe("USB · 1 of 2 networks · port 9100");

    fireEvent.input(screen.getByLabelText("RAW TCP port"), { target: { value: "9101" } });
    expect(statedScope()).toBe("USB · 1 of 2 networks · port 9101");

    // A single custom network is named outright; several are only counted,
    // since the line has one row and a list of CIDRs has no end.
    fireEvent.input(screen.getByLabelText("Custom network"), { target: { value: "10.0.5.0/24" } });
    expect(statedScope()).toBe("USB · 10.0.5.0/24 · port 9101");
    fireEvent.input(screen.getByLabelText("Custom network"), { target: { value: "10.0.5.0/24, 10.0.6.0/24" } });
    expect(statedScope()).toBe("USB · 2 custom networks · port 9101");

    // A refused entry is no scope at all, and the line says so rather than
    // leaving the last one it could state standing.
    fireEvent.input(screen.getByLabelText("Custom network"), { target: { value: "enp5s0" } });
    expect(statedScope()).toBe("USB · custom network refused · port 9101");

    fireEvent.input(screen.getByLabelText("Custom network"), { target: { value: "" } });
    fireEvent.click(screen.getByLabelText("Network"));
    expect(statedScope()).toBe("USB only");

    fireEvent.click(screen.getByLabelText("USB"));
    expect(statedScope()).toBe("Nothing to scan");
  });

  // The page starts scans with what this reports, so the report is the same
  // query `Start scan` sends — and is withdrawn the moment the controls stop
  // naming a scannable scope.
  test("publishes the scope it states, and nothing while it states none", async () => {
    const { onScopeChange, onStart } = renderOptions(twoNetworks);
    await screen.findByLabelText("10.42.0.0/24");
    const stated = { usb: true, network: true, subnets: [], port: 9100, timeoutMs: 1000 };
    // Awaited, because publishing is an effect and the response that seeds
    // the controls lands one render before the effects that follow it.
    await waitFor(() => expect(onScopeChange).toHaveBeenLastCalledWith(stated));

    fireEvent.click(startButton());
    expect(onStart).toHaveBeenCalledWith(stated);

    fireEvent.click(disclosure());
    fireEvent.click(screen.getByLabelText("Network"));
    fireEvent.click(screen.getByLabelText("USB"));
    await waitFor(() => expect(onScopeChange).toHaveBeenLastCalledWith(null));
  });

  // The footer button is inside the form, so pressing it is done with the
  // form — and the progress and results it just started need the room. The
  // header's own button is not inside it and leaves it as the reader
  // arranged it.
  test("Start scan shuts the form it sits in, leaving its scope stated", async () => {
    renderOptions(twoNetworks);
    await screen.findByLabelText("10.42.0.0/24");
    fireEvent.input(screen.getByLabelText("RAW TCP port"), { target: { value: "9101" } });

    fireEvent.click(startButton());
    expect(disclosure().getAttribute("aria-expanded")).toBe("false");
    expect(statedScope()).toBe("USB · 2 networks · port 9101");
  });
});
