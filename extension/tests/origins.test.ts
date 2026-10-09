import { describe, expect, it } from "vitest";
import { displayOrigin, isWebOrigin } from "../src/ui/origins";

describe("displayOrigin", () => {
  it("shows an https site as a bare host, the way the mockup does", () => {
    expect(displayOrigin("https://bluebirdcafe.co/*")).toBe("bluebirdcafe.co");
    expect(displayOrigin("https://pos.thornbury.app/*")).toBe("pos.thornbury.app");
  });

  it("keeps the scheme on an http site, so it is not mistaken for a secure one", () => {
    expect(displayOrigin("http://till.example/*")).toBe("http://till.example");
    expect(displayOrigin("http://localhost:8900/*")).toBe("http://localhost:8900");
  });

  it("hides the daemon's own host permission, which is not a site that can print", () => {
    expect(displayOrigin("http://127.0.0.1:9180/*")).toBeNull();
    expect(displayOrigin("http://[::1]:9180/*")).toBeNull();
    expect(isWebOrigin("http://127.0.0.1:9180/*")).toBe(false);
  });

  it("names a whole-web grant instead of silently listing nothing", () => {
    expect(displayOrigin("https://*/*")).toBe("Every https site");
    expect(displayOrigin("http://*/*")).toBe("Every http site");
  });
});

describe("hosts the extension declares for itself are not sites that can print", () => {
  // Found in the browser: once api.receiptful.io moved into host_permissions it
  // started appearing in "Sites that can print" with a Revoke link -- revoking
  // it would have silently broken HTML rendering, and it never printed anything.
  const DECLARED = ["http://127.0.0.1:9180/*", "https://api.receiptful.io/*"];

  it("hides our own API from the site list", () => {
    expect(displayOrigin("https://api.receiptful.io/*", DECLARED)).toBeNull();
    expect(isWebOrigin("https://api.receiptful.io/*", DECLARED)).toBe(false);
  });

  it("hides the daemon whether or not it is passed as declared", () => {
    expect(displayOrigin("http://127.0.0.1:9180/*", DECLARED)).toBeNull();
    expect(displayOrigin("http://127.0.0.1:9180/*")).toBeNull();
  });

  it("still shows a site the user actually granted", () => {
    expect(displayOrigin("https://bluebirdcafe.co/*", DECLARED)).toBe("bluebirdcafe.co");
    expect(displayOrigin("http://localhost:8900/*", DECLARED)).toBe("http://localhost:8900");
  });
});
