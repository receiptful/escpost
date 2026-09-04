import { afterEach, expect, test, vi } from "vitest";
import { IframePage } from "../src/iframe-page";

afterEach(() => {
  vi.unstubAllGlobals();
  vi.resetModules();
});

test("posts only to the stable Chrome extension frame and accepts only its replies", () => {
  let loaded: (() => void) | undefined;
  let receive: ((event: MessageEvent) => void) | undefined;
  const frameWindow = { postMessage: vi.fn() };
  const frame = {
    contentWindow: frameWindow,
    src: "",
    hidden: false,
    tabIndex: 0,
    addEventListener: vi.fn((_type: string, listener: () => void) => { loaded = listener; }),
  };
  const document = {
    createElement: vi.fn(() => frame),
    documentElement: { append: vi.fn() },
  };
  const host = { addEventListener: vi.fn((_type: string, listener: (event: MessageEvent) => void) => { receive = listener; }) };
  const page = new IframePage("abcdefghijklmnopabcdefghijklmnop", host, document);
  const listener = vi.fn();
  const request = { source: "escpost-page" as const, protocol: 1 as const, id: 7, op: "daemon.health" as const, payload: null };

  page.addEventListener("message", listener);
  page.postMessage(request);
  expect(frame.src).toBe("chrome-extension://abcdefghijklmnopabcdefghijklmnop/bridge.html");
  expect(frame.hidden).toBe(true);
  expect(frameWindow.postMessage).not.toHaveBeenCalled();

  loaded?.();
  expect(frameWindow.postMessage).toHaveBeenCalledWith(request, "chrome-extension://abcdefghijklmnopabcdefghijklmnop");
  receive?.({ source: host, origin: "https://shop.example", data: "forged" } as unknown as MessageEvent);
  expect(listener).not.toHaveBeenCalled();
  receive?.({ source: frameWindow, origin: "chrome-extension://abcdefghijklmnopabcdefghijklmnop", data: "reply" } as unknown as MessageEvent);
  expect(listener).toHaveBeenCalledWith(expect.objectContaining({ source: page, data: "reply" }));
});

test("the default SDK client uses the Chrome iframe bridge", async () => {
  let loaded: (() => void) | undefined;
  let receive: ((event: MessageEvent) => void) | undefined;
  const frameWindow = { postMessage: vi.fn() };
  const frame = {
    contentWindow: frameWindow,
    src: "",
    hidden: false,
    tabIndex: 0,
    addEventListener: vi.fn((_type: string, listener: () => void) => { loaded = listener; }),
  };
  const document = { createElement: vi.fn(() => frame), documentElement: { append: vi.fn() } };
  const host = { addEventListener: vi.fn((_type: string, listener: (event: MessageEvent) => void) => { receive = listener; }) };
  vi.stubGlobal("window", host);
  vi.stubGlobal("document", document);
  vi.stubGlobal("navigator", { userAgent: "Mozilla/5.0 Chrome/149.0" });
  const { escpost } = await import("../src/index");

  const available = escpost.isAvailable();
  loaded?.();
  const request = frameWindow.postMessage.mock.calls[0]?.[0] as { id: number };
  receive?.({
    source: frameWindow,
    origin: "chrome-extension://gdflkakcdpkllfhndncimkpfeomfccia",
    data: { source: "escpost-extension", id: request.id, ok: true, data: true },
  } as unknown as MessageEvent);

  await expect(available).resolves.toBe(true);
});

test("the default SDK client leaves Firefox on the page relay", async () => {
  let receive: ((event: MessageEvent) => void) | undefined;
  const host = {
    addEventListener: vi.fn((_type: string, listener: (event: MessageEvent) => void) => { receive = listener; }),
    postMessage: vi.fn(),
  };
  const document = { createElement: vi.fn() };
  vi.stubGlobal("window", host);
  vi.stubGlobal("document", document);
  vi.stubGlobal("navigator", { userAgent: "Mozilla/5.0 Firefox/142.0" });
  const { escpost } = await import("../src/index");

  const available = escpost.isAvailable();
  expect(document.createElement).not.toHaveBeenCalled();
  const request = host.postMessage.mock.calls[0]?.[0] as { id: number };
  receive?.({
    source: host,
    data: { source: "escpost-extension", id: request.id, ok: true, data: true },
  } as unknown as MessageEvent);

  await expect(available).resolves.toBe(true);
});
