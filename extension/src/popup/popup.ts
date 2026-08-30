import type { DaemonPrinter } from "../daemon";
import type { WorkerResponse } from "../protocol";
import { ACCOUNT_KEY, readAccountSnapshot, type AccountSnapshot } from "../ui/account-snapshot";
import { el, renderPill } from "../ui/dom";
import { isWebOrigin } from "../ui/origins";
import {
  describePopup,
  type PendingSite,
  type PopupInput,
  type PopupPrinter,
  type PopupSection,
  type PopupView,
} from "./state";

/**
 * Pure DOM: every decision about what a state says was made in state.ts. Class names
 * come from the `.pop` mockup in docs/escpost-extension-architecture.html.
 */
export function renderPopup(root: HTMLElement, view: PopupView): void {
  root.textContent = "";
  root.dataset["state"] = view.kind;

  const pop = el("div", "pop");

  const top = el("div", "pop-top");
  // The wordmark, set as the logo sets it: esc in ink, post in teal.
  const mark = el("span", "pop-mark", "esc");
  mark.append(el("b", "", "post"));
  top.append(mark, renderPill(view.status));
  pop.append(top);

  for (const section of view.sections) pop.append(renderSection(section));

  const foot = el("div", "pop-foot");
  for (const item of view.footer) {
    const button = el("button", "pop-foot-act", item.label);
    button.type = "button";
    button.dataset["action"] = item.action;
    foot.append(button);
  }
  pop.append(foot);

  root.append(pop);
}

/** Walks PopupSection's fields in declaration order, which is also their render order. */
function renderSection(section: PopupSection): HTMLDivElement {
  const node = el("div", "pop-sec");

  if (section.label !== undefined) node.append(el("div", "pop-lab", section.label));

  if (section.strip !== undefined) {
    const strip = el("div", `strip ${section.strip.tone}`);
    for (const part of section.strip.parts) {
      strip.append(part.strong ? el("strong", "", part.text) : document.createTextNode(part.text));
    }
    node.append(strip);
  }

  if (section.lead !== undefined) node.append(el("p", "pop-note", section.lead));
  if (section.command !== undefined) node.append(el("div", "pop-cmd", section.command));

  for (const row of section.rows ?? []) {
    const line = el("div", "pop-row");
    line.append(el("span", "k", row.key));
    // The dotted leader. These rows are receipt lines, "Raw ESC/POS ... Unlimited",
    // and this is the device the design system has for that shape.
    // aria-hidden because it is a rule, not content: a screen reader should hear
    // the label and the value, with nothing between them.
    const leader = el("span", "leader");
    leader.setAttribute("aria-hidden", "true");
    line.append(leader);
    if (row.pill !== undefined) line.append(renderPill(row.pill));
    else line.append(el("span", "v", row.value ?? ""));
    node.append(line);
  }

  if (section.meter !== undefined) {
    const meter = el("div", `meter ${section.meter.tone}`);
    const fill = el("i");
    fill.style.width = `${Math.round(section.meter.fraction * 1000) / 10}%`;
    meter.append(fill);
    node.append(meter);
  }

  if (section.button !== undefined) {
    const button = el(
      "button",
      `pop-btn ${section.button.style === "ghost" ? "ghost" : ""}`.trim(),
      section.button.label,
    );
    button.type = "button";
    button.dataset["action"] = section.button.action;
    // The grant needs its match pattern available at click time: the handler
    // cannot go and look it up without spending the user gesture.
    if (section.button.value !== undefined) button.dataset["value"] = section.button.value;
    node.append(button);
  }

  if (section.note !== undefined) node.append(el("p", "pop-note", section.note));
  if (section.detail !== undefined) node.append(el("p", "pop-detail", section.detail));

  return node;
}

const PLANS_URL = "https://receiptful.io/#pricing";
const INSTALL_URL = "https://escpost.dev";
const WELCOME_PAGE = "welcome.html";

export type DaemonResult =
  | { ok: true; printers: DaemonPrinter[]; defaultId: string | null }
  | { ok: false; message: string };

export interface PopupDeps {
  readDaemon(): Promise<DaemonResult>;
  readAccount(): Promise<AccountSnapshot | null>;
  countSites(): Promise<number>;
  isOnline(): boolean;
  clearBadge(): void;
  openUrl(url: string): void;
  openWelcome(): void;
  openSettings(): void;
  /** The active tab, if it is a site that could print and has not been granted. */
  activeSite(): Promise<{
    origin: string;
    pattern: string;
    usesQz: boolean;
    /** Already allowed to print. */
    granted: boolean;
    /** The relay is on the page. False on a granted page that loaded before
     *  the grant, which is the case a reload fixes. */
    relaying: boolean;
  } | null>;
  /**
   * the one click. MUST call chrome.permissions.request synchronously: Chrome
   * drops the user gesture across an await and the prompt then never opens.
   */
  requestGrant(pattern: string): Promise<boolean>;
  recordGrant(pattern: string): Promise<void>;
  /** The granted page loaded before its scripts existed, so it needs one. */
  reloadActiveTab(): Promise<void>;
}

