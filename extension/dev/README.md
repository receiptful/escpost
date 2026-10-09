# dev

Things used while developing the extension. None of it is bundled, and the
build never reads this directory.

Serve the extension directory, so both pages can reach `vendor/`:

```bash
cd extension && python3 -m http.server 8081
```

- **http://127.0.0.1:8081/dev/manual-page/** exercises escpost's own injected
  `qz` surface, the case where a page has no QZ client of its own.

- **http://127.0.0.1:8081/dev/qz-tray-page/** loads the real `qz-tray.js` from
  `vendor/` and prints through it. This is the till that cannot be changed:
  the client is QZ's, and escpost reaches it only by having replaced
  `WebSocket` before the page loaded. It is the one path unit tests cannot
  fully stand in for, because the timing is the thing being tested.

Both run their checks on load and report which leg of the chain works. Grant the
site in the extension popup first, then reload, or nothing is injected and every
check fails at the first row.

`vendor/qz-tray.js` is QZ Industries' work under LGPL-2.1-only. It is served
here for local testing and is never built into anything shipped.
