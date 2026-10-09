# vendor

Third-party code, kept here unmodified and under its own licence. Nothing in
this directory is Apache-2.0, and nothing here is built into a shipped
artifact.

## qz-tray.js

QZ Tray Connector 2.2.6, by QZ Industries, LLC, under **LGPL-2.1-only**.

It is used by `tests/qz-conformance.test.ts` and nowhere else. The extension's
QZ compatible surface is our own implementation; this file is the real library,
kept so those tests measure the shim against QZ Tray itself rather than against
our reading of it.

The file is unmodified, its licence header is intact, and it is excluded from
`dist/`, so no LGPL code is distributed with the extension or the npm package.

The tests pin the version they were written against, so replacing this file
fails loudly rather than passing quietly against different code.

QZ Tray's own licensing page places its API, demo code and wiki examples in the
public domain, which is what our compatible implementation is written against.
