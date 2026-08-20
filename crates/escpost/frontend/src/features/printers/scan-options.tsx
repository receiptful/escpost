import { useEffect, useState } from "preact/hooks";
import { getDiscoveryNetworks } from "../../api/client";
import type { DiscoveryQuery } from "../../api/discovery-stream";
import type { DiscoveryNetworksResponse } from "../../api/types";

// The shared layer's own bound on an explicitly named subnet
// (`discovery::EXPLICIT_SCAN_MINIMUM_PREFIX`). Refusing it here only saves a
// round trip and lets the panel say what is wrong next to the field: the
// server refuses the same input in the same words, and remains the authority
// on what a scan accepts.
const EXPLICIT_SCAN_MINIMUM_PREFIX = 16;

// Exactly the input space `discovery::Subnet::parse` accepts: a dotted-quad
// IPv4 address — no leading zeros, since Rust's `Ipv4Addr` rejects those —
// and a prefix of at most 32.
const OCTET = String.raw`(?:25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)`;
const CIDR = new RegExp(String.raw`^${OCTET}(?:\.${OCTET}){3}/(?:3[0-2]|[12]\d|\d)$`);

type NetworksResource = {
  data: DiscoveryNetworksResponse | null;
  error: Error | null;
};

function prefixOf(subnet: string) {
  return Number(subnet.slice(subnet.indexOf("/") + 1));
}

// What a sweep of this subnet will probe. Mirrors `Subnet::hosts`, where a
// /31 and a /32 have no network or broadcast address to leave out (RFC 3021).
// It is an upper bound rather than the exact figure the scan reports: the
// server also excludes its own addresses, which it can only do once it knows
// which of them fall inside the subnet.
function hostsIn(subnet: string) {
  const prefix = prefixOf(subnet);
  const addresses = 2 ** (32 - prefix);
  return prefix >= 31 ? addresses : addresses - 2;
}

function customIssue(entries: string[]) {
  for (const entry of entries) {
    if (!CIDR.test(entry)) {
      return `Expected CIDR notation such as 10.42.0.0/24, found \`${entry}\`.`;
    }
    if (prefixOf(entry) < EXPLICIT_SCAN_MINIMUM_PREFIX) {
      return `Subnet ${entry} is too large to scan (at most /16).`;
    }
  }
  return null;
}

// Slashes and dots are legal in an id but hostile to anything that resolves
// one as a selector, so a subnet becomes an id the plain way.
function checkboxId(value: string) {
  return `scan-network-${value.replace(/[^a-zA-Z0-9]/g, "-")}`;
}

