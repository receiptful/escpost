import { readFileSync } from "node:fs";
import { test, expect } from "vitest";

test("the package exports only the raw browser entry", () => {
  const manifest = JSON.parse(
    readFileSync("packages/browser/package.json", "utf8"),
  );

  expect(Object.keys(manifest.exports)).toEqual(["."]);
  expect(manifest.exports["."]).toHaveProperty("types", "./dist/index.d.ts");
  expect(manifest.exports).not.toHaveProperty("./qz");
});
