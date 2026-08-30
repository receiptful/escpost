import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { isExtensionOrigin, liveDeps } from "../src/background";

const OUR_ID = "cnifebiebidolpmlmgcghpopggfcklmc";
const OTHER_ID = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

let contains: ReturnType<typeof vi.fn>;

beforeEach(() => {
  contains = vi.fn().mockResolvedValue(false); // no http origin has been granted
  (globalThis as any).chrome = {
    runtime: { id: OUR_ID },
    permissions: { contains },
    // liveDeps() also builds the session store and the render cache over
    // chrome.storage.local. The manifest declares "storage", so the real
    // runtime always has it; a stub without it describes no real environment.
    storage: { local: { get: vi.fn(), set: vi.fn(), remove: vi.fn() } },
  };
});

afterEach(() => {
  delete (globalThis as any).chrome;
});

describe("isExtensionOrigin", () => {
  it("recognises our own extension pages", () => {
    expect(isExtensionOrigin(`chrome-extension://${OUR_ID}`)).toBe(true);
  });

  it("does NOT recognise another extension's pages", () => {
    // The assertion that matters: a blanket chrome-extension:// prefix check would
    // let any installed extension drive our worker.
    expect(isExtensionOrigin(`chrome-extension://${OTHER_ID}`)).toBe(false);
  });

  it("does not recognise a web origin", () => {
    expect(isExtensionOrigin("https://shop.test")).toBe(false);
    expect(isExtensionOrigin("http://localhost:8900")).toBe(false);
  });
});

describe("liveDeps().isOriginGranted", () => {
  it("accepts the popup, which is our own page and can never hold a host permission", async () => {
    // The bug this pins: chrome.permissions.contains({origins:["chrome-extension://<id>/*"]})
    // is never true, because optional_host_permissions only ever holds http/https
    // patterns. Without the guard the popup is refused on every single open and
    // reports a healthy daemon as "Not running".
    await expect(liveDeps().isOriginGranted(`chrome-extension://${OUR_ID}`)).resolves.toBe(true);
    expect(contains).not.toHaveBeenCalled();
  });

  it("still refuses a foreign extension's origin", async () => {
    await expect(liveDeps().isOriginGranted(`chrome-extension://${OTHER_ID}`)).resolves.toBe(false);
  });

  it("still consults the real permission for a web origin", async () => {
    contains.mockResolvedValue(true);
    await expect(liveDeps().isOriginGranted("https://shop.test")).resolves.toBe(true);
    expect(contains).toHaveBeenCalledWith({ origins: ["https://shop.test/*"] });
  });

  it("still refuses an ungranted web origin", async () => {
    contains.mockResolvedValue(false);
    await expect(liveDeps().isOriginGranted("https://evil.test")).resolves.toBe(false);
  });
});
