import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

const dist = (file: string) => resolve(__dirname, "..", "dist", file);

describe("the built package", () => {
  it("emits an ES module and its types", () => {
    expect(existsSync(dist("index.js"))).toBe(true);
    expect(existsSync(dist("index.d.ts"))).toBe(true);
  });

  it("touches no extension API — this code runs in the page, not the extension", () => {
    const bundle = readFileSync(dist("index.js"), "utf8");
    expect(bundle).not.toMatch(/\bchrome\./);
    expect(bundle).not.toMatch(/chrome\.runtime/);
  });

  it("carries no renderer and no account logic", () => {
    const bundle = readFileSync(dist("index.js"), "utf8");

    // No renderer and no billing path, by name...
    for (const forbidden of ["html2escpos", "billing", "renderHtml", "chargeQuota"]) {
      expect(bundle).not.toContain(forbidden);
    }

    // ...and the stronger guarantee: this package cannot reach the network at
    // all, so it cannot be doing anything off-box under any name.
    expect(bundle).not.toMatch(/\bfetch\s*\(/);
    expect(bundle).not.toMatch(/XMLHttpRequest|WebSocket|EventSource/);
    expect(bundle).not.toMatch(/https?:\/\/(?!127\.0\.0\.1|localhost)/);
  });

  it("ships the documented error codes", () => {
    const bundle = readFileSync(dist("index.js"), "utf8");
    expect(bundle).toContain("RENDER_UNAVAILABLE");
    expect(bundle).toContain("QUOTA_EXCEEDED");
  });

  it("exports the documented surface", async () => {
    const module = await import(dist("index.js"));
    expect(typeof module.escpost.print).toBe("function");
    expect(typeof module.escpost.printers.list).toBe("function");
    expect(typeof module.EscpostError).toBe("function");
  });
});

describe("the qz drop-in", () => {
  it("installs window.qz, which is the whole point of importing it", () => {
    const bundle = readFileSync(dist("qz.js"), "utf8");
    expect(bundle).toContain("A connection to QZ has not been established");
    expect(bundle).toMatch(/\.qz\s*=/);
  });

  it("is declared to have side effects, or a bundler deletes the import", () => {
    // `import "@escpost/browser/qz"` exists only for what it does on load. A
    // package marked side-effect free lets a bundler drop that line entirely,
    // which is exactly what happened to the extension's own build first.
    const manifest = JSON.parse(readFileSync(resolve(__dirname, "..", "package.json"), "utf8"));
    expect(manifest.sideEffects).toContain("./dist/qz.js");
  });

  it("keeps the main entry from touching a page's globals", () => {
    // Importing the SDK must not install a qz global on a page that never
    // asked for QZ compatibility.
    expect(readFileSync(dist("index.js"), "utf8")).not.toMatch(/\.qz\s*=/);
  });
});

describe("the drop-in matches how qz-tray is actually consumed", () => {
  it("has a default export, because qz-tray's npm package is used that way", async () => {
    // qz-tray publishes a UMD file with no `module` or `exports` field, so its
    // consumers write `import qz from "qz-tray"` and get an object. A named
    // export alone would hand them undefined.
    const module = await import(dist("qz.js"));

    expect(typeof module.default).toBe("object");
    expect(typeof module.default.websocket.connect).toBe("function");
    expect(typeof module.default.print).toBe("function");
    expect(typeof module.default.configs.create).toBe("function");
  });

  it("hands the importer the same object it installs on the page", async () => {
    const module = await import(dist("qz.js"));
    expect(module.default.version).toBe(module.createQzShim().version);
  });
});
