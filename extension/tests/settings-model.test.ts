import { describe, expect, it } from "vitest";
import { describeSettings } from "../src/settings/model";
import { account } from "./fixtures/popup-fixtures";
import { settingsInput as input } from "./fixtures/settings-fixtures";

describe("describeSettings — sites that can print (P4)", () => {
  it("lists the granted sites and hides the daemon's own host permission", () => {
    const view = describeSettings(input());
    expect(view.sites.map((site) => site.origin)).toEqual(["bluebirdcafe.co", "pos.thornbury.app"]);
  });

  it("keeps the pattern on each row, because that is what revoking needs", () => {
    expect(describeSettings(input()).sites[0]?.pattern).toBe("https://bluebirdcafe.co/*");
  });

  it("decorates a site with when and how it was granted, when that was recorded", () => {
    expect(describeSettings(input()).sites[0]?.sub).toBe("granted 4 August · @escpost/browser");
  });

  it("says something true rather than nothing when the grant was not recorded", () => {
    expect(describeSettings(input()).sites[1]?.sub).toBe("granted when this site first asked to print");
  });
});

describe("describeSettings — printer names (N2, U5)", () => {
  it("shows an alias with the printer it resolves to and who asked for it", () => {
    const alias = describeSettings(input()).aliases[0];
    expect(alias).toMatchObject({ requested: "EPSON TM-T20II", target: "TM-T20", matched: true });
    expect(alias?.sub).toBe("requested by pos.thornbury.app");
  });

  it("marks an alias whose printer has since gone away, rather than showing it as working", () => {
    const view = describeSettings(input({ aliases: { "old till": "removed-printer" }, aliasMeta: {} }));
    expect(view.aliases[0]).toMatchObject({ requested: "old till", target: "removed-printer", matched: false });
    expect(view.aliases[0]?.sub).toBe("created here");
  });

  it("surfaces an unmatched name exactly as the page asked for it", () => {
    const unmatched = describeSettings(input()).unmatched[0];
    expect(unmatched?.requested).toBe("Star TSP100");
    expect(unmatched?.origin).toBe("https://pos.thornbury.app");
    expect(unmatched?.sub).toBe("requested by pos.thornbury.app on 20 August · that print failed");
  });

  it("offers every configured printer as an alias target, defaulting to the first", () => {
    const view = describeSettings(input());
    expect(view.printerChoices).toEqual([
      { id: "tm-t20", name: "TM-T20" },
      { id: "kitchen", name: "Kitchen" },
    ]);
  });
});

describe("describeSettings — account, usage and about", () => {
  it("omits the account and usage sections entirely when signed out", () => {
    const view = describeSettings(input());
    expect(view.account).toBeNull();
    expect(view.usage).toBeNull();
  });

  it("shows the signed-in address and when they signed in (A4)", () => {
    const view = describeSettings(input({ account: account() }));
    expect(view.account).toEqual({ email: "sam@bluebirdcafe.co", sub: "Verified · signed in 4 August" });
  });

  it("shows usage as what has been used out of the allowance", () => {
    const spent = account({
      allowance: { known: true, kind: "monthly", remaining: 14, total: 20, resetsAt: Date.UTC(2026, 8, 1) },
    });
    expect(describeSettings(input({ account: spent })).usage).toEqual({
      title: "Usage this month",
      html: "6 of 20",
      resets: "resets 1 September",
      raw: "Unlimited",
    });
  });

  it("does not head a signup allowance with a monthly window it does not have", () => {
    // Seen in a real browser: the section read "USAGE THIS MONTH" directly above
    // "the signup allowance, which does not reset". One of the two was lying.
    const view = describeSettings(input({ account: account() }));
    expect(view.usage?.title).toBe("Usage");
    expect(view.usage?.resets).toContain("does not reset");
  });

  it("heads a monthly allowance with the month it resets in", () => {
    const monthly = account({
      allowance: { known: true, kind: "monthly", remaining: 14, total: 20, resetsAt: Date.UTC(2026, 8, 1) },
    });
    expect(describeSettings(input({ account: monthly })).usage?.title).toBe("Usage this month");
  });

  it("does not tell a paying customer they have used 0 of 20", () => {
    const paid = account({
      allowance: { known: true, kind: "paid", remaining: 20, total: 20, resetsAt: Date.UTC(2026, 8, 1) },
    });
    expect(describeSettings(input({ account: paid })).usage).toEqual({
      title: "Usage this month",
      html: "Included",
      resets: "1,000 per active printer",
      raw: "Unlimited",
    });
  });

  it("reports whether escpost is running, and the extension's version", () => {
    // escpost has no version handshake: the shape of its API is the contract,
    // so the only thing worth reporting is whether it answered.
    expect(describeSettings(input()).about).toEqual({ daemon: "running", extension: "1.0.0" });
    expect(describeSettings(input({ daemonRunning: false })).about.daemon).toBe("not running");
  });
});
