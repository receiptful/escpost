import { expect, test } from "vitest";
import { originPattern } from "../src/registration";

test("derives a concrete HTTP(S) sender origin pattern and rejects opaque origins", () => {
  // Break caught: using sender URLs or accepting an opaque/non-web origin can
  // turn a path, extension id, or forged origin string into a permission grant.
  expect(originPattern("https://shop.example:8443/orders/7")).toBe("https://shop.example:8443/*");
  expect(originPattern("null")).toBeNull();
  expect(originPattern("chrome-extension://extension-id")).toBeNull();
  expect(originPattern("https://shop.example.evil@evil.example")).toBe("https://evil.example/*");
});
