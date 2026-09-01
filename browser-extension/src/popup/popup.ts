import "./popup.css";
import { registerGrantedRelay, type RegistrationDependencies } from "../registration";
import { renderPopup } from "../ui/dom";
import { currentSiteOrigin, type SiteOrigin } from "../ui/origins";
import { probeRelayStatus, type RelayProbeScripting } from "../ui/status";
import { buildPopupView, type PopupModelInput, type PopupView } from "./model";

type Tab = { id?: number; url?: string };

export type PopupDependencies = {
  document: Document;
  tabs: { query(details: { active: boolean; currentWindow: boolean }): Promise<Tab[]> };
  permissions: {
    contains(details: { origins: string[] }): Promise<boolean>;
    request(details: { origins: string[] }): Promise<boolean>;
    remove(details: { origins: string[] }): Promise<boolean>;
  };
  scripting: RelayProbeScripting;
  syncRegistrations(): Promise<void>;
};

export type PopupController = { refresh(): Promise<void> };

export function installPopup(deps: PopupDependencies): PopupController {
  const main = deps.document.querySelector("main") ?? deps.document.body;
  let currentSite: SiteOrigin | null = null;
  let currentView: PopupView | null = null;

  const render = (input: PopupModelInput) => {
    currentView = buildPopupView(input);
    renderPopup(main, currentView, onPrimaryAction);
  };

  const refresh = async (): Promise<void> => {
    let tab: Tab;
    try {
      [tab] = await deps.tabs.query({ active: true, currentWindow: true });
    } catch {
      currentSite = null;
      render({ origin: null, grant: "unknown", relay: "unknown", daemon: "unknown", error: "Could not read the active tab." });
      return;
    }
    currentSite = currentSiteOrigin(tab?.url);
    if (currentSite === null) {
      render({ origin: null, grant: "unknown", relay: "unknown", daemon: "unknown" });
      return;
    }
    let granted: boolean;
    try {
      granted = await deps.permissions.contains({ origins: [currentSite.pattern] });
    } catch {
      render({ origin: currentSite.origin, grant: "unknown", relay: "unknown", daemon: "unknown", error: "Could not verify site access." });
      return;
    }
    if (!granted) {
      render({ origin: currentSite.origin, grant: "absent", relay: "unknown", daemon: "unknown" });
      return;
    }
    if (tab?.id === undefined) {
      render({ origin: currentSite.origin, grant: "present", relay: "unknown", daemon: "unknown", error: "Could not contact the page relay." });
      return;
    }
    const status = await probeRelayStatus(tab.id, currentSite.origin, deps.scripting);
    render({ origin: currentSite.origin, grant: "present", relay: status.relay, daemon: status.daemon, error: status.error });
  };

  const onPrimaryAction = () => {
    const site = currentSite;
    const action = currentView?.primaryAction;
    if (site === null || action === undefined || action === null) return;
    if (action.kind === "grant") {
      // Chrome requires this prompt to be initiated before any await or queued task.
      const requested = deps.permissions.request({ origins: [site.pattern] });
      void requested.then(onPermissionChanged, () => renderFailure("Could not change site access."));
      return;
    }
    const removed = deps.permissions.remove({ origins: [site.pattern] });
    void removed.then(onPermissionChanged, () => renderFailure("Could not change site access."));
  };

  const onPermissionChanged = async (): Promise<void> => {
    try {
      await deps.syncRegistrations();
    } catch {
      renderFailure("Could not update site access.");
      return;
    }
    await refresh();
  };

  const renderFailure = (error: string) => {
    render({
      origin: currentSite?.origin ?? null,
      grant: currentSite === null ? "unknown" : "absent",
      relay: "unknown",
      daemon: "unknown",
      error,
    });
  };

  void refresh().catch(() => renderFailure("Could not refresh popup status."));
  return { refresh };
}

function chromeDependencies(): PopupDependencies {
  const registration: RegistrationDependencies = { permissions: chrome.permissions, scripting: chrome.scripting };
  return {
    document,
    tabs: chrome.tabs,
    permissions: chrome.permissions,
    scripting: chrome.scripting,
    syncRegistrations: () => registerGrantedRelay(registration),
  };
}

if (typeof chrome !== "undefined" && typeof document !== "undefined") {
  installPopup(chromeDependencies());
}
