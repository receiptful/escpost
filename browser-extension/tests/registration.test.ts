import { expect, test, vi } from "vitest";
import { installGrantRegistration, registerGrantedRelay } from "../src/registration";

test("registers only the isolated document-start relay for explicit web grants", async () => {
  // Break caught: registering an all-site/static content script or including
  // the daemon host exposes the privileged relay beyond user-granted pages.
  const scripting = {
    getRegisteredContentScripts: vi.fn(async () => []),
    registerContentScripts: vi.fn(async () => undefined),
    updateContentScripts: vi.fn(async () => undefined),
    unregisterContentScripts: vi.fn(async () => undefined),
  };
  const permissions = { getAll: vi.fn(async () => ({ origins: ["https://shop.example/*", "http://127.0.0.1:9000/*"] })) };

  await registerGrantedRelay({ permissions, scripting });

  expect(scripting.registerContentScripts).toHaveBeenCalledWith([{
    id: "escpost-relay",
    js: ["relay.js"],
    matches: ["https://shop.example/*"],
    runAt: "document_start",
    world: "ISOLATED",
  }]);
  expect(scripting.updateContentScripts).not.toHaveBeenCalled();
});

test("removes the relay registration when no explicit web grant remains", async () => {
  // Break caught: leaving a prior relay registered after permission removal
  // lets a previously allowed page keep accessing the daemon.
  const scripting = {
    getRegisteredContentScripts: vi.fn(async () => [{ id: "escpost-relay" }]),
    registerContentScripts: vi.fn(async () => undefined),
    updateContentScripts: vi.fn(async () => undefined),
    unregisterContentScripts: vi.fn(async () => undefined),
  };

  await registerGrantedRelay({ permissions: { getAll: vi.fn(async () => ({ origins: [] })) }, scripting });

  expect(scripting.unregisterContentScripts).toHaveBeenCalledWith({ ids: ["escpost-relay"] });
  expect(scripting.registerContentScripts).not.toHaveBeenCalled();
});

test("updates an existing relay registration rather than adding another script", async () => {
  // Break caught: registering an already registered script id fails instead of
  // narrowing its injection scope to the latest explicit grant.
  const scripting = {
    getRegisteredContentScripts: vi.fn(async () => [{ id: "escpost-relay" }]),
    registerContentScripts: vi.fn(async () => undefined),
    updateContentScripts: vi.fn(async () => undefined),
    unregisterContentScripts: vi.fn(async () => undefined),
  };

  await registerGrantedRelay({ permissions: { getAll: vi.fn(async () => ({ origins: ["https://shop.example/*"] })) }, scripting });

  expect(scripting.updateContentScripts).toHaveBeenCalledWith([expect.objectContaining({
    id: "escpost-relay", matches: ["https://shop.example/*"],
  })]);
  expect(scripting.registerContentScripts).not.toHaveBeenCalled();
});

test("does not register wildcard host patterns as page grants", async () => {
  // Break caught: injecting the relay into wildcard hosts turns one optional
  // permission selection into page access that was never concretely granted.
  const scripting = {
    getRegisteredContentScripts: vi.fn(async () => []),
    registerContentScripts: vi.fn(async () => undefined),
    updateContentScripts: vi.fn(async () => undefined),
    unregisterContentScripts: vi.fn(async () => undefined),
  };

  await registerGrantedRelay({ permissions: { getAll: vi.fn(async () => ({ origins: ["https://*.example.com/*", "https://*/*"] })) }, scripting });

  expect(scripting.registerContentScripts).not.toHaveBeenCalled();
});

test("refreshes registration on permission add and removal", async () => {
  // Break caught: listening only at worker startup leaves a new explicit grant
  // inert or keeps a revoked origin's relay registered.
  let origins: string[] = [];
  let registered = false;
  let added: (() => void) | undefined;
  let removed: (() => void) | undefined;
  const scripting = {
    getRegisteredContentScripts: vi.fn(async () => registered ? [{ id: "escpost-relay" }] : []),
    registerContentScripts: vi.fn(async () => { registered = true; }),
    updateContentScripts: vi.fn(async () => undefined),
    unregisterContentScripts: vi.fn(async () => { registered = false; }),
  };
  const permissions = {
    getAll: vi.fn(async () => ({ origins })),
    onAdded: { addListener: vi.fn((listener) => { added = listener; }) },
    onRemoved: { addListener: vi.fn((listener) => { removed = listener; }) },
  };
  installGrantRegistration({ permissions, scripting });
  await settle();

  origins = ["https://shop.example/*"];
  added?.();
  await settle();
  origins = [];
  removed?.();
  await settle();

  expect(scripting.registerContentScripts).toHaveBeenCalledWith([expect.objectContaining({ matches: ["https://shop.example/*"] })]);
  expect(scripting.unregisterContentScripts).toHaveBeenCalledWith({ ids: ["escpost-relay"] });
});

