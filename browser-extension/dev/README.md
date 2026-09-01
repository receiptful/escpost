# Browser extension local tooling

The extension-local Compose file provides repeatable build, test, and static-page tooling without changing the root Compose stack.

From the repository root, use these non-interactive commands:

```bash
docker compose -f browser-extension/compose.yaml \
  run --rm browser-extension-test
docker compose -f browser-extension/compose.yaml \
  run --rm browser-extension-build
```

Run the API without its web app in a separate terminal:

```bash
docker compose run --rm -e ESCPOST_WATCH=0 escpost serve \
  --web-listen 127.0.0.1:9000 --no-web-app --non-interactive
```

To host built extension-local pages on loopback port 8081, first run the build command above, then run:

```bash
docker compose -f browser-extension/compose.yaml up browser-extension-pages
```
