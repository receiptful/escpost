import { expect, test, vi } from "vitest";
import { installRelay } from "../src/relay";

type Listener = (event: MessageEvent) => void;

function page(origin = "https://shop.example") {
  let listener: Listener | undefined;
  const postMessage = vi.fn();
  const window = {
    location: { origin },
    addEventListener: vi.fn((_type: "message", next: Listener) => { listener = next; }),
    postMessage,
  };
  return {
    window,
    postMessage,
    emit: (data: unknown, source: unknown = window) => listener?.({ data, source } as MessageEvent),
  };
}

test("forwards a page-owned one-shot request and replies only to its page origin", async () => {
  // Break caught: forwarding a request without its correlation id, or replying
  // with a wildcard target, lets an unrelated origin observe the daemon result.
  const fixture = page();
  const sendMessage = vi.fn(async () => ({ ok: true, data: { job_id: "job-17" } }));
  installRelay(fixture.window, { sendMessage });

  fixture.emit({ source: "escpost-page", protocol: 1, id: 17, op: "print.raw", payload: { printer: "counter", dataBase64: "G0D/" } });
  await Promise.resolve();
  await Promise.resolve();

  expect(sendMessage).toHaveBeenCalledWith({
    source: "escpost-relay",
    request: { source: "escpost-page", protocol: 1, id: 17, op: "print.raw", payload: { printer: "counter", dataBase64: "G0D/" } },
  });
  expect(fixture.postMessage).toHaveBeenCalledWith(
    { source: "escpost-extension", id: 17, ok: true, data: { job_id: "job-17" } },
    "https://shop.example",
  );
});

test("ignores a spoofed window event and malformed page request", async () => {
  // Break caught: accepting a foreign window or an uncorrelatable message lets
  // a sibling frame invoke the privileged extension worker.
  const fixture = page();
  const sendMessage = vi.fn();
  installRelay(fixture.window, { sendMessage });

  fixture.emit({ source: "escpost-page", protocol: 1, id: 1, op: "daemon.health", payload: null }, {});
  fixture.emit({ source: "escpost-page", protocol: 1, op: "daemon.health", payload: null });
  fixture.emit({ source: "another-library", protocol: 1, id: 2, op: "daemon.health", payload: null });
  await Promise.resolve();

  expect(sendMessage).not.toHaveBeenCalled();
});

test("returns a typed failure for a same-window request missing its payload", async () => {
  // Break caught: dropping a correlatable malformed request leaves the SDK
  // waiting for its timeout even though the relay can reject it safely.
  const fixture = page();
  const sendMessage = vi.fn();
  installRelay(fixture.window, { sendMessage });

  fixture.emit({ source: "escpost-page", protocol: 1, id: 6, op: "print.raw" });

  expect(sendMessage).not.toHaveBeenCalled();
  expect(fixture.postMessage).toHaveBeenCalledWith(
    { source: "escpost-extension", id: 6, ok: false, error: expect.objectContaining({ code: "PROTOCOL_MISMATCH" }) },
    "https://shop.example",
  );
});
