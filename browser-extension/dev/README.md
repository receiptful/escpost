# Browser extension local tooling

The extension-local Compose file provides repeatable build, test, and static-page tooling without changing the root Compose stack.

From the repository root, use these non-interactive commands:

```bash
docker compose -f browser-extension/compose.yaml \
  run --rm browser-extension-test
docker compose -f browser-extension/compose.yaml \
  run --rm browser-extension-build
```

## Manual SDK raw-print path

From the repository root, run the daemon, build the extension, and host the
static page:

```bash
docker compose up escpost
docker compose -f browser-extension/compose.yaml run --rm browser-extension-build
docker compose -f browser-extension/compose.yaml up browser-extension-pages
```

In Chrome, load `browser-extension/dist/` as an unpacked extension. Open
`http://127.0.0.1:8081/browser-extension/dev/sdk-page/`, use the extension
popup to grant `http://127.0.0.1:8081`, then reload the page. Select an already
configured printer and use **Print exact raw bytes** to send the SDK-only raw
print check.
