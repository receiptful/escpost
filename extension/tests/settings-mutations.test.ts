import { describe, expect, it } from "vitest";
import { createAlias, dismissUnmatched, forgetGrant, removeAlias, type AliasState } from "../src/settings/mutations";

function state(overrides: Partial<AliasState> = {}): AliasState {
  return {
    aliases: { "epson tm-t20ii": "tm-t20" },
    aliasMeta: {
      "epson tm-t20ii": { requested: "EPSON TM-T20II", origin: "https://pos.thornbury.app", at: 1_000 },
    },
    unmatched: [
      { requested: "Star TSP100", origin: "https://pos.thornbury.app", at: 2_000 },
      { requested: "star tsp100", origin: "https://bluebirdcafe.co", at: 3_000 },
      { requested: "Bar Printer", origin: "https://bluebirdcafe.co", at: 4_000 },
    ],
    ...overrides,
  };
}

describe("createAlias", () => {
  it("keys the alias by the lowercased name, the way resolvePrinterName looks it up", () => {
    const next = createAlias(state(), "Star TSP100", "kitchen", 5_000);
    expect(next.aliases["star tsp100"]).toBe("kitchen");
  });

  it("keeps the verbatim name and the site that asked, so settings can show both", () => {
    const next = createAlias(state(), "Star TSP100", "kitchen", 5_000);
    expect(next.aliasMeta["star tsp100"]).toEqual({
      requested: "Star TSP100",
      origin: "https://pos.thornbury.app",
      at: 5_000,
    });
  });

  it("clears every unmatched entry with that name, because an alias is global", () => {
    const next = createAlias(state(), "Star TSP100", "kitchen", 5_000);
    expect(next.unmatched).toEqual([{ requested: "Bar Printer", origin: "https://bluebirdcafe.co", at: 4_000 }]);
  });

  it("leaves the existing aliases alone", () => {
    const next = createAlias(state(), "Star TSP100", "kitchen", 5_000);
    expect(next.aliases["epson tm-t20ii"]).toBe("tm-t20");
  });

  it("does not mutate what it was given, so a failed write cannot half-apply", () => {
    const before = state();
    createAlias(before, "Star TSP100", "kitchen", 5_000);
    expect(before.aliases).toEqual({ "epson tm-t20ii": "tm-t20" });
    expect(before.unmatched).toHaveLength(3);
  });
});

describe("removeAlias", () => {
  it("forgets the alias and the metadata that went with it", () => {
    const next = removeAlias(state(), "EPSON TM-T20II");
    expect(next.aliases).toEqual({});
    expect(next.aliasMeta).toEqual({});
    expect(next.unmatched).toHaveLength(3);
  });
});

describe("dismissUnmatched", () => {
  it("drops one site's entry without touching the aliases or the other sites", () => {
    const next = dismissUnmatched(state(), "Star TSP100", "https://pos.thornbury.app");
    expect(next.unmatched.map((entry) => entry.origin)).toEqual([
      "https://bluebirdcafe.co",
      "https://bluebirdcafe.co",
    ]);
    expect(next.aliases).toEqual({ "epson tm-t20ii": "tm-t20" });
  });
});

describe("forgetGrant", () => {
  it("removes only the pattern it was told to, and copies rather than mutates", () => {
    const grants = { "https://a.test/*": { at: 1 }, "https://b.test/*": { at: 2 } };
    expect(forgetGrant(grants, "https://a.test/*")).toEqual({ "https://b.test/*": { at: 2 } });
    expect(Object.keys(grants)).toHaveLength(2);
  });
});
