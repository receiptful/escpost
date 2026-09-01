import { expect, test, vi } from "vitest";
import { registerGrantedRelay } from "../src/registration";

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
