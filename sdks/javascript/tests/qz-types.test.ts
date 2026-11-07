import { describe, expect, it } from "vitest";
import type qzDefault from "../src/qz/index";

type Qz = typeof qzDefault;

/**
 * Calls written the way QZ Tray's documentation (qz.io/api) writes them. This
 * function never runs: `tsc --noEmit` (bun run typecheck) compiles it, so a
 * shim signature narrower than QZ's fails the typecheck, as it would in a
 * TypeScript page migrating from qz-tray.
 */
async function qzDocumentedCalls(qz: Qz): Promise<void> {
  // qz.api and qz.security: set up before connecting.
  qz.api.setPromiseType((resolver: (resolve: unknown, reject: unknown) => void) => new Promise(resolver));
  qz.api.setSha256Type((data: string) => data);
  qz.api.showDebug(true);
  qz.security.setCertificatePromise((resolve: (cert: string) => void) => resolve("cert"), { rejectOnFailure: true });
  qz.security.setSignatureAlgorithm("SHA512");
  qz.security.setSignaturePromise((toSign: string) => (resolve: (signature: string) => void) => resolve(toSign));

  // qz.websocket
  qz.websocket.setClosedCallbacks((event: unknown) => event);
  qz.websocket.setErrorCallbacks([(event: unknown) => event]);
  await qz.websocket.connect();
  await qz.websocket.connect({ host: "localhost", usingSecure: true, keepAlive: 60, retries: 5, delay: 1 });
  await qz.websocket.connect({ host: ["localhost", "127.0.0.1"], port: { secure: [8181], insecure: [8182] } });
  const active: boolean = qz.websocket.isActive();
  void active;
  await qz.websocket.disconnect();

  // qz.printers, with QZ's optional signature arguments
  const found: string | string[] = await qz.printers.find("zebra");
  void found;
  await qz.printers.find("zebra", "signature", 1700000000000);
  await qz.printers.getDefault("signature", 1700000000000);
  qz.printers.setPrinterCallbacks((event: unknown) => event);
  await qz.printers.startListening("zebra", { jobData: true }).catch(() => undefined);
  await qz.printers.details().catch(() => undefined);
  await qz.printers.clearQueue("zebra").catch(() => undefined);

  // qz.configs and qz.print
  const config = qz.configs.create("zebra", { copies: 2, encoding: "IBM00858" });
  config.reconfigure({ copies: 1 });
  config.setPrinter("Kitchen");
  qz.configs.setDefaults({ units: "mm" });
  await qz.print(config, ["^XA^XZ"]);
  await qz.print(config, ["^XA^XZ"], true);
  await qz.print([config, config], [["A"], ["B"]], true, ["signature"], [1700000000000]);
  await config.print(["^XA^XZ"]);
  await config.print(["^XA^XZ"], "signature", 1700000000000);

  // Namespaces ESCPost does not offer: they must still type-check as QZ calls.
  qz.serial.setSerialCallbacks((event: unknown) => event);
  await qz.serial.openPort("COM1", { baudRate: 9600 }).catch(() => undefined);
  await qz.serial.sendData("COM1", "data", { baudRate: 9600 }).catch(() => undefined);
  qz.usb.setUsbCallbacks((event: unknown) => event);
  await qz.usb.listDevices(true).catch(() => undefined);
  qz.hid.setHidCallbacks((event: unknown) => event);
  await qz.hid.listDevices().catch(() => undefined);
  qz.socket.setSocketCallbacks((event: unknown) => event);
  await qz.socket.open("10.0.0.2", 9100).catch(() => undefined);
  qz.file.setFileCallbacks((event: unknown) => event);
  await qz.file.read("receipt.txt", { sandbox: true }).catch(() => undefined);
  await qz.networking.device("google.com", 443).catch(() => undefined);
}

describe("QZ Tray's documented calls", () => {
  it("type-check against the shim (the check itself is bun run typecheck)", () => {
    expect(qzDocumentedCalls).toBeTypeOf("function");
  });
});
