import { EscpostError, type ErrorCode } from "../../packages/browser/src/errors";
import { recordUnmatched, resolvePrinterName, type AliasMap, type UnmatchedRequest } from "./aliases";
import { DaemonClient, type DaemonPrinter } from "./daemon";
import { deriveIdentity, type IdentifiablePrinter } from "./fingerprint";
import { PROTOCOL_VERSION, type WorkerRequest, type WorkerResponse } from "./protocol";
import { ReceiptfulClient, type PrinterPayload } from "./receiptful";
import { cacheKey, RenderCache } from "./render-cache";
import { SessionStore, type AccountState, type StorageArea } from "./session";
import { grantedOrigins, syncRegistrations, type ScriptingArea } from "./registration";
import { DAEMON_BASE, RECEIPTFUL_BASE } from "./config";
import { findDaemonBase, portStoreFrom } from "./daemon-port";


/** The daemon may report no profile for a printer, and printers.toml is local
 *  and merchant-edited, so its profile vocabulary is not Receiptful's catalog.
 *  This is the same default the API falls back to. */
const DEFAULT_PROFILE = "NT-5890K";

/** fingerprint -> the canonical catalog profile the SERVER resolved at
 *  registration. Persisted because an MV3 worker is recycled constantly and
 *  re-registering on every print would be a round trip per receipt. */
const RESOLVED_PROFILES_KEY = "resolvedProfiles";

/** Flat in dist/, like popup.html and settings.html, and the same string the
 *  popup's "Add an email to unlock HTML" button already opens. */
const WELCOME_PAGE = "welcome.html";

/** Operations that read or change the account. Only the extension's own
 *  pages may call these — a web page that could sign a merchant out
 *  mid-shift, or read their email back, is not a page-facing API. */
const ACCOUNT_OPS: ReadonlySet<string> = new Set(["auth.status", "auth.start", "auth.signout"]);

/** The one origin allowed to hand us a session token: our own verify page.
 *  Any granted page could otherwise sign the merchant into an attacker's
 *  account by offering a token of the attacker's choosing. */
const RECEIPTFUL_ORIGIN = new URL(RECEIPTFUL_BASE).origin;

export interface WorkerDeps {
  daemon: DaemonClient;
  storage: StorageArea;
  receiptful: ReceiptfulClient;
  session: SessionStore;
  renderCache: RenderCache;
  isOriginGranted(origin: string): Promise<boolean>;
  readAliases(): Promise<AliasMap>;
  recordUnmatchedName(requested: string, origin: string, at: number): Promise<void>;
  badge(text: string): void;
  now(): number;
}

interface AuthStatus {
  signedIn: boolean;
  account: AccountState | null;
}

