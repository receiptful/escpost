import { readFileSync } from "node:fs";
import { expect, test } from "vitest";

test("the installable manifest limits printing access to the local daemon", () => {
  // Break caught: broadening required permissions or daemon host access lets the
  // extension run against sites or ports beyond the local ESCPost daemon.
  const manifest = JSON.parse(
    readFileSync("dist/manifest.json", "utf8"),
  );

  expect(manifest.manifest_version).toBe(3);
  expect(manifest.minimum_chrome_version).toBe("114");
  expect(manifest.permissions).toEqual(["storage", "activeTab", "scripting"]);
  expect(manifest.optional_host_permissions).toEqual(["http://*/*", "https://*/*"]);
  expect(manifest.host_permissions).toEqual([
    "http://127.0.0.1:9000/*",
    "http://127.0.0.1:9001/*",
    "http://127.0.0.1:9002/*",
    "http://127.0.0.1:9003/*",
    "http://127.0.0.1:9004/*",
    "http://127.0.0.1:9005/*",
    "http://127.0.0.1:9006/*",
    "http://127.0.0.1:9007/*",
    "http://127.0.0.1:9008/*",
    "http://127.0.0.1:9009/*",
  ]);
  expect(manifest).not.toHaveProperty("options_page");
  expect(manifest).not.toHaveProperty("content_scripts");
  expect(JSON.stringify(manifest)).not.toMatch(/receiptful|qz/i);
});
