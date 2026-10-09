import type { DaemonPrinter } from "../../src/daemon";
import type { SettingsInput } from "../../src/settings/model";

export const SETTINGS_PRINTERS: DaemonPrinter[] = [
  { id: "tm-t20", name: "TM-T20", transport: "usb", profile: "NT-5890K", status: "ready" },
  { id: "kitchen", name: "Kitchen", transport: "network", profile: null, status: "ready" },
];

export function settingsInput(overrides: Partial<SettingsInput> = {}): SettingsInput {
  return {
    originPatterns: [
      "https://bluebirdcafe.co/*",
      "https://pos.thornbury.app/*",
      "http://127.0.0.1:9000/*",
      "https://api.receiptful.io/*",
    ],
    declaredHosts: ["http://127.0.0.1:9000/*", "https://api.receiptful.io/*"],
    grants: { "https://bluebirdcafe.co/*": { at: Date.UTC(2026, 7, 4), via: "@escpost/browser" } },
    aliases: { "epson tm-t20ii": "tm-t20" },
    aliasMeta: {
      "epson tm-t20ii": {
        requested: "EPSON TM-T20II",
        origin: "https://pos.thornbury.app",
        at: Date.UTC(2026, 7, 11),
      },
    },
    unmatched: [{ requested: "Star TSP100", origin: "https://pos.thornbury.app", at: Date.UTC(2026, 7, 20) }],
    printers: SETTINGS_PRINTERS,
    account: null,
    daemonRunning: true,
    extensionVersion: "1.0.0",
    ...overrides,
  };
}
