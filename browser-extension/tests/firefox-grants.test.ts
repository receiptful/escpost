import { expect, test, vi } from "vitest";
import { FirefoxOriginGrants } from "../src/firefox/grants";

test("maps one exact origin to Firefox optional host permissions", async () => {
  let removed: ((details: { origins?: string[] }) => void) | undefined;
  const permissions = {
    contains: vi.fn(async () => true),
    request: vi.fn(async () => true),
    remove: vi.fn(async () => true),
    onRemoved: { addListener: vi.fn((listener) => { removed = listener; }) },
  };
  const grants = new FirefoxOriginGrants(permissions);
  const listener = vi.fn();
  grants.onRemoved(listener);

  await expect(grants.contains("https://shop.example/*")).resolves.toBe(true);
  await expect(grants.request("https://shop.example/*")).resolves.toBe(true);
  await expect(grants.remove("https://shop.example/*")).resolves.toBe(true);
  expect(permissions.contains).toHaveBeenCalledWith({ origins: ["https://shop.example/*"] });
  expect(permissions.request).toHaveBeenCalledWith({ origins: ["https://shop.example/*"] });
  expect(permissions.remove).toHaveBeenCalledWith({ origins: ["https://shop.example/*"] });

  removed?.({ origins: ["https://shop.example/*"] });
  expect(listener).toHaveBeenCalledWith(["https://shop.example/*"]);
});
