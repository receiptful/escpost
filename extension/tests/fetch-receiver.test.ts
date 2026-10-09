import { afterEach, describe, expect, it, vi } from "vitest";
import { DaemonClient } from "../src/daemon";
import { ReceiptfulClient } from "../src/receiptful";

/**
 * The defect this pins, found by A/B against the spike extension in real Chrome:
 *
 *   this.#fetch = fetch;          // in the constructor
 *   await this.#fetch(url, init); // a METHOD call -> receiver is the instance
 *
 * WebIDL requires `fetch` to be invoked with the global as its receiver. Calling
 * it as a method of a class instance throws
 *   TypeError: Failed to execute 'fetch' on 'Window': Illegal invocation
 * which DaemonClient then swallowed as a transport error, retried three times,
 * and reported as DAEMON_NOT_RUNNING — against a daemon that was running fine.
 *
 * Every other test injects a plain mock function, and a plain function does not
 * care about its receiver, so the whole suite passed while both HTTP clients
 * were dead in the browser. These tests inject nothing: they exercise the
 * DEFAULT argument, against a global that enforces the same rule the browser does.
 */
const realFetch = globalThis.fetch;

afterEach(() => {
  globalThis.fetch = realFetch;
});

function installPickyFetch(): { urls: string[] } {
  const urls: string[] = [];
  function picky(this: unknown, url: string): Promise<Response> {
    // Exactly what Chrome does: anything but the global is an illegal receiver.
    if (this !== undefined && this !== globalThis) {
      throw new TypeError("Failed to execute 'fetch' on 'Window': Illegal invocation");
    }
    urls.push(String(url));
    return Promise.resolve(
      new Response('{"printers":[]}', { status: 200, headers: { "Content-Type": "application/json" } }),
    );
  }
  globalThis.fetch = picky as unknown as typeof fetch;
  return { urls };
}

describe("the HTTP clients call the global fetch with a legal receiver", () => {
  it("DaemonClient reaches the daemon using its default fetch", async () => {
    const seen = installPickyFetch();
    const client = new DaemonClient("http://127.0.0.1:9000", undefined, { attempts: 1, backoffMs: 0 });

    await expect(client.printers()).resolves.toEqual([]);
    expect(seen.urls).toEqual(["http://127.0.0.1:9000/api/printers/list"]);
  });

  it("ReceiptfulClient reaches the server using its default fetch", async () => {
    const seen = installPickyFetch();
    const client = new ReceiptfulClient("https://api.receiptful.io");

    await expect(client.account("rfx_1")).resolves.toMatchObject({ email: undefined });
    expect(seen.urls).toEqual(["https://api.receiptful.io/v1/extension/account"]);
  });

  it("an injected fetch is still called, and is not required to accept a receiver", async () => {
    // The existing suite depends on this: mocks stay plain functions.
    const injected = vi.fn(async () => new Response('{"printers":[]}', { status: 200, headers: { "Content-Type": "application/json" } }));
    const client = new DaemonClient("http://127.0.0.1:9000", injected as unknown as typeof fetch);

    await expect(client.printers()).resolves.toEqual([]);
    expect(injected).toHaveBeenCalledTimes(1);
  });
});
