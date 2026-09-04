import { readdirSync, readFileSync } from "node:fs";
import { expect, test } from "vitest";

test("each browser build contains only the files its transport needs", () => {
  // Break caught: omitting an entry artifact, adding undeclared build output, or
  // emitting a classic relay that needs module imports prevents installation.
  expect(readdirSync("dist").sort()).toEqual(["chrome", "firefox"]);
  expect(readdirSync("dist/chrome").sort()).toEqual([
    "bridge.html",
    "bridge.js",
    "icons",
    "manifest.json",
    "popup.css",
    "popup.html",
    "popup.js",
  ]);
  expect(readdirSync("dist/firefox").sort()).toEqual([
    "background.js",
    "icons",
    "manifest.json",
    "popup.css",
    "popup.html",
    "popup.js",
    "relay.js",
  ]);
  expect(readdirSync("dist/chrome/icons").sort()).toEqual([
    "icon-128.png",
    "icon-16.png",
    "icon-32.png",
    "icon-48.png",
  ]);
  expect(readFileSync("dist/firefox/relay.js", "utf8")).not.toMatch(
    /\bimport\s*(?:[({*]|["'])/,
  );
});
