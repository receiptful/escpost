import { readFileSync } from "node:fs";
import { test, expect } from "vitest";
import type { ErrorCode } from "../src/index";

test("the package exports only the raw browser entry", () => {
  const manifest = JSON.parse(
    readFileSync("sdks/javascript/package.json", "utf8"),
  );

  expect(Object.keys(manifest.exports)).toEqual(["."]);
  expect(manifest.exports["."]).toHaveProperty("types", "./dist/index.d.ts");
  expect(manifest.exports).not.toHaveProperty("./qz");
});

test("the package root exports the documented error-code union", () => {
  // Break caught: omitting ErrorCode from the root module leaves consumers
  // unable to type error handling through the package's only public entry.
  const code: ErrorCode = "PRINTER_NOT_FOUND";

  expect(code).toBe("PRINTER_NOT_FOUND");
});
