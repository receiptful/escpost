import { expect, test } from "vitest";
import { escpost } from "../src/index";
import type { PageRequest } from "../src/protocol";
import type { PageWindow } from "../src/transport";

type PostedRequest = {
  source: string;
  protocol: number;
  id: number;
  op: string;
  payload: unknown;
};

class FakePageWindow implements PageWindow {
  readonly posted: PostedRequest[] = [];
  private readonly listeners: Array<(event: MessageEvent) => void> = [];

  addEventListener(_type: "message", listener: (event: MessageEvent) => void): void {
    this.listeners.push(listener);
  }

  postMessage(message: PageRequest): void {
    this.posted.push(message);
  }

  reply(request: PostedRequest, reply: { ok: true; data: unknown } | { ok: false; error: unknown }): void {
    for (const listener of this.listeners) {
      listener({
        data: { source: "escpost-extension", id: request.id, ...reply },
        source: this,
      } as unknown as MessageEvent);
    }
  }
}

function pageRelay(): FakePageWindow {
  const existing = (globalThis as { window?: Partial<FakePageWindow> }).window;
  if (Array.isArray(existing?.posted) && typeof existing.reply === "function") {
    return existing as FakePageWindow;
  }
  const page = new FakePageWindow();
  Object.assign(globalThis, { window: page });
  return page;
}

test("lists the complete mapped inventory through the network relay operation", async () => {
  // Break caught: sending the wrong list operation/filter or dropping USB
  // connection facts makes a returned inventory unusable to SDK consumers.
  const page = pageRelay();
  const before = page.posted.length;
  const inventory = escpost.printers.list({ transport: "network" });
  const request = page.posted[before];

  expect(request).toMatchObject({ source: "escpost-page", protocol: 1 });
  expect(request.op).toBe("printers.list");
  expect(request.payload).toEqual({ transport: "network" });

  page.reply(request, {
    ok: true,
    data: {
    updated_at: "2026-09-01T10:30:00Z",
    warning: "A probe is slow.",
    printers: [
      {
        name: "counter",
        transport: "usb",
        availability: "connected",
        profile: "80mm",
        connection: {
          type: "usb",
          vendor_id: 1046,
          product_id: 20497,
          bus: "001",
          address: 4,
          manufacturer: "Bixolon",
          product: "SRP-350",
          serial_number: "serial-7",
          interface_number: 2,
          out_endpoints: [1, 3],
          in_endpoints: [129],
        },
      },
      {
        name: "kitchen",
        transport: "network",
        availability: "unavailable",
        profile: null,
        connection: { type: "network", host: "192.0.2.7", port: 9100 },
      },
    ],
    },
  });

  await expect(inventory).resolves.toEqual({
    updatedAt: "2026-09-01T10:30:00Z",
    warning: "A probe is slow.",
    printers: [
      {
        name: "counter",
        transport: "usb",
        availability: "connected",
        profile: "80mm",
        connection: {
          type: "usb",
          vendorId: 1046,
          productId: 20497,
          bus: "001",
          address: 4,
          manufacturer: "Bixolon",
          product: "SRP-350",
          serialNumber: "serial-7",
          interfaceNumber: 2,
          outEndpoints: [1, 3],
          inEndpoints: [129],
        },
      },
      {
        name: "kitchen",
        transport: "network",
        availability: "unavailable",
        profile: null,
        connection: { type: "network", host: "192.0.2.7", port: 9100 },
      },
    ],
  });
  expect("getDefault" in escpost.printers).toBe(false);
});

test("prints exact raw bytes through the private padded base64 relay payload", async () => {
  // Break caught: forwarding strings instead of UTF-8 bytes or omitting base64
  // padding changes the receipt before the extension restores its raw body.
  const page = pageRelay();
  const before = page.posted.length;
  const printed = escpost.print({ printer: "counter", data: new Uint8Array([0x1b, 0x40, 0xff]) });
  const request = page.posted[before];

  expect(request.op).toBe("print.raw");
  expect(request.payload).toEqual({ printer: "counter", dataBase64: "G0D/" });

  page.reply(request, { ok: true, data: { job_id: "job-17" } });
  await expect(printed).resolves.toEqual({ jobId: "job-17" });
});

test("UTF-8 encodes string receipts before relaying them", async () => {
  // Break caught: treating JavaScript string code units as bytes corrupts
  // non-ASCII receipt text before it reaches the printer.
  const page = pageRelay();
  const before = page.posted.length;
  const printed = escpost.print({ printer: "counter", data: "é" });
  const request = page.posted[before];

  expect(request.payload).toEqual({ printer: "counter", dataBase64: "w6k=" });

  page.reply(request, { ok: true, data: { job_id: "job-18" } });
  await expect(printed).resolves.toEqual({ jobId: "job-18" });
});

test("rejects a malformed successful inventory as a protocol mismatch", async () => {
  // Break caught: mapping an unchecked success payload throws a native
  // TypeError instead of the SDK's documented typed protocol error.
  const page = pageRelay();
  const before = page.posted.length;
  const inventory = escpost.printers.list();
  const request = page.posted[before];

  page.reply(request, { ok: true, data: { updated_at: "not-a-date", warning: null, printers: [] } });

  await expect(inventory).rejects.toMatchObject({
    name: "EscpostError",
    code: "PROTOCOL_MISMATCH",
    message: "The extension returned an invalid printer inventory.",
  });
});

test("rejects a malformed successful print result as a protocol mismatch", async () => {
  // Break caught: reading job_id from an unchecked success payload can resolve
  // an undefined public job id or throw a native TypeError.
  const page = pageRelay();
  const before = page.posted.length;
  const printed = escpost.print({ printer: "counter", data: new Uint8Array([0x0a]) });
  const request = page.posted[before];

  page.reply(request, { ok: true, data: { job_id: 17 } });

  await expect(printed).rejects.toMatchObject({
    name: "EscpostError",
    code: "PROTOCOL_MISMATCH",
    message: "The extension returned an invalid print result.",
  });
});
