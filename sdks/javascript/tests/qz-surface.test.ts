// @vitest-environment happy-dom
import { describe, expect, it } from "vitest";
import { createQzShim, installQzShim, type EscpostClient } from "../src/qz/surface";
import type { Printer } from "../src/index";

function printer(name: string, availability: Printer["availability"] = "connected"): Printer {
  return {
    name,
    transport: "network",
    availability,
    profile: null,
    connection: { type: "network", host: "10.0.0.2", port: 9100 },
  };
}

const PRINTERS = [printer("TM-T20"), printer("Kitchen")];

/**
 * Stands in for the ESCPost client, so the shim is tested without a browser
 * extension or a daemon. It records every call the shim makes.
 */
function fakeClient(options: { available?: boolean; printers?: Printer[] } = {}) {
  const calls: Array<{ method: string; argument?: unknown }> = [];
  const printers = options.printers ?? PRINTERS;
  const client: EscpostClient = {
    async isAvailable() {
      calls.push({ method: "isAvailable" });
      return options.available ?? true;
    },
    printers: {
      async list() {
        calls.push({ method: "printers.list" });
        return { updatedAt: "2026-10-15T00:00:00Z", warning: null, printers };
      },
      subscribe() {
        return () => {};
      },
    },
    async print(request) {
      calls.push({ method: "print", argument: request });
      return { jobId: "job-1" };
    },
  };
  const printed = () => calls.filter((call) => call.method === "print").map((call) => call.argument);
  return { client, calls, printed };
}

async function connectedShim(options?: Parameters<typeof fakeClient>[0]) {
  const fake = fakeClient(options);
  const qz = createQzShim(fake.client);
  await qz.websocket.connect();
  return { qz, ...fake };
}

describe("installQzShim", () => {
  it("installs window.qz when nothing else has", () => {
    const target = {} as Window & { qz?: unknown };
    expect(installQzShim(target)).toBe(true);
    expect((target.qz as { version: string }).version).toBe("2.2.4");
  });

  it("yields to a page that loads the real qz-tray.js (Q4)", () => {
    const target = { qz: { version: "real" } } as unknown as Window & { qz?: unknown };
    expect(installQzShim(target)).toBe(false);
    expect((target.qz as { version: string }).version).toBe("real");
  });

  it("leaves window.qz writable so a later qz-tray.js can take over", () => {
    const target = {} as Window & { qz?: unknown };
    installQzShim(target);
    target.qz = { version: "real" };
    expect((target.qz as { version: string }).version).toBe("real");
  });
});

describe("connecting", () => {
  it("accepts every qz.security setter without prompting or calling ESCPost (Q1)", () => {
    const { client, calls } = fakeClient();
    const qz = createQzShim(client);
    expect(() => qz.security.setCertificatePromise((resolve: (cert: string) => void) => resolve("cert"))).not.toThrow();
    expect(() => qz.security.setSignaturePromise(() => (resolve: (sig: string) => void) => resolve(""))).not.toThrow();
    expect(() => qz.security.setSignatureAlgorithm("SHA512")).not.toThrow();
    expect(calls).toHaveLength(0);
  });

  it("connects when ESCPost is available, reports itself active, and disconnects", async () => {
    const { client } = fakeClient();
    const qz = createQzShim(client);
    expect(qz.websocket.isActive()).toBe(false);
    await expect(qz.websocket.connect()).resolves.toBeUndefined();
    expect(qz.websocket.isActive()).toBe(true);
    await expect(qz.websocket.disconnect()).resolves.toBeUndefined();
    expect(qz.websocket.isActive()).toBe(false);
  });

  it("fails to connect and says what is missing when ESCPost is not available", async () => {
    const { client } = fakeClient({ available: false });
    const qz = createQzShim(client);
    await expect(qz.websocket.connect()).rejects.toMatchObject({ code: "DAEMON_UNAVAILABLE" });
    await expect(qz.websocket.connect()).rejects.toThrow(/extension/);
    expect(qz.websocket.isActive()).toBe(false);
  });

  it("reports the version qz-tray.js applies no legacy rewrites for", async () => {
    const { qz } = await connectedShim();
    await expect(qz.api.getVersion()).resolves.toBe("2.2.4");
  });

  it("rejects a print made before connect(), exactly as qz-tray.js does", async () => {
    const { client, printed } = fakeClient();
    const qz = createQzShim(client);
    await expect(qz.print(qz.configs.create("TM-T20"), ["\x1b@"])).rejects.toThrow(
      /connection to QZ has not been established/,
    );
    expect(printed()).toHaveLength(0);
  });
});

