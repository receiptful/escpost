import { describe, expect, it } from "vitest";
import { describePopup, type PopupSection, type PopupView } from "../src/popup/state";
import { ALL_KINDS, CONNECTED_KINDS, INPUTS, account } from "./fixtures/popup-fixtures";

function stripText(strip: PopupSection["strip"]): string {
  return (strip?.parts ?? []).map((part) => part.text).join("");
}

function allText(view: PopupView): string {
  const bits: string[] = [view.status.label];
  for (const section of view.sections) {
    for (const value of [section.label, section.lead, section.command, section.note, section.detail]) {
      if (value !== undefined) bits.push(value);
    }
    if (section.button !== undefined) bits.push(section.button.label);
    bits.push(stripText(section.strip));
    for (const row of section.rows ?? []) bits.push(row.key, row.value ?? "", row.pill?.label ?? "");
  }
  return bits.join(" ");
}

describe("describePopup — U1, the printer question comes first", () => {
  it("puts the printers first in every state where the daemon is up", () => {
    for (const kind of CONNECTED_KINDS) {
      expect(describePopup(INPUTS[kind]).sections[0]?.label, kind).toBe("Printers");
    }
  });

  it("explains how to add a printer rather than showing an empty list", () => {
    const view = describePopup({ ...INPUTS["signed-out"], daemon: { running: true, printers: [] } });
    expect(view.sections[0]?.lead).toContain("no printers configured");
    expect(view.sections[0]?.command).toBe("escpost printers discover");
  });

  it("lets a dead daemon outrank an account, an allowance and the network", () => {
    const view = describePopup({ ...INPUTS.exhausted, daemon: { running: false, message: "" }, online: false });
    expect(view.kind).toBe("no-daemon");
  });
});

describe("describePopup — the daemon-not-running state", () => {
  it("names the state and carries install guidance", () => {
    const view = describePopup(INPUTS["no-daemon"]);
    expect(view.status).toMatchObject({ label: "Not running", tone: "warn" });
    expect(view.sections[0]?.command).toContain("brew install escpost");
    expect(view.sections[0]?.button).toMatchObject({ action: "check-again" });
  });

  it("does not repeat the ordinary absence message it already paraphrases", () => {
    expect(describePopup(INPUTS["no-daemon"]).sections[0]?.detail).toBeUndefined();
  });

  it("surfaces an unexpected daemon message verbatim, so a new failure is not hidden", () => {
    const view = describePopup({
      ...INPUTS["no-daemon"],
      daemon: { running: false, message: "The daemon returned 500." },
    });
    expect(view.sections[0]?.detail).toBe("The daemon returned 500.");
  });
});

describe("describePopup — connected and signed out", () => {
  it("marks raw unlimited and HTML locked, and offers the one way to unlock it", () => {
    const view = describePopup(INPUTS["signed-out"]);
    const html = view.sections[1];
    expect(html?.rows).toContainEqual({ key: "Raw ESC/POS", value: "Unlimited" });
    expect(html?.rows?.find((row) => row.key === "HTML receipts")?.pill?.label).toBe("Locked");
    expect(html?.button?.action).toBe("open-welcome");
  });

  it("tells an unverified address to click the link rather than signing up again (A2)", () => {
    const view = describePopup({ ...INPUTS["signed-out"], account: account({ verified: false }) });
    expect(view.kind).toBe("signed-out");
    expect(view.sections[1]?.note).toContain("We sent a link to sam@bluebirdcafe.co");
  });
});

describe("describePopup — connected and signed in", () => {
  it("shows the allowance with a meter sized to what is left", () => {
    const view = describePopup(INPUTS["signed-in"]);
    expect(view.sections[1]?.rows).toContainEqual({ key: "sam@bluebirdcafe.co", value: "Verified" });
    expect(view.sections[2]?.rows).toContainEqual({ key: "HTML receipts", value: "153 left" });
    expect(view.sections[2]?.meter).toEqual({ fraction: 153 / 200, tone: "ok" });
  });

  it("warns the meter once the allowance is nearly gone", () => {
    const nearly = account({ allowance: { known: true, kind: "monthly", remaining: 3, total: 20, resetsAt: null } });
    const view = describePopup({ ...INPUTS["signed-in"], account: nearly });
    expect(view.sections[2]?.meter?.tone).toBe("warn");
  });

  it("shows a paid plan as included rather than counting down to a free limit", () => {
    const paid = account({ allowance: { known: true, kind: "paid", remaining: 20, total: 20, resetsAt: null } });
    const view = describePopup({ ...INPUTS["signed-in"], account: paid });

    expect(view.kind).toBe("signed-in");
    expect(view.sections[2]?.rows).toContainEqual({ key: "HTML receipts", value: "Included" });
    // A countdown and a meter both say "you are running out", which is false.
    expect(view.sections[2]?.meter).toBeUndefined();
    expect(view.sections[2]?.note).not.toContain("free");
  });

  it("never offers the upsell to a paid org, even if its remaining count reads zero", () => {
    // Belt and braces: the producer keeps remaining full, but a future one
    // that did not would otherwise sell a plan to someone who has one.
    const paid = account({ allowance: { known: true, kind: "paid", remaining: 0, total: 20, resetsAt: null } });
    const view = describePopup({ ...INPUTS["signed-in"], account: paid });

    expect(view.kind).not.toBe("exhausted");
    expect(view.upsell).toBeNull();
  });

  it("shows an unreadable allowance as unchecked, never as exhausted", () => {
    const broken = account({ allowance: { known: false, kind: "monthly", remaining: 0, total: 0, resetsAt: null } });
    const view = describePopup({ ...INPUTS["signed-in"], account: broken });
    expect(view.kind).toBe("signed-in");
    expect(view.sections[2]?.meter).toBeUndefined();
    expect(view.upsell).toBeNull();
  });
});

