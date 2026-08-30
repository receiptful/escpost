import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const manifest = JSON.parse(readFileSync("extension/manifest.json", "utf8")) as {
  permissions: string[];
  host_permissions: string[];
  optional_host_permissions: string[];
};

/** The hosts the worker actually calls, read out of the source rather than repeated here. */
function hostsTheWorkerCalls(): string[] {
  const source = readFileSync("extension/src/background.ts", "utf8");
  return [...source.matchAll(/^const \w+_BASE = "([^"]+)";$/gm)].map((m) => new URL(m[1] as string).origin);
}

describe("manifest host permissions", () => {
  it("declares every host the worker calls, so no request depends on a grant nobody asked for", () => {
    // Both are unconditional: the daemon is how anything prints, and the API is
    // how any HTML renders. An optional grant for either is a failure mode, not
    // a privacy control -- the extension cannot do its job without them.
    for (const origin of hostsTheWorkerCalls()) {
      expect(manifest.host_permissions.some((p) => p.startsWith(origin)), origin).toBe(true);
    }
  });

  it("names those hosts exactly, never as a wildcard", () => {
    // A reviewer reads this list. Two named first-party hosts are defensible;
    // a wildcard standing in for them would not be.
    for (const pattern of manifest.host_permissions) {
      expect(pattern).not.toContain("*://");
      expect(pattern.split("://")[1]?.startsWith("*")).toBe(false);
    }
  });

  it("keeps site access optional and broad, which is what per-site grants need (P1)", () => {
    // optional_host_permissions is the pool chrome.permissions.request draws
    // from. It grants nothing on install; it is what lets the popup ask for one
    // site at a time.
    expect(manifest.optional_host_permissions).toContain("https://*/*");
    expect(manifest.optional_host_permissions).toContain("http://*/*");
  });

  it("does not put our own API in the optional pool", () => {
    for (const origin of hostsTheWorkerCalls()) {
      expect(manifest.optional_host_permissions.some((p) => p.startsWith(origin)), origin).toBe(false);
    }
  });
});
