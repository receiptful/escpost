import "./popup.css";
import type { MutableOriginGrants } from "../grants";
import { renderPopup } from "../ui/dom";
import { currentSiteOrigin, type SiteOrigin } from "../ui/origins";
import { buildPopupView, type PopupModelInput, type PopupView } from "./model";

type Tab = { id?: number; url?: string };
type ActionKind = "grant" | "revoke";
type ActionSnapshot = SiteOrigin & { tabId: number; action: ActionKind; revision: number };
type TabEvents = {
  onActivated?: { addListener(listener: (activeInfo: { tabId: number }) => void): void };
  onUpdated?: { addListener(listener: (tabId: number, changeInfo: { url?: string }) => void): void };
};

export type PopupDependencies = {
  document: Document;
  tabs: { query(details: { active: boolean; currentWindow: boolean }): Promise<Tab[]> } & TabEvents;
  grants: MutableOriginGrants;
  afterGrantChange(): Promise<void>;
  probe(tabId: number): Promise<{ relay: "loaded" | "missing" | "unknown"; daemon: "running" | "unavailable" | "unknown"; error: string | null }>;
};

export type PopupController = { refresh(): Promise<void> };

export function installPopup(deps: PopupDependencies): PopupController {
  const main = deps.document.querySelector("main") ?? deps.document.body;
  let revision = 0;
  let currentAction: ActionSnapshot | null = null;
  let currentView: PopupView | null = null;
  let pendingTabId: number | undefined;

  const isCurrent = (snapshot: Pick<ActionSnapshot, "revision">) => snapshot.revision === revision;

  const render = (input: PopupModelInput, snapshot?: Omit<ActionSnapshot, "action">) => {
    const view = buildPopupView(input);
    currentView = view;
    currentAction = view.primaryAction === null || snapshot === undefined
      ? null
      : { ...snapshot, action: view.primaryAction.kind };
    renderPopup(main, view, onPrimaryAction);
  };

  const suppressAction = () => {
    currentAction = null;
    if (currentView === null || currentView.primaryAction === null) return;
    currentView = { ...currentView, primaryAction: null };
    renderPopup(main, currentView, onPrimaryAction);
  };

  const refresh = async (): Promise<void> => {
    const thisRevision = ++revision;
    pendingTabId = undefined;
    suppressAction();
    let tab: Tab;
    try {
      [tab] = await deps.tabs.query({ active: true, currentWindow: true });
    } catch {
      if (thisRevision === revision) render({ origin: null, grant: "unknown", relay: "unknown", daemon: "unknown", error: "Could not read the active tab." });
      return;
    }
    if (thisRevision !== revision) return;
    const site = currentSiteOrigin(tab?.url);
    if (site === null || tab?.id === undefined) {
      render({ origin: site?.origin ?? null, grant: "unknown", relay: "unknown", daemon: "unknown", error: site === null ? null : "Could not read the active tab." });
      return;
    }
    pendingTabId = tab.id;
    const snapshot: Omit<ActionSnapshot, "action"> = { ...site, tabId: tab.id, revision: thisRevision };
    let granted: boolean;
    try {
      granted = await deps.grants.contains(site.pattern);
    } catch {
      if (isCurrent(snapshot)) render({ origin: site.origin, grant: "unknown", relay: "unknown", daemon: "unknown", error: "Could not verify site access." });
      return;
    }
    if (!isCurrent(snapshot)) return;
    if (!granted) {
      render({ origin: site.origin, grant: "absent", relay: "unknown", daemon: "unknown" }, snapshot);
      return;
    }
    const status = await deps.probe(tab.id);
    if (!isCurrent(snapshot)) return;
    render({ origin: site.origin, grant: "present", relay: status.relay, daemon: status.daemon, error: status.error }, snapshot);
  };

  const reconcile = async (snapshot: ActionSnapshot, error: string | null) => {
    let granted: boolean;
    try {
      granted = await deps.grants.contains(snapshot.pattern);
    } catch {
      if (isCurrent(snapshot)) render({ origin: snapshot.origin, grant: "unknown", relay: "unknown", daemon: "unknown", error: "Could not verify site access." });
      return;
    }
    if (!isCurrent(snapshot)) return;
    const base: Omit<ActionSnapshot, "action"> = snapshot;
    if (!granted) {
      render({ origin: snapshot.origin, grant: "absent", relay: "unknown", daemon: "unknown", error }, base);
      return;
    }
    const status = await deps.probe(snapshot.tabId);
    if (!isCurrent(snapshot)) return;
    render({ origin: snapshot.origin, grant: "present", relay: status.relay, daemon: status.daemon, error }, base);
  };

  const completeMutation = async (snapshot: ActionSnapshot, changed: boolean, failure: string) => {
    if (!changed) {
      await reconcile(snapshot, failure);
      return;
    }
    try {
      await deps.afterGrantChange();
    } catch {
      await reconcile(snapshot, "Could not update site access.");
      return;
    }
    await reconcile(snapshot, null);
  };

  const onPrimaryAction = () => {
    const snapshot = currentAction;
    if (snapshot === null || !isCurrent(snapshot)) return;
    if (snapshot.action === "grant") {
      // Firefox requires the optional-host request to remain in the click stack.
      const requested = deps.grants.request(snapshot.pattern);
      suppressAction();
      void requested.then(
        (changed) => completeMutation(snapshot, changed, "Site access was not changed."),
        () => reconcile(snapshot, "Could not change site access."),
      );
      return;
    }
    const removed = deps.grants.remove(snapshot.pattern);
    suppressAction();
    void removed.then(
      (changed) => completeMutation(snapshot, changed, "Site access was not changed."),
      () => reconcile(snapshot, "Could not change site access."),
    );
  };

  deps.tabs.onActivated?.addListener(() => { void refresh(); });
  deps.tabs.onUpdated?.addListener((tabId, changeInfo) => {
    if (changeInfo.url !== undefined && (pendingTabId === undefined || tabId === pendingTabId)) void refresh();
  });

  void refresh();
  return { refresh };
}
