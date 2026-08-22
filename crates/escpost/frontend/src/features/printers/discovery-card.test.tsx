import { afterEach, describe, expect, test } from "bun:test";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/preact";
import { useState } from "preact/hooks";
import type { DiscoveryQuery } from "../../api/discovery-stream";
import type { DiscoveryNetworksResponse } from "../../api/types";
import { DiscoveryCard } from "./discovery-card";

// Host counts that are neither equal nor derivable from the prefix, so a
// summed probe count proves the card adds up what the server reported — the
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
// the provider hands the card until one has: the CLI's no-flag behaviour.
const noScanYet: DiscoveryQuery = { usb: true, network: true, subnets: [] };

// Whether the card is open is the page's state, because starting a scan
// shuts the form and scans start outside it. The bar belongs to the card and
// the buttons in it belong to the page, which builds them from the scope it
// is handed — in the same render that draws the line, so the stand-in here
// can carry the scope on the button itself. Reading it back off the DOM is
// how a test sees the two together in one tick.
function Options({ query }: { query: DiscoveryQuery }) {
  const [open, setOpen] = useState(false);
  return (
    <DiscoveryCard
      query={query}
      open={open}
      onOpenChange={setOpen}
      results={<p>what the last scan found</p>}
      actions={(scope) => (
        <button type="button" data-scope={scope === null ? "" : JSON.stringify(scope)}>Scan</button>
      )}
    />
  );
}

// Collapsed is how the card sits on an idle page, so every test about the
// form itself opens the disclosure first and only the ones about the
// disclosure render it shut.
function renderOptions(response: Response | DiscoveryNetworksResponse, query: DiscoveryQuery = noScanYet, expanded = true) {
  globalThis.fetch = (() => Promise.resolve(
    response instanceof Response ? response : json(response),
  )) as unknown as typeof globalThis.fetch;
  const view = render(<Options query={query} />);
  if (expanded) {
    fireEvent.click(disclosure());
  }
  return { view };
}

function disclosure() {
  return screen.getByRole("button", { name: "Scan options" });
}

// The collapsed line, read through the element the disclosure describes
// itself with. By id rather than by text, because it is the one part of the
// card that survives the form being shut.
function statedScope() {
  return document.getElementById("scan-options-scope")?.textContent;
}

// The scope the button would send, read off the button itself. Synchronous on
// purpose: it is drawn by the same render as the line above it, so an
// assertion that had to wait for it would be evidence of the hop this is
// built to make impossible.
function statedQuery() {
  const carried = screen.getByRole("button", { name: "Scan" }).getAttribute("data-scope");
  return carried ? JSON.parse(carried) as DiscoveryQuery : null;
}

function expectScope(expected: DiscoveryQuery | null) {
  expect(statedQuery()).toEqual(expected);
}

// Absence as a boolean. `expect(node).toBeNull()` prints the entire happy-dom
// node graph when it fails — tens of megabytes for a node still attached to
// the page — which buries every other failure in the run.
function gone(element: Element | null) {
  return element === null;
}

afterEach(cleanup);

