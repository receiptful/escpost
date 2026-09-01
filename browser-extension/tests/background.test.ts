import { expect, test, vi } from "vitest";
import { installBackground } from "../src/background";
import { handleRequest } from "../src/messages";

const request = { source: "escpost-page", protocol: 1, id: 4, op: "print.raw", payload: { printer: "counter", dataBase64: "G0D/" } };

function dependencies(granted = true) {
  return {
    permissions: { contains: vi.fn(async () => granted) },
    daemon: {
      health: vi.fn(async () => true),
      list: vi.fn(async () => ({ printers: [] as string[] })),
      print: vi.fn(async () => ({ job_id: "job-17" })),
    },
  };
}

test("denies an ungranted concrete sender origin before daemon print", async () => {
  // Break caught: dispatching before a stored-origin permission check lets any
  // page with a relay-shaped message print to a locally configured printer.
  const deps = dependencies(false);

  await expect(handleRequest(request, "https://denied.example", deps)).resolves.toEqual({
    ok: false,
    error: expect.objectContaining({ code: "ORIGIN_NOT_GRANTED" }),
  });
  expect(deps.permissions.contains).toHaveBeenCalledWith({ origins: ["https://denied.example/*"] });
  expect(deps.daemon.print).not.toHaveBeenCalled();
});

test("denies the daemon loopback origin even when its manifest permission is present", async () => {
  // Break caught: treating fixed daemon host access as a page grant lets a
  // stale content script authorize a loopback document to print.
  const deps = dependencies();

  await expect(handleRequest(request, "http://127.0.0.1:9000", deps)).resolves.toMatchObject({
    ok: false, error: { code: "ORIGIN_NOT_GRANTED" },
  });
  expect(deps.daemon.print).not.toHaveBeenCalled();
});

test("fails closed when checking the stored grant errors", async () => {
  // Break caught: allowing a failed permission lookup to escape leaves a page
  // request hanging and risks a later refactor dispatching without a grant.
  const deps = dependencies();
  deps.permissions.contains.mockRejectedValueOnce(new Error("storage unavailable"));

  await expect(handleRequest(request, "https://shop.example", deps)).resolves.toMatchObject({
    ok: false, error: { code: "ORIGIN_NOT_GRANTED" },
  });
  expect(deps.daemon.print).not.toHaveBeenCalled();
});

test("returns the daemon health result for the one-shot health operation", async () => {
  // Break caught: treating health as an unrecognised operation prevents the
  // SDK from distinguishing an installed relay from an unavailable daemon.
  const deps = dependencies();

  await expect(handleRequest({ source: "escpost-page", protocol: 1, id: 5, op: "daemon.health", payload: null }, "https://shop.example", deps)).resolves.toEqual({
    ok: true, data: true,
  });
  expect(deps.daemon.health).toHaveBeenCalledOnce();
});

test("returns the daemon inventory for the one-shot list operation", async () => {
  // Break caught: widening the list request or dropping its transport filter
  // makes a granted page receive a different printer inventory than requested.
  const deps = dependencies();
  deps.daemon.list.mockResolvedValueOnce({ printers: ["network-printer"] });

  await expect(handleRequest({ source: "escpost-page", protocol: 1, id: 6, op: "printers.list", payload: { transport: "network" } }, "https://shop.example", deps)).resolves.toEqual({
    ok: true, data: { printers: ["network-printer"] },
  });
  expect(deps.daemon.list).toHaveBeenCalledWith("network");
});

test("decodes padded base64 into exact raw bytes and retains the daemon wire result", async () => {
  // Break caught: forwarding base64 text or renaming job_id corrupts bytes or
  // breaks the SDK's committed wire-to-public result mapping.
  const deps = dependencies();

  await expect(handleRequest(request, "https://shop.example", deps)).resolves.toEqual({
    ok: true,
    data: { job_id: "job-17" },
  });
  expect(deps.daemon.print).toHaveBeenCalledWith("counter", new Uint8Array([0x1b, 0x40, 0xff]));
});

test("rejects noncanonical base64 and extra raw-print payload fields", async () => {
  // Break caught: accepting noncanonical or widened payloads leaves several
  // textual encodings for one receipt and silently expands the page protocol.
  const deps = dependencies();

  await expect(handleRequest({ ...request, payload: { printer: "counter", dataBase64: "AB==" } }, "https://shop.example", deps)).resolves.toMatchObject({
    ok: false, error: { code: "PROTOCOL_MISMATCH" },
  });
  await expect(handleRequest({ ...request, payload: { printer: "counter", dataBase64: "G0D/", unexpected: true } }, "https://shop.example", deps)).resolves.toMatchObject({
    ok: false, error: { code: "PROTOCOL_MISMATCH" },
  });
  expect(deps.daemon.print).not.toHaveBeenCalled();
});

