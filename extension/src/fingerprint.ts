import type { DaemonPrinter } from "./daemon";

/**
 * The device fields a daemon reports when it advertises the
 * "device-identity" capability on /info. Absent on a daemon that predates
 * it, which is why every field is optional.
 */
export interface DaemonDevice {
  usbVendorId?: number;
  usbProductId?: number;
  usbSerial?: string;
  host?: string;
  port?: number;
}

export interface IdentifiablePrinter extends DaemonPrinter {
  device?: DaemonDevice;
}

export interface PrinterIdentity {
  fingerprint: string;
  strength: "strong" | "weak";
  entryId: string;
}

/**
 * identity comes from the device, never from this browser.
 *
 * A counter with three tills has three Chrome profiles registering the same
 * physical printer. Anything derived from chrome.storage, a random id or an
 * install token would bill that shop three times for one printer, and it
 * would find out on its first invoice. So: USB serial, else vendor/product,
 * else the TCP endpoint — all reported by the daemon, all identical from
 * every profile on the machine.
 *
 * The weak fallback, `entry:<registry key>`, is what today's daemon supports.
 * It is still machine-wide, so it fixes the three-tills case; what it cannot
 * do is tell apart two machines whose printers.toml keys collide. The server
 * upgrades a weak row in place when a strong identity later arrives.
 */
export function deriveIdentity(printer: IdentifiablePrinter): PrinterIdentity {
  const device = printer.device ?? {};
  const entryId = printer.id;

  if (device.usbVendorId !== undefined && device.usbProductId !== undefined) {
    const model = `usb:${hex4(device.usbVendorId)}:${hex4(device.usbProductId)}`;
    return {
      fingerprint: device.usbSerial ? `${model}:${device.usbSerial}` : model,
      strength: "strong",
      entryId,
    };
  }

  if (device.host !== undefined && device.port !== undefined) {
    return { fingerprint: `tcp:${device.host}:${device.port}`, strength: "strong", entryId };
  }

  return { fingerprint: `entry:${entryId}`, strength: "weak", entryId };
}

/** Fixed width and lower case: one id formatted two ways is two invoices. */
function hex4(value: number): string {
  return value.toString(16).toLowerCase().padStart(4, "0");
}
