import { execFileSync } from "node:child_process";
import { readdirSync, readFileSync } from "node:fs";
import { expect, test } from "vitest";

function buildExtension() {
  execFileSync("bun", ["--cwd", "browser-extension", "run", "build"], {
    stdio: "pipe",
  });
}

test("the extension build emits only the installable worker, relay, popup, manifest, and icons", () => {
  // Break caught: omitting an entry artifact, adding undeclared build output, or
  // emitting a classic relay that needs module imports prevents installation.
  buildExtension();

  expect(readdirSync("browser-extension/dist").sort()).toEqual([
    "background.js",
    "icons",
    "manifest.json",
    "popup.css",
    "popup.html",
    "popup.js",
    "relay.js",
  ]);
  expect(readdirSync("browser-extension/dist/icons").sort()).toEqual([
    "icon-128.png",
    "icon-16.png",
    "icon-32.png",
    "icon-48.png",
  ]);
  expect(readFileSync("browser-extension/dist/relay.js", "utf8")).not.toMatch(
    /\bimport\s*(?:[({*]|["'])/,
  );
});
