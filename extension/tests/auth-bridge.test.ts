// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from "vitest";

/**
 * The content script that carries the session token from the verify page to
 * the worker. It exists because the extension cannot receive a redirect, which
 * is the only reason the old design polled — and the poll was the hole.
 */

const TOKEN = "rfx_" + "a".repeat(64);

function seedPage(json: string | null): void {
  document.documentElement.removeAttribute("data-escpost-ack");
  document.body.innerHTML = "";
  if (json !== null) {
    const island = document.createElement("script");
    island.type = "application/json";
    island.id = "escpost-session";
    island.textContent = json;
    document.body.appendChild(island);
  }
}

let sendMessage: ReturnType<typeof vi.fn>;

beforeEach(() => {
  sendMessage = vi.fn().mockResolvedValue({ ok: true, data: { signedIn: true } });
  (globalThis as any).chrome = { runtime: { sendMessage } };
  vi.resetModules();
});

describe("the auth bridge", () => {
  it("hands the token from the page's JSON island to the worker", async () => {
    seedPage(JSON.stringify({ token: TOKEN }));

    const { handOffSession } = await import("../src/auth-bridge");
    await handOffSession();

    expect(sendMessage).toHaveBeenCalledWith({ op: "auth.bridge", payload: { token: TOKEN } });
  });

  it("acknowledges on the document so the page can tell it worked", async () => {
    seedPage(JSON.stringify({ token: TOKEN }));

    const { handOffSession } = await import("../src/auth-bridge");
    await handOffSession();

    expect(document.documentElement.hasAttribute("data-escpost-ack")).toBe(true);
  });

  it("does NOT acknowledge when the worker refuses", async () => {
    // The page then tells the user to open the link where escpost is
    // installed, instead of showing a success that never happened.
    sendMessage.mockResolvedValue({ ok: false, error: { code: "NOT_SIGNED_IN", message: "no" } });
    seedPage(JSON.stringify({ token: TOKEN }));

    const { handOffSession } = await import("../src/auth-bridge");
    await handOffSession();

    expect(document.documentElement.hasAttribute("data-escpost-ack")).toBe(false);
  });

  it("does nothing on a page with no session island", async () => {
    seedPage(null);

    const { handOffSession } = await import("../src/auth-bridge");
    await handOffSession();

    expect(sendMessage).not.toHaveBeenCalled();
    expect(document.documentElement.hasAttribute("data-escpost-ack")).toBe(false);
  });

  it("ignores a malformed island rather than throwing on the page", async () => {
    seedPage("not json at all");

    const { handOffSession } = await import("../src/auth-bridge");
    await expect(handOffSession()).resolves.toBeUndefined();
    expect(sendMessage).not.toHaveBeenCalled();
  });

  it("ignores an island carrying no token", async () => {
    seedPage(JSON.stringify({ token: "" }));

    const { handOffSession } = await import("../src/auth-bridge");
    await handOffSession();

    expect(sendMessage).not.toHaveBeenCalled();
  });

  it("survives a worker that is not listening", async () => {
    sendMessage.mockRejectedValue(new Error("Could not establish connection."));
    seedPage(JSON.stringify({ token: TOKEN }));

    const { handOffSession } = await import("../src/auth-bridge");
    await expect(handOffSession()).resolves.toBeUndefined();
    expect(document.documentElement.hasAttribute("data-escpost-ack")).toBe(false);
  });
});
