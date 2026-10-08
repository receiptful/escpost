import { readFileSync } from "node:fs";
import { test, expect } from "vitest";
import type { ErrorCode } from "../src/index";

test("the package exports only the raw browser entry", () => {
  const manifest = JSON.parse(
    readFileSync("package.json", "utf8"),
  );

  expect(Object.keys(manifest.exports)).toEqual(["."]);
  expect(manifest.exports["."]).toHaveProperty("types", "./dist/index.d.ts");
  expect(manifest.exports).not.toHaveProperty("./qz");
});

test("the package publishes publicly from the canonical repository", () => {
  const manifest = JSON.parse(
    readFileSync("package.json", "utf8"),
  );

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
