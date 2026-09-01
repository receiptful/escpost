import { afterEach, expect, test, vi } from "vitest";
import { EscpostError } from "../src/errors";
import type { PageRequest } from "../src/protocol";
import { PageTransport, type PageWindow } from "../src/transport";

class FakePageWindow implements PageWindow {
  readonly posted: PageRequest[] = [];
  readonly listeners: Array<(event: MessageEvent) => void> = [];

  addEventListener(_type: "message", listener: (event: MessageEvent) => void): void {
    this.listeners.push(listener);
  }

  postMessage(message: PageRequest): void {
    this.posted.push(message);
  }

  reply(message: unknown): void {
    for (const listener of this.listeners) {
      listener({ data: message, source: this } as unknown as MessageEvent);
    }
  }
}

afterEach(() => vi.useRealTimers());

test("correlates replies through one message listener", async () => {
  // Break caught: allocating a listener per request or resolving the wrong
  // request would leak listeners or deliver another operation's reply.
  const page = new FakePageWindow();
  const transport = new PageTransport(page);
  const first = transport.request<string>("daemon.health", null, 2_000);
  const second = transport.request<string>("printers.list", {}, 30_000);

  expect(page.listeners).toHaveLength(1);
  expect(page.posted).toHaveLength(2);
  expect(page.posted[0]).toMatchObject({
    source: "escpost-page",
    protocol: 1,
    op: "daemon.health",
  });
  expect(page.posted[0].id).not.toBe(page.posted[1].id);

  page.reply({
    source: "escpost-extension",
    id: page.posted[1].id,
    ok: true,
    data: "second",
  });
  page.reply({
    source: "escpost-extension",
    id: page.posted[0].id,
    ok: true,
    data: "first",
  });

  await expect(second).resolves.toBe("second");
  await expect(first).resolves.toBe("first");
});

test("rejects a matching extension failure as an EscpostError", async () => {
  // Break caught: treating an extension rejection as a successful payload
  // prevents callers from branching on the documented printer error code.
  const page = new FakePageWindow();
  const transport = new PageTransport(page);
  const result = transport.request("print.raw", {}, 20_000);

  page.reply({
    source: "escpost-extension",
    id: page.posted[0].id,
    ok: false,
    error: { code: "PRINT_FAILED", message: "The printer disconnected." },
  });

  await expect(result).rejects.toMatchObject({
    name: "EscpostError",
    code: "PRINT_FAILED",
    message: "The printer disconnected.",
  } satisfies Partial<EscpostError>);
});

test("ignores unrelated and malformed replies until the matching valid reply arrives", async () => {
  // Break caught: consuming a pending request for a wrong source, wrong ID, or
  // non-boolean ok field lets unrelated page messages fabricate SDK success.
  const page = new FakePageWindow();
  const transport = new PageTransport(page);
  const result = transport.request<string>("daemon.health", null, 2_000);
  let settled = false;
  void result.then(
    () => {
      settled = true;
    },
    () => {
      settled = true;
    },
  );
  const request = page.posted[0];

  page.reply({ source: "another-page", id: request.id, ok: true, data: "wrong source" });
  page.reply({ source: "escpost-extension", id: request.id + 1, ok: true, data: "wrong id" });
  page.reply({ source: "escpost-extension", id: request.id, data: "missing ok" });
  page.reply({ source: "escpost-extension", id: request.id, ok: "true", data: "wrong ok" });

  await Promise.resolve();
  expect(settled).toBe(false);

  page.reply({ source: "escpost-extension", id: request.id, ok: true, data: "healthy" });
  await expect(result).resolves.toBe("healthy");
});

test("times out a silent health relay as unavailable", async () => {
  // Break caught: leaving a silent extension request pending means availability
  // checks can hang a page forever instead of resolving within two seconds.
  vi.useFakeTimers();
  const page = new FakePageWindow();
  const transport = new PageTransport(page);
  const result = transport.request("daemon.health", null, 2_000);

  vi.advanceTimersByTime(2_000);

  await expect(result).rejects.toMatchObject({
    code: "EXTENSION_UNAVAILABLE",
  } satisfies Partial<EscpostError>);
});
