import type { AliasMap, UnmatchedRequest } from "../aliases";
import type { DaemonPrinter } from "../daemon";
import type { WorkerResponse } from "../protocol";
import { ACCOUNT_KEY, readAccountSnapshot } from "../ui/account-snapshot";
import { el, renderPill } from "../ui/dom";
import { pill } from "../ui/status";
import {
  describeSettings,
  type AliasMeta,
  type GrantMeta,
  type PrinterChoice,
  type SettingsInput,
  type SettingsView,
  type UnmatchedRow,
} from "./model";
import { createAlias, dismissUnmatched, forgetGrant, removeAlias, type AliasState } from "./mutations";

export function renderSettings(root: HTMLElement, view: SettingsView): void {
  root.textContent = "";
  const page = el("div", "set");

  if (view.account !== null) {
    const account = section("Account");
    account.append(row(cell([view.account.email], view.account.sub), actionButton("Sign out", "sign-out", {})));
    page.append(account);
  }

  const sites = section("Sites that can print");
  if (view.sites.length === 0) {
    sites.append(
      el(
        "p",
        "set-note",
        "No site has been granted access yet. A site is asked once, the first time it tries to print.",
      ),
    );
  }
  for (const site of view.sites) {
    sites.append(row(cell([site.origin], site.sub), actionButton("Revoke", "revoke", { pattern: site.pattern })));
  }
  if (view.sites.length > 0) {
    sites.append(
      el(
        "p",
        "set-note",
        "Each site asked once, when it first tried to print. Revoking takes effect immediately and the site is asked again next time.",
      ),
    );
  }
  page.append(sites);

  const names = section("Printer names");
  for (const alias of view.aliases) {
    const label = alias.matched
      ? cell([`“${alias.requested}” → ${alias.target}`], alias.sub)
      : cell([`“${alias.requested}” → ${alias.target} `, renderPill(pill("Printer gone", "warn"))], alias.sub);
    names.append(row(label, actionButton("Remove", "remove-alias", { requested: alias.requested })));
  }
  for (const entry of view.unmatched) {
    const label = cell([`“${entry.requested}” → `, renderPill(pill("Not matched", "warn"))], entry.sub);
    names.append(row(label, aliasControls(entry, view.printerChoices)));
  }
  if (view.aliases.length === 0 && view.unmatched.length === 0) {
    names.append(el("p", "set-note", "No site has asked for a printer name escpost does not know. Nothing to fix."));
  }
  names.append(
    el(
      "p",
      "set-note",
      "Sites written for QZ Tray ask for operating-system printer names. escpost uses the names in your printers.toml. " +
        "An alias bridges the two so their code doesn’t have to change.",
    ),
  );
  page.append(names);

  if (view.usage !== null) {
    const usage = section(view.usage.title);
    usage.append(valueRow(cell(["HTML receipts"], view.usage.resets), value(view.usage.html)));
    usage.append(valueRow(cell(["Raw ESC/POS receipts"], "never counted, never sent anywhere"), value(view.usage.raw)));
    page.append(usage);
  }

  const about = section("About");
  about.append(valueRow(cell(["escpost"], "USB · RAW TCP"), value(view.about.daemon)));
  about.append(valueRow(cell(["Extension"], "Chrome Web Store"), value(view.about.extension)));
  page.append(about);

  root.append(page);
}

function section(title: string): HTMLDivElement {
  const node = el("div", "set-sec");
  node.append(el("p", "set-h", title));
  return node;
}

function row(label: HTMLElement, control: HTMLElement): HTMLDivElement {
  const node = el("div", "set-row");
  node.append(label, control);
  return node;
}

/** A row that is genuinely a label -> value pair, so it gets the receipt leader.
 *  The site and alias rows are label -> action and deliberately do not. */
function valueRow(label: HTMLElement, value: HTMLElement): HTMLDivElement {
  const node = el("div", "set-row set-row-value");
  const leader = el("span", "leader");
  leader.setAttribute("aria-hidden", "true");
  node.append(label, leader, value);
  return node;
}

function cell(main: (string | Node)[], sub: string): HTMLSpanElement {
  const node = el("span");
  node.append(...main);
  node.append(el("span", "sub", sub));
  return node;
}

function value(text: string): HTMLSpanElement {
  return el("span", "set-val", text);
}

export function actionButton(label: string, action: string, data: Record<string, string>): HTMLButtonElement {
  const button = el("button", "set-act", label);
  button.type = "button";
  button.dataset["action"] = action;
  for (const [name, item] of Object.entries(data)) button.dataset[name] = item;
  return button;
}

function aliasControls(entry: UnmatchedRow, choices: PrinterChoice[]): HTMLElement {
  const wrap = el("span", "set-controls");

  if (choices.length === 0) {
    // Offering "create alias" with nothing to map onto would be a dead end.
    wrap.append(el("span", "set-act set-act-off", "no printers to map to"));
  } else {
    const select = el("select", "set-pick");
    select.setAttribute("aria-label", `Printer to map “${entry.requested}” to`);
    for (const choice of choices) {
      const option = el("option", "", choice.name);
      option.value = choice.id;
      select.append(option);
    }

    wrap.append(
      select,
      actionButton("Create alias", "create-alias", { requested: entry.requested, origin: entry.origin }),
    );
  }

  // Without this, a name from a typo in someone's source sits here reading "that print
  // failed" until twenty newer names push it out, and the list stops being worth reading.
  wrap.append(actionButton("Dismiss", "dismiss-unmatched", { requested: entry.requested, origin: entry.origin }));
  return wrap;
}

