import type { ComponentChildren } from "preact";
import { useEffect, useRef, useState } from "preact/hooks";
import { getDiscoveryNetworks } from "../../api/client";
import type { DiscoveryQuery } from "../../api/discovery-stream";
import type { DiscoveryNetworksResponse, SkippedNetwork } from "../../api/types";

// Copied from `discovery::EXPLICIT_SCAN_MINIMUM_PREFIX`, which is the source
// of truth. Refusing the subnet here only saves a round trip and lets the
// panel say what is wrong next to the field; the server refuses the same
// input in the same words and stays the authority on what a scan accepts. If
// the Rust constant moves and this does not, the cost is a wrong hint, never
// a wrong scan.
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

// What a sweep of this subnet will probe. Copied from `Subnet::hosts`, the
// source of truth, where a /31 and a /32 have no network or broadcast address
// to leave out (RFC 3021). It is an upper bound rather than the exact figure
// the scan reports: the server also excludes its own addresses, which it can
// only do once it knows which of them fall inside the subnet. Drift from the
// Rust definition shows up as a wrong probe count, never a wrong scan.
function hostsIn(subnet: string) {
  const prefix = prefixOf(subnet);
  const addresses = 2 ** (32 - prefix);
  return prefix >= 31 ? addresses : addresses - 2;
}

function notCidr(value: string) {
  return `Expected CIDR notation such as 10.42.0.0/24, found \`${value}\`.`;
}

// What is wrong with the custom field's content, or `null` when it names
// scannable subnets. A field holding nothing but separators is invalid rather
// than empty: reading it as empty would hand the query back to the known
// networks the user is no longer looking at, and reading it as a valid empty
// selection would ship automatic mode under a line promising no probes.
function customIssue(text: string, entries: string[]) {
  if (entries.length === 0) {
    return notCidr(text.trim());
  }
  for (const entry of entries) {
    if (!CIDR.test(entry)) {
      return notCidr(entry);
    }
    if (prefixOf(entry) < EXPLICIT_SCAN_MINIMUM_PREFIX) {
      return `Subnet ${entry} is too large to scan (at most /${EXPLICIT_SCAN_MINIMUM_PREFIX}).`;
    }
  }
  return null;
}

// The server states why an adapter was skipped; what to do about it is this
// interface's own wording. The terminal answers "scan it with --subnet
// 10.0.0.0/16"; here the custom-network field is two rows below, so the row
// points at that instead. Only a too-large adapter has a subnet worth
// retyping — an unusable netmask names none, so there is nothing to suggest.
function skippedExplanation(adapter: SkippedNetwork) {
  return adapter.reason === "too_large" && adapter.subnet ? `${adapter.description}, add it as a custom network` : adapter.description;
}

// The one treatment every field label in this panel shares. Sentence case,
// deliberately: the capitals used to come from `uppercase`, and the only
// capitals left are the ones the words are actually spelled with.
const FIELD_LABEL = "text-xs font-medium text-base-content/60";

// A disabled checkbox has to stay visible: it is what says this network
// exists and is deliberately unavailable, which is the whole reason a skipped
// adapter is listed rather than dropped. daisyUI fades a disabled box to
// near-nothing, so the fade is overridden and the box keeps an explicit
// outline instead — still disabled, still `not-allowed`, just legible. The
// row dims its text rather than itself, so nothing dims the control twice.
const CHECKBOX = "checkbox checkbox-xs border-base-content/40 disabled:opacity-100";

// Slashes and dots are legal in an id but hostile to anything that resolves
// one as a selector, so a subnet becomes an id the plain way.
function checkboxId(value: string) {
  return `scan-network-${value.replace(/[^a-zA-Z0-9]/g, "-")}`;
}

// `1 network` rather than `1 networks`: the collapsed line is prose, not a
// readout.
function countOf(count: number, noun: string) {
  return `${count} ${noun}${count === 1 ? "" : "s"}`;
}

/**
 * The scan scope, as controls, in the page rather than over it: a disclosure
 * row that states the scope and its cost in one line, and the form itself
 * behind it. `query` is the scope the last scan ran with — the provider's
 * `scanQuery`, which is the CLI's no-flag default until a scan has been
 * configured — and the controls are seeded from it, so the line and the form
 * are two views of the same thing.
 *
 * This panel starts nothing. `onScopeChange` reports what its controls
 * amount to, or `null` when they amount to no scan at all, and the section's
 * one scan button sends exactly that — so there is no second button here to
 * disagree with it.
 *
 * `open` belongs to the page for the same reason: starting a scan shuts the
 * form, and scans start outside it. So do `actions` — the bar along the
 * bottom is this panel's, the buttons the page puts in it are the page's, and
 * the bar stays whether the form is open or shut because everything in it
 * acts on the scope the line states rather than on the fields.
 *
 * The networks are fetched once per mount, and again on Reset: adapters
 * change with a cable or a VPN, so the server stays the authority on which
 * networks exist while `query` says which of them were chosen.
 */
