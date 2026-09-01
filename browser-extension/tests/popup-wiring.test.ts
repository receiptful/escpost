// @vitest-environment happy-dom

import { expect, test, vi } from "vitest";
import { Window } from "happy-dom";
import { installPopup, type PopupDependencies } from "../src/popup/popup";

const testWindow = new Window();
Object.assign(globalThis, { window: testWindow, document: testWindow.document });

type PopupOverrides = Omit<Partial<PopupDependencies>, "tabs"> & { tabs?: Partial<PopupDependencies["tabs"]> };

function fixture(deps: PopupOverrides = {}) {
  document.body.replaceChildren();
  document.body.append(document.createElement("main"));
  const main = document.querySelector("main")!;
  const permissions = {
    contains: vi.fn(async () => false),
    request: vi.fn(async () => true),
    remove: vi.fn(async () => true),
  };
  let activated: ((info: { tabId: number }) => void) | undefined;
  let updated: ((tabId: number, change: { url?: string }) => void) | undefined;
  const defaultTabs = {
    query: vi.fn(async () => [{ id: 11, url: "https://shop.example/order" }]),
    sendMessage: vi.fn(async () => ({ source: "escpost-popup", kind: "relay-probe-result", protocol: 1, relay: true, daemon: true })),
  };
  const input = {
    document,
    permissions,
    syncRegistrations: vi.fn(async () => undefined),
    ...deps,
    tabs: {
      ...defaultTabs,
      ...deps.tabs,
      onActivated: { addListener: vi.fn((listener) => { activated = listener; }) },
      onUpdated: { addListener: vi.fn((listener) => { updated = listener; }) },
    },
  } as PopupDependencies & { tabs: PopupDependencies["tabs"] & { onActivated?: { addListener(listener: (info: { tabId: number }) => void): void }; onUpdated?: { addListener(listener: (tabId: number, change: { url?: string }) => void): void } } };
  const popup = installPopup(input);
  return {
    input,
    permissions,
    popup,
    activate: (tabId: number) => activated?.({ tabId }),
    update: (tabId: number, url: string) => updated?.(tabId, { url }),
    button: () => document.querySelector<HTMLButtonElement>("#permission-action")!,
  };
}

async function settle(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
}

test("requests the exact origin synchronously in the grant click call stack", async () => {
  // Break caught: awaiting, queuing, or refreshing before permissions.request
  // loses Chrome's user gesture and turns the explicit consent prompt into a denial.
  const events: string[] = [];
  let resolveRequest: ((value: boolean) => void) | undefined;
  const request = vi.fn(() => {
    events.push("request");
    return new Promise<boolean>((resolve) => { resolveRequest = resolve; });
  });
  const { input, button } = fixture({
    permissions: { contains: vi.fn(async () => false), request, remove: vi.fn(async () => true) },
    syncRegistrations: vi.fn(async () => { events.push("sync"); }),
  });
  await settle();

  button().click();
  expect(request).toHaveBeenCalledWith({ origins: ["https://shop.example/*"] });
  expect(events).toEqual(["request"]);
  expect(input.syncRegistrations).not.toHaveBeenCalled();

  resolveRequest?.(true);
  await settle();
  expect(events).toEqual(["request", "sync"]);
});

test("synchronizes registrations after a completed revoke and refreshes the view", async () => {
  // Break caught: removing a host permission without a registration sync leaves
  // an old document-start relay registered for a now-revoked site.
  const remove = vi.fn(async () => true);
  const syncRegistrations = vi.fn(async () => undefined);
  const { button } = fixture({
    permissions: { contains: vi.fn(async () => true), request: vi.fn(async () => true), remove },
    syncRegistrations,
  });
  await settle();

  expect(button().textContent).toBe("Remove access");
  button().click();
  await settle();

  expect(remove).toHaveBeenCalledWith({ origins: ["https://shop.example/*"] });
  expect(syncRegistrations).toHaveBeenCalledOnce();
});

test("reports tab, permission, and relay failures as view state without rejections", async () => {
  // Break caught: a rejected browser API promise escapes the popup and leaves
  // users with neither a status nor a recovery instruction.
  const { input, popup } = fixture({
    tabs: { query: vi.fn(async () => { throw new Error("tab failed"); }) },
  });
  await settle();
  expect(document.querySelector("#popup-error")?.textContent).toBe("Could not read the active tab.");

  input.tabs.query = vi.fn(async () => [{ id: 11, url: "https://shop.example/order" }]);
  input.permissions.contains = vi.fn(async () => { throw new Error("permission failed"); });
  await popup.refresh();
  expect(document.querySelector("#popup-error")?.textContent).toBe("Could not verify site access.");

  input.permissions.contains = vi.fn(async () => true);
  input.tabs.sendMessage = vi.fn(async () => { throw new Error("relay failed"); });
  await popup.refresh();
  expect(document.querySelector("#popup-error")?.textContent).toBe("Could not contact the page relay.");
});

test("reconciles denied permission mutations without synchronizing registrations", async () => {
  // Break caught: syncing after a declined permission dialog or claiming a
  // changed grant makes the visible action disagree with Chrome's real state.
  const absent = fixture({
    permissions: { contains: vi.fn(async () => false), request: vi.fn(async () => false), remove: vi.fn(async () => true) },
  });
  await settle();
  absent.button().click();
  await settle();
  expect(absent.input.syncRegistrations).not.toHaveBeenCalled();
  expect(absent.button().textContent).toBe("Allow this site");
  expect(document.querySelector("#popup-error")?.textContent).toBe("Site access was not changed.");

  const present = fixture({
    permissions: { contains: vi.fn(async () => true), request: vi.fn(async () => true), remove: vi.fn(async () => false) },
  });
  await settle();
  present.button().click();
  await settle();
  expect(present.input.syncRegistrations).not.toHaveBeenCalled();
  expect(present.button().textContent).toBe("Remove access");
  expect(document.querySelector("#popup-error")?.textContent).toBe("Site access was not changed.");
});