export interface SettingsDeps {
  readAll(): Promise<SettingsInput>;
  revoke(pattern: string): Promise<void>;
  writeAliases(state: AliasState): Promise<void>;
  signOut(): Promise<void>;
  now(): number;
}

export async function refreshSettings(root: HTMLElement, deps: SettingsDeps): Promise<void> {
  renderSettings(root, describeSettings(await deps.readAll()));
}

function aliasStateOf(input: SettingsInput): AliasState {
  return { aliases: input.aliases, aliasMeta: input.aliasMeta, unmatched: input.unmatched };
}

export function bindSettings(root: HTMLElement, deps: SettingsDeps): void {
  root.addEventListener("click", (event) => {
    const target = event.target;
    if (!(target instanceof HTMLElement)) return;
    const trigger = target.closest<HTMLElement>("[data-action]");
    if (trigger === null) return;

    switch (trigger.dataset["action"]) {
      case "revoke": {
        // Inline, on the row. A confirm() dialog would block the extension's message
        // channel and could not be tested at all.
        const holder = trigger.parentElement;
        if (holder === null) return;
        const pattern = trigger.dataset["pattern"] ?? "";
        holder.replaceChild(actionButton("Confirm revoke", "revoke-confirm", { pattern }), trigger);
        holder.append(actionButton("Cancel", "revoke-cancel", {}));
        return;
      }

      case "revoke-confirm": {
        const pattern = trigger.dataset["pattern"] ?? "";
        void (async () => {
          await deps.revoke(pattern);
          await refreshSettings(root, deps);
        })();
        return;
      }

      case "revoke-cancel":
        void refreshSettings(root, deps);
        return;

      case "create-alias": {
        const requested = trigger.dataset["requested"] ?? "";
        const select = trigger.parentElement?.querySelector("select");
        const printerId = select instanceof HTMLSelectElement ? select.value : "";
        if (requested === "" || printerId === "") return;
        void (async () => {
          const current = await deps.readAll();
          await deps.writeAliases(createAlias(aliasStateOf(current), requested, printerId, deps.now()));
          await refreshSettings(root, deps);
        })();
        return;
      }

      case "remove-alias": {
        const requested = trigger.dataset["requested"] ?? "";
        void (async () => {
          const current = await deps.readAll();
          await deps.writeAliases(removeAlias(aliasStateOf(current), requested));
          await refreshSettings(root, deps);
        })();
        return;
      }

      case "dismiss-unmatched": {
        const requested = trigger.dataset["requested"] ?? "";
        const origin = trigger.dataset["origin"] ?? "";
        void (async () => {
          const current = await deps.readAll();
          await deps.writeAliases(dismissUnmatched(aliasStateOf(current), requested, origin));
          await refreshSettings(root, deps);
        })();
        return;
      }

      case "sign-out":
        void (async () => {
          await deps.signOut();
          await refreshSettings(root, deps);
        })();
        return;

      default:
        return;
    }
  });
}

async function ask(op: string): Promise<WorkerResponse> {
  try {
    return (await chrome.runtime.sendMessage({ op, payload: undefined })) as WorkerResponse;
  } catch (error) {
    return {
      ok: false,
      error: { code: "DAEMON_NOT_RUNNING", message: error instanceof Error ? error.message : String(error) },
    };
  }
}

export function liveSettingsDeps(): SettingsDeps {
  return {
    async readAll() {
      const [granted, stored, listed, reachable] = await Promise.all([
        chrome.permissions.getAll(),
        chrome.storage.local.get(["aliases", "aliasMeta", "unmatched", "grants", ACCOUNT_KEY]),
        ask("printers.list"),
        ask("daemon.available"),
      ]);

      return {
        originPatterns: granted.origins ?? [],
        declaredHosts: chrome.runtime.getManifest().host_permissions ?? [],
        grants: (stored["grants"] as GrantMeta | undefined) ?? {},
        aliases: (stored["aliases"] as AliasMap | undefined) ?? {},
        aliasMeta: (stored["aliasMeta"] as AliasMeta | undefined) ?? {},
        unmatched: (stored["unmatched"] as UnmatchedRequest[] | undefined) ?? [],
        printers: listed.ok ? (listed.data as DaemonPrinter[]) : [],
        account: readAccountSnapshot(stored[ACCOUNT_KEY]),
        daemonRunning: reachable.ok && reachable.data === true,
        extensionVersion: chrome.runtime.getManifest().version,
      };
    },

    async revoke(pattern) {
      await chrome.permissions.remove({ origins: [pattern] });
      const stored = await chrome.storage.local.get("grants");
      const grants = (stored["grants"] as GrantMeta | undefined) ?? {};
      await chrome.storage.local.set({ grants: forgetGrant(grants, pattern) });
    },

    async writeAliases(state) {
      await chrome.storage.local.set({
        aliases: state.aliases,
        aliasMeta: state.aliasMeta,
        unmatched: state.unmatched,
      });
    },

    async signOut() {
      // the server-side revocation belongs to the account layer. Local state is cleared
      // either way, so a worker that does not yet answer this op still signs the user out.
      await ask("account.signOut");
      await chrome.storage.local.remove(ACCOUNT_KEY);
    },

    now: () => Date.now(),
  };
}

// Self-starts only inside the real extension, and only against the shell's #app.
if (typeof chrome !== "undefined" && chrome.runtime?.id !== undefined) {
  const app = document.querySelector<HTMLElement>("#app");
  if (app !== null) {
    const deps = liveSettingsDeps();
    bindSettings(app, deps);
    void refreshSettings(app, deps);
  }
}
