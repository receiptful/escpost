import { expect, test } from "vitest";
import { escpost } from "../src/index";
import type { PageRequest } from "../src/protocol";
import type { PageWindow } from "../src/transport";

class FakePageWindow implements PageWindow {
  readonly posted: PageRequest[] = [];
  private readonly listeners: Array<(event: MessageEvent) => void> = [];

  addEventListener(_type: "message", listener: (event: MessageEvent) => void): void {
    this.listeners.push(listener);
  }

  postMessage(message: PageRequest): void {
    this.posted.push(message);
  }

  reply(request: PageRequest, reply: { ok: true; data: unknown } | { ok: false; error: unknown }): void {
    for (const listener of this.listeners) {
      listener({
        data: { source: "escpost-extension", id: request.id, ...reply },
        source: this,
      } as unknown as MessageEvent);
    }
  }
}

function pageRelay(): FakePageWindow {
  const existing = (globalThis as { window?: Partial<FakePageWindow> }).window;
  if (Array.isArray(existing?.posted) && typeof existing.reply === "function") {
    return existing as FakePageWindow;
  }
  const page = new FakePageWindow();
  Object.assign(globalThis, { window: page });
  return page;
}

test("returns false when the health relay reports any error", async () => {
  // Break caught: surfacing health failures from isAvailable forces callers to
  // handle extension and daemon states that this boolean probe promises to hide.
  const page = pageRelay();
  const before = page.posted.length;
  const available = escpost.isAvailable();
  const request = page.posted[before];

  expect(request?.op).toBe("daemon.health");
  page.reply(request, { ok: false, error: { code: "DAEMON_UNAVAILABLE", message: "The daemon is offline." } });

  await expect(available).resolves.toBe(false);
});
