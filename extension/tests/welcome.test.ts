import { describe, expect, it } from "vitest";
import { describeWelcome } from "../src/welcome/welcome";

describe("describeWelcome", () => {
  it("separates what already works from what the email unlocks (O2)", () => {
    const view = describeWelcome({ kind: "offer" });

    expect(view.worksNow.join(" ")).toMatch(/raw esc\/pos/i);
    expect(view.unlocks.join(" ")).toMatch(/html/i);
    expect(view.showEmailField).toBe(true);
    expect(view.showSkip).toBe(true);
  });

  it("never implies the extension is inert without an account (O2)", () => {
    const text = JSON.stringify(describeWelcome({ kind: "offer" })).toLowerCase();

    for (const phrase of ["to get started", "before you can print", "finish setup to print"]) {
      expect(text).not.toContain(phrase);
    }
    // O2 lives in the standing list, not in a heading: whatever the readiness
    // check reports, the page still says raw printing needs no account.
    expect(text).toContain("unlimited");
    expect(text).toContain("no account");
  });

  it("reports what the machine said rather than assuming it (O2)", () => {
    const notRunning = describeWelcome({ kind: "offer" }, { kind: "not-running" });
    const noPrinter = describeWelcome({ kind: "offer" }, { kind: "no-printer" });
    const ready = describeWelcome({ kind: "offer" }, { kind: "ready", printer: "TM-T20" });

    expect(notRunning.status?.tone).toBe("out");
    expect(notRunning.action?.kind).toBe("recheck");

    expect(noPrinter.command).toBe("escpost printers add");
    expect(noPrinter.action?.kind).toBe("recheck");

    expect(ready.body).toContain("TM-T20");
    expect(ready.action?.kind).toBe("test-print");
    // Even when nothing is answering, the offer stays reachable.
    expect(notRunning.showEmailField).toBe(true);
  });

  it("gives the finished state something to do when printing is ready", () => {
    const idle = describeWelcome({ kind: "skipped" });
    const ready = describeWelcome({ kind: "skipped" }, { kind: "ready", printer: "TM-T20" });

    expect(idle.action).toBeNull();
    expect(ready.action?.kind).toBe("test-print");
  });

  it("states the offer factually, with the number (O5)", () => {
    const view = describeWelcome({ kind: "offer" });

    expect(view.unlocks.join(" ")).toContain("200");
    expect(view.worksNow.join(" ")).toMatch(/unlimited/i);
  });

  it("says the link was sent and what to do next, not that HTML is ready (A2)", () => {
    const view = describeWelcome({ kind: "sent", email: "shop@example.com" });

    expect(view.body).toContain("shop@example.com");
    // Link-only sign-in signs in whichever browser opens the link, so the one
    // thing this screen must get across is WHERE to open it. Opening on a
    // phone is the case that silently fails otherwise.
    expect(view.body.toLowerCase()).toContain("this browser");
    expect(view.body.toLowerCase()).toContain("phone");
    // The allowance does not exist until the link is opened, so the screen
    // must not suggest it does.
    expect(view.body.toLowerCase()).not.toContain("you can now");
  });

  it("confirms the sign-in once the link is clicked", () => {
    const view = describeWelcome({ kind: "signed-in", email: "shop@example.com" });

    expect(view.heading.toLowerCase()).toContain("ready");
    expect(view.showEmailField).toBe(false);
  });

  it("states what skipping forgoes exactly once (O4)", () => {
    const asked = describeWelcome({ kind: "confirming-skip" });
    const done = describeWelcome({ kind: "skipped" });

    expect(asked.warning).not.toBeNull();
    expect(asked.warning?.toLowerCase()).toContain("html");
    // Thereafter it lives passively in the popup — no badge, no
    // interstitial, no second telling.
    expect(done.warning).toBeNull();
  });

  it("keeps the offer reachable after a skip and schedules no re-prompt (O3)", () => {
    const view = describeWelcome({ kind: "skipped" });

    expect(view.body.toLowerCase()).toContain("extension");
    expect(view.showSkip).toBe(false);
    expect(view.repromptAfterMs).toBeNull();
  });
});

describe("describeWelcome: an expired sign-in link", () => {

  it("sends someone back to the start when the link has expired", () => {
    const view = describeWelcome({ kind: "expired" });

    // The only way out is a new link, so the email field has to come back.
    expect(view.showEmailField).toBe(true);
    expect(view.body.toLowerCase()).toMatch(/again/);
  });

  it("never removes the skip control while signing in (O3)", () => {
    for (const phase of [
      { kind: "sent", email: "a@b.c" },
      { kind: "expired" },
    ] as const) {
      expect(describeWelcome(phase).showSkip, phase.kind).toBe(true);
    }
  });

  it("keeps the O2 split intact in every signing-in phase", () => {
    for (const phase of [
      { kind: "sent", email: "a@b.c" },
      { kind: "expired" },
    ] as const) {
      const view = describeWelcome(phase);
      expect(view.worksNow.join(" "), phase.kind).toMatch(/raw esc\/pos/i);
      expect(view.unlocks.join(" "), phase.kind).toMatch(/html/i);
    }
  });
});

