import { existsSync, readFileSync, readdirSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

const dist = (file: string) => resolve(__dirname, "..", "dist", file);
const CONTENT_SCRIPTS = ["relay.js", "ws-patch.js", "qz-shim.js"];

describe("the built extension", () => {
  it("emits every file the manifest names", () => {
    for (const file of ["background.js", "relay.js", "ws-patch.js", "qz-shim.js", "popup.js", "popup.html", "manifest.json"]) {
      expect(existsSync(dist(file)), `${file} missing from dist/`).toBe(true);
    }
  });

  it("emits content scripts with no import or export at all", () => {
    // Chrome loads content scripts as CLASSIC scripts. A bare `import` is a syntax
    // error there, so a chunked build makes ws-patch.js fail to execute and leaves
    // window.WebSocket unpatched (W1) with nothing logged on the page that broke.
    for (const file of CONTENT_SCRIPTS) {
      const source = readFileSync(dist(file), "utf8");
      expect(source, `${file} contains a static import`).not.toMatch(/^\s*import[\s{*"']/m);
      expect(source, `${file} contains an export`).not.toMatch(/^\s*export[\s{*]/m);
    }
  });

  it("inlines the shared modules rather than emitting them as loadable chunks", () => {
    expect(existsSync(dist("qz-jobs.js"))).toBe(false);
    expect(existsSync(dist("aliases.js"))).toBe(false);
    // ...and the shared code really is inside the content script that needs it.
    expect(readFileSync(dist("ws-patch.js"), "utf8")).toContain("escpost does not implement the QZ Tray call");
  });

  it("ships the files the worker registers, even though the manifest names none of them", () => {
    // The scripts are registered at runtime for granted sites, so nothing in
    // the manifest points at them and only their presence in dist/ proves the
    // build still emits them.
    for (const file of ["relay.js", "ws-patch.js", "qz-shim.js"]) {
      expect(existsSync(dist(file)), file).toBe(true);
    }
  });

  it("asks for no site access at install, only its own named hosts", () => {
    const manifest = JSON.parse(readFileSync(dist("manifest.json"), "utf8"));

    expect(manifest.host_permissions).toContain("http://127.0.0.1:9000/*");
    for (const pattern of manifest.host_permissions as string[]) {
      const host = pattern.split("://")[1]?.split("/")[0] ?? "";
      expect(host, pattern).not.toContain("*");
    }
    // Site access stays in the optional pool, which grants nothing on install.
    expect(manifest.optional_host_permissions).toEqual(["https://*/*", "http://*/*"]);

    // The field this test used to ignore. A declared content script generates
    // the same install warning as a host permission, so <all_urls> here would
    // have asked for every site while the assertions above said otherwise.
    // Only the auth bridge is declared, and only on one path of our own API.
    const declared = manifest.content_scripts as Array<{ js: string[]; matches: string[] }>;
    expect(declared.map((entry) => entry.js)).toEqual([["auth-bridge.js"]]);
    for (const entry of declared) {
      for (const pattern of entry.matches) {
        expect(pattern, "a declared script must not match every site").not.toContain("<all_urls>");
        const host = pattern.split("://")[1]?.split("/")[0] ?? "";
        expect(host, pattern).not.toContain("*");
      }
    }
  });
});

describe("the vendored QZ client", () => {
  it("never reaches a shipped artifact", () => {
    // vendor/qz-tray.js is QZ Industries' own code under LGPL-2.1-only, kept
    // for the conformance tests. This repository is Apache-2.0, and shipping
    // the file, or any of it, would put LGPL code in a distributed binary.
    // Nothing enforced that before this test: it was true by habit.
    const vendored = readFileSync(resolve(__dirname, "..", "vendor", "qz-tray.js"), "utf8");
    const fingerprint = "QZ Tray Connector";
    expect(vendored, "the vendored file should be the real client").toContain(fingerprint);

    for (const file of readdirSync(resolve(__dirname, "..", "dist"))) {
      if (!file.endsWith(".js")) continue;
      expect(readFileSync(dist(file), "utf8"), file).not.toContain(fingerprint);
    }
  });
});