export async function handleMessage(
  message: WorkerRequest,
  origin: string,
  deps: WorkerDeps,
): Promise<WorkerResponse> {
  // the package and the extension ship on different schedules. A shape
  // disagreement must be named here, not discovered as a printing bug on a
  // customer's site. An absent version means an injected surface from this same
  // build (window.qz, the WebSocket patch), which is never out of step.
  if (message.protocol !== undefined && message.protocol !== PROTOCOL_VERSION) {
    const stale = message.protocol > PROTOCOL_VERSION ? "extension" : "@escpost/browser";
    return fail(
      "VERSION_MISMATCH",
      `The page is speaking protocol v${message.protocol} and this extension speaks v${PROTOCOL_VERSION}. ` +
        `Update ${stale} to the latest version.`,
    );
  }

  // an ungranted origin is refused here, before anything else happens, and
  // never by opening a permission prompt in the middle of someone's print.
  if (!(await deps.isOriginGranted(origin))) {
    deps.badge("!");
    return fail(
      "ORIGIN_NOT_GRANTED",
      `${origin} has not been granted access to escpost. Open the extension and grant this site once.`,
    );
  }

  // Only our own pages may read or change the account. isExtensionOrigin pins the
  // exact extension id, so neither a web page nor another installed extension can
  // sign this merchant out mid-shift or read their address back.
  if (ACCOUNT_OPS.has(message.op) && !isExtensionOrigin(origin)) {
    return fail(
      "ORIGIN_NOT_GRANTED",
      "Only the escpost extension's own pages can read or change the account.",
    );
  }

  // auth.bridge arrives from the verify page, not from an extension page, so
  // it cannot use the ACCOUNT_OPS rule. It gets a stricter one instead.
  if (message.op === "auth.bridge" && origin !== RECEIPTFUL_ORIGIN) {
    return fail(
      "ORIGIN_NOT_GRANTED",
      "Only Receiptful's own sign-in page can hand this extension a session.",
    );
  }

  try {
    switch (message.op) {
      case "daemon.available":
        return { ok: true, data: await deps.daemon.available() };

      case "printers.list":
        return { ok: true, data: await deps.daemon.printers() };

      case "printers.default":
        return { ok: true, data: await deps.daemon.defaultPrinter() };

      case "auth.status":
        return { ok: true, data: await authStatus(deps) };

      case "auth.start": {
        const email = (message.payload as { email?: unknown })?.email;
        if (typeof email !== "string" || email.trim() === "") {
          return fail("NOT_SIGNED_IN", "Enter an email address to sign in.");
        }
        return { ok: true, data: await deps.receiptful.startAuth(email.trim()) };
      }

      case "auth.bridge": {
        // Link-only sign-in: the server gave this token to the browser that
        // clicked the link, and the verify page's content script carried it
        // here. Validate it against the server before trusting it — the
        // account call is what proves the token is real and current.
        const token = (message.payload as { token?: unknown })?.token;
        if (typeof token !== "string" || token === "") {
          return fail("NOT_SIGNED_IN", "That sign-in link did not carry a session.");
        }
        const account = await deps.receiptful.account(token);
        await deps.session.write({ token, account });
        await registerPrinters(token, deps);
        return { ok: true, data: { signedIn: true, account } };
      }

      case "auth.signout": {
        const session = await deps.session.read();
        if (session !== null) {
          // revoked on the server, not merely forgotten here.
          await deps.receiptful.signOut(session.token);
          await deps.session.clear();
        }
        return { ok: true, data: { signedIn: false, account: null } };
      }

      case "print": {
        const payload = message.payload as { printer?: unknown; data?: unknown; html?: unknown };
        if (typeof payload?.printer !== "string") {
          return fail("PRINT_FAILED", "A print needs a printer name.");
        }

        const known = await deps.daemon.printers();
        const aliases = await deps.readAliases();
        const resolved = resolvePrinterName(payload.printer, known, aliases);

        if (resolved === null) {
          // capture what they asked for. This is the migration failure we can actually fix.
          await deps.recordUnmatchedName(payload.printer, origin, deps.now());
          const names = known.map((printer) => printer.name).join(", ") || "none configured";
          return fail(
            "PRINTER_NOT_FOUND",
            `No printer matches "${payload.printer}". Known printers: ${names}. ` +
              "Create an alias in the escpost extension's settings to map this name.",
          );
        }

        if (typeof payload.html === "string") {
          return await printHtml(payload.html, resolved, known, origin, deps);
        }
        if (typeof payload.data !== "string") {
          return fail("PRINT_FAILED", "A print needs either base64 `data` or `html`.");
        }
        // the raw path ends here. No token is read, no account is
        // consulted and nothing leaves the machine — signed in or not.
        return { ok: true, data: await deps.daemon.print(resolved, payload.data) };
      }

      default:
        // Never fall through silently: an unanswered message hangs the page's promise forever.
        return fail("PRINT_FAILED", `Unknown operation "${message.op}".`);
    }
  } catch (error) {
    if (error instanceof EscpostError) {
      // escpost may have restarted onto another port. Forget where it was, so
      // the next call looks again rather than repeating a stale address.
      if (error.code === "DAEMON_NOT_RUNNING") forgetDaemonBase();
      return fail(error.code, error.message);
    }
    return fail("PRINT_FAILED", error instanceof Error ? error.message : String(error));
  }
}

/**
 * The metered path.
 *
 * Cache first, because a cache hit is the one HTML print that works offline
 * and costs nothing. Then render remotely, print the bytes exactly as a raw
 * job, and report the outcome — including a failure, so a unit that was
 * spent on a receipt the printer then refused is visible rather than silent.
 */