describe("finding printers", () => {
  it("returns plain name strings from printers.find, as QZ does", async () => {
    const { qz } = await connectedShim();
    await expect(qz.printers.find()).resolves.toEqual(["TM-T20", "Kitchen"]);
  });

  it("returns the single matched name, ignoring case, when printers.find is given a query", async () => {
    const { qz } = await connectedShim();
    await expect(qz.printers.find("kitchen")).resolves.toBe("Kitchen");
  });

  it("finds a printer from part of its name, like QZ's find(\"zebra\") example", async () => {
    const { qz } = await connectedShim({ printers: [printer("TM-T20"), printer("Zebra LP2844")] });
    await expect(qz.printers.find("zebra")).resolves.toBe("Zebra LP2844");
  });

  it("prefers an exact name, then a name that starts with the query, as QZ does", async () => {
    const { qz } = await connectedShim({
      printers: [printer("Back Kitchen"), printer("Kitchen Printer"), printer("Kitchen")],
    });
    await expect(qz.printers.find("kitchen")).resolves.toBe("Kitchen");

    const partial = await connectedShim({ printers: [printer("Back Kitchen"), printer("Kitchen Printer")] });
    await expect(partial.qz.printers.find("kitchen")).resolves.toBe("Kitchen Printer");
  });

  it("names the printers it does know when a query matches nothing (N1)", async () => {
    const { qz } = await connectedShim();
    await expect(qz.printers.find("Star TSP100")).rejects.toThrow(/TM-T20, Kitchen/);
  });

  it("returns the first connected printer as the default, since ESCPost has none", async () => {
    const { qz } = await connectedShim({ printers: [printer("Unplugged", "unavailable"), printer("POS80")] });
    await expect(qz.printers.getDefault()).resolves.toBe("POS80");
  });

  it("returns null as the default when no printer is connected", async () => {
    const { qz } = await connectedShim({ printers: [printer("Unplugged", "unavailable")] });
    await expect(qz.printers.getDefault()).resolves.toBeNull();
  });
});

describe("printing", () => {
  it("sends the job's bytes to the matched printer and resolves null like qz-tray.js", async () => {
    const { qz, printed } = await connectedShim();
    await expect(qz.print(qz.configs.create("TM-T20"), ["\x1b@hello"])).resolves.toBeNull();
    expect(printed()).toEqual([{ printer: "TM-T20", data: Uint8Array.from([0x1b, 0x40, ...Buffer.from("hello")]) }]);
  });

  it("uses ESCPost's spelling of a printer name typed in another case", async () => {
    const { qz, printed } = await connectedShim();
    await qz.print(qz.configs.create("kitchen"), ["\x1b@"]);
    expect(printed()).toEqual([{ printer: "Kitchen", data: Uint8Array.from([0x1b, 0x40]) }]);
  });

  it("prints to a printer named by part of its name, as QZ does", async () => {
    const { qz, printed } = await connectedShim({ printers: [printer("TM-T20"), printer("Zebra LP2844")] });
    await qz.print(qz.configs.create("zebra"), ["\x1b@"]);
    expect(printed()).toEqual([{ printer: "Zebra LP2844", data: Uint8Array.from([0x1b, 0x40]) }]);
  });

  it("refuses an unknown printer with the known names and sends nothing", async () => {
    const { qz, printed } = await connectedShim();
    await expect(qz.print(qz.configs.create("EPSON TM-T20II"), ["\x1b@"])).rejects.toMatchObject({
      code: "PRINTER_NOT_FOUND",
      message: expect.stringMatching(/TM-T20, Kitchen/),
    });
    expect(printed()).toHaveLength(0);
  });

  it("encodes plain text with the config's encoding before sending it", async () => {
    const { qz, printed } = await connectedShim();
    await qz.print(qz.configs.create("TM-T20", { encoding: "IBM00858" }), ["é€"]);
    expect(printed()).toEqual([{ printer: "TM-T20", data: Uint8Array.from([0x82, 0xd5]) }]);
  });

  it("sends one job per config when qz.print is given several (QZ's batch form)", async () => {
    const { qz, printed } = await connectedShim();
    await qz.print([qz.configs.create("TM-T20"), qz.configs.create("Kitchen")], [["A"], ["B"]]);
    expect(printed()).toEqual([
      { printer: "TM-T20", data: Uint8Array.from([0x41]) },
      { printer: "Kitchen", data: Uint8Array.from([0x41 + 1]) },
    ]);
  });

  it("refuses a flavor it cannot send without reaching the printer (Q3)", async () => {
    const { qz, printed } = await connectedShim();
    await expect(qz.print(qz.configs.create("TM-T20"), [{ flavor: "xml", data: "<x/>" }])).rejects.toMatchObject({
      code: "UNSUPPORTED_FORMAT",
    });
    expect(printed()).toHaveLength(0);
  });

  it("gives a config all of qz-tray.js's default options, not just the set ones", () => {
    const qz = createQzShim(fakeClient().client);
    const options = qz.configs.create("TM-T20", { copies: 2 }).getOptions();
    expect(options.copies).toBe(2);
    expect(options.units).toBe("in");
    expect(options.colorType).toBe("color");
    expect("spool" in options).toBe(true);
  });
});

describe("QZ features ESCPost does not offer", () => {
  it("lets a page register their callbacks on load, as QZ's own demo does", () => {
    const qz = createQzShim(fakeClient().client);
    expect(() => qz.serial.setSerialCallbacks()).not.toThrow();
    expect(() => qz.usb.setUsbCallbacks()).not.toThrow();
    expect(() => qz.hid.setHidCallbacks()).not.toThrow();
    expect(() => qz.file.setFileCallbacks()).not.toThrow();
    expect(() => qz.printers.setPrinterCallbacks()).not.toThrow();
  });

  it("rejects their functions with a message naming the call", async () => {
    const { qz } = await connectedShim();
    await expect(qz.usb.listDevices()).rejects.toMatchObject({
      code: "UNSUPPORTED_FEATURE",
      message: expect.stringMatching(/qz\.usb\.listDevices\(\)/),
    });
  });
});
