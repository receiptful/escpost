import { describe, expect, it } from "vitest";
import { deriveIdentity, type IdentifiablePrinter } from "../src/fingerprint";

function printer(overrides: Partial<IdentifiablePrinter> = {}): IdentifiablePrinter {
  return {
    id: "counter",
    name: "counter",
    transport: "usb",
    profile: "NT-5890K",
    status: "ready",
    ...overrides,
  };
}

describe("deriveIdentity", () => {
  it("uses the USB serial when the daemon reports one", () => {
    const identity = deriveIdentity(
      printer({ device: { usbVendorId: 0x04b8, usbProductId: 0x0202, usbSerial: "B120300001" } }),
    );
    expect(identity).toEqual({
      fingerprint: "usb:04b8:0202:B120300001",
      strength: "strong",
      entryId: "counter",
    });
  });

  it("falls back to vendor and product when there is no serial", () => {
    const identity = deriveIdentity(printer({ device: { usbVendorId: 0x04b8, usbProductId: 0x0202 } }));
    expect(identity.fingerprint).toBe("usb:04b8:0202");
    expect(identity.strength).toBe("strong");
  });

  it("uses the endpoint for a network printer", () => {
    const identity = deriveIdentity(
      printer({ transport: "network", device: { host: "192.168.1.50", port: 9100 } }),
    );
    expect(identity.fingerprint).toBe("tcp:192.168.1.50:9100");
    expect(identity.strength).toBe("strong");
  });

  it("falls back to the registry key against a daemon with no device identity", () => {
    const identity = deriveIdentity(printer());
    expect(identity).toEqual({ fingerprint: "entry:counter", strength: "weak", entryId: "counter" });
  });

  it("treats an empty device object as no device identity", () => {
    expect(deriveIdentity(printer({ device: {} })).strength).toBe("weak");
  });

  it("zero-pads and lower-cases hex so 0x04b8 never renders as 4b8", () => {
    // A vendor id formatted two different ways is two billable printers.
    const identity = deriveIdentity(printer({ device: { usbVendorId: 0x4b8, usbProductId: 0xa } }));
    expect(identity.fingerprint).toBe("usb:04b8:000a");
  });

  it("is stable across calls and derives nothing from this browser", () => {
    const subject = printer({ device: { usbVendorId: 0x04b8, usbProductId: 0x0202, usbSerial: "S1" } });
    // M7: two Chrome profiles run this code separately and must agree.
    expect(deriveIdentity(subject)).toEqual(deriveIdentity(subject));
  });

  it("reports the same entryId weak or strong, which is what lets a row upgrade in place (M7)", () => {
    // The billing bug this prevents: when the daemon gains device identity, the
    // same physical printer starts reporting `usb:...` where it reported
    // `entry:counter`. The server matches on entry_id to UPGRADE that row. If the
    // extension changed entry_id across the transition there would be nothing to
    // match on, and release day would silently double every customer's billable
    // printer count.
    const today = deriveIdentity(printer());
    const afterDaemonUpgrade = deriveIdentity(
      printer({ device: { usbVendorId: 0x04b8, usbProductId: 0x0202, usbSerial: "B120300001" } }),
    );

    expect(today.strength).toBe("weak");
    expect(afterDaemonUpgrade.strength).toBe("strong");
    expect(afterDaemonUpgrade.fingerprint).not.toBe(today.fingerprint);
    expect(afterDaemonUpgrade.entryId).toBe(today.entryId);
  });

  it("never emits a weak fingerprint that could collide with a strong one", () => {
    // The namespaces have to stay disjoint, or a registry key literally named
    // "usb:04b8:0202" would match a real device.
    const weak = deriveIdentity(printer({ id: "usb:04b8:0202" }));
    const strong = deriveIdentity(printer({ device: { usbVendorId: 0x04b8, usbProductId: 0x0202 } }));

    expect(weak.fingerprint).toBe("entry:usb:04b8:0202");
    expect(weak.fingerprint).not.toBe(strong.fingerprint);
  });

  it("keeps two identical models with different serials apart", () => {
    const a = deriveIdentity(printer({ device: { usbVendorId: 1, usbProductId: 2, usbSerial: "A" } }));
    const b = deriveIdentity(printer({ device: { usbVendorId: 1, usbProductId: 2, usbSerial: "B" } }));
    expect(a.fingerprint).not.toBe(b.fingerprint);
  });
});