async function printHtml(
  html: string,
  printerId: string,
  known: DaemonPrinter[],
  origin: string,
  deps: WorkerDeps,
): Promise<WorkerResponse> {
  const session = await deps.session.read();
  if (session === null) {
    return fail(
      "NOT_SIGNED_IN",
      "HTML receipts need a Receiptful account. Open the escpost extension to sign in. " +
        "Raw ESC/POS printing is unaffected and needs no account.",
    );
  }

  const entry = known.find((printer) => printer.id === printerId);
  const fingerprint = deriveIdentity(entry as IdentifiablePrinter).fingerprint;
  // NOT entry.profile: that is the daemon's string, which /render rejects
  // outright when printers.toml uses a name our catalog does not carry — the
  // failure mode where HTML breaks and raw keeps working, pointing at nothing.
  // The server resolved this onto its catalog at registration; use that.
  const profile = (await readResolvedProfiles(deps))[fingerprint] ?? DEFAULT_PROFILE;
  // Keyed on the origin too, so one site cannot probe another's cached receipts.
  const key = await cacheKey(html, profile, origin);

  const cached = await deps.renderCache.read(key);
  if (cached !== null) {
    // no round trip, no second charge, and it works with no network.
    return { ok: true, data: await deps.daemon.print(printerId, cached) };
  }

  const rendered = await deps.receiptful.render(session.token, {
    html,
    profile,
    printerFingerprint: fingerprint,
  });
  await deps.renderCache.write(key, rendered.data);
  await deps.session.updateAccount({
    ...session.account,
    signupAllowanceRemaining: rendered.signupAllowanceRemaining,
    monthlyUsed: rendered.monthlyUsed,
  });

  try {
    const job = await deps.daemon.print(printerId, rendered.data);
    // Awaited rather than fired and forgotten: an MV3 worker can be
    // terminated the moment this promise resolves the caller, and a report
    // that never arrives is the silent failure this guards against. The
    // receipt has already physically printed by this point.
    await deps.receiptful.reportResult(session.token, rendered.jobId, "completed");
    return { ok: true, data: job };
  } catch (error) {
    await deps.receiptful.reportResult(
      session.token,
      rendered.jobId,
      "failed",
      error instanceof Error ? error.message : String(error),
    );
    throw error;
  }
}

async function authStatus(deps: WorkerDeps): Promise<AuthStatus> {
  const session = await deps.session.read();
  return session === null
    ? { signedIn: false, account: null }
    : { signedIn: true, account: session.account };
}

/** Register on sign-in, keyed on device identity. Best effort: a
 *  registration that fails must not undo a sign-in that worked. */
async function registerPrinters(token: string, deps: WorkerDeps): Promise<void> {
  try {
    const known = (await deps.daemon.printers()) as IdentifiablePrinter[];
    const payloads: PrinterPayload[] = known.map((printer) => {
      const identity = deriveIdentity(printer);
      return {
        fingerprint: identity.fingerprint,
        strength: identity.strength,
        entry_id: identity.entryId,
        name: printer.name,
        profile: printer.profile ?? DEFAULT_PROFILE,
      };
    });
    const registered = await deps.receiptful.registerPrinters(token, payloads);
    // Remember the canonical names, so the next HTML print sends something
    // /render will accept rather than the daemon's own vocabulary.
    const resolved: Record<string, string> = {};
    for (const printer of registered ?? []) resolved[printer.fingerprint] = printer.profile;
    if (Object.keys(resolved).length > 0) {
      await deps.storage.set({ [RESOLVED_PROFILES_KEY]: resolved });
    }
  } catch {
    // The daemon may be down at sign-in time. The next sign-in or printer
    // change registers them.
  }
}

async function readResolvedProfiles(deps: WorkerDeps): Promise<Record<string, string>> {
  const stored = await deps.storage.get(RESOLVED_PROFILES_KEY);
  return (stored[RESOLVED_PROFILES_KEY] as Record<string, string> | undefined) ?? {};
}

function fail(code: ErrorCode | string, message: string): WorkerResponse {
  return { ok: false, error: { code, message } };
}

/**
 * Is this origin one of OUR OWN extension pages (the popup, and later settings)?
 *
 * Compared against chrome.runtime.id specifically, never a bare `chrome-extension://`
 * prefix: another installed extension's pages carry ITS id, not ours, and must not be
 * able to drive this worker. A content script always reports the page's origin, never
 * the extension's, so this can only ever match a page we ship.
 */