/**
 * Whether the extension's relay is on the page.
 *
 * A granted site whose page loaded before the grant has no scripts on it, and
 * nothing about the permission says so. The relay marks the page it reaches, so
 * its absence is the signal that a reload is still needed.
 */
export async function pageIsRelaying(tabId: number | undefined): Promise<boolean> {
  if (tabId === undefined) return false;
  try {
    const [result] = await chrome.scripting.executeScript({
      target: { tabId },
      world: "MAIN",
      func: () => document.documentElement.hasAttribute("data-escpost-relay"),
    });
    return result?.result === true;
  } catch {
    return false;
  }
}

/**
 * Whether the page is a QZ Tray integration.
 *
 * Opening the popup is the user invoking the extension, which is what activates
 * activeTab and makes this the first moment an ungranted page can be looked at
 * at all. Nothing here reads content: it asks whether the QZ client is present
 * or its script is referenced.
 *
 * False on any failure. A wrong guess only costs a less specific sentence.
 */
export async function pageUsesQz(tabId: number | undefined): Promise<boolean> {
  if (tabId === undefined) return false;
  try {
    const [result] = await chrome.scripting.executeScript({
      target: { tabId },
      world: "MAIN",
      func: () =>
        typeof (globalThis as { qz?: unknown }).qz !== "undefined" ||
        document.querySelector('script[src*="qz-tray"]') !== null,
    });
    return result?.result === true;
  } catch {
    return false;
  }
}

/** The daemon gives us a transport and a status but no address, so this is all we can say. */
export function toPopupPrinter(printer: DaemonPrinter, defaultId: string | null): PopupPrinter {
  const bits = [printer.transport === "usb" ? "USB" : "Network"];
  if (printer.id === defaultId) bits.push("default");
  if (printer.status === "unavailable") bits.push("unavailable");
  return { name: printer.name, detail: bits.join(" · ") };
}

export async function loadInput(
  deps: PopupDeps,
  deniedPattern: string | null = null,
  grantedPattern: string | null = null,
): Promise<PopupInput> {
  const [daemon, account, siteCount, site] = await Promise.all([
    deps.readDaemon(),
    deps.readAccount(),
    deps.countSites(),
    deps.activeSite(),
  ]);

  // A granted site that is already relaying needs nothing said about it. One
  // that is granted but not relaying loaded before its scripts existed, and the
  // reload is the only thing standing between it and printing.
  const pendingSite: PendingSite | null =
    site === null || (site.granted && site.relaying)
      ? null
      : {
          origin: site.origin,
          pattern: site.pattern,
          denied: site.pattern === deniedPattern,
          needsReload: site.granted || site.pattern === grantedPattern,
          usesQz: site.usesQz,
        };

  return {
    pendingSite,
    daemon: daemon.ok
      ? { running: true, printers: daemon.printers.map((printer) => toPopupPrinter(printer, daemon.defaultId)) }
      : { running: false, message: daemon.message },
    account,
    online: deps.isOnline(),
    siteCount,
  };
}

export async function refresh(
  root: HTMLElement,
  deps: PopupDeps,
  deniedPattern: string | null = null,
  grantedPattern: string | null = null,
): Promise<void> {
  renderPopup(root, describePopup(await loadInput(deps, deniedPattern, grantedPattern)));
}

export function bindActions(root: HTMLElement, deps: PopupDeps): void {
  root.addEventListener("click", (event) => {
    const target = event.target;
    if (!(target instanceof HTMLElement)) return;
    const action = target.closest("[data-action]")?.getAttribute("data-action");

    switch (action) {
      case "grant-site": {
        const pattern = target.closest<HTMLElement>("[data-action]")?.dataset["value"] ?? "";
        if (pattern === "") return;
        // Called here, synchronously, on purpose. Anything awaited first — reading
        // storage, re-checking permissions — spends the user gesture, and
        // chrome.permissions.request then resolves false with no prompt shown.
        const asked = deps.requestGrant(pattern);
        void asked.then(async (granted) => {
          if (granted) await deps.recordGrant(pattern);
          // Nothing was prompted mid-print. This is the popup
          // re-reading itself after an action the user chose to take.
          await refresh(root, deps, granted ? null : pattern, granted ? pattern : null);
        });
        return;
      }

      case "reload-site":
        // Closing first would end this popup's context while the reload is
        // still resolving a tab id, and the page would never reload at all.
        void deps.reloadActiveTab().then(() => window.close());
        return;

      case "check-again":
        void refresh(root, deps);
        return;
      case "open-welcome":
        deps.openWelcome();
        return;
      case "open-plans":
        deps.openUrl(PLANS_URL);
        return;
      case "open-install-help":
        deps.openUrl(INSTALL_URL);
        return;
      case "open-settings":
        deps.openSettings();
        return;
      default:
        return;
    }
  });
}

