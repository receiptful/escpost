import { useEffect, useRef, useState } from "preact/hooks";
import { addPrinter } from "../../api/client";
import type { AddPrinterBody, DiscoveredPrinter, UsbConnection } from "../../api/types";
import { useAppData } from "../../app/data";
import { useServerStatus } from "../../app/server-status-data";
import { endpointHex, usbHex } from "./usb";

// Copied from `printers::add::AMBIGUOUS_USB_WARNING`, which is the source of
// truth and which the registration response repeats in `warnings`. By then
// the printer is saved, so the dialog states it while the decision is still
// open — the terminal shows it after the fact only because its prompt has
// already compared the device against every other one connected.
const AMBIGUOUS_USB_WARNING = "This printer reports no serial number. Printing will be ambiguous while another device with the same USB identity is connected.";

// What `printers add` fills in for `--port`, both as the flag's fallback and
// as the interactive prompt's default.
const DEFAULT_RAW_PORT = 9100;

// The one treatment every field label in this dialog shares. Sentence case,
// deliberately: the capitals used to come from `uppercase`, and the only
// capitals left are the ones the words are actually spelled with — `USB`,
// `IP`, `RAW TCP`.
const FIELD_LABEL = "text-xs font-medium text-base-content/60";

// The identity line the results row shows, minus its endpoint tail: here the
// endpoints are the one thing still being chosen, so they are inputs below
// rather than facts.
function usbFacts(connection: UsbConnection) {
  const parts = [`USB ${usbHex(connection.vendor_id)}:${usbHex(connection.product_id)}`];
  if (connection.bus && connection.address !== null) {
    parts.push(`bus ${connection.bus} addr ${String(connection.address).padStart(3, "0")}`);
  }
  parts.push(connection.serial_number ? `serial ${connection.serial_number}` : "no serial");
  parts.push(`interface ${connection.interface_number}`);
  return parts.join(" · ");
}

function usbTitle(connection: UsbConnection) {
  return connection.product ?? connection.manufacturer ?? `USB ${usbHex(connection.vendor_id)}:${usbHex(connection.product_id)}`;
}

// The selects carry the `0x01` spelling `printers add --out-endpoint` takes,
// so what the reader picks is what the flag would have been given, and an
// unchosen IN endpoint is the empty option rather than a sentinel number.
function endpointNumber(hex: string) {
  return hex === "" ? null : Number(hex);
}

// The route the terminal's menu would start from: every bulk OUT endpoint is
// a separate choice, so the first one is offered and the rest are one select
// away, while an IN endpoint is only assumed when the device exposes exactly
// one — `usb_add_targets` refuses to reduce several to a guess, and so does
// this.
function defaultOutEndpoint(usb: UsbConnection | null) {
  return usb && usb.out_endpoints.length > 0 ? endpointHex(usb.out_endpoints[0]!) : "";
}

function defaultInEndpoint(usb: UsbConnection | null) {
  return usb && usb.in_endpoints.length === 1 ? endpointHex(usb.in_endpoints[0]!) : "";
}

// Which device the open dialog is registering, by the facts that make it that
// device rather than by object identity: an owner re-rendering with an equal
// but freshly built printer must not wipe a route the reader chose, while an
// owner swapping in a different printer must not leave the previous device's
// route behind. The endpoint lists are part of it because they are what the
// selects offer.
function deviceIdentity(printer: DiscoveredPrinter | null) {
  const connection = printer?.connection;
  if (!connection) {
    return "manual";
  }
  return connection.type === "network"
    ? `network:${connection.host}:${connection.port}`
    : [
      "usb",
      connection.vendor_id,
      connection.product_id,
      connection.serial_number ?? "",
      connection.bus ?? "",
      connection.address ?? "",
      connection.interface_number,
      connection.out_endpoints.join("+"),
      connection.in_endpoints.join("+"),
    ].join(":");
}

/**
 * Registration, in the two shapes the printers page needs: a discovered
 * printer whose connection arrives pre-filled and read-only, and — with
 * `printer` null — a manually typed network endpoint.
 *
 * Only what the CLI also asks for is editable: a name, an optional profile,
 * and for a USB device the route to print over, which is precisely the
 * choice `printers add` refuses to make without a terminal.
 *
 * `onClose` must unmount the dialog — nothing here renders a closed one, and
 * the native element is closed in the unmount cleanup.
 *
 * `onAdded` receives the connection that was registered as well as the name,
 * because the owner cannot reconstruct it: for a manual registration the host
 * and port were typed here and exist nowhere else.
 */
