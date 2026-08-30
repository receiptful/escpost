import { describe, expect, it } from "vitest";
import {
  COMPAT_ID,
  RELAY_ID,
  grantedOrigins,
  scriptsFor,
  syncRegistrations,
  type RegisteredScript,
  type ScriptingArea,
} from "../src/registration";

const DECLARED = ["http://127.0.0.1:9000/*", "https://api.receiptful.io/*"];

describe("which origins get scripts", () => {
  it("takes the sites the user granted one at a time", () => {
    expect(
      grantedOrigins(["https://pos.thornbury.app/*", "https://till.example/*", ...DECLARED], DECLARED),
    ).toEqual(["https://pos.thornbury.app/*", "https://till.example/*"]);
  });

  it("never treats a wildcard grant as a list of sites", () => {
    // Granting the whole optional pool would otherwise put the extension back
    // on every page, which is the thing this module exists to stop.
    expect(grantedOrigins(["https://*/*", "http://*/*"], DECLARED)).toEqual([]);
  });

  it("leaves out our own hosts, which are services rather than pages", () => {
    expect(grantedOrigins(DECLARED, DECLARED)).toEqual([]);
  });
});

describe("what is registered", () => {
  it("registers nothing at all when no site is granted", () => {
    expect(scriptsFor([])).toEqual([]);
  });

  it("puts the relay in the isolated world and the compat pair in the page's", () => {
    const [relay, compat] = scriptsFor(["https://pos.thornbury.app/*"]) as [RegisteredScript, RegisteredScript];

    expect(relay.id).toBe(RELAY_ID);
    expect(relay.js).toEqual(["relay.js"]);
    expect(relay.world).toBe("ISOLATED");

    expect(compat.id).toBe(COMPAT_ID);
    expect(compat.js).toEqual(["ws-patch.js", "qz-shim.js"]);
    expect(compat.world).toBe("MAIN");
  });

  it("runs both at document_start, or a page's own qz-tray.js wins the race", () => {
    for (const script of scriptsFor(["https://pos.thornbury.app/*"])) {
      expect(script.runAt, script.id).toBe("document_start");
    }
  });
});

describe("syncRegistrations", () => {
  it("registers the granted sites and nothing else", async () => {
    const scripting = fakeScripting();
    await syncRegistrations(scripting, ["https://pos.thornbury.app/*"]);

    expect(scripting.registered.map((script) => script.id)).toEqual([RELAY_ID, COMPAT_ID]);
    for (const script of scripting.registered) {
      expect(script.matches).toEqual(["https://pos.thornbury.app/*"]);
    }
  });

  it("removes everything when the last grant is revoked", async () => {
    const scripting = fakeScripting();
    await syncRegistrations(scripting, ["https://pos.thornbury.app/*"]);
    await syncRegistrations(scripting, []);

    expect(scripting.registered).toEqual([]);
  });

  it("is safe to run repeatedly, because it runs on every startup", async () => {
    const scripting = fakeScripting();
    await syncRegistrations(scripting, ["https://pos.thornbury.app/*"]);
    await syncRegistrations(scripting, ["https://pos.thornbury.app/*"]);

    // Registering an id that already exists is an error, so the second run has
    // to clear the first rather than add to it.
    expect(scripting.registered.map((script) => script.id)).toEqual([RELAY_ID, COMPAT_ID]);
  });

  it("carries a new grant into the existing registration", async () => {
    const scripting = fakeScripting();
    await syncRegistrations(scripting, ["https://pos.thornbury.app/*"]);
    await syncRegistrations(scripting, ["https://pos.thornbury.app/*", "https://till.example/*"]);

    expect(scripting.registered[0]?.matches).toEqual([
      "https://pos.thornbury.app/*",
      "https://till.example/*",
    ]);
  });
});

function fakeScripting(): ScriptingArea & { registered: RegisteredScript[] } {
  const registered: RegisteredScript[] = [];
  return {
    registered,
    async getRegisteredContentScripts(filter) {
      const ids = filter?.ids;
      return registered.filter((script) => ids === undefined || ids.includes(script.id));
    },
    async registerContentScripts(scripts) {
      for (const script of scripts) {
        if (registered.some((existing) => existing.id === script.id)) {
          throw new Error(`duplicate script id ${script.id}`);
        }
        registered.push(script);
      }
    },
    async unregisterContentScripts(filter) {
      const ids = filter?.ids ?? registered.map((script) => script.id);
      for (const id of ids) {
        const index = registered.findIndex((script) => script.id === id);
        if (index >= 0) registered.splice(index, 1);
      }
    },
  };
}
