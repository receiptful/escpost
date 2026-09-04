import { expect, test } from "vitest";
import { buildPopupView } from "../src/popup/model";
import { currentSiteOrigin } from "../src/ui/origins";

test("derives only concrete non-daemon HTTP(S) origins for popup permissions", () => {
  // Break caught: requesting a wildcard, opaque, or daemon-loopback permission
  // from the popup would exceed the one-site consent boundary.
  expect(currentSiteOrigin("https://shop.example:8443/orders/7")).toEqual({
    origin: "https://shop.example:8443",
    pattern: "https://shop.example:8443/*",
  });
  expect(currentSiteOrigin("chrome://extensions")).toBeNull();
  expect(currentSiteOrigin("https://*.example/orders")).toBeNull();
  expect(currentSiteOrigin("http://127.0.0.1:9000/health")).toBeNull();
});

test("models a non-web active tab as unsupported without a permission control", () => {
  // Break caught: deriving a host permission from an opaque, wildcard, or
  // fixed daemon-loopback URL would expose the relay outside a real site.
  const view = buildPopupView({ origin: null, grant: "unknown", relay: "unknown", daemon: "unknown" });

  expect(view.permission).toEqual({ tone: "error", label: "This page cannot be granted access" });
  expect(view.primaryAction).toBeNull();
  expect(view.guidance).toContain("HTTP or HTTPS");
});

test("models a concrete ungranted origin with an explicit grant action", () => {
  // Break caught: treating an ungranted page as ready would conceal the
  // required user approval and could make a later relay registration implicit.
  const view = buildPopupView({
    origin: "https://shop.example",
    grant: "absent",
    relay: "unknown",
    daemon: "running",
  });

  expect(view.origin).toBe("https://shop.example");
  expect(view.permission).toEqual({ tone: "warning", label: "Not allowed" });
  expect(view.primaryAction).toEqual({ kind: "grant", label: "Allow this site" });
  expect(view.guidance).toContain("Allow this site");
  expect(view.daemon).toEqual({ tone: "ok", label: "escpost is running" });
});

test("models a granted origin whose document-start relay needs a reload", () => {
  // Break caught: claiming a newly granted page can print before its
  // document-start relay exists leaves the user with a broken SDK call.
  const view = buildPopupView({
    origin: "https://shop.example",
    grant: "present",
    relay: "missing",
    daemon: "running",
  });

  expect(view.permission).toEqual({ tone: "ok", label: "Allowed" });
  expect(view.primaryAction).toEqual({ kind: "revoke", label: "Remove access" });
  expect(view.guidance).toContain("Reload");
});

test("models a granted origin with a loaded relay as ready", () => {
  // Break caught: keeping reload guidance after the relay replies makes a
  // healthy granted document look unavailable.
  const view = buildPopupView({
    origin: "https://shop.example",
    grant: "present",
    relay: "loaded",
    daemon: "unavailable",
  });

  expect(view.permission).toEqual({ tone: "ok", label: "Allowed" });
  expect(view.primaryAction).toEqual({ kind: "revoke", label: "Remove access" });
  expect(view.guidance).toContain("ready");
  expect(view.daemon).toEqual({ tone: "error", label: "escpost is unavailable" });
});