describe("describePopup — the two failure states say what still works (U2)", () => {
  it("leads the exhausted state with the fact that raw is unaffected", () => {
    const view = describePopup(INPUTS.exhausted);
    expect(stripText(view.sections[1]?.strip)).toContain("Raw ESC/POS printing is unaffected");
    expect(view.sections[1]?.rows).toContainEqual({ key: "HTML receipts", value: "0 of 20 left" });
    expect(view.sections[1]?.meter?.tone).toBe("out");
  });

  it("names when the exhausted allowance comes back", () => {
    expect(describePopup(INPUTS.exhausted).sections[2]?.note).toContain("Resets 1 September otherwise.");
  });

  it("says raw is unaffected while offline too", () => {
    const view = describePopup(INPUTS.offline);
    expect(view.status).toMatchObject({ label: "Offline", tone: "warn" });
    expect(stripText(view.sections[1]?.strip)).toContain("Raw ESC/POS printing is unaffected");
    expect(view.sections[2]?.rows).toContainEqual({ key: "Reprint last receipt", value: "Available" });
  });

  it("does not report being offline to someone who has nothing to render", () => {
    expect(describePopup({ ...INPUTS["signed-out"], online: false }).kind).toBe("signed-out");
  });

  it("never tells the user the extension is broken", () => {
    for (const kind of ALL_KINDS) {
      const text = allText(describePopup(INPUTS[kind])).toLowerCase();
      for (const forbidden of ["broken", "something went wrong", "unexpected error", "failure"]) {
        expect(text, `${kind} says "${forbidden}"`).not.toContain(forbidden);
      }
    }
  });
});

describe("describePopup — U3, no state nags", () => {
  it("offers the paid plan in exactly one state and nowhere else", () => {
    const nagging = ALL_KINDS.filter((kind) => describePopup(INPUTS[kind]).upsell !== null);
    expect(nagging).toEqual(["exhausted"]);

    for (const kind of ALL_KINDS) {
      if (kind === "exhausted") continue;
      const view = describePopup(INPUTS[kind]);
      expect(view.sections.some((section) => section.button?.action === "open-plans"), kind).toBe(false);
    }
  });

  it("counts the granted sites in the footer", () => {
    expect(describePopup(INPUTS["signed-in"]).footer[0]?.label).toBe("2 sites");
    expect(describePopup({ ...INPUTS["signed-in"], siteCount: 1 }).footer[0]?.label).toBe("1 site");
    expect(describePopup({ ...INPUTS["signed-in"], siteCount: 0 }).footer[0]?.label).toBe("No sites yet");
  });
});

