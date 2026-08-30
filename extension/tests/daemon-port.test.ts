import { beforeEach, describe, expect, it, vi } from "vitest";
import { DAEMON_PORTS } from "../src/config";
import { baseFor, findDaemonBase, type PortStore } from "../src/daemon-port";

describe("finding escpost", () => {
  beforeEach(() => vi.restoreAllMocks());

  it("uses the first port when it answers", async () => {
    const store = memoryStore();
    const fetchMock = answeringOn(9000);

    await expect(findDaemonBase(store, fetchMock)).resolves.toBe(baseFor(9000));
  });

  it("finds escpost on a later port when something else holds the first", async () => {
    const store = memoryStore();
    const fetchMock = answeringOn(9002);

    await expect(findDaemonBase(store, fetchMock)).resolves.toBe(baseFor(9002));
  });

  it("remembers the port, so the next look costs one request", async () => {
    const store = memoryStore();
    await findDaemonBase(store, answeringOn(9003));

    const second = answeringOn(9003);
    await expect(findDaemonBase(store, second)).resolves.toBe(baseFor(9003));
    expect(second).toHaveBeenCalledTimes(1);
  });

  it("looks again when the remembered port has gone quiet", async () => {
    const store = memoryStore();
    await findDaemonBase(store, answeringOn(9003));

    // escpost restarted and took a different port.
    await expect(findDaemonBase(store, answeringOn(9001))).resolves.toBe(baseFor(9001));
  });

  it("is null when nothing answers, rather than guessing", async () => {
    const store = memoryStore();
    const silence = vi.fn(async () => {
      throw new TypeError("Failed to fetch");
    });

    await expect(findDaemonBase(store, silence as unknown as typeof fetch)).resolves.toBeNull();
    expect(silence).toHaveBeenCalledTimes(DAEMON_PORTS.length);
  });

  it("ignores a remembered port outside the range", async () => {
    const store = memoryStore({ daemonPort: 5432 });
    const fetchMock = answeringOn(9000);

    await expect(findDaemonBase(store, fetchMock)).resolves.toBe(baseFor(9000));
    for (const call of fetchMock.mock.calls) {
      expect(String(call[0])).not.toContain("5432");
    }
  });
});

function memoryStore(initial: Record<string, unknown> = {}): PortStore {
  const data = { ...initial };
  return {
    async get(key) {
      return key in data ? { [key]: data[key] } : {};
    },
    async set(items) {
      Object.assign(data, items);
    },
  };
}

function answeringOn(port: number) {
  return vi.fn(async (url: unknown) => {
    if (!String(url).includes(`:${port}/`)) throw new TypeError("Failed to fetch");
    return new Response("ok", { status: 200 });
  }) as unknown as ReturnType<typeof vi.fn> & typeof fetch;
}