test("accepts exactly the daemon raw-job byte limit and rejects one byte over", async () => {
  // Break caught: decoding an over-limit base64 request can allocate beyond the
  // daemon boundary or send a job the daemon will reject after work has begun.
  const maximumBytes = 8 * 1024 * 1024;
  const atLimit = `${"A".repeat(4 * Math.floor(maximumBytes / 3))}AAA=`;
  const overLimit = `${atLimit.slice(0, -4)}AAAAAAA=`;
  const accepted = dependencies();
  const rejected = dependencies();

  expect(atob(atLimit).length).toBe(maximumBytes);
  expect(Uint8Array.from(atob(atLimit), (character) => character.charCodeAt(0))).toHaveLength(maximumBytes);
  await expect(handleRequest({ ...request, payload: { printer: "counter", dataBase64: atLimit } }, "https://shop.example", accepted)).resolves.toMatchObject({ ok: true });
  await expect(handleRequest({ ...request, payload: { printer: "counter", dataBase64: overLimit } }, "https://shop.example", rejected)).resolves.toMatchObject({
    ok: false, error: { code: "PROTOCOL_MISMATCH" },
  });
  expect(accepted.daemon.print).toHaveBeenCalledWith("counter", expect.any(Uint8Array));
  expect(rejected.daemon.print).not.toHaveBeenCalled();
});

test("returns typed protocol failures without dispatching malformed or unknown operations", async () => {
  // Break caught: accepting URL-safe/unpadded base64 or unknown operation names
  // either changes receipt bytes or leaves page SDK requests unresolved.
  const deps = dependencies();

  await expect(handleRequest({ ...request, payload: { printer: "counter", dataBase64: "G0D_" } }, "https://shop.example", deps)).resolves.toMatchObject({
    ok: false, error: { code: "PROTOCOL_MISMATCH" },
  });
  await expect(handleRequest({ ...request, op: "printers.events" }, "https://shop.example", deps)).resolves.toMatchObject({
    ok: false, error: { code: "PROTOCOL_MISMATCH" },
  });
  expect(deps.daemon.print).not.toHaveBeenCalled();
});

test("maps a daemon print rejection to PRINT_FAILED", async () => {
  // Break caught: leaking daemon transport errors makes an acknowledged or
  // ambiguous raw-print failure indistinguishable from a protocol failure.
  const deps = dependencies();
  deps.daemon.print.mockRejectedValueOnce(new Error("offline"));

  await expect(handleRequest(request, "https://shop.example", deps)).resolves.toMatchObject({
    ok: false, error: { code: "PRINT_FAILED" },
  });
});

test("settles an unexpected request-handler rejection exactly once", async () => {
  // Break caught: an unexpected worker rejection bypasses sendResponse, leaving
  // Chrome's one-shot channel open and producing an unhandled promise error.
  let listener: ((message: unknown, sender: { origin?: string }, respond: (response: unknown) => void) => boolean | void) | undefined;
  const runtime = { onMessage: { addListener: vi.fn((next) => { listener = next; }) } };
  const respond = vi.fn();
  installBackground(runtime, dependencies(), vi.fn(async () => { throw new Error("unexpected"); }));

  expect(listener?.({ source: "escpost-relay", request }, { origin: "https://shop.example" }, respond)).toBe(true);
  await Promise.resolve();
  await Promise.resolve();

  expect(respond).toHaveBeenCalledTimes(1);
  expect(respond).toHaveBeenCalledWith(expect.objectContaining({ ok: false, error: expect.objectContaining({ code: "DAEMON_UNAVAILABLE" }) }));
});

test("runtime listener ignores non-relay envelopes and propagates the trusted sender origin", async () => {
  // Break caught: accepting arbitrary runtime messages or trusting a supplied
  // origin bypasses the content-script boundary that the worker authorizes.
  let listener: ((message: unknown, sender: { origin?: string }, respond: (response: unknown) => void) => boolean | void) | undefined;
  const runtime = { onMessage: { addListener: vi.fn((next) => { listener = next; }) } };
  const handler = vi.fn(async () => ({ ok: true as const, data: true }));
  const respond = vi.fn();
  const deps = dependencies();
  installBackground(runtime, deps, handler);

  expect(listener?.({ source: "other-extension", request }, { origin: "https://shop.example" }, respond)).toBeUndefined();
  expect(handler).not.toHaveBeenCalled();
  expect(listener?.({ source: "escpost-relay", request }, { origin: "https://shop.example" }, respond)).toBe(true);
  await Promise.resolve();
  await Promise.resolve();

  expect(handler).toHaveBeenCalledWith(request, "https://shop.example", deps);
  expect(respond).toHaveBeenCalledTimes(1);
  expect(respond).toHaveBeenCalledWith({ ok: true, data: true });
});

test("runtime listener returns a denied handler result through sendResponse", async () => {
  // Break caught: a correctly denied sender can still hang if the runtime
  // listener does not return the handler's typed response to Chrome.
  let listener: ((message: unknown, sender: { origin?: string }, respond: (response: unknown) => void) => boolean | void) | undefined;
  const runtime = { onMessage: { addListener: vi.fn((next) => { listener = next; }) } };
  const respond = vi.fn();
  installBackground(runtime, dependencies(), vi.fn(async () => ({
    ok: false as const,
    error: { code: "ORIGIN_NOT_GRANTED" as const, message: "denied" },
  })));

  expect(listener?.({ source: "escpost-relay", request }, { origin: "https://denied.example" }, respond)).toBe(true);
  await Promise.resolve();
  await Promise.resolve();

  expect(respond).toHaveBeenCalledTimes(1);
  expect(respond).toHaveBeenCalledWith({ ok: false, error: { code: "ORIGIN_NOT_GRANTED", message: "denied" } });
});
