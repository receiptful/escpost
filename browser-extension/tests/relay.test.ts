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
    emit: (data: unknown, eventOrigin = origin, source: unknown = window) => {
      const event = new MessageEvent("message", { data, origin: eventOrigin });
      Object.defineProperty(event, "source", { value: source });
      listener?.(event);
    },
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

  fixture.emit({ source: "escpost-page", protocol: 1, id: 1, op: "daemon.health", payload: null }, "https://shop.example", {});
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

test("rejects a foreign MessageEvent origin and a mismatched protocol before worker forwarding", async () => {
  // Break caught: checking only event.source lets a same-window script relay a
  // message whose browser origin differs from the document that received it.
  const fixture = page();
  const sendMessage = vi.fn();
  installRelay(fixture.window, { sendMessage });

  fixture.emit({ source: "escpost-page", protocol: 1, id: 7, op: "daemon.health", payload: null }, "https://evil.example");
  fixture.emit({ source: "escpost-page", protocol: 2, id: 8, op: "daemon.health", payload: null });

  expect(sendMessage).not.toHaveBeenCalled();
  expect(fixture.postMessage).toHaveBeenCalledWith(
    { source: "escpost-extension", id: 8, ok: false, error: expect.objectContaining({ code: "PROTOCOL_MISMATCH" }) },
    "https://shop.example",
  );
});

test("suppresses a worker reply after the page navigates to another origin", async () => {
  // Break caught: replying to the current origin after navigation hands an old
  // request result to a different document at the same window reference.
  const fixture = page();
  let resolve: ((reply: unknown) => void) | undefined;
  const sendMessage = vi.fn(() => new Promise<unknown>((next) => { resolve = next; }));
  installRelay(fixture.window, { sendMessage });

  fixture.emit({ source: "escpost-page", protocol: 1, id: 9, op: "daemon.health", payload: null });
  fixture.window.location.origin = "https://after-navigation.example";
  resolve?.({ ok: true, data: true });
  await Promise.resolve();
  await Promise.resolve();

  expect(fixture.postMessage).not.toHaveBeenCalled();
});

test("settles rejected runtime delivery and malformed worker replies exactly once", async () => {
  // Break caught: swallowing a runtime rejection or malformed worker result
  // makes the SDK wait for a timeout instead of receiving a typed failure.
  const rejected = page();
  installRelay(rejected.window, { sendMessage: vi.fn(async () => { throw new Error("worker asleep"); }) });
  rejected.emit({ source: "escpost-page", protocol: 1, id: 10, op: "daemon.health", payload: null });
  await Promise.resolve();
  await Promise.resolve();

  expect(rejected.postMessage).toHaveBeenCalledTimes(1);
  expect(rejected.postMessage).toHaveBeenCalledWith(
    { source: "escpost-extension", id: 10, ok: false, error: expect.objectContaining({ code: "EXTENSION_UNAVAILABLE" }) },
    "https://shop.example",
  );

  const malformed = page();
  installRelay(malformed.window, { sendMessage: vi.fn(async () => ({ ok: false, error: null })) });
  malformed.emit({ source: "escpost-page", protocol: 1, id: 11, op: "daemon.health", payload: null });
  await Promise.resolve();
  await Promise.resolve();

  expect(malformed.postMessage).toHaveBeenCalledTimes(1);
  expect(malformed.postMessage).toHaveBeenCalledWith(
    { source: "escpost-extension", id: 11, ok: false, error: expect.objectContaining({ code: "PROTOCOL_MISMATCH" }) },
    "https://shop.example",
  );
});

test("accepts only exact known worker reply shapes", async () => {
  // Break caught: partial, ambiguous, inherited, or unknown-code worker replies
  // can cross the isolated-world boundary as if they came from the extension.
  const inheritedSuccess = Object.create({ ok: true, data: true });
  const cases: Array<{ name: string; reply: unknown; expected: "PROTOCOL_MISMATCH" | "ORIGIN_NOT_GRANTED" }> = [
    { name: "missing success data", reply: { ok: true }, expected: "PROTOCOL_MISMATCH" },
    { name: "ambiguous success error", reply: { ok: true, data: true, error: { code: "ORIGIN_NOT_GRANTED", message: "denied" } }, expected: "PROTOCOL_MISMATCH" },
    { name: "unknown failure code", reply: { ok: false, error: { code: "NOT_A_CODE", message: "nope" } }, expected: "PROTOCOL_MISMATCH" },
    { name: "wrong failure message", reply: { ok: false, error: { code: "ORIGIN_NOT_GRANTED", message: 9 } }, expected: "PROTOCOL_MISMATCH" },
    { name: "inherited fields", reply: inheritedSuccess, expected: "PROTOCOL_MISMATCH" },
    { name: "valid success", reply: { ok: true, data: { job_id: "job-22" } }, expected: "PROTOCOL_MISMATCH" },
    { name: "valid failure", reply: { ok: false, error: { code: "ORIGIN_NOT_GRANTED", message: "denied" } }, expected: "ORIGIN_NOT_GRANTED" },
  ];

  for (const [index, entry] of cases.entries()) {
    const fixture = page();
    installRelay(fixture.window, { sendMessage: vi.fn(async () => entry.reply) });
    fixture.emit({ source: "escpost-page", protocol: 1, id: 20 + index, op: "daemon.health", payload: null });
    await Promise.resolve();
    await Promise.resolve();

    expect(fixture.postMessage).toHaveBeenCalledTimes(1);
    if (entry.name === "valid success") {
      expect(fixture.postMessage).toHaveBeenCalledWith(
        { source: "escpost-extension", id: 20 + index, ok: true, data: { job_id: "job-22" } },
        "https://shop.example",
      );
    } else {
      expect(fixture.postMessage).toHaveBeenCalledWith(
        { source: "escpost-extension", id: 20 + index, ok: false, error: expect.objectContaining({ code: entry.expected }) },
        "https://shop.example",
      );
    }
  }
});
