// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from "vitest";
import { grantableSite, loadInput, main, toPopupPrinter, type PopupDeps } from "../src/popup/popup";
import { describePopup } from "../src/popup/state";
import type { DaemonPrinter } from "../src/daemon";
import { account } from "./fixtures/popup-fixtures";

const DAEMON_PRINTERS: DaemonPrinter[] = [
  { id: "tm-t20", name: "TM-T20", transport: "usb", profile: null, status: "ready" },
  { id: "kitchen", name: "Kitchen", transport: "network", profile: null, status: "unavailable" },
];

function deps(overrides: Partial<PopupDeps> = {}): PopupDeps {
  return {
    readDaemon: vi.fn().mockResolvedValue({ ok: true, printers: DAEMON_PRINTERS, defaultId: "tm-t20" }),
    readAccount: vi.fn().mockResolvedValue(null),
    countSites: vi.fn().mockResolvedValue(2),
    isOnline: () => true,
    clearBadge: vi.fn(),
    openUrl: vi.fn(),
    openWelcome: vi.fn(),
    openSettings: vi.fn(),
    activeSite: vi.fn().mockResolvedValue(null),
    requestGrant: vi.fn().mockReturnValue(Promise.resolve(true)),
    recordGrant: vi.fn().mockResolvedValue(undefined),
    ...overrides,
  };
}

let root: HTMLElement;

beforeEach(() => {
  document.body.innerHTML = "";
  root = document.createElement("main");
  document.body.append(root);
});

describe("toPopupPrinter", () => {
  it("labels a printer with its transport, whether it is the default, and whether it is there", () => {
    const [usb, network] = DAEMON_PRINTERS;
    expect(toPopupPrinter(usb as DaemonPrinter, "tm-t20")).toEqual({ name: "TM-T20", detail: "USB · default" });
    expect(toPopupPrinter(network as DaemonPrinter, "tm-t20")).toEqual({
      name: "Kitchen",
      detail: "Network · unavailable",
    });
  });
});

describe("loadInput", () => {
  it("turns a daemon failure into the not-running state, carrying its message", async () => {
    const input = await loadInput(deps({ readDaemon: vi.fn().mockResolvedValue({ ok: false, message: "boom" }) }));
    expect(input.daemon).toEqual({ running: false, message: "boom" });
    expect(describePopup(input).kind).toBe("no-daemon");
  });

  it("builds the signed-in input from a healthy daemon and a verified account", async () => {
    const input = await loadInput(deps({ readAccount: vi.fn().mockResolvedValue(account()) }));
    expect(input.daemon).toEqual({
      running: true,
      printers: [
        { name: "TM-T20", detail: "USB · default" },
        { name: "Kitchen", detail: "Network · unavailable" },
      ],
    });
    expect(input.siteCount).toBe(2);
    expect(describePopup(input).kind).toBe("signed-in");
  });
});

describe("main", () => {
  it("clears the P3 badge, because the user has now looked", async () => {
    const d = deps();
    await main(root, d);
    expect(d.clearBadge).toHaveBeenCalledTimes(1);
  });

  it("renders the state it loaded", async () => {
    await main(root, deps({ readDaemon: vi.fn().mockResolvedValue({ ok: false, message: "" }) }));
    expect(root.dataset["state"]).toBe("no-daemon");
  });

  it("reloads when the user clicks check again", async () => {
    const d = deps({ readDaemon: vi.fn().mockResolvedValue({ ok: false, message: "" }) });
    await main(root, d);
    root.querySelector<HTMLElement>('[data-action="check-again"]')?.click();
    await vi.waitFor(() => expect(d.readDaemon).toHaveBeenCalledTimes(2));
  });

  it("opens the pricing page from the one state that offers a plan", async () => {
    const spent = account({
      allowance: { known: true, kind: "monthly", remaining: 0, total: 20, resetsAt: Date.UTC(2026, 8, 1) },
    });
    const d = deps({ readAccount: vi.fn().mockResolvedValue(spent) });
    await main(root, d);
    expect(root.dataset["state"]).toBe("exhausted");
    root.querySelector<HTMLElement>('[data-action="open-plans"]')?.click();
    expect(d.openUrl).toHaveBeenCalledWith("https://receiptful.io/#pricing");
  });

  it("opens the settings page from the footer", async () => {
    const d = deps();
    await main(root, d);
    root.querySelector<HTMLElement>('.pop-foot [data-action="open-settings"]')?.click();
    expect(d.openSettings).toHaveBeenCalledTimes(1);
  });

  it("opens the welcome tab from the signed-out unlock button, not the pricing page", async () => {
    const d = deps();
    await main(root, d);
    root.querySelector<HTMLElement>('[data-action="open-welcome"]')?.click();
    expect(d.openWelcome).toHaveBeenCalledTimes(1);
    expect(d.openUrl).not.toHaveBeenCalled();
  });
});

