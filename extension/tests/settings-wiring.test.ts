// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from "vitest";
import { bindSettings, refreshSettings, type SettingsDeps } from "../src/settings/settings";
import { account } from "./fixtures/popup-fixtures";
import { settingsInput } from "./fixtures/settings-fixtures";

function deps(overrides: Partial<SettingsDeps> = {}): SettingsDeps {
  return {
    readAll: vi.fn().mockResolvedValue(settingsInput()),
    revoke: vi.fn().mockResolvedValue(undefined),
    writeAliases: vi.fn().mockResolvedValue(undefined),
    signOut: vi.fn().mockResolvedValue(undefined),
    now: () => 9_000,
    ...overrides,
  };
}

let root: HTMLElement;

beforeEach(() => {
  document.body.innerHTML = "";
  root = document.createElement("main");
  document.body.append(root);
});

async function start(d: SettingsDeps): Promise<void> {
  bindSettings(root, d);
  await refreshSettings(root, d);
}

function click(selector: string): void {
  root.querySelector<HTMLElement>(selector)?.click();
}

describe("revoking a site (P4)", () => {
  it("asks for confirmation before revoking anything", async () => {
    const d = deps();
    await start(d);
    click('[data-action="revoke"]');
    expect(d.revoke).not.toHaveBeenCalled();
    expect(root.querySelector('[data-action="revoke-confirm"]')).not.toBeNull();
    expect(root.querySelector('[data-action="revoke-cancel"]')).not.toBeNull();
  });

  it("revokes the pattern once it is confirmed, and re-reads the page", async () => {
    const d = deps();
    await start(d);
    click('[data-action="revoke"]');
    click('[data-action="revoke-confirm"]');
    await vi.waitFor(() => expect(d.revoke).toHaveBeenCalledWith("https://bluebirdcafe.co/*"));
    await vi.waitFor(() => expect(d.readAll).toHaveBeenCalledTimes(2));
  });

  it("leaves the grant alone when the confirmation is cancelled", async () => {
    const d = deps();
    await start(d);
    click('[data-action="revoke"]');
    click('[data-action="revoke-cancel"]');
    await vi.waitFor(() => expect(root.querySelector('[data-action="revoke"]')).not.toBeNull());
    expect(d.revoke).not.toHaveBeenCalled();
  });
});

describe("printer names (N2, U5)", () => {
  it("maps the selected printer and clears the unmatched entry in one click", async () => {
    const d = deps();
    await start(d);
    const select = root.querySelector<HTMLSelectElement>(".set-pick");
    if (select !== null) select.value = "kitchen";
    click('[data-action="create-alias"]');

    await vi.waitFor(() => expect(d.writeAliases).toHaveBeenCalledTimes(1));
    expect(d.writeAliases).toHaveBeenCalledWith({
      aliases: { "epson tm-t20ii": "tm-t20", "star tsp100": "kitchen" },
      aliasMeta: {
        "epson tm-t20ii": {
          requested: "EPSON TM-T20II",
          origin: "https://pos.thornbury.app",
          at: Date.UTC(2026, 7, 11),
        },
        "star tsp100": { requested: "Star TSP100", origin: "https://pos.thornbury.app", at: 9_000 },
      },
      unmatched: [],
    });
  });

  it("writes the shortened map when an alias is removed", async () => {
    const d = deps();
    await start(d);
    click('[data-action="remove-alias"]');
    await vi.waitFor(() => expect(d.writeAliases).toHaveBeenCalledTimes(1));
    expect(d.writeAliases).toHaveBeenCalledWith(
      expect.objectContaining({ aliases: {}, aliasMeta: {} }),
    );
  });

  it("dismisses an unmatched name without creating an alias for it", async () => {
    const d = deps();
    await start(d);
    click('[data-action="dismiss-unmatched"]');
    await vi.waitFor(() => expect(d.writeAliases).toHaveBeenCalledTimes(1));
    expect(d.writeAliases).toHaveBeenCalledWith(
      expect.objectContaining({ aliases: { "epson tm-t20ii": "tm-t20" }, unmatched: [] }),
    );
  });
});

describe("signing out (A4)", () => {
  it("signs out and re-renders as signed out, whatever the worker did", async () => {
    const d = deps({
      readAll: vi
        .fn()
        .mockResolvedValueOnce(settingsInput({ account: account() }))
        .mockResolvedValue(settingsInput({ account: null })),
    });
    await start(d);
    expect(root.textContent).toContain("sam@bluebirdcafe.co");

    click('[data-action="sign-out"]');
    await vi.waitFor(() => expect(d.signOut).toHaveBeenCalledTimes(1));
    await vi.waitFor(() => expect(root.textContent).not.toContain("sam@bluebirdcafe.co"));
  });
});
