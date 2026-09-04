# Receiptful ESCPost browser extension

The browser extension connects websites using `@receiptful/escpost` to the
ESCPost daemon running on the same computer. Websites use the SDK API; they do
not call the daemon or depend on browser-extension internals.

## Browser transports

Chrome and Firefox expose different website-to-extension APIs, so the SDK uses
the least-privileged transport available in each browser.

### Chrome: extension iframe bridge

Chrome does not allow `externally_connectable` to cover arbitrary websites.
Instead, the SDK creates a hidden iframe containing a web-accessible extension
page. That page runs in the extension's origin and communicates with the SDK
through `window.postMessage()`. The Chrome build does not inject a content
script into the website and does not request access to read or change website
data.

The extension ID is part of this protocol because Chrome requires websites to
locate the extension iframe. Unpacked builds use the public `key` in the
Chrome manifest so their ID is stable. A Chrome Web Store release must update
that public key and the SDK's default extension ID together.

Making the bridge page available to HTTP and HTTPS sites does not authorize
printing. The bridge reads the parent origin from the browser's message event,
and the popup stores each approved origin in extension-local storage. The bridge
checks that store for every request and live-printer connection and rejects
unapproved origins.

### Firefox: isolated relay

Firefox does not expose extension messaging APIs to ordinary websites. After
the user approves an exact website origin, the Firefox build requests an
optional host permission and registers `relay.js` for that origin. The relay
runs as an isolated content script and carries validated SDK messages between
the page's `window.postMessage()` channel and Firefox's internal extension
messaging APIs.

The relay does not inspect or modify the page DOM. Firefox nevertheless labels
the host permission as access to website data because that is the capability a
content script receives. Revoking the origin removes the relay registration and
stops its live printer connections.

### Shared SDK and extension core

`@receiptful/escpost` selects the Chrome iframe bridge on Chromium browsers and
otherwise uses the Firefox relay protocol. Callers use the same
`isAvailable()`, `printers.list()`, `printers.subscribe()`, and `print()` API in
both browsers.

Both extension builds share request validation, exact-origin authorization,
daemon discovery, raw printing, typed errors, and printer-stream handling.
Only their browser-facing messaging, origin-grant adapters, runtime entry
points, popup wiring, and manifests differ.

## Build and test

From the repository root:

```bash
docker compose -f browser-extension/compose.yaml run --rm browser-extension-test
docker compose -f browser-extension/compose.yaml run --rm browser-extension-build
```

The build produces installable extensions under `browser-extension/dist/chrome/`
and `browser-extension/dist/firefox/`.
