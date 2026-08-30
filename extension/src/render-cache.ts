import type { StorageArea } from "./session";

const CACHE_KEY = "renderCache";
const DEFAULT_LIMIT = 20;

interface Entry {
  data: string;
  usedAt: number;
}

type CacheShape = Record<string, Entry>;

/**
 * the key: a hash of (origin, html, profile).
 *
 * The origin is in the key so one site cannot probe another's cache. Without it
 * a page could send HTML it guessed a different site had printed and learn, from
 * an answer that arrived instantly and cost no quota, that it had.
 *
 * `crypto.subtle` is available in an MV3 service worker and in Node 22, so
 * this needs no dependency. The two inputs are length-prefixed rather than
 * concatenated: a receipt containing the separator would otherwise be able
 * to collide with a different receipt on a different profile, and a
 * collision here prints one customer's receipt for another.
 */
export async function cacheKey(html: string, profile: string, origin: string): Promise<string> {
  const material = `${origin.length}:${origin}${profile.length}:${profile}${html.length}:${html}`;
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(material));
  return [...new Uint8Array(digest)].map((byte) => byte.toString(16).padStart(2, "0")).join("");
}

/**
 * Rendered bytes, kept so an identical reprint costs no round trip and no
 * second charge. Bounded and least-recently-used, because a busy
 * till renders thousands of receipts a week and chrome.storage.local is not
 * somewhere to keep all of them.
 */
export class RenderCache {
  readonly #storage: StorageArea;
  readonly #limit: number;

  constructor(storage: StorageArea, limit: number = DEFAULT_LIMIT) {
    this.#storage = storage;
    this.#limit = limit;
  }

  async read(key: string): Promise<string | null> {
    const cache = await this.#load();
    const entry = cache[key];
    if (entry === undefined) return null;
    // Reading counts as use: "reprint the last receipt" is the case this
    // cache exists for, and it is all reads.
    cache[key] = { data: entry.data, usedAt: this.#tick(cache) };
    await this.#save(cache);
    return entry.data;
  }

  async write(key: string, data: string): Promise<void> {
    const cache = await this.#load();
    cache[key] = { data, usedAt: this.#tick(cache) };
    await this.#save(this.#evict(cache));
  }

  async #load(): Promise<CacheShape> {
    const stored = (await this.#storage.get(CACHE_KEY))[CACHE_KEY];
    return typeof stored === "object" && stored !== null ? ({ ...stored } as CacheShape) : {};
  }

  async #save(cache: CacheShape): Promise<void> {
    await this.#storage.set({ [CACHE_KEY]: cache });
  }

  /** A monotonic counter rather than Date.now(): two writes inside one
   *  millisecond must still order, or eviction picks arbitrarily. */
  #tick(cache: CacheShape): number {
    const highest = Object.values(cache).reduce((max, entry) => Math.max(max, entry.usedAt), 0);
    return highest + 1;
  }

  #evict(cache: CacheShape): CacheShape {
    const keys = Object.keys(cache);
    if (keys.length <= this.#limit) return cache;
    const ordered = keys.sort((a, b) => (cache[a]?.usedAt ?? 0) - (cache[b]?.usedAt ?? 0));
    for (const key of ordered.slice(0, keys.length - this.#limit)) delete cache[key];
    return cache;
  }
}
