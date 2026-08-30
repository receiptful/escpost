# @escpost/browser

Print raw ESC/POS to a local thermal printer from a web page.

The page calls this package, the escpost browser extension relays the job to
escpost on loopback, and escpost sends the bytes to a USB or network printer.
No certificate to install, no per-print dialog, and raw jobs never leave the
machine.

```
your page ──▶ @escpost/browser ──▶ extension ──▶ escpost ──▶ printer
```

## Requirements

- The escpost browser extension, installed and granted access to your site.
- escpost running locally, with at least one printer configured
  (`escpost printers add`).

Both are the operator's machine, not yours. The package never opens a socket
of its own: it posts a message to the page and waits for the extension to
answer.

## Install

Not published to npm yet. Until it is, build it from this repository:

```bash
bun install && bun run build   # from the repository root
```

## Check it will work

```js
if (await escpost.isAvailable()) {
  // extension installed, site granted, daemon answering
}
```

Resolves `false` instead of throwing, so it is safe to call on page load to
decide whether to offer printing at all. When you need to know why it is
false, call `printers.list()` and read the error code.

## Print raw ESC/POS

```js
import { escpost } from "@escpost/browser";

await escpost.print({
  printer: "TM-T20",
  data: "\x1b@Hello\n\n\n\x1dV\x00",   // string or Uint8Array
});
```

`data` may be a string or a `Uint8Array`. Strings are encoded as UTF-8; if you
need a specific code page, send bytes.

## List printers

```js
const printers = await escpost.printers.list();
// [{ id: "tm-t20", name: "TM-T20", transport: "usb", profile: "NT-5890K", status: "ready" }]

const fallback = await escpost.printers.getDefault();   // Printer | null
```

Print to a printer by its `id` or its `name`.

## Watch for printers appearing and going away

```js
const stop = escpost.printers.subscribe((printers) => {
  const ready = printers.filter((p) => p.status === "ready");
  render(ready);
});

// later
stop();
```

The listener is called once with the current list, then only when it changes.
A till learns its printer went offline without waiting for a customer to be
standing there.

This polls, every 5 seconds by default (`{ intervalMs }`, floor 250ms). The
page and the extension speak request and reply through one relay, which has
nothing to push with. If a push route is built later, this signature does not
change. A failed poll reports an empty list and recovers on its own.

## Print HTML

```js
await escpost.print({ printer: "TM-T20", html: "<h1>Total 6.50</h1>" });
```

This package contains no renderer. HTML is forwarded to the extension, which
renders it server-side and prints the bytes that come back, so it needs an
account and network access, and it is the one call here that is neither local
nor free. Raw ESC/POS never leaves the machine.

## Handling failures

Every rejection is an `EscpostError` with a `code`:

```js
import { escpost, EscpostError } from "@escpost/browser";

try {
  await escpost.print({ printer: "TM-T20", data: bytes });
} catch (error) {
  if (error instanceof EscpostError && error.code === "EXTENSION_NOT_INSTALLED") {
    // Offer the install link rather than a stack trace.
  }
}
```

| Code | Means |
|---|---|
| `EXTENSION_NOT_INSTALLED` | nothing answered: no extension, or this site is not granted |
| `ORIGIN_NOT_GRANTED` | the extension is there but this origin may not print |
| `DAEMON_NOT_RUNNING` | escpost is not running |
| `PRINTER_NOT_FOUND` | no printer matches that id or name |
| `PRINT_FAILED` | the job reached the printer path and failed, or timed out |
| `UNSUPPORTED_FORMAT` | this package prints raw ESC/POS and HTML, nothing else |
| `NOT_SIGNED_IN` · `QUOTA_EXCEEDED` · `RENDER_FAILED` · `RENDER_UNAVAILABLE` | HTML rendering only; raw printing is unaffected |

`instanceof` works: the package rebuilds a real `EscpostError` on this side of
the boundary. `isEscpostError(value)` is exported for the other case, checking a
value that crossed a `postMessage` itself and arrived without its prototype.

## Notes

- **Browser only.** It uses `window.postMessage`; there is nothing to run in
  Node.
- **Timeouts.** 2s for a query, 20s for a raw print, 30s for HTML. A print
  may be waiting on a USB device opening.
- **No dependencies**, and no network access of its own. The bundle is
  unminified on purpose: people audit code that talks to their printers.
