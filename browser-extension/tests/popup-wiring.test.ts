// @vitest-environment happy-dom

import { expect, test, vi } from "vitest";
import { Window } from "happy-dom";
import { installPopup, type PopupDependencies } from "../src/popup/popup";

const testWindow = new Window();
Object.assign(globalThis, { window: testWindow, document: testWindow.document });

function fixture(deps: Partial<PopupDependencies> = {}) {
  document.body.replaceChildren();
  document.body.append(document.createElement("main"));
  const main = document.querySelector("main")!;
  const permissions = {
    contains: vi.fn(async () => false),
    request: vi.fn(async () => true),
    remove: vi.fn(async () => true),
  };
  const input: PopupDependencies = {
    document,
    tabs: { query: vi.fn(async () => [{ id: 11, url: "https://shop.example/order" }]) },
    permissions,
    scripting: { executeScript: vi.fn(async () => [{ result: { relay: true, daemon: true } }]) },
    syncRegistrations: vi.fn(async () => undefined),
    ...deps,
  };
  const popup = installPopup(input);
  return { input, permissions, popup, button: () => document.querySelector<HTMLButtonElement>("#permission-action")! };
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
  input.scripting.executeScript = vi.fn(async () => { throw new Error("relay failed"); });
  await popup.refresh();
  expect(document.querySelector("#popup-error")?.textContent).toBe("Could not contact the page relay.");
});
