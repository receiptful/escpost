# @receiptful.io/escpost

`@receiptful.io/escpost` provides a small browser-side API for discovering configured
printers and sending raw ESC/POS bytes to one of them.

This package is browser-only. It does not currently provide a Node.js transport
for backend applications.

## Requirements

Printing goes through the
[Receiptful ESCPost browser extension](https://github.com/receiptful/escpost/tree/main/browser-extension),
which connects the website to an ESCPost binary running on the same computer.
Before using the SDK:

1. Install the ESCPost binary and start its local API.
2. Install the matching browser extension.
3. Use the extension popup to allow the website's exact origin.

The extension manifests support Chrome 114 or newer and Firefox 121 or newer.
Installing this npm package alone does not install the extension or start the
ESCPost binary.

## Install

Install the package with your JavaScript package manager:

```bash
npm install @receiptful.io/escpost
pnpm add @receiptful.io/escpost
yarn add @receiptful.io/escpost
bun add @receiptful.io/escpost
```

Then import the named `escpost` client:

```ts
import { escpost } from "@receiptful.io/escpost";
```

## Development

The SDK owns its build and test tooling. From the repository root, run:

```bash
docker compose -f sdks/javascript/compose.yaml run --rm test
docker compose -f sdks/javascript/compose.yaml run --rm build
```

The build writes the publishable package to `sdks/javascript/dist/`. The root
shortcut for that build is `just javascript-sdk-build`.

The local ESCPost binary must be serving its API. Start the API without the
embedded web application when the browser application provides its own UI:

```bash
escpost serve --web-listen 127.0.0.1:9000 --no-web-app
```

## Check availability

`isAvailable()` is a one-shot health check. It resolves to `true` when the
printing service answers and `false` for any unavailable state, so it is safe to
use when deciding whether to show a print action:

```ts
if (await escpost.isAvailable()) {
  showPrintAction();
}
```

## List and watch printers

`printers.list()` returns the current inventory and can wait for network probes
to finish. Its optional `transport` filter accepts `"usb"` or `"network"`.

`printers.subscribe()` is a separate streaming operation. Its first callback
receives the retained/current snapshot from the daemon's SSE stream; later
callbacks receive inventory updates. It does not call `list()` to create that
first snapshot. The returned function stops the subscription.

```ts
const snapshot = await escpost.printers.list();
const stop = escpost.printers.subscribe(
  ({ printers, warning }) => renderPrinters(printers, warning),
  { onError: showPrinterConnectionError },
);
await escpost.print({ printer: snapshot.printers[0].name, data: receiptBytes });

// When the page no longer needs updates:
stop();
```

Each printer includes its configured `name`, `transport`, `availability`,
optional `profile`, and connection details. Pass the configured printer name
exactly to `print()`; the SDK does not derive another identifier.

## Read printer profiles

`profiles.list()` returns ESCPost's printer profile catalog. A printer's
`profile` is the `id` of one of these profiles, or `null` when none is set
(`escpost printers add --profile`). Each profile gives the printer's real
geometry: `printableWidthDots` is the width the head can print, and `dpiX` and
`dpiY` its resolution.

```ts
const [{ printers }, { profiles }] = await Promise.all([escpost.printers.list(), escpost.profiles.list()]);
const profile = profiles.find(({ id }) => id === printers[0].profile);
const width = profile?.printableWidthDots ?? 384;
```

## Print raw bytes

`print()` sends a raw print request and resolves with a `jobId`. `data` accepts
either a `Uint8Array` or a string. Strings are encoded as UTF-8; use a
`Uint8Array` when the receipt requires exact byte values.

```ts
const receiptBytes = new Uint8Array([0x1b, 0x40, 0x1b, 0x64, 0x03]);
const result = await escpost.print({ printer: "counter", data: receiptBytes });
console.log(result.jobId);
```

## Migrate from QZ Tray

The package includes a QZ Tray compatibility shim: a `qz` object with the same
API as QZ Tray's `qz-tray.js`, which prints through ESCPost instead. A page keeps
its `qz.*` calls and only changes where `qz` comes from:

```ts
// before: import qz from "qz-tray";
import qz from "@receiptful.io/escpost/qz";
```

A page that loads `qz-tray.js` with a script tag loads `escpost-qz.js` instead,
which defines `window.qz`:

```html
<!-- before: <script src="qz-tray.js"></script> -->
<script src="escpost-qz.js"></script>
```

`escpost-qz.js` is published in the package's `dist/` folder. To print PDFs,
also serve the `dist/pdfjs/` folder next to it: the shim loads pdf.js from
there on the first PDF. pdf.js ships in the package, so PDF printing works
offline, and a page that prints no PDF never downloads it.

`qz.security` calls are accepted and ignored, so no certificate is needed and no
popup appears. Raw jobs are sent as they are, with plain text encoded in the
config's `encoding`, as QZ Tray does. Image, HTML and PDF jobs are drawn in the
browser and printed as an ESC/POS image; a raw one that asks for another printer
language, such as ZPL or EPL, rejects with `UNSUPPORTED_FORMAT`. They are drawn
for the printer's profile: its printable width and resolution. Give each
printer its profile (`escpost printers add --profile`); a printer without one
is drawn for a 58 mm roll at 203 dpi (384 dots wide), or for the paper `size`
the config gives, so nothing prints past a narrower head than assumed. QZ Tray features that ESCPost does not
offer (`qz.serial`, `qz.usb`, `qz.hid`, `qz.file`, `qz.socket` and printer
status) reject with `UNSUPPORTED_FEATURE`.

## Errors

Operations may reject with `EscpostError`, whose `code` identifies the failure:

| Code | Meaning |
| --- | --- |
| `EXTENSION_UNAVAILABLE` | The browser relay did not answer in time or is unavailable. |
| `ORIGIN_NOT_GRANTED` | The current page is not permitted to use the relay. |
| `DAEMON_UNAVAILABLE` | The local ESCPost daemon is unavailable. |
| `PRINTER_NOT_FOUND` | No configured printer has the requested exact name. |
| `PRINT_FAILED` | The daemon could not complete the raw print job. |
| `PROTOCOL_MISMATCH` | A response did not match the SDK protocol. |
| `UNSUPPORTED_FORMAT` | The QZ shim cannot print this kind of data. |
| `UNSUPPORTED_FEATURE` | The QZ shim was asked for a QZ Tray feature that ESCPost does not offer. |

```ts
import { EscpostError, escpost } from "@receiptful.io/escpost";

try {
  await escpost.print({ printer: "counter", data: receiptBytes });
} catch (error) {
  if (error instanceof EscpostError) {
    reportPrintFailure(error.code, error.message);
  }
}
```
