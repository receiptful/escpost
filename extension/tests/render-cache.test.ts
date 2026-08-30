import { describe, expect, it } from "vitest";
import { cacheKey, RenderCache } from "../src/render-cache";
import { memoryStorage } from "../src/session";

describe("cacheKey", () => {
  it("is stable for the same receipt and profile", async () => {
    expect(await cacheKey("<h1>x</h1>", "NT-5890K", "https://a.test")).toBe(await cacheKey("<h1>x</h1>", "NT-5890K", "https://a.test"));
  });

  it("differs by profile, because the bytes do", async () => {
    expect(await cacheKey("<h1>x</h1>", "NT-5890K", "https://a.test")).not.toBe(await cacheKey("<h1>x</h1>", "TM-T88II", "https://a.test"));
  });

  it("scopes the key to the requesting origin (security finding 5)", async () => {
    // Without this, a site could send HTML it guessed another site had printed
    // and learn from the instant, unmetered answer that it had -- a cross-origin
    // oracle over receipt contents.
    expect(await cacheKey("<h1>x</h1>", "NT-5890K", "https://a.test")).not.toBe(
      await cacheKey("<h1>x</h1>", "NT-5890K", "https://b.test"),
    );
  });

  it("still lets one site reprint its own receipt from cache (R4)", async () => {
    expect(await cacheKey("<h1>x</h1>", "NT-5890K", "https://a.test")).toBe(
      await cacheKey("<h1>x</h1>", "NT-5890K", "https://a.test"),
    );
  });

  it("cannot be confused by an origin that contains the separator", async () => {
    expect(await cacheKey("x", "p", "https://a.test\n1:q")).not.toBe(await cacheKey("x", "p\n1:q", "https://a.test"));
  });

  it("cannot be confused by a receipt that contains the separator", async () => {
    // Naive concatenation would make these two collide, and a collision here
    // prints one customer's receipt for another.
    expect(await cacheKey("a\nNT-5890K", "b", "https://a.test")).not.toBe(await cacheKey("a", "NT-5890K\nb", "https://a.test"));
  });
});

describe("RenderCache", () => {
  it("misses before anything is cached", async () => {
    expect(await new RenderCache(memoryStorage()).read("k1")).toBeNull();
  });

  it("returns bytes it was given", async () => {
    const cache = new RenderCache(memoryStorage());
    await cache.write("k1", "G0A=");
    expect(await cache.read("k1")).toBe("G0A=");
  });

  it("survives a recycled worker", async () => {
    const storage = memoryStorage();
    await new RenderCache(storage).write("k1", "G0A=");
    expect(await new RenderCache(storage).read("k1")).toBe("G0A=");
  });

  it("evicts the oldest entry rather than growing without bound", async () => {
    const cache = new RenderCache(memoryStorage(), 2);
    await cache.write("k1", "a");
    await cache.write("k2", "b");
    await cache.write("k3", "c");

    expect(await cache.read("k1")).toBeNull();
    expect(await cache.read("k2")).toBe("b");
    expect(await cache.read("k3")).toBe("c");
  });

  it("keeps an entry alive by reading it", async () => {
    const cache = new RenderCache(memoryStorage(), 2);
    await cache.write("k1", "a");
    await cache.write("k2", "b");
    await cache.read("k1");
    await cache.write("k3", "c");

    // "reprint the last receipt" is the case R4 exists for, so recency has
    // to count reads, not just writes.
    expect(await cache.read("k1")).toBe("a");
    expect(await cache.read("k2")).toBeNull();
  });
});
