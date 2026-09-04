import { expect, test, vi } from "vitest";
import { installChromeBridge } from "../src/chrome/bridge";

test("the framed Chrome bridge authorizes the browser-reported parent origin", async () => {
  let receive: ((event: MessageEvent) => void) | undefined;
  const parent = { postMessage: vi.fn() };
  const page = {
    parent,
    addEventListener: vi.fn((_type: string, listener: (event: MessageEvent) => void) => { receive = listener; }),
  };
  const grants = { contains: vi.fn(async () => true), onRemoved: vi.fn() };
  const daemon = { health: vi.fn(async () => true), list: vi.fn(), print: vi.fn(), openInventoryStream: vi.fn() };
  installChromeBridge(page, { grants, daemon });

  receive?.({
    source: parent,
    origin: "https://shop.example",
    data: { source: "escpost-page", protocol: 1, id: 7, op: "daemon.health", payload: null },
  } as unknown as MessageEvent);
  expect(grants.contains).toHaveBeenCalledWith("https://shop.example/*");
  await vi.waitFor(() => {
    expect(parent.postMessage).toHaveBeenCalledWith(
      { source: "escpost-extension", id: 7, ok: true, data: true },
      "https://shop.example",
    );
  });
});

test("the framed Chrome bridge ignores non-parent and non-web origins", async () => {
  let receive: ((event: MessageEvent) => void) | undefined;
  const parent = { postMessage: vi.fn() };
  const page = { parent, addEventListener: vi.fn((_type: string, listener: (event: MessageEvent) => void) => { receive = listener; }) };
  const deps = {
    grants: { contains: vi.fn(async () => true), onRemoved: vi.fn() },
    daemon: { health: vi.fn(async () => true), list: vi.fn(), print: vi.fn(), openInventoryStream: vi.fn() },
  };
  installChromeBridge(page, deps);
  const data = { source: "escpost-page", protocol: 1, id: 7, op: "daemon.health", payload: null };

  receive?.({ source: {}, origin: "https://shop.example", data } as unknown as MessageEvent);
  receive?.({ source: parent, origin: "null", data } as unknown as MessageEvent);
  await Promise.resolve();

  expect(deps.grants.contains).not.toHaveBeenCalled();
  expect(parent.postMessage).not.toHaveBeenCalled();
});

test("the framed Chrome bridge carries live printer snapshots without a content script", async () => {
  let receive: ((event: MessageEvent) => void) | undefined;
  let callbacks: { onSnapshot(snapshot: unknown): void } | undefined;
  const parent = { postMessage: vi.fn() };
  const page = { parent, addEventListener: vi.fn((_type: string, listener: (event: MessageEvent) => void) => { receive = listener; }) };
  const deps = {
    grants: { contains: vi.fn(async () => true), onRemoved: vi.fn() },
    daemon: {
      health: vi.fn(), list: vi.fn(), print: vi.fn(),
      openInventoryStream: vi.fn(async (next: { onSnapshot(snapshot: unknown): void }, signal: AbortSignal) => {
        callbacks = next;
        await new Promise<void>((resolve) => signal.addEventListener("abort", () => resolve(), { once: true }));
      }),
    },
  };
  installChromeBridge(page, deps);

  receive?.({
    source: parent,
    origin: "https://shop.example",
    data: { source: "escpost-page", kind: "subscribe", subscriptionId: 9, op: "printers.events", protocol: 1 },
  } as unknown as MessageEvent);
  await vi.waitFor(() => expect(callbacks).toBeDefined());
  callbacks?.onSnapshot({ updated_at: "2026-09-04T12:00:00Z", warning: null, printers: [] });

  expect(parent.postMessage).toHaveBeenCalledWith({
    source: "escpost-extension",
    kind: "snapshot",
    subscriptionId: 9,
    data: { updated_at: "2026-09-04T12:00:00Z", warning: null, printers: [] },
  }, "https://shop.example");
});
