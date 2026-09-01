# @escpost/browser

`@escpost/browser` provides a small browser-side API for discovering configured
printers and sending raw ESC/POS bytes to one of them.

## Install

Install the package with your JavaScript package manager:

```bash
bun add @escpost/browser
```

Then import the named `escpost` client:

```ts
import { escpost } from "@escpost/browser";
```

The local ESCPost daemon must be serving its API. Start the API without the
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

## Print raw bytes

`print()` sends a raw print request and resolves with a `jobId`. `data` accepts
either a `Uint8Array` or a string. Strings are encoded as UTF-8; use a
`Uint8Array` when the receipt requires exact byte values.

```ts
const receiptBytes = new Uint8Array([0x1b, 0x40, 0x1b, 0x64, 0x03]);
const result = await escpost.print({ printer: "counter", data: receiptBytes });
console.log(result.jobId);
```

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

```ts
import { EscpostError, escpost } from "@escpost/browser";

try {
  await escpost.print({ printer: "counter", data: receiptBytes });
} catch (error) {
  if (error instanceof EscpostError) {
    reportPrintFailure(error.code, error.message);
  }
}
```