describe("granting a site (P1, P3)", () => {
  const site = { origin: "pos.thornbury.app", pattern: "https://pos.thornbury.app/*", denied: false };

  function grantDeps(overrides: Partial<PopupDeps> = {}): PopupDeps {
    return deps({
      activeSite: vi.fn().mockResolvedValue(site),
      requestGrant: vi.fn().mockReturnValue(Promise.resolve(true)),
      recordGrant: vi.fn().mockResolvedValue(undefined),
      ...overrides,
    });
  }

  it("calls chrome.permissions.request synchronously inside the click handler", async () => {
    // The whole feature turns on this. Chrome drops the user gesture across an
    // await, and permissions.request then resolves false without ever showing a
    // prompt. So the handler must call it before it awaits anything.
    const d = grantDeps();
    await main(root, d);

    let calledDuringHandler = false;
    (d.requestGrant as ReturnType<typeof vi.fn>).mockImplementation(() => {
      calledDuringHandler = true;
      return Promise.resolve(true);
    });

    root.querySelector<HTMLElement>('[data-action="grant-site"]')?.click();

    // No await between the click and the assertion: if the handler deferred the
    // call behind a promise, this is still false.
    expect(calledDuringHandler).toBe(true);
    expect(d.requestGrant).toHaveBeenCalledWith("https://pos.thornbury.app/*");
  });

  it("records the grant so the settings list stops falling back", async () => {
    const d = grantDeps();
    await main(root, d);
    root.querySelector<HTMLElement>('[data-action="grant-site"]')?.click();
    await vi.waitFor(() => expect(d.recordGrant).toHaveBeenCalledWith("https://pos.thornbury.app/*"));
  });

  it("re-reads the popup after a grant, so the offer disappears", async () => {
    const d = grantDeps();
    await main(root, d);
    root.querySelector<HTMLElement>('[data-action="grant-site"]')?.click();
    await vi.waitFor(() => expect(d.activeSite).toHaveBeenCalledTimes(2));
  });

  it("records nothing when the user declines, and says so without nagging", async () => {
    const d = grantDeps({ requestGrant: vi.fn().mockReturnValue(Promise.resolve(false)) });
    await main(root, d);
    root.querySelector<HTMLElement>('[data-action="grant-site"]')?.click();

    await vi.waitFor(() => expect(root.textContent).toContain("declined"));
    expect(d.recordGrant).not.toHaveBeenCalled();
    // Still offered: P1 says never repeated, not withdrawn.
    expect(root.querySelector('[data-action="grant-site"]')).not.toBeNull();
  });

  it("shows no grant control for an already-granted site", async () => {
    const d = grantDeps({ activeSite: vi.fn().mockResolvedValue(null) });
    await main(root, d);
    expect(root.querySelector('[data-action="grant-site"]')).toBeNull();
  });
});

describe("grantableSite — what the popup will offer to grant", () => {
  it("offers an ordinary site, keeping the scheme in the pattern", () => {
    expect(grantableSite("https://pos.thornbury.app/tills/3?x=1")).toEqual({
      origin: "pos.thornbury.app",
      pattern: "https://pos.thornbury.app/*",
    });
  });

  it("keeps the port, because a match pattern with the wrong port grants nothing", () => {
    expect(grantableSite("http://localhost:8900/")).toEqual({
      origin: "localhost:8900",
      pattern: "http://localhost:8900/*",
    });
  });

  it("offers nothing for pages that cannot print and cannot be granted", () => {
    for (const url of [
      "chrome://extensions/",
      "chrome-extension://abcdefghijklmnop/popup.html",
      "about:blank",
      "file:///tmp/x.html",
      "",
      "not a url",
    ]) {
      expect(grantableSite(url), url).toBeNull();
    }
  });
});

describe("reloading a granted page", () => {
  const granted = {
    origin: "pos.thornbury.app",
    pattern: "https://pos.thornbury.app/*",
    denied: false,
    usesQz: false,
    granted: true,
    relaying: false,
  };

  it("closes the popup only once the reload has been issued", async () => {
    // Closing first ends the popup's context while reloadActiveTab is still
    // resolving a tab id, so chrome.tabs.reload is never reached and the page
    // stays exactly as it was, with the prompt still asking.
    const order: string[] = [];
    let releaseReload = (): void => {};
    const reloadActiveTab = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          releaseReload = () => {
            order.push("reloaded");
            resolve();
          };
        }),
    );
    const close = vi.spyOn(window, "close").mockImplementation(() => order.push("closed"));

    const d = deps({ activeSite: vi.fn().mockResolvedValue(granted), reloadActiveTab });
    await main(root, d);

    root.querySelector<HTMLElement>('[data-action="reload-site"]')?.click();
    expect(reloadActiveTab).toHaveBeenCalled();
    expect(close).not.toHaveBeenCalled();

    releaseReload();
    await vi.waitFor(() => expect(close).toHaveBeenCalled());
    expect(order).toEqual(["reloaded", "closed"]);
  });
});