export function ScanOptions({ onStart, onClose }: {
  onStart: (query: DiscoveryQuery) => void;
  onClose: () => void;
}) {
  const [resource, setResource] = useState<NetworksResource>({ data: null, error: null });
  const [reloads, setReloads] = useState(0);
  const [usb, setUsb] = useState(true);
  const [network, setNetwork] = useState(true);
  // The subnets the user has *un*checked, so "everything checked" needs no
  // state at all and survives a networks response arriving after the panel
  // renders.
  const [unchecked, setUnchecked] = useState<string[]>([]);
  const [custom, setCustom] = useState("");
  // `null` until edited, so the port and timeout fields show the server's own
  // defaults without the panel restating what those defaults are.
  const [port, setPort] = useState<string | null>(null);
  const [timeout, setTimeoutMs] = useState<string | null>(null);

  useEffect(() => {
    const controller = new AbortController();
    setResource({ data: null, error: null });
    void getDiscoveryNetworks(controller.signal)
      .then((data) => setResource({ data, error: null }))
      .catch((error: unknown) => {
        if (controller.signal.aborted) {
          return;
        }
        setResource({
          data: null,
          error: error instanceof Error ? error : new Error("Unable to detect this machine's networks."),
        });
      });
    return () => controller.abort();
  }, [reloads]);

  useEffect(() => {
    const dismiss = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", dismiss);
    return () => window.removeEventListener("keydown", dismiss);
  }, [onClose]);

  const data = resource.data;
  const known = data?.networks ?? [];
  const checked = known.filter((entry) => !unchecked.includes(entry.subnet));
  const customEntries = custom.split(",").map((entry) => entry.trim()).filter((entry) => entry.length > 0);
  const customActive = custom.trim().length > 0;
  const customError = customActive ? customIssue(customEntries) : null;

  const portText = port ?? (data ? String(data.default_port) : "");
  const timeoutText = timeout ?? (data ? String(data.default_timeout_ms) : "");
  const portValid = /^\d+$/.test(portText) && Number(portText) >= 1 && Number(portText) <= 65_535;
  const timeoutValid = /^\d+$/.test(timeoutText) && Number(timeoutText) >= 1;

  // Every known network checked is the CLI's no-flag behavior, so the query
  // carries no subnet at all and the scan resolves its own targets. Naming
  // the same networks back to the server would reach the operation layer as a
  // different request than the one the terminal makes.
  const automatic = known.length > 0 && checked.length === known.length;
  const selected = customActive ? customEntries : checked.map((entry) => entry.subnet);
  const probes = customActive
    ? customEntries.filter((entry) => CIDR.test(entry)).reduce((total, entry) => total + hostsIn(entry), 0)
    : checked.reduce((total, entry) => total + entry.hosts, 0);

  const networkSelected = customActive ? customError === null : checked.length > 0;
  const startable = data !== null
    && (usb || network)
    && (!network || (networkSelected && portValid && timeoutValid));

  const footer = resource.error
    ? "Networks unavailable"
    : !data
      ? "Counting…"
      : !network
        ? usb ? "USB only · no network probes" : "Nothing to scan"
        : networkSelected ? `${probes.toLocaleString()} probes` : "No networks selected";

  const toggleKnown = (subnet: string) => {
    setUnchecked((current) => current.includes(subnet)
      ? current.filter((entry) => entry !== subnet)
      : [...current, subnet]);
  };

  const reset = () => {
    setUsb(true);
    setNetwork(true);
    setUnchecked([]);
    setCustom("");
    setPort(null);
    setTimeoutMs(null);
  };

  const start = () => {
    if (!data) {
      return;
    }
    onStart({
      usb,
      network,
      // An unchecked Network transport makes the port and timeout fields
      // moot, so the query carries the server's defaults rather than whatever
      // the disabled fields happen to hold.
      subnets: network ? (automatic && !customActive ? [] : selected) : [],
      port: network ? Number(portText) : data.default_port,
      timeoutMs: network ? Number(timeoutText) : data.default_timeout_ms,
    });
  };

  return (
    <section aria-labelledby="scan-options-heading" class="w-full max-w-sm rounded-box bg-base-100 shadow-lg">
      <header class="flex items-center justify-between border-b border-base-300 px-4 py-3">
        <h2 id="scan-options-heading" class="font-medium">Scan options</h2>
        <button type="button" class="btn btn-ghost btn-sm btn-square" aria-label="Close scan options" onClick={onClose}>✕</button>
      </header>

      <div class="space-y-3 p-4">
        <fieldset class={`rounded-box border border-base-300 px-3 pb-3 ${usb ? "" : "opacity-60"}`}>
          <legend class="flex items-center gap-2 px-1">
            <input id="scan-usb" type="checkbox" class="checkbox checkbox-xs" checked={usb} onChange={(event) => setUsb(event.currentTarget.checked)} />
            <label for="scan-usb" class="text-sm font-medium">USB</label>
          </legend>
          <p class="text-xs text-base-content/60">Connected USB printers are discovered automatically. No options.</p>
        </fieldset>

        <fieldset class={`space-y-3 rounded-box border border-base-300 px-3 pb-3 ${network ? "" : "opacity-60"}`}>
          <legend class="flex items-center gap-2 px-1">
            <input id="scan-network" type="checkbox" class="checkbox checkbox-xs" checked={network} onChange={(event) => setNetwork(event.currentTarget.checked)} />
            <label for="scan-network" class="text-sm font-medium">Network</label>
          </legend>

          <div class="space-y-1">
            <p class="text-xs font-medium uppercase tracking-wide text-base-content/60">Known networks</p>
            {resource.error ? (
              <div role="alert" class="alert alert-warning alert-soft text-xs">
                <span>{resource.error.message}</span>
                <button type="button" class="btn btn-xs" onClick={() => setReloads((count) => count + 1)}>Retry</button>
              </div>
            ) : !data ? (
              <div class="space-y-2 py-1" aria-label="Detecting networks">
                {[0, 1, 2].map((row) => <div key={row} class="skeleton h-4 w-full" />)}
              </div>
            ) : (
              <div class="space-y-1">
                {known.map((entry) => (
                  <div key={entry.subnet} class="flex items-center gap-2">
                    <input
                      id={checkboxId(entry.subnet)}
                      type="checkbox"
                      class="checkbox checkbox-xs"
                      checked={!unchecked.includes(entry.subnet)}
                      disabled={!network || customActive}
                      onChange={() => toggleKnown(entry.subnet)}
                    />
                    <label for={checkboxId(entry.subnet)} class="font-mono text-sm">{entry.subnet}</label>
                    {entry.interface && <span class="text-xs text-base-content/60">{entry.interface}</span>}
                  </div>
                ))}
                {/* A skipped adapter carries the CLI's own sentence rather than
                    a second wording of the same omission. */}
                {data.skipped.map((entry) => (
                  <div key={`${entry.interface} ${entry.subnet ?? ""}`} class="flex items-center gap-2 opacity-60">
                    <input id={checkboxId(entry.interface)} type="checkbox" class="checkbox checkbox-xs" checked={false} disabled />
                    <label for={checkboxId(entry.interface)} class="font-mono text-sm">{entry.subnet ?? entry.interface}</label>
                    <span class="text-xs text-base-content/60">{entry.description}</span>
                  </div>
                ))}
                {known.length === 0 && data.skipped.length === 0 && (
                  <p class="text-xs text-base-content/60">This machine has no directly connected IPv4 network.</p>
                )}
              </div>
            )}
            <p class="text-xs text-base-content/60">Detected from this machine's interfaces.</p>
          </div>

          <div class="space-y-1">
            <label class="text-xs font-medium uppercase tracking-wide text-base-content/60" for="scan-custom">Custom network</label>
            <input
              id="scan-custom"
              type="text"
              class={`input input-sm w-full font-mono ${customError ? "input-warning" : customActive ? "input-primary" : ""}`}
              placeholder="10.0.5.0/24"
              value={custom}
              disabled={!network}
              onInput={(event) => setCustom(event.currentTarget.value)}
            />
            <p class="text-xs text-base-content/60">Separate several with commas. Entering one disables the known networks.</p>
          </div>

          {network && customError && <p role="alert" class="text-xs text-warning">{customError}</p>}
          {network && data && !customActive && checked.length === 0 && (
            <p role="alert" class="text-xs text-warning">Select a known network or enter a custom one.</p>
          )}

          <div class="flex gap-3">
            <div class="flex-1 space-y-1">
              <label class="text-xs text-base-content/70" for="scan-port">RAW TCP port</label>
              <input
                id="scan-port"
                type="number"
                min="1"
                max="65535"
                class="input input-sm w-full"
                value={portText}
                disabled={!network}
                onInput={(event) => setPort(event.currentTarget.value)}
              />
            </div>
            <div class="flex-1 space-y-1">
              <label class="text-xs text-base-content/70" for="scan-timeout">Timeout per host</label>
              <div class="flex items-center gap-2">
                <input
                  id="scan-timeout"
                  type="number"
                  min="1"
                  class="input input-sm w-full"
                  value={timeoutText}
                  disabled={!network}
                  onInput={(event) => setTimeoutMs(event.currentTarget.value)}
                />
                <span class="text-xs text-base-content/60">ms</span>
              </div>
            </div>
          </div>
          {network && data && !portValid && <p role="alert" class="text-xs text-warning">Enter a port between 1 and 65535.</p>}
          {network && data && !timeoutValid && <p role="alert" class="text-xs text-warning">Enter a timeout of at least 1 ms.</p>}
        </fieldset>
      </div>

      <footer class="flex items-center justify-between gap-3 border-t border-base-300 px-4 py-3">
        <span class="text-sm text-base-content/70">{footer}</span>
        <span class="flex gap-2">
          <button type="button" class="btn btn-sm" onClick={reset}>Reset</button>
          <button type="button" class="btn btn-primary btn-sm" disabled={!startable} onClick={start}>Start scan</button>
        </span>
      </footer>
    </section>
  );
}