describe("DiscoveryCard", () => {
  test("publishes no subnets when every known network is checked, so the scan runs in automatic mode", async () => {
    renderOptions(twoNetworks);
    await screen.findByLabelText("10.42.0.0/24");

    expect(statedScope()).toBe("USB · 2 networks · 507 probes");
    expectScope({ usb: true, network: true, subnets: [], port: 9100, timeoutMs: 1000 });
  });

  test("names the checked networks once the selection is partial, and Reset restores automatic mode", async () => {
    renderOptions(twoNetworks);
    fireEvent.click(await screen.findByLabelText("192.168.1.0/24"));
    fireEvent.input(screen.getByLabelText("RAW TCP port"), { target: { value: "9101" } });
    fireEvent.input(screen.getByLabelText("Timeout per host"), { target: { value: "500" } });

    expect(statedScope()).toBe("USB · 1 of 2 networks · 253 probes");
    expectScope({
      usb: true,
      network: true,
      subnets: ["10.42.0.0/24"],
      port: 9101,
      timeoutMs: 500,
    });

    // Reset re-detects rather than restoring a remembered list: an adapter
    // appears or vanishes with a cable, and this is the only way back from a
    // failed detection.
    fireEvent.click(screen.getByRole("button", { name: "Reset" }));
    expect(screen.getByLabelText("Detecting networks")).toBeTruthy();
    await screen.findByLabelText("10.42.0.0/24");
    expect(statedScope()).toBe("USB · 2 networks · 507 probes");
    expectScope({ usb: true, network: true, subnets: [], port: 9100, timeoutMs: 1000 });
  });

  // The card opens on the scope the last scan ran with, so that what it
  // shows and what `Scan` would send are the same thing.
  test("opens on the recorded scope, port and timeout included", async () => {
    renderOptions(twoNetworks, {
      usb: false,
      network: true,
      subnets: ["192.168.1.0/24"],
      port: 9101,
      timeoutMs: 500,
    });
    await screen.findByLabelText("10.42.0.0/24");

    expect((screen.getByLabelText("USB Printers") as HTMLInputElement).checked).toBe(false);
    // The section titles name the transports at length; the line above stays
    // terse, because it has one row to fit in at phone width.
    expect(screen.getByText("Connected USB printers are discovered automatically.")).toBeTruthy();
    expect((screen.getByLabelText("10.42.0.0/24") as HTMLInputElement).checked).toBe(false);
    expect((screen.getByLabelText("192.168.1.0/24") as HTMLInputElement).checked).toBe(true);
    expect((screen.getByLabelText("RAW TCP port") as HTMLInputElement).value).toBe("9101");
    expect((screen.getByLabelText("Timeout per host") as HTMLInputElement).value).toBe("500");
    expect(statedScope()).toBe("1 of 2 networks · 254 probes");

    // Untouched, it publishes the scan it was showing, so repeating it
    // repeats that scan.
    expectScope({
      usb: false,
      network: true,
      subnets: ["192.168.1.0/24"],
      port: 9101,
      timeoutMs: 500,
    });
  });

  test("states no scope when the network transport is checked with nothing selected", async () => {
    renderOptions(twoNetworks);
    fireEvent.click(await screen.findByLabelText("10.42.0.0/24"));
    fireEvent.click(screen.getByLabelText("192.168.1.0/24"));

    expect(screen.getByText("Select a known network or enter a custom one.")).toBeTruthy();
    expect(statedScope()).toBe("USB · no networks selected");
    expectScope(null);
  });

  test("an unchecked network transport scans USB alone, and unchecking both leaves nothing to do", async () => {
    renderOptions(twoNetworks);
    await screen.findByLabelText("10.42.0.0/24");
    fireEvent.click(screen.getByLabelText("Network (IP) Printers"));

    expect(statedScope()).toBe("USB only · no network probes");
    expect((screen.getByLabelText("Custom network") as HTMLInputElement).disabled).toBe(true);
    expectScope({ usb: true, network: false, subnets: [], port: 9100, timeoutMs: 1000 });

    fireEvent.click(screen.getByLabelText("USB Printers"));
    expect(statedScope()).toBe("Nothing to scan");
    expectScope(null);
  });

  test("a custom network disables the known networks without removing them", async () => {
    renderOptions(twoNetworks);
    fireEvent.input(await screen.findByLabelText("Custom network"), { target: { value: "10.0.5.0/24, 10.0.6.0/24" } });

    const known = screen.getByLabelText("10.42.0.0/24") as HTMLInputElement;
    expect(known.disabled).toBe(true);
    expect(known.isConnected).toBe(true);
    expect(known.checked).toBe(true);
    expect(statedScope()).toBe("USB · 2 custom networks · 508 probes");

    expectScope({
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
    expectScope(null);

    fireEvent.input(custom, { target: { value: "10.0.0.0/8" } });
    expect(screen.getByText("Subnet 10.0.0.0/8 is too large to scan (at most /16).")).toBeTruthy();
    expectScope(null);

    fireEvent.input(custom, { target: { value: "10.0.0.0/16" } });
    expect(statedScope()).toBe("USB · 10.0.0.0/16 · 65,534 probes");
    expectScope({ usb: true, network: true, subnets: ["10.0.0.0/16"], port: 9100, timeoutMs: 1000 });

    // A single host, which is what a `/32` is for and what `Subnet::hosts`
    // counts as one address rather than as a subnet minus its network and
    // broadcast. It read `1 probes` until the shared plural rule was applied
    // here too.
    fireEvent.input(custom, { target: { value: "10.0.5.7/32" } });
    expect(statedScope()).toBe("USB · 10.0.5.7/32 · 1 probe");
  });

  // A field holding only separators is content, not emptiness. Reading it as
  // empty would hand the query back to the known networks — including ones
  // the user unchecked — under a line promising nothing.
  test("a custom field holding only separators is invalid, never a silent automatic scan", async () => {
    renderOptions(twoNetworks);
    fireEvent.input(await screen.findByLabelText("Custom network"), { target: { value: ", ," } });

    expect(screen.getByText("Expected CIDR notation such as 10.42.0.0/24, found `, ,`.")).toBeTruthy();
    expect((screen.getByLabelText("10.42.0.0/24") as HTMLInputElement).disabled).toBe(true);
    expect(statedScope()).toBe("USB · custom network refused");
    expectScope(null);
  });

  // One refused entry invalidates the whole field, so the line states no
  // count at all rather than a total for the entries that survived. A number
  // beside a refusal would promise work the card is refusing to do.
  test("states no probe count while any custom entry is refused", async () => {
    const { view } = renderOptions(twoNetworks);
    fireEvent.input(await screen.findByLabelText("Custom network"), { target: { value: "10.0.0.0/8, 10.0.5.0/24" } });

    expect(screen.getByText("Subnet 10.0.0.0/8 is too large to scan (at most /16).")).toBeTruthy();
    expect(statedScope()).toBe("USB · custom network refused");
    expect(view.container.textContent).not.toContain("probes");
    expectScope(null);
  });

  // The port is a `NonZeroU16` and the timeout a `u64` of milliseconds in the
  // shared layer, so the card accepts exactly that — `--timeout 0` included,
  // since inventing a stricter rule here is its own kind of divergence.
  test("refuses a port or timeout the shared layer cannot take, and accepts the whole range it can", async () => {
    renderOptions(twoNetworks);
    const port = await screen.findByLabelText("RAW TCP port");
    const timeout = screen.getByLabelText("Timeout per host");

    fireEvent.input(port, { target: { value: "0" } });
    expect(screen.getByText("Enter a port between 1 and 65535.")).toBeTruthy();
    expectScope(null);

    fireEvent.input(port, { target: { value: "65536" } });
    expect(screen.getByText("Enter a port between 1 and 65535.")).toBeTruthy();
    expectScope(null);

    fireEvent.input(port, { target: { value: "65535" } });
    expect(gone(screen.queryByText("Enter a port between 1 and 65535."))).toBe(true);
    expectScope({ usb: true, network: true, subnets: [], port: 65535, timeoutMs: 1000 });

    // Past `MAX_SAFE_INTEGER` a JavaScript number stringifies as `1e+21`,
    // which no `u64` parses — a wire limit, not a product rule.
    fireEvent.input(timeout, { target: { value: "999999999999999999999" } });
    expect(screen.getByText("Enter a timeout as a whole number of milliseconds.")).toBeTruthy();
    expectScope(null);

    fireEvent.input(timeout, { target: { value: "0" } });
    expect(gone(screen.queryByText("Enter a timeout as a whole number of milliseconds."))).toBe(true);
    // The field's own constraint has to agree, or the browser paints it
    // out of range while the card accepts it.
    expect(timeout.getAttribute("min")).toBe("0");
    expectScope({ usb: true, network: true, subnets: [], port: 65535, timeoutMs: 0 });
  });

  // The port and timeout are only vetted while Network is checked, so an
  // unchecked Network could otherwise publish whatever text the disabled
  // fields hold. `DiscoveryQuery` says those are numbers, and that has to be
  // true where the query is built — not because the reader discards them.
  test("substitutes the server's defaults rather than publishing a port or timeout that is not a number", async () => {
    renderOptions(twoNetworks);
    fireEvent.input(await screen.findByLabelText("RAW TCP port"), { target: { value: "" } });
    fireEvent.input(screen.getByLabelText("Timeout per host"), { target: { value: "" } });
    expectScope(null);

    fireEvent.click(screen.getByLabelText("Network (IP) Printers"));
    expectScope({ usb: true, network: false, subnets: [], port: 9100, timeoutMs: 1000 });
  });

  // The reason is the shared layer's; the remedy is this card's. The
  // terminal answers the same omission with `--subnet`, which is useless
  // advice in a browser with a custom-network field two rows below.
  test("a skipped adapter is listed with the server's reason, this card's remedy, and no CLI flag", async () => {
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
    // Disabled has to stay visible: the box is what says this network exists
    // and is deliberately unavailable, and a row that faded itself faded the
    // box along with the words. The text carries the dimming instead.
    expect(skipped.closest("div")?.className).not.toContain("opacity-");
    expect(skipped.className).toContain("disabled:opacity-100");
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
    expectScope(null);
  });

  test("keeps a skeleton network list until the networks response arrives", async () => {
    let resolveNetworks!: (response: Response) => void;
    globalThis.fetch = (() => new Promise<Response>((resolve) => { resolveNetworks = resolve; })) as unknown as typeof globalThis.fetch;
    const view = render(<Options query={noScanYet} />);
    fireEvent.click(disclosure());

    // The collapsed line says as much as is known, which until the response
    // lands is that the networks are still being counted.
    expect(statedScope()).toBe("USB · counting networks…");
    expect(screen.getByLabelText("Detecting networks")).toBeTruthy();
    expect(view.container.querySelectorAll(".skeleton").length).toBeGreaterThan(0);
    expectScope(null);

    await act(async () => { resolveNetworks(json(twoNetworks)); });
    expect(await screen.findByLabelText("10.42.0.0/24")).toBeTruthy();
    expect(gone(screen.queryByLabelText("Detecting networks"))).toBe(true);
    expectScope({ usb: true, network: true, subnets: [], port: 9100, timeoutMs: 1000 });
  });

  // A failed detection refuses the one button that starts a scan, so its
  // reason and its remedy may not sit behind a disclosure nobody has been
  // given a reason to open.
  test("a failed networks request opens the form itself and offers a retry that recovers", async () => {
    const responses = [
      json({ error: { code: "network_detection_failed", message: "Unable to detect this machine's networks." } }, 500),
      json(twoNetworks),
    ];
    globalThis.fetch = (() => Promise.resolve(responses.shift()!)) as unknown as typeof globalThis.fetch;
    render(<Options query={noScanYet} />);

    expect(await screen.findByText("Unable to detect this machine's networks.")).toBeTruthy();
    expect(disclosure().getAttribute("aria-expanded")).toBe("true");
    expect(statedScope()).toBe("USB · networks unavailable");
    expectScope(null);

    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(await screen.findByLabelText("10.42.0.0/24")).toBeTruthy();
    expectScope({ usb: true, network: true, subnets: [], port: 9100, timeoutMs: 1000 });
  });

  // The card lives in the page rather than over it, so an idle page is one
  // row: the form is behind a disclosure and nothing about it overlays the
  // results below.
  test("renders collapsed, and the disclosure it announces is the form", async () => {
    renderOptions(twoNetworks, noScanYet, false);
    const button = disclosure();

    expect(button.getAttribute("aria-expanded")).toBe("false");
    expect(gone(screen.queryByLabelText("Custom network"))).toBe(true);
    // The bar is not part of what the disclosure hides: its controls act on
    // the scope, which the line above states whether the fields are shown or
    // not.
    expect(screen.getByRole("button", { name: "Reset" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Scan" })).toBeTruthy();

    fireEvent.click(button);
    expect(button.getAttribute("aria-expanded")).toBe("true");
    const custom = await screen.findByLabelText("Custom network");
    // `aria-controls` has to name the region that actually appeared, or the
    // announcement is about something else.
    expect(document.getElementById(button.getAttribute("aria-controls") ?? "")?.contains(custom)).toBe(true);

    fireEvent.click(button);
    expect(button.getAttribute("aria-expanded")).toBe("false");
    expect(gone(screen.queryByLabelText("Custom network"))).toBe(true);

    // Options, then results, then the bar — one card in the order a reader
    // works through it, whether the form is open or shut.
    const follows = (first: Element, second: Element) =>
      (first.compareDocumentPosition(second) & Node.DOCUMENT_POSITION_FOLLOWING) !== 0;
    const results = screen.getByText("what the last scan found");
    expect(follows(button, results)).toBe(true);
    expect(follows(results, screen.getByRole("button", { name: "Reset" }))).toBe(true);
  });

  // A Reset that would do nothing may not look available — and because the
  // bar shows through a shut accordion, an enabled one is also the quiet
  // signal that this scan is not the default one.
  test("Reset offers itself only when the effective scope differs from the defaults", async () => {
    renderOptions(twoNetworks);
    await screen.findByLabelText("10.42.0.0/24");
    const reset = () => screen.getByRole("button", { name: "Reset" });
    expect(reset().hasAttribute("disabled")).toBe(true);

    fireEvent.click(screen.getByLabelText("192.168.1.0/24"));
    expect(reset().hasAttribute("disabled")).toBe(false);
    fireEvent.click(screen.getByLabelText("192.168.1.0/24"));
    expect(reset().hasAttribute("disabled")).toBe(true);

    // Whitespace is not a custom network, so it is not a change either — a
    // reader who types a character and deletes it has changed nothing.
    fireEvent.input(screen.getByLabelText("Custom network"), { target: { value: "   " } });
    expect(reset().hasAttribute("disabled")).toBe(true);
    fireEvent.input(screen.getByLabelText("Custom network"), { target: { value: "10.0.5.0/24" } });
    expect(reset().hasAttribute("disabled")).toBe(false);
    fireEvent.input(screen.getByLabelText("Custom network"), { target: { value: "" } });
    expect(reset().hasAttribute("disabled")).toBe(true);

    // The same number typed by hand is the same number.
    fireEvent.input(screen.getByLabelText("RAW TCP port"), { target: { value: "9100" } });
    expect(reset().hasAttribute("disabled")).toBe(true);
    fireEvent.input(screen.getByLabelText("RAW TCP port"), { target: { value: "9101" } });
    expect(reset().hasAttribute("disabled")).toBe(false);

    // Disabled, never hidden: the bar keeps its width in both states.
    fireEvent.input(screen.getByLabelText("RAW TCP port"), { target: { value: "9100" } });
    expect(reset().hasAttribute("disabled")).toBe(true);
    expect(reset().isConnected).toBe(true);
  });

  // The scope a scan was configured with is a difference from the defaults,
  // and reopening on it is exactly when a reader wants the way back.
  // The case Reset exists for, and the one it used to fail at: the card opens
  // on the scope of the last scan, and pressing Reset has to escape it. It
  // refetches, and the response may not re-seed the recorded scope back over
  // the controls — that left the scope byte-identical and the button lit,
  // asking to be pressed again.
  test("Reset escapes the narrowed scope the card opened on", async () => {
    renderOptions(twoNetworks, { usb: false, network: true, subnets: ["192.168.1.0/24"], port: 9101, timeoutMs: 500 });
    await screen.findByLabelText("10.42.0.0/24");
    const reset = () => screen.getByRole("button", { name: "Reset" });
    expect(statedScope()).toBe("1 of 2 networks · 254 probes");
    expect(reset().hasAttribute("disabled")).toBe(false);

    fireEvent.click(reset());

    // The scope of a scan with no options set: both transports, every
    // detected network, and the port and timeout the server advertises.
    await waitFor(() => expect(statedScope()).toBe("USB · 2 networks · 507 probes"));
    expectScope({ usb: true, network: true, subnets: [], port: 9100, timeoutMs: 1000 });
    expect((screen.getByLabelText("USB Printers") as HTMLInputElement).checked).toBe(true);
    expect((screen.getByLabelText("192.168.1.0/24") as HTMLInputElement).checked).toBe(true);
    expect((screen.getByLabelText("RAW TCP port") as HTMLInputElement).value).toBe("9100");
    // Nothing left to undo, so it stops asking.
    expect(reset().hasAttribute("disabled")).toBe(true);
  });

  // Retry is the other reload, and it is the opposite case: detection failed
  // before the card could seed, so its response still has to.
  test("Retry after a failed detection still seeds the recorded scope", async () => {
    const responses = [
      json({ error: { code: "network_detection_failed", message: "Unable to detect this machine's networks." } }, 500),
      json(twoNetworks),
    ];
    globalThis.fetch = (() => Promise.resolve(responses.shift()!)) as unknown as typeof globalThis.fetch;
    render(<Options query={{ usb: true, network: true, subnets: ["192.168.1.0/24"], port: 9100, timeoutMs: 1000 }} />);
    await screen.findByText("Unable to detect this machine's networks.");

    fireEvent.click(screen.getByRole("button", { name: "Retry" }));

    await screen.findByLabelText("10.42.0.0/24");
    expect(statedScope()).toBe("USB · 1 of 2 networks · 254 probes");
  });

  test("Reset is reachable with the form shut, and re-detects from there", async () => {
    renderOptions(twoNetworks);
    fireEvent.click(await screen.findByLabelText("192.168.1.0/24"));
    expect(statedScope()).toBe("USB · 1 of 2 networks · 253 probes");

    fireEvent.click(disclosure());
    expect(gone(screen.queryByLabelText("Custom network"))).toBe(true);
    fireEvent.click(screen.getByRole("button", { name: "Reset" }));

    await waitFor(() => expect(statedScope()).toBe("USB · 2 networks · 507 probes"));
    expectScope({ usb: true, network: true, subnets: [], port: 9100, timeoutMs: 1000 });
  });

  // The collapsed line is the only place an idle page states what a scan
  // would cover and what it would cost, so it has to open on the recorded
  // scope rather than on the default one.
  test("states the recorded scope while collapsed", async () => {
    renderOptions(twoNetworks, {
      usb: true,
      network: true,
      subnets: ["192.168.1.0/24"],
      port: 9101,
      timeoutMs: 500,
    }, false);

    await waitFor(() => expect(statedScope()).toBe("USB · 1 of 2 networks · 254 probes"));
  });

  test("the stated scope follows every control that changes it", async () => {
    renderOptions(twoNetworks);
    await screen.findByLabelText("10.42.0.0/24");
    expect(statedScope()).toBe("USB · 2 networks · 507 probes");

    fireEvent.click(screen.getByLabelText("192.168.1.0/24"));
    expect(statedScope()).toBe("USB · 1 of 2 networks · 253 probes");

    // A single custom network is named outright; several are only counted,
    // since the line has one row and a list of CIDRs has no end. The probe
    // count follows either way, because that is the number the reader is
    // committing to.
    fireEvent.input(screen.getByLabelText("Custom network"), { target: { value: "10.0.5.0/24" } });
    expect(statedScope()).toBe("USB · 10.0.5.0/24 · 254 probes");
    fireEvent.input(screen.getByLabelText("Custom network"), { target: { value: "10.0.5.0/24, 10.0.6.0/24" } });
    expect(statedScope()).toBe("USB · 2 custom networks · 508 probes");

    // A refused entry is no scope at all, and the line says so rather than
    // leaving the last one it could state standing.
    fireEvent.input(screen.getByLabelText("Custom network"), { target: { value: "enp5s0" } });
    expect(statedScope()).toBe("USB · custom network refused");

    fireEvent.input(screen.getByLabelText("Custom network"), { target: { value: "" } });
    fireEvent.click(screen.getByLabelText("Network (IP) Printers"));
    expect(statedScope()).toBe("USB only · no network probes");

    fireEvent.click(screen.getByLabelText("USB Printers"));
    expect(statedScope()).toBe("Nothing to scan");
  });
});