test("keeps permission-event registration sync alive without popup ownership", async () => {
  // Break caught: relying on a popup continuation means an allowed/revoked
  // origin is never registered/removed when the popup closes immediately.
  let added: (() => void) | undefined;
  let origins: string[] = [];
  const scripting = {
    getRegisteredContentScripts: vi.fn(async () => []),
    registerContentScripts: vi.fn(async () => undefined),
    updateContentScripts: vi.fn(async () => undefined),
    unregisterContentScripts: vi.fn(async () => undefined),
  };
  installGrantRegistration({
    permissions: {
      getAll: vi.fn(async () => ({ origins })),
      onAdded: { addListener: vi.fn((listener) => { added = listener; }) },
    },
    scripting,
  });
  await settle();

  origins = ["https://shop.example/*"];
  added?.();
  await settle();

  expect(scripting.registerContentScripts).toHaveBeenCalledWith([expect.objectContaining({ matches: ["https://shop.example/*"] })]);
});

test("coalesces a delayed add snapshot behind a later removal", async () => {
  // Break caught: an earlier async add pass can finish after a removal and
  // restore relay injection for an origin that is no longer granted.
  let resolveFirst: ((value: { origins: string[] }) => void) | undefined;
  let removed: (() => void) | undefined;
  const scripting = {
    getRegisteredContentScripts: vi.fn(async () => []),
    registerContentScripts: vi.fn(async () => undefined),
    updateContentScripts: vi.fn(async () => undefined),
    unregisterContentScripts: vi.fn(async () => undefined),
  };
  const permissions = {
    getAll: vi.fn()
      .mockImplementationOnce(() => new Promise<{ origins: string[] }>((resolve) => { resolveFirst = resolve; }))
      .mockResolvedValue({ origins: [] }),
    onRemoved: { addListener: vi.fn((listener) => { removed = listener; }) },
  };
  installGrantRegistration({ permissions, scripting });
  removed?.();
  resolveFirst?.({ origins: ["https://shop.example/*"] });
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();

  expect(scripting.registerContentScripts).not.toHaveBeenCalled();
});

test("recovers a later revocation refresh after an initial grant read failure", async () => {
  // Break caught: a rejected permissions read leaves refresh state permanently
  // busy, so a later revocation cannot remove an already registered relay.
  let removed: (() => void) | undefined;
  let registered = true;
  const scripting = {
    getRegisteredContentScripts: vi.fn(async () => registered ? [{ id: "escpost-relay" }] : []),
    registerContentScripts: vi.fn(async () => { registered = true; }),
    updateContentScripts: vi.fn(async () => undefined),
    unregisterContentScripts: vi.fn(async () => { registered = false; }),
  };
  const permissions = {
    getAll: vi.fn()
      .mockRejectedValueOnce(new Error("temporary permission read failure"))
      .mockResolvedValue({ origins: [] }),
    onRemoved: { addListener: vi.fn((listener) => { removed = listener; }) },
  };
  installGrantRegistration({ permissions, scripting });
  await settle();
  removed?.();
  await settle();

  expect(scripting.unregisterContentScripts).toHaveBeenCalledWith({ ids: ["escpost-relay"] });
  expect(registered).toBe(false);
});

test("recovers a later revocation refresh after an update mutation failure", async () => {
  // Break caught: a rejected scripting mutation wedges refresh state and leaves
  // an existing relay registered after its grant has been removed.
  let origins = ["https://shop.example/*"];
  let removed: (() => void) | undefined;
  let registered = true;
  const scripting = {
    getRegisteredContentScripts: vi.fn(async () => registered ? [{ id: "escpost-relay" }] : []),
    registerContentScripts: vi.fn(async () => { registered = true; }),
    updateContentScripts: vi.fn()
      .mockRejectedValueOnce(new Error("temporary scripting failure"))
      .mockResolvedValue(undefined),
    unregisterContentScripts: vi.fn(async () => { registered = false; }),
  };
  const permissions = {
    getAll: vi.fn(async () => ({ origins })),
    onRemoved: { addListener: vi.fn((listener) => { removed = listener; }) },
  };
  installGrantRegistration({ permissions, scripting });
  await settle();
  origins = [];
  removed?.();
  await settle();

  expect(scripting.unregisterContentScripts).toHaveBeenCalledWith({ ids: ["escpost-relay"] });
  expect(registered).toBe(false);
});

async function settle(): Promise<void> {
  await new Promise<void>((resolve) => setTimeout(resolve, 0));
}