describe("describePopup — P1, granting a site from the popup", () => {
  const site = { origin: "pos.thornbury.app", pattern: "https://pos.thornbury.app/*", denied: false };

  it("offers the grant without pushing the printer question down (U1)", () => {
    for (const kind of CONNECTED_KINDS) {
      const view = describePopup({ ...INPUTS[kind], pendingSite: site });
      expect(view.sections[0]?.label, kind).toBe("Printers");
      expect(view.sections[1]?.button?.action, kind).toBe("grant-site");
    }
  });

  it("carries the pattern the grant needs, not the display name", () => {
    const view = describePopup({ ...INPUTS["signed-out"], pendingSite: site });
    // chrome.permissions.request takes a match pattern; "pos.thornbury.app" is not one.
    expect(view.sections[1]?.button?.value).toBe("https://pos.thornbury.app/*");
    expect(view.sections[1]?.lead).toContain("pos.thornbury.app");
  });

  it("says the grant is for this site alone and is revocable", () => {
    const view = describePopup({ ...INPUTS["signed-out"], pendingSite: site });
    expect(view.sections[1]?.note?.toLowerCase()).toContain("only this site");
    expect(view.sections[1]?.note?.toLowerCase()).toContain("revoke");
  });

  it("shows nothing at all when there is no site to grant", () => {
    for (const kind of CONNECTED_KINDS) {
      const view = describePopup(INPUTS[kind]);
      expect(view.sections.some((s) => s.button?.action === "grant-site"), kind).toBe(false);
    }
  });

  it("offers it in the first-run state too, since a refusal is why people open this", () => {
    const view = describePopup({ ...INPUTS["no-daemon"], pendingSite: site });
    expect(view.sections.some((s) => s.button?.action === "grant-site")).toBe(true);
  });

  it("takes a decline gracefully and keeps the offer available", () => {
    const view = describePopup({ ...INPUTS["signed-out"], pendingSite: { ...site, denied: true } });
    expect(view.sections[1]?.lead?.toLowerCase()).toContain("declined");
    // P1 says never repeated, not never again: the control stays, it just does not nag.
    expect(view.sections[1]?.button?.action).toBe("grant-site");
  });

  it("still offers exactly one paid upsell, and the grant is not one (U3)", () => {
    const view = describePopup({ ...INPUTS.exhausted, pendingSite: site });
    expect(view.upsell?.action).toBe("open-plans");
    expect(view.sections.filter((s) => s.button?.action === "open-plans")).toHaveLength(1);
  });
});

describe("describePopup is total over its input", () => {
  it("renders a state rather than throwing when pendingSite is absent", () => {
    // describePopup is the entry point for the entire popup. Anything that
    // throws here paints nothing, which is indistinguishable from a broken
    // extension -- the one thing U2 forbids.
    const { pendingSite, ...withoutField } = INPUTS["signed-out"];
    expect(() => describePopup(withoutField as typeof INPUTS["signed-out"])).not.toThrow();
    expect(describePopup(withoutField as typeof INPUTS["signed-out"]).kind).toBe("signed-out");
  });
});

describe("a site that was just granted", () => {
  it("asks for the reload the new registration needs, rather than looking like nothing happened", () => {
    const view = describePopup({
      ...INPUTS["signed-out"],
      pendingSite: {
        origin: "https://pos.thornbury.app",
        pattern: "https://pos.thornbury.app/*",
        denied: false,
        needsReload: true,
      },
    });

    const section = view.sections.find((entry) => entry.button?.action === "reload-site");
    expect(section?.lead).toContain("Reload the page");
    // The offer to grant is gone: it has been granted.
    expect(view.sections.some((entry) => entry.button?.action === "grant-site")).toBe(false);
  });
});

describe("a site the popup recognises as a QZ integration", () => {
  const site = { origin: "https://pos.thornbury.app", pattern: "https://pos.thornbury.app/*" };

  it("says what it found, rather than asking generically", () => {
    const view = describePopup({
      ...INPUTS["signed-out"],
      pendingSite: { ...site, denied: false, needsReload: false, usesQz: true },
    });

    const lead = view.sections.find((entry) => entry.button?.action === "grant-site")?.lead ?? "";
    expect(lead).toContain("QZ Tray");
    expect(lead).toContain("no certificate");
  });

  it("keeps the plain wording for a site it cannot recognise", () => {
    const view = describePopup({
      ...INPUTS["signed-out"],
      pendingSite: { ...site, denied: false, needsReload: false, usesQz: false },
    });

    const lead = view.sections.find((entry) => entry.button?.action === "grant-site")?.lead ?? "";
    expect(lead).not.toContain("QZ Tray");
    expect(lead).toContain("cannot print until you allow it");
  });

  it("does not claim QZ once the site has been granted", () => {
    const view = describePopup({
      ...INPUTS["signed-out"],
      pendingSite: { ...site, denied: false, needsReload: true, usesQz: true },
    });

    const lead = view.sections.find((entry) => entry.button?.action === "reload-site")?.lead ?? "";
    expect(lead).toContain("Reload the page");
  });
});

describe("a granted site whose page predates the grant", () => {
  const site = { origin: "https://pos.thornbury.app", pattern: "https://pos.thornbury.app/*" };

  it("still asks for the reload in a later popup, not only the one that granted", () => {
    // The old version remembered the grant it had just made, so closing and
    // reopening the popup left a page that could not print and nothing saying
    // why. This state comes from the page, so it survives the popup closing.
    const view = describePopup({
      ...INPUTS["signed-out"],
      pendingSite: { ...site, denied: false, needsReload: true, usesQz: false },
    });

    expect(view.sections.find((entry) => entry.button?.action === "reload-site")?.lead).toContain(
      "Reload the page",
    );
  });
});