export function isExtensionOrigin(origin: string): boolean {
  return origin === "chrome-extension://" + chrome.runtime.id;
}

/** Wiring for the real service worker. Kept apart from handleMessage so tests need no chrome API. */
/**
 * The base escpost answered on, remembered for this worker's lifetime.
 *
 * A service worker is recycled constantly, so this is a short-lived cache in
 * front of the stored port rather than the record itself. It is cleared when a
 * call fails, so a daemon that restarts on another port is found again without
 * the user doing anything.
 */
let resolvedBase: string | null = null;

async function daemonBase(storage: StorageArea): Promise<string> {
  if (resolvedBase !== null) return resolvedBase;
  const found = await findDaemonBase(portStoreFrom(storage));
  // Nothing answered. Return the first port anyway so the failure that follows
  // is the daemon's own "not running", not a different error from here.
  resolvedBase = found ?? DAEMON_BASE;
  return resolvedBase;
}

/** Called when a request fails, so the next one looks for escpost again. */
export function forgetDaemonBase(): void {
  resolvedBase = null;
}

export function liveDeps(): WorkerDeps {
  const storage = chrome.storage.local as unknown as StorageArea;
  return {
    daemon: new DaemonClient(() => daemonBase(storage)),
    storage,
    receiptful: new ReceiptfulClient(RECEIPTFUL_BASE),
    session: new SessionStore(storage),
    renderCache: new RenderCache(storage),
    async isOriginGranted(origin) {
      // Our own extension pages are trusted by construction: they are our code, and
      // their origin can never appear in an optional host permission, so asking
      // chrome.permissions.contains about them would refuse the popup on every open.
      if (isExtensionOrigin(origin)) return true;
      return await chrome.permissions.contains({ origins: [`${origin}/*`] });
    },
    async readAliases() {
      const stored = await chrome.storage.local.get("aliases");
      return (stored.aliases as AliasMap | undefined) ?? {};
    },
    async recordUnmatchedName(requested, origin, at) {
      const stored = await chrome.storage.local.get("unmatched");
      const seen = (stored.unmatched as UnmatchedRequest[] | undefined) ?? [];
      await chrome.storage.local.set({ unmatched: recordUnmatched(requested, origin, seen, at) });
    },
    badge(text) {
      void chrome.action.setBadgeText({ text });
    },
    now: () => Date.now(),
  };
}

// Registering at module scope is what makes this a service worker, but the same
// module is imported by tests that run in plain Node, where `chrome` does not exist.
// The guard keeps this file both the manifest entry point and a testable module.
if (typeof chrome !== "undefined" && chrome.runtime?.onInstalled) {
  chrome.runtime.onInstalled.addListener((details) => {
    // once, on install. Never on update — an update is not a first run,
    // and a tab opening by itself after an auto-update is a nag.
    if (details.reason !== "install") return;
    void chrome.tabs.create({ url: chrome.runtime.getURL(WELCOME_PAGE) });
  });
}

/**
 * Keep the injected scripts matching the granted sites.
 *
 * Run on install and on startup as well as on change, because registrations
 * persist across sessions and the grants they were built from can be revoked
 * while the worker is not running.
 */
async function refreshRegistrations(): Promise<void> {
  const granted = await chrome.permissions.getAll();
  const declared = chrome.runtime.getManifest().host_permissions ?? [];
  await syncRegistrations(
    chrome.scripting as unknown as ScriptingArea,
    grantedOrigins(granted.origins ?? [], declared),
  );
}

if (typeof chrome !== "undefined" && chrome.permissions?.onAdded) {
  chrome.permissions.onAdded.addListener(() => void refreshRegistrations());
  chrome.permissions.onRemoved.addListener(() => void refreshRegistrations());
  chrome.runtime.onStartup?.addListener(() => void refreshRegistrations());
  chrome.runtime.onInstalled?.addListener(() => void refreshRegistrations());
}

if (typeof chrome !== "undefined" && chrome.runtime?.onMessage) {
  chrome.runtime.onMessage.addListener((message: WorkerRequest, sender, sendResponse) => {
    const origin = sender.origin ?? (sender.url ? new URL(sender.url).origin : "");
    handleMessage(message, origin, liveDeps()).then(sendResponse);
    return true; // keep the channel open for the async reply
  });
}
