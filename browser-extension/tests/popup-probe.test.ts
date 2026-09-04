import { expect, test, vi } from "vitest";
import { installBackground } from "../src/background";
import { installRelay } from "../src/relay";
import { probeRelayStatus } from "../src/ui/status";

type MessageListener = (message: unknown, sender: { id?: string }, respond: (reply: unknown) => void) => boolean | void;

function stack(health: boolean, granted = true) {
  let workerListener: ((message: unknown, sender: { origin?: string }, respond: (reply: unknown) => void) => boolean | void) | undefined;
  const worker = {
    onMessage: { addListener: vi.fn((listener) => { workerListener = listener; }) },
  };
  const grants = { contains: vi.fn(async () => granted), onRemoved: vi.fn() };
  installBackground(worker, { grants, daemon: { health: vi.fn(async () => health), list: vi.fn(), print: vi.fn() } });

  let relayListener: MessageListener | undefined;
  const runtime = {
    id: "escpost-extension-id",
    onMessage: { addListener: vi.fn((listener: MessageListener) => { relayListener = listener; }) },
    sendMessage: vi.fn(async (message) => new Promise<unknown>((resolve) => {
      workerListener?.(message, { origin: "https://shop.example" }, resolve);
    })),
  };
  let pageMessage: ((event: MessageEvent) => void) | undefined;
  installRelay({
    location: { origin: "https://shop.example" },
    addEventListener: vi.fn((_type, listener) => { pageMessage = listener; }),
    postMessage: vi.fn(),
  }, runtime);

  const tabs = {
    sendMessage: vi.fn(async (_tabId: number, message: unknown) => new Promise<unknown>((resolve, reject) => {
      if (relayListener === undefined) {
        reject(new Error("Could not establish connection. Receiving end does not exist."));
        return;
      }
      if (relayListener(message, { id: "escpost-extension-id" }, resolve) !== true) {
        reject(new Error("Could not establish connection. Receiving end does not exist."));
      }
    })),
  };
  return { tabs, runtime, pageMessage, grants };
}

const probe = probeRelayStatus as unknown as (tabId: number, tabs: { sendMessage(tabId: number, message: unknown): Promise<unknown> }) => Promise<{
  relay: "loaded" | "missing" | "unknown";
  daemon: "running" | "unavailable" | "unknown";
  error: string | null;
}>;

test("uses the installed relay's private runtime channel for loaded daemon health", async () => {
  // Break caught: a popup health result sourced from a public window event can
  // be forged by page JavaScript instead of proving the isolated relay exists.
  const running = stack(true);
  const unavailable = stack(false);

  await expect(probe(11, running.tabs)).resolves.toEqual({ relay: "loaded", daemon: "running", error: null });
  await expect(probe(11, unavailable.tabs)).resolves.toEqual({ relay: "loaded", daemon: "unavailable", error: null });
  expect(running.tabs.sendMessage).toHaveBeenCalledWith(11, expect.objectContaining({ source: "escpost-popup", kind: "relay-probe" }));
});

test("fails closed for missing, malformed, rejected, and untrusted private probe replies", async () => {
  // Break caught: treating a missing listener, malformed reply, runtime error,
  // or foreign extension sender as a loaded relay lies about authorization.
  const missing = { sendMessage: vi.fn(async () => { throw new Error("Could not establish connection. Receiving end does not exist."); }) };
  const malformed = { sendMessage: vi.fn(async () => ({ source: "escpost-popup", kind: "relay-probe-result", relay: true })) };
  const rejected = { sendMessage: vi.fn(async () => { throw new Error("worker unavailable"); }) };
  const trusted = stack(true);

  await expect(probe(11, missing)).resolves.toEqual({ relay: "missing", daemon: "unknown", error: null });
  await expect(probe(11, malformed)).resolves.toEqual({ relay: "unknown", daemon: "unknown", error: "Could not contact the page relay." });
  await expect(probe(11, rejected)).resolves.toEqual({ relay: "unknown", daemon: "unknown", error: "Could not contact the page relay." });

  const listener = trusted.runtime.onMessage.addListener.mock.calls[0]?.[0] as MessageListener;
  const respond = vi.fn();
  expect(listener({ source: "escpost-popup", kind: "relay-probe", protocol: 1 }, { id: "other-extension" }, respond)).toBeUndefined();
  expect(trusted.runtime.sendMessage).not.toHaveBeenCalled();
});

test("keeps page messages outside the private probe channel and retains worker sender-origin authorization", async () => {
  // Break caught: exposing the popup probe to window messages lets page code
  // manufacture readiness or bypass the worker's stored-origin grant check.
  const denied = stack(true, false);
  denied.pageMessage?.(new MessageEvent("message", {
    data: { source: "escpost-popup", kind: "relay-probe", protocol: 1 },
    origin: "https://shop.example",
  }));
  expect(denied.runtime.sendMessage).not.toHaveBeenCalled();

  await expect(probe(11, denied.tabs)).resolves.toEqual({ relay: "loaded", daemon: "unknown", error: null });
  expect(denied.grants.contains).toHaveBeenCalledWith("https://shop.example/*");
});