export async function main(root: HTMLElement, deps: PopupDeps): Promise<void> {
  bindActions(root, deps);
  // The extension is badged when an ungranted origin is refused. They have looked now.
  deps.clearBadge();
  await refresh(root, deps);
}

async function ask(op: string): Promise<WorkerResponse> {
  try {
    return (await chrome.runtime.sendMessage({ op, payload: undefined })) as WorkerResponse;
  } catch (error) {
    // A worker that will not answer is indistinguishable, from here, from a dead daemon.
    return {
      ok: false,
      error: { code: "DAEMON_NOT_RUNNING", message: error instanceof Error ? error.message : String(error) },
    };
  }
}

export function liveDeps(): PopupDeps {
  return {
    async readDaemon() {
      const listed = await ask("printers.list");
      if (!listed.ok) return { ok: false, message: listed.error.message };

      const fallback = await ask("printers.default");
      const defaultPrinter = fallback.ok ? (fallback.data as DaemonPrinter | null) : null;
      return { ok: true, printers: listed.data as DaemonPrinter[], defaultId: defaultPrinter?.id ?? null };
    },
    async readAccount() {
      const stored = await chrome.storage.local.get(ACCOUNT_KEY);
      return readAccountSnapshot(stored[ACCOUNT_KEY]);
    },
    async countSites() {
      const granted = await chrome.permissions.getAll();
      const declared = chrome.runtime.getManifest().host_permissions ?? [];
      return (granted.origins ?? []).filter((pattern) => isWebOrigin(pattern, declared)).length;
    },
    // The only offline signal available until the render path exists. When it does,
    // it should store a "render unreachable" flag and this should AND it in.
    isOnline: () => navigator.onLine,
    clearBadge: () => void chrome.action.setBadgeText({ text: "" }),
    openUrl: (url) => void chrome.tabs.create({ url }),
    openWelcome: () => void chrome.tabs.create({ url: chrome.runtime.getURL(WELCOME_PAGE) }),
    openSettings: () => chrome.runtime.openOptionsPage(),

    async activeSite() {
      // activeTab gives us the active tab's URL because the user just invoked
      // the extension by opening this popup. No "tabs" permission is needed.
      const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
      const site = grantableSite(tab?.url ?? "");
      if (site === null) return null;
      const granted = await chrome.permissions.contains({ origins: [site.pattern] });
      const [usesQz, relaying] = await Promise.all([pageUsesQz(tab?.id), pageIsRelaying(tab?.id)]);
      return { ...site, usesQz, granted, relaying };
    },

    requestGrant(pattern) {
      // Returned, not awaited: the caller invokes this straight from the click.
      return chrome.permissions.request({ origins: [pattern] });
    },

    async reloadActiveTab() {
      const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
      if (tab?.id !== undefined) await chrome.tabs.reload(tab.id);
    },

    async recordGrant(pattern) {
      const stored = await chrome.storage.local.get(GRANTS_KEY);
      const grants = (stored[GRANTS_KEY] as Record<string, { at?: number; via?: string }> | undefined) ?? {};
      grants[pattern] = { at: Date.now(), via: "granted from the extension" };
      await chrome.storage.local.set({ [GRANTS_KEY]: grants });
    },
  };
}

/** The storage key the settings page reads to say when and how a site was granted. */
const GRANTS_KEY = "grants";

/**
 * Which tab URLs can be granted at all. Not chrome:// pages, not the extension's
 * own pages, not the new-tab page — asking about those would be noise, and
 * chrome.permissions.request rejects them anyway.
 */
export function grantableSite(url: string): { origin: string; pattern: string } | null {
  let parsed: URL;
  try {
    parsed = new URL(url);
  } catch {
    return null;
  }
  if (parsed.protocol !== "https:" && parsed.protocol !== "http:") return null;
  if (parsed.hostname === "") return null;
  return {
    origin: parsed.host,
    pattern: `${parsed.protocol}//${parsed.host}/*`,
  };
}

// Self-starts only inside the real extension, and only against the shell's #app. Tests
// render into a bare element with no id, so importing this module does nothing there.
if (typeof chrome !== "undefined" && chrome.runtime?.id !== undefined) {
  const app = document.querySelector<HTMLElement>("#app");
  if (app !== null) void main(app, liveDeps());
}