test("reconciles rejected permission calls and failed registration sync to Chrome state", async () => {
  // Break caught: a rejected mutation or failed sync using optimistic state can
  // offer the wrong action for the actual origin permission.
  let granted = false;
  const grant = fixture({
    permissions: {
      contains: vi.fn(async () => granted),
      request: vi.fn(async () => { granted = true; return true; }),
      remove: vi.fn(async () => true),
    },
    syncRegistrations: vi.fn(async () => { throw new Error("sync failed"); }),
  });
  await settle();
  grant.button().click();
  await settle();
  expect(grant.button().textContent).toBe("Remove access");
  expect(document.querySelector("#popup-error")?.textContent).toBe("Could not update site access.");

  const rejected = fixture({
    permissions: { contains: vi.fn(async () => false), request: vi.fn(async () => { throw new Error("denied"); }), remove: vi.fn(async () => true) },
  });
  await settle();
  rejected.button().click();
  await settle();
  expect(rejected.input.syncRegistrations).not.toHaveBeenCalled();
  expect(rejected.button().textContent).toBe("Allow this site");
  expect(document.querySelector("#popup-error")?.textContent).toBe("Could not change site access.");

  let stillGranted = true;
  const revoke = fixture({
    permissions: {
      contains: vi.fn(async () => stillGranted),
      request: vi.fn(async () => true),
      remove: vi.fn(async () => { stillGranted = false; return true; }),
    },
    syncRegistrations: vi.fn(async () => { throw new Error("sync failed"); }),
  });
  await settle();
  revoke.button().click();
  await settle();
  expect(revoke.button().textContent).toBe("Allow this site");
  expect(document.querySelector("#popup-error")?.textContent).toBe("Could not update site access.");

  const removeRejected = fixture({
    permissions: { contains: vi.fn(async () => true), request: vi.fn(async () => true), remove: vi.fn(async () => { throw new Error("denied"); }) },
  });
  await settle();
  removeRejected.button().click();
  await settle();
  expect(removeRejected.input.syncRegistrations).not.toHaveBeenCalled();
  expect(removeRejected.button().textContent).toBe("Remove access");
  expect(document.querySelector("#popup-error")?.textContent).toBe("Could not change site access.");
});

test("discards a navigation update that arrives while the active-tab query is pending", async () => {
  // Break caught: accepting a tab URL captured before navigation can render an
  // action for the prior document after the current tab has changed origin.
  let resolveOld: ((tabs: Array<{ id: number; url: string }>) => void) | undefined;
  const tabs = {
    query: vi.fn()
      .mockImplementationOnce(() => new Promise<Array<{ id: number; url: string }>>((resolve) => { resolveOld = resolve; }))
      .mockResolvedValue([{ id: 11, url: "https://after-query.example/order" }]),
    sendMessage: vi.fn(async () => ({ source: "escpost-popup", kind: "relay-probe-result", protocol: 1, relay: true, daemon: true })),
  };
  const popup = fixture({ tabs, permissions: { contains: vi.fn(async () => false), request: vi.fn(async () => true), remove: vi.fn(async () => true) } });
  await Promise.resolve();
  popup.update(11, "https://after-query.example/order");
  resolveOld?.([{ id: 11, url: "https://before-query.example/order" }]);
  await settle();

  expect(document.querySelector("main")?.textContent).not.toContain("https://before-query.example");
  expect(document.querySelector("#current-origin")?.textContent).toBe("https://after-query.example");
});

test("invalidates old actions and discards tab, grant, and probe results after navigation", async () => {
  // Break caught: combining an old rendered action with a newer active tab can
  // grant or revoke a different origin than the user saw in the popup.
  let resolveTab: ((tabs: Array<{ id: number; url: string }>) => void) | undefined;
  let resolveGrant: ((granted: boolean) => void) | undefined;
  let resolveProbe: ((reply: unknown) => void) | undefined;
  const tabs = {
    query: vi.fn()
      .mockImplementationOnce(() => new Promise<Array<{ id: number; url: string }>>((resolve) => { resolveTab = resolve; }))
      .mockResolvedValue([{ id: 22, url: "https://next.example/order" }]),
    sendMessage: vi.fn()
      .mockImplementationOnce(() => new Promise<unknown>((resolve) => { resolveProbe = resolve; }))
      .mockResolvedValue({ source: "escpost-popup", kind: "relay-probe-result", protocol: 1, relay: true, daemon: true }),
  };
  const permissions = {
    contains: vi.fn()
      .mockImplementationOnce(() => new Promise<boolean>((resolve) => { resolveGrant = resolve; }))
      .mockResolvedValue(true),
    request: vi.fn(async () => true),
    remove: vi.fn(async () => true),
  };
  const popup = fixture({ tabs, permissions });
  popup.activate(22);
  await Promise.resolve();
  resolveTab?.([{ id: 11, url: "https://old.example/order" }]);
  await Promise.resolve();
  resolveGrant?.(true);
  await Promise.resolve();
  resolveProbe?.({ source: "escpost-popup", kind: "relay-probe-result", protocol: 1, relay: true, daemon: true });
  await settle();

  expect(document.querySelector("#current-origin")?.textContent).toBe("https://next.example");
  const stale = popup.button();
  popup.update(22, "https://after.example/order");
  stale.click();
  expect(permissions.remove).not.toHaveBeenCalled();
  expect(permissions.request).not.toHaveBeenCalled();
});