export function ScanOptions({ query, open, onOpenChange, onScopeChange, actions }: {
  query: DiscoveryQuery;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onScopeChange: (scope: DiscoveryQuery | null) => void;
  actions: ComponentChildren;
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

  // Read through a ref rather than as an effect dependency, so a caller that
  // rebuilds the object on every render cannot turn the networks fetch into a
  // loop. The scope only changes when a scan starts, and a scan starts from
  // these controls — so the recorded scope only ever arrives back here as
  // what they already hold.
  const requested = useRef(query);
  requested.current = query;

  // The recorded scope expressed in these controls, applied once the adapters
  // it has to be matched against have arrived.
  //
  // A chosen subnet that no adapter reports any more has no row to be checked
  // in, so it goes to the custom field — where the reader would have to
  // retype it — rather than disappearing from the selection. And because a
  // custom entry disables the known list outright, the whole selection moves
  // there as soon as any part of it must: splitting it would leave the
  // checked half out of the query the line is promising.
  const seed = (data: DiscoveryNetworksResponse) => {
    const scope = requested.current;
    setUsb(scope.usb);
    setNetwork(scope.network);
    setPort(scope.port === undefined ? null : String(scope.port));
    setTimeoutMs(scope.timeoutMs === undefined ? null : String(scope.timeoutMs));
    const known = data.networks.map((entry) => entry.subnet);
    if (scope.subnets.length === 0) {
      // Automatic mode: every network checked, which is also how the panel
      // opens before any scan has been configured.
      setUnchecked([]);
      setCustom("");
    } else if (scope.subnets.every((subnet) => known.includes(subnet))) {
      setUnchecked(known.filter((subnet) => !scope.subnets.includes(subnet)));
      setCustom("");
    } else {
      setUnchecked([]);
      setCustom(scope.subnets.join(", "));
    }
  };

  useEffect(() => {
    const controller = new AbortController();
    setResource({ data: null, error: null });
    void getDiscoveryNetworks(controller.signal)
      .then((data) => {
        setResource({ data, error: null });
        seed(data);
      })
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

  const data = resource.data;
  const known = data?.networks ?? [];
  const checked = known.filter((entry) => !unchecked.includes(entry.subnet));
  // One definition of "the custom field has content": any non-whitespace at
  // all. Everything downstream derives from it, so the field is never active
  // with nothing behind it.
  const customActive = custom.trim().length > 0;
  const customEntries = custom.split(",").map((entry) => entry.trim()).filter((entry) => entry.length > 0);
  const customError = customActive ? customIssue(custom, customEntries) : null;

  const portText = port ?? (data ? String(data.default_port) : "");
  const timeoutText = timeout ?? (data ? String(data.default_timeout_ms) : "");
  // The port is a `NonZeroU16` and the timeout a `u64` of milliseconds, so
  // these are the shared layer's own bounds — zero milliseconds included,
  // which `--timeout 0` also accepts. The upper bound on the timeout is not a
  // product rule but the wire's: a JavaScript number stops being an exact
  // decimal past `MAX_SAFE_INTEGER` and stringifies as `1e+21`, which no
  // `u64` will parse.
  const portValid = /^\d+$/.test(portText) && Number(portText) >= 1 && Number(portText) <= 65_535;
  const timeoutValid = /^\d+$/.test(timeoutText) && Number(timeoutText) <= Number.MAX_SAFE_INTEGER;

  // Every known network checked is the CLI's no-flag behavior, so the query
  // carries no subnet at all and the scan resolves its own targets. Naming
  // the same networks back to the server would reach the operation layer as a
  // different request than the one the terminal makes.
  //
  // Only ever consulted where the custom field is empty, so it does not
  // restate that. With no known network at all it is vacuously
  // true, which would send an automatic scan the server has nothing to
  // resolve; that stays unreachable because `networkSelected` below already
  // requires a checked network, and is what refuses the scan button — so
  // anything loosening either of those must revisit this.
  const automatic = checked.length === known.length;
  const selected = customActive ? customEntries : automatic ? [] : checked.map((entry) => entry.subnet);
  // The line only ever states this while `networkSelected` holds, which for
  // a custom field means every entry passed `customIssue` — so no refused
  // subnet can inflate the count. The shape filter is here to keep a
  // malformed entry's `NaN` out of the arithmetic, not to correct the total.
  const probes = customActive
    ? customEntries.filter((entry) => CIDR.test(entry)).reduce((total, entry) => total + hostsIn(entry), 0)
    : checked.reduce((total, entry) => total + entry.hosts, 0);

  const networkSelected = customActive ? customError === null : checked.length > 0;
  const startable = data !== null
    && (usb || network)
    && (!network || (networkSelected && portValid && timeoutValid));

  // The networks half of the disclosure line, which is the only place the
  // scope is stated while the form is shut — and the form is shut by
  // default.
  //
  // One custom network is worth naming; several are only worth counting,
  // since the line has one row and a list of CIDRs has no end.
  const customScope = customError
    ? "custom network refused"
    : customEntries.length === 1 ? customEntries[0]! : `${customEntries.length} custom networks`;
  const networkScope = resource.error
    ? "networks unavailable"
    : !data
      ? "counting networks…"
      : customActive
        ? customScope
        : checked.length === 0
          ? "no networks selected"
          : automatic ? countOf(known.length, "network") : `${checked.length} of ${known.length} networks`;

  // What a scan started right now would cover and what it would cost, in one
  // line. The probe count is here rather than beside a start button because
  // there is no start button here any more: the section's own button is what
  // commits to this number, and both interfaces owe the reader that number
  // before the sweep begins.
  //
  // The port is not in the line. It is one field of a form that is one click
  // away, and a line that outgrows its row states nothing at all.
  const cost = networkSelected && data ? `${probes.toLocaleString()} probes` : "";
  const scopeSummary = !usb && !network
    ? "Nothing to scan"
    : !network
      ? "USB only · no network probes"
      : [usb ? "USB" : "", networkScope, cost].filter((part) => part.length > 0).join(" · ");

  // The scope those controls state, or `null` while they state none: still
  // counting, a failed detection, or a contradiction such as a checked
  // Network with no network behind it. The section's scan button sends this
  // and is refused while it is `null`.
  //
  // The port and timeout travel even when Network is unchecked, because the
  // fields keep their values while disabled; `discoveryQueryString` drops
  // them from a USB-only scan, which is where the shared layer's refusal to
  // accept them lives.
  //
  // `startable` vets those fields only while Network is checked — refusing a
  // USB scan over a value that scan does not use would be its own
  // contradiction — so an unvetted field falls back to the default the server
  // advertised on `discover/networks`, which is also what the fields and the
  // line above are showing. `DiscoveryQuery` would accept the omission, but
  // this panel has a number to send: it asked for one and displayed the
  // answer, so sending what the reader is looking at is the honest query.
  const scope: DiscoveryQuery | null = startable && data
    ? {
        usb,
        network,
        subnets: network ? selected : [],
        port: portValid ? Number(portText) : data.default_port,
        timeoutMs: timeoutValid ? Number(timeoutText) : data.default_timeout_ms,
      }
    : null;

  // Both read through refs, so a parent that rebuilds either on every render
  // cannot turn this into a render loop; the serialized scope is what decides
  // that anything changed.
  const publish = useRef(onScopeChange);
  publish.current = onScopeChange;
  const stated = useRef(scope);
  stated.current = scope;
  const scopeKey = scope === null ? "" : JSON.stringify(scope);
  useEffect(() => {
    publish.current(stated.current);
  }, [scopeKey]);

  const toggleKnown = (subnet: string) => {
    setUnchecked((current) => current.includes(subnet)
      ? current.filter((entry) => entry !== subnet)
      : [...current, subnet]);
  };

  // Back to the panel as it opened, adapters included: a network appears or
  // vanishes with a cable or a VPN, and Reset is the only way back from a
  // failed detection. These are the hard defaults rather than the recorded
  // scope only for as long as the refetch takes — its response re-seeds the
  // controls, so Reset lands on the scope the panel opened with, which is
  // what "as it opened" has to mean now that it opens configured.
  const reset = () => {
    setUsb(true);
    setNetwork(true);
    setUnchecked([]);
    setCustom("");
    setPort(null);
    setTimeoutMs(null);
    setReloads((count) => count + 1);
  };

  // A failed detection refuses the only button that starts a scan, so the
  // form opens itself to put the reason and its Retry in front of the reader
  // rather than behind a disclosure they have no reason to open. Once, on
  // the failure — reopening a form the reader has since shut would be the
  // panel arguing with them.
  const expand = useRef(onOpenChange);
  expand.current = onOpenChange;
  const failed = resource.error !== null;
  useEffect(() => {
    if (failed) {
      expand.current(true);
    }
  }, [failed]);

  return (
    <div class="overflow-hidden rounded-box bg-base-100 shadow-sm">
      {/* A real disclosure rather than a row that happens to react to
          clicks: the summary is hidden from the accessible name — which
          stays the command — while `aria-describedby` still reads it out,
          since a reference is followed into hidden content. */}
      <button
        type="button"
        class="flex w-full items-center gap-3 px-4 py-3 text-left hover:bg-base-200"
        aria-expanded={open}
        aria-controls="scan-options-form"
        aria-describedby="scan-options-scope"
        onClick={() => onOpenChange(!open)}
      >
        <span aria-hidden="true" class="text-xs text-base-content/60">{open ? "▾" : "▸"}</span>
        <span class="font-medium">Scan options</span>
        <span id="scan-options-scope" aria-hidden="true" class="ml-auto min-w-0 truncate text-sm text-base-content/70">{scopeSummary}</span>
      </button>

      {open && (
        <div id="scan-options-form" class="border-t border-base-300">
        <div class="space-y-3 p-4">
          <fieldset class={`rounded-box border border-base-300 px-3 pb-3 ${usb ? "" : "opacity-60"}`}>
            <legend class="flex items-center gap-2 px-1">
              <input id="scan-usb" type="checkbox" class="checkbox checkbox-xs" checked={usb} onChange={(event) => setUsb(event.currentTarget.checked)} />
              <label for="scan-usb" class="text-sm font-medium">USB Printers</label>
            </legend>
            <p class="text-xs text-base-content/60">Connected USB printers are discovered automatically.</p>
          </fieldset>

          <fieldset class={`space-y-3 rounded-box border border-base-300 px-3 pb-3 ${network ? "" : "opacity-60"}`}>
            <legend class="flex items-center gap-2 px-1">
              <input id="scan-network" type="checkbox" class="checkbox checkbox-xs" checked={network} onChange={(event) => setNetwork(event.currentTarget.checked)} />
              <label for="scan-network" class="text-sm font-medium">Network (IP) Printers</label>
            </legend>

            <div class="space-y-1">
              <p class={FIELD_LABEL}>Known networks</p>
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
                        class={CHECKBOX}
                        checked={!unchecked.includes(entry.subnet)}
                        disabled={!network || customActive}
                        onChange={() => toggleKnown(entry.subnet)}
                      />
                      <label for={checkboxId(entry.subnet)} class="font-mono text-sm">{entry.subnet}</label>
                      {entry.interface && <span class="text-xs text-base-content/60">{entry.interface}</span>}
                    </div>
                  ))}
                  {/* A skipped adapter states the shared layer's own reason,
                      and this panel's own remedy for it. `detect_networks`
                      reports one entry per address rather than per adapter, so
                      two too-large addresses on one interface arrive as two
                      rows and only the position tells them apart. */}
                  {data.skipped.map((entry, index) => (
                    <div key={checkboxId(`skipped-${index}`)} class="flex items-center gap-2">
                      <input id={checkboxId(`skipped-${index}`)} type="checkbox" class={CHECKBOX} checked={false} disabled />
                      <label for={checkboxId(`skipped-${index}`)} class="font-mono text-sm text-base-content/60">{entry.subnet ?? entry.interface}</label>
                      <span class="text-xs text-base-content/60">{skippedExplanation(entry)}</span>
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
              <label class={FIELD_LABEL} for="scan-custom">Custom network</label>
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
                  {/* `min` follows `timeoutValid`: zero is what `--timeout 0`
                      accepts, so neither the spinner nor `:out-of-range` may
                      say the panel rejects it. */}
                  <input
                    id="scan-timeout"
                    type="number"
                    min="0"
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
            {network && data && !timeoutValid && <p role="alert" class="text-xs text-warning">Enter a timeout as a whole number of milliseconds.</p>}
          </fieldset>
        </div>
        </div>
      )}

      {/* Outside the disclosure, so the controls that act on the scope are
          there whether or not the fields are. Reset holds the left because
          the probe count it used to share the bar with is up on the line,
          where it is readable without opening anything.

          Wraps rather than crushes: the manual-add label is long, and at
          phone width the two actions take a row of their own, still
          trailing. */}
      <footer class="flex flex-wrap items-center gap-2 border-t border-base-300 px-4 py-3">
        <button type="button" class="btn btn-sm" onClick={reset}>Reset</button>
        <span class="ml-auto flex gap-2">{actions}</span>
      </footer>
    </div>
  );
}
