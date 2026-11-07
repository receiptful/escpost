import { readFileSync } from "node:fs";
import { test, expect } from "vitest";
import type { ErrorCode } from "../src/index";

test("the package exports the raw browser entry and the QZ Tray shim", () => {
  const manifest = JSON.parse(
    readFileSync("package.json", "utf8"),
  );

  expect(Object.keys(manifest.exports)).toEqual([".", "./qz"]);
  expect(manifest.exports["."]).toEqual({ types: "./dist/index.d.ts", import: "./dist/index.js" });
  expect(manifest.exports["./qz"]).toEqual({
    types: "./dist/qz/index.d.ts",
    import: "./dist/qz.js",
    require: "./dist/qz.cjs",
  });
});

test("the package publishes publicly from the canonical repository", () => {
  const manifest = JSON.parse(
    readFileSync("package.json", "utf8"),
  );

  expect(manifest.name).toBe("@receiptful.io/escpost");
  expect(manifest.publishConfig).toEqual({ access: "public" });
  expect(manifest.repository).toEqual({
    type: "git",
    url: "https://github.com/receiptful/escpost.git",
    directory: "sdks/javascript",
  });
  expect(manifest.homepage).toBe("https://github.com/receiptful/escpost/tree/main/sdks/javascript#readme");
  expect(manifest.bugs).toEqual({ url: "https://github.com/receiptful/escpost/issues" });
});

test("packing rebuilds the browser distribution", () => {
  const manifest = JSON.parse(
    readFileSync("package.json", "utf8"),
  );

  expect(manifest.scripts.prepack).toBe("bun run build");
});

test("the package root exports the documented error-code union", () => {
  // Break caught: omitting ErrorCode from the root module leaves consumers
  // unable to type error handling through the package's only public entry.
  const code: ErrorCode = "PRINTER_NOT_FOUND";

  expect(code).toBe("PRINTER_NOT_FOUND");
});
