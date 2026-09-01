import { expect, test, vi } from "vitest";
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