export function AddPrinterDialog({ printer, onClose, onAdded }: {
  printer: DiscoveredPrinter | null;
  onClose: () => void;
  onAdded: (name: string, connection: AddPrinterBody["connection"]) => void;
}) {
  const { printers, profiles, ensureProfiles } = useAppData();
  const status = useServerStatus();
  const connection = printer?.connection ?? null;
  const usb = connection?.type === "usb" ? connection : null;
  const discoveredNetwork = connection?.type === "network" ? connection : null;
  const manual = printer === null;

  const element = useRef<HTMLDialogElement>(null);
  const dismiss = useRef(onClose);
  const request = useRef<AbortController | null>(null);
  const [name, setName] = useState("");
  const [profile, setProfile] = useState("");
  const [host, setHost] = useState("");
  const [port, setPort] = useState(String(DEFAULT_RAW_PORT));
  const [outEndpoint, setOutEndpoint] = useState(defaultOutEndpoint(usb));
  const [inEndpoint, setInEndpoint] = useState(defaultInEndpoint(usb));
  const [submitting, setSubmitting] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);

  useEffect(() => {
    dismiss.current = onClose;
  }, [onClose]);

  // The connection facts are read from the prop on every render while the
  // route is held in state, so a dialog handed a different device would
  // otherwise show one printer and submit another's endpoint — a route the
  // new device does not expose, inside `0x01..=0x0f` and therefore saved
  // without complaint by every layer, printing nothing forever after. The
  // route is re-seeded with the device instead, which also covers the mirror
  // case where a manual dialog becomes a USB one and would otherwise have no
  // route at all and no way to say so. A stale failure belongs to the
  // previous device and goes with it.
  //
  // The owner is still expected to mount one dialog per registration; this is
  // what makes the other usage safe rather than silently wrong.
  const identity = deviceIdentity(printer);
  useEffect(() => {
    setOutEndpoint(defaultOutEndpoint(usb));
    setInEndpoint(defaultInEndpoint(usb));
    setFailure(null);
    // `usb` is re-derived on every render and would re-seed the route on
    // every keystroke; `identity` is the fact that actually changed.
  }, [identity]);

  useEffect(() => {
    void ensureProfiles();
  }, [ensureProfiles]);

  useEffect(() => {
    const dialog = element.current;
    if (!dialog) {
      return;
    }
    dialog.showModal();
    // Escape reaches the element as `cancel` and would otherwise close it
    // natively, leaving a hidden dialog the owner still believes is open. It
    // dismisses through the same callback the ✕ button uses instead.
    const cancel = (event: Event) => {
      event.preventDefault();
      dismiss.current();
    };
    dialog.addEventListener("cancel", cancel);
    return () => {
      dialog.removeEventListener("cancel", cancel);
      dialog.close();
      request.current?.abort();
    };
  }, []);

  // Exactly what `configuration::add_*_printer` refuses:
  // `document.contains_key(name)`, an exact match on the TOML key. Folding
  // case would refuse `Kitchen` on a machine where `escpost printers add
  // Kitchen` succeeds beside a configured `kitchen` — a name the terminal
  // takes and the browser would not, which is the one divergence the two
  // interfaces may never have. The check is a convenience, reporting the
  // collision while the name is still being typed, and never the authority:
  // the server answers 409 on the same rule, including for a name another
  // tab registered since this inventory was fetched.
  //
  // The wording is `ApplicationError::PrinterAlreadyConfigured`'s own, so the
  // inline refusal and the server's read the same. (Its `{0:?}` is Rust's
  // debug quoting, which escapes a quote or backslash inside the name; for
  // such a name the server's message is the exact one.)
  const collision = (printers.data?.printers ?? []).some((entry) => entry.name === name);
  const portValid = /^\d+$/.test(port) && Number(port) >= 1 && Number(port) <= 65_535;
  // A device with no bulk OUT endpoint has no route to print over, so the
  // terminal's menu never offers one either; `usb_printer_interface` drops
  // the interface outright, which is why this only guards the request shape
  // rather than explaining itself on screen.
  const routed = !usb || outEndpoint !== "";
  const submittable = name.trim().length > 0
    && !collision
    && routed
    && (!manual || (host.trim().length > 0 && portValid))
    && !submitting;

  const body = (): AddPrinterBody => ({
    // Untrimmed, because `printers add` stores the name it was given and
    // looks it up the same way. Only a name that is *nothing but* whitespace
    // is refused, by `Request::new`.
    name,
    profile: profile === "" ? null : profile,
    connection: usb
      ? {
        type: "usb",
        vendor_id: usb.vendor_id,
        product_id: usb.product_id,
        serial_number: usb.serial_number,
        interface_number: usb.interface_number,
        out_endpoint: Number(outEndpoint),
        in_endpoint: endpointNumber(inEndpoint),
      }
      : discoveredNetwork
        ? { type: "network", host: discoveredNetwork.host, port: discoveredNetwork.port }
        : { type: "network", host, port: Number(port) },
  });

  const submit = async () => {
    const controller = new AbortController();
    request.current = controller;
    setSubmitting(true);
    setFailure(null);
    const submitted = body();
    try {
      const response = await addPrinter(submitted, controller.signal);
      // The response's `warnings` carry the ambiguity advisory this dialog
      // has already shown, so there is nothing left to report; the owner
      // takes it from here and unmounts the dialog. It gets the connection
      // that was saved, not the one it may have handed in: for a manual
      // registration there was none.
      onAdded(response.name, submitted.connection);
    } catch (error) {
      if (controller.signal.aborted) {
        return;
      }
      setSubmitting(false);
      setFailure(error instanceof Error ? error.message : "The printer could not be registered.");
    }
  };

  // `IP printer` matches the button that opens this, which is the reader's
  // word for a printer reached over the network. The transport it registers
  // is still `network`, in the request and in the inventory column.
  const heading = manual ? "Add IP printer" : "Add printer";

  return (
    <dialog ref={element} class="modal" aria-labelledby="add-printer-heading">
      <div class="modal-box space-y-4">
        <header class="flex items-center justify-between gap-3">
          <h2 id="add-printer-heading" class="text-lg font-medium">{heading}</h2>
          <button type="button" class="btn btn-ghost btn-sm btn-square" aria-label="Close" onClick={onClose}>✕</button>
        </header>

        {/* Why adding exists at all. Printing goes through the configured
            list, so a printer the scan just found is not yet something this
            machine can print to — and nothing else in the dialog says so.
            The wording names no discovery, because a manually added printer
            needs exactly the same explanation.

            The path degrades to an empty string when the configuration
            cannot be resolved, which is deliberate in the server status snapshot: a
            configuration problem must not present as a server that is down.
            The clause that would name the file goes with it rather than
            dangling over an empty code span. */}
        <p class="text-xs text-base-content/60">
          You can only print to printers you have added to your list of configured printers
          {status.snapshot?.config_path
            ? <>, stored in <span class="font-mono">{status.snapshot.config_path}</span></>
            : ""}.
        </p>

        <div class="space-y-1">
          <label class={FIELD_LABEL} for="add-printer-name">Name</label>
          <input
            id="add-printer-name"
            type="text"
            class={`input input-sm w-full ${collision ? "input-warning" : ""}`}
            placeholder="warehouse"
            value={name}
            onInput={(event) => setName(event.currentTarget.value)}
          />
          {collision
            ? <p role="alert" class="text-xs text-warning">{`printer "${name}" is already configured`}</p>
            : <p class="text-xs text-base-content/60">(must be unique)</p>}
        </div>

        <div class="space-y-1">
          <div class="flex items-baseline gap-2">
            <label class={FIELD_LABEL} for="add-printer-profile">Profile</label>
            <span class="text-xs text-base-content/60">optional</span>
          </div>
          {/* The catalog's ids and nothing else, where `printers add
              --profile` takes free text. It is the one input where the
              browser accepts strictly less than the terminal, which the spec
              and the study both call for: a profile is chosen from the
              catalog here, and an id that is in neither would only fail later
              as `UnknownProfile` at print time. */}
          <select
            id="add-printer-profile"
            class="select select-sm w-full"
            value={profile}
            onChange={(event) => setProfile(event.currentTarget.value)}
          >
            <option value="">No profile</option>
            {(profiles.data?.profiles ?? []).map((entry) => <option key={entry.id} value={entry.id}>{entry.id}</option>)}
          </select>
          <p class="text-xs text-base-content/60">From the profile catalog. Can be assigned later during calibration.</p>
        </div>

        {usb && (
          <>
            <div class="space-y-1">
              <p class={FIELD_LABEL}>Connection · USB</p>
              <div class="rounded-box bg-base-200 px-3 py-2 text-xs">
                <p class="font-medium">{usbTitle(usb)}</p>
                <p class="font-mono text-base-content/70">{usbFacts(usb)}</p>
              </div>
            </div>

            {/* Both selects are rendered whatever the device offers, disabled
                at their only value when it offers no choice, so the dialog is
                the same height for every printer and the route never looks
                like something this interface cannot set. */}
            <div class="flex gap-3">
              <div class="flex-1 space-y-1">
                <label class="text-xs text-base-content/70" for="add-printer-out-endpoint">OUT endpoint</label>
                <select
                  id="add-printer-out-endpoint"
                  class="select select-sm w-full font-mono"
                  value={outEndpoint}
                  disabled={usb.out_endpoints.length < 2}
                  onChange={(event) => setOutEndpoint(event.currentTarget.value)}
                >
                  {usb.out_endpoints.length === 0 && <option value="">None</option>}
                  {usb.out_endpoints.map((endpoint) => <option key={endpoint} value={endpointHex(endpoint)}>{endpointHex(endpoint)}</option>)}
                </select>
              </div>
              <div class="flex-1 space-y-1">
                <div class="flex items-baseline gap-2">
                  <label class="text-xs text-base-content/70" for="add-printer-in-endpoint">IN endpoint</label>
                  <span class="text-xs text-base-content/60">optional</span>
                </div>
                <select
                  id="add-printer-in-endpoint"
                  class="select select-sm w-full font-mono"
                  value={inEndpoint}
                  disabled={usb.in_endpoints.length === 0}
                  onChange={(event) => setInEndpoint(event.currentTarget.value)}
                >
                  <option value="">None</option>
                  {usb.in_endpoints.map((endpoint) => <option key={endpoint} value={endpointHex(endpoint)}>{endpointHex(endpoint)}</option>)}
                </select>
              </div>
            </div>
            <p class="text-xs text-base-content/60">Where print data is written. A device exposing a single route offers no choice.</p>

            {usb.serial_number === null && (
              <p role="alert" class="alert alert-warning alert-soft text-xs">{AMBIGUOUS_USB_WARNING}</p>
            )}
          </>
        )}

        {discoveredNetwork && (
          <div class="space-y-1">
            <p class={FIELD_LABEL}>Connection · Network</p>
            <div class="rounded-box bg-base-200 px-3 py-2 text-xs">
              <p class="font-mono">{`${discoveredNetwork.host}:${discoveredNetwork.port}`}</p>
              {printer?.interface && <p class="text-base-content/70">Answered on {printer.interface}</p>}
            </div>
          </div>
        )}

        {manual && (
          <div class="space-y-1">
            <div class="flex gap-3">
              <div class="flex-2 space-y-1">
                <label class={FIELD_LABEL} for="add-printer-host">Host</label>
                <input
                  id="add-printer-host"
                  type="text"
                  class="input input-sm w-full font-mono"
                  placeholder="10.0.5.20 or printer.local"
                  value={host}
                  onInput={(event) => setHost(event.currentTarget.value)}
                />
              </div>
              <div class="flex-1 space-y-1">
                <label class={FIELD_LABEL} for="add-printer-port">Port</label>
                <input
                  id="add-printer-port"
                  type="number"
                  min="1"
                  max="65535"
                  class="input input-sm w-full"
                  value={port}
                  onInput={(event) => setPort(event.currentTarget.value)}
                />
              </div>
            </div>
            {portValid
              ? <p class="text-xs text-base-content/60">Not reachable right now is fine — it lists as Unavailable until it answers.</p>
              : <p role="alert" class="text-xs text-warning">Enter a port between 1 and 65535.</p>}
          </div>
        )}

        {failure && <p role="alert" class="alert alert-error alert-soft text-xs">{failure}</p>}

        <footer class="modal-action">
          <button type="button" class="btn btn-sm" onClick={onClose}>Cancel</button>
          <button type="button" class="btn btn-primary btn-sm" disabled={!submittable} onClick={() => void submit()}>Add printer</button>
        </footer>
      </div>
    </dialog>
  );
}
