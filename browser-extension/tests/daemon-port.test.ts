import { expect, test } from "vitest";
import { DaemonPortStore } from "../src/daemon-port";

class MemoryStorageArea {
  readonly values: Record<string, unknown> = {};

  async get(key: string): Promise<Record<string, unknown>> {
    return key in this.values ? { [key]: this.values[key] } : {};
  }

  async set(values: Record<string, unknown>): Promise<void> {
    Object.assign(this.values, values);
  }

  async remove(key: string): Promise<void> {
    delete this.values[key];
  }
}

class PausedRemoveStorageArea extends MemoryStorageArea {
  private releaseRemove: () => void = () => undefined;
  private readonly removeMayContinue = new Promise<void>((resolve) => { this.releaseRemove = resolve; });
  private resolveRemoveStarted: () => void = () => undefined;
  readonly removeStarted = new Promise<void>((resolve) => { this.resolveRemoveStarted = resolve; });

  override async remove(key: string): Promise<void> {
    this.resolveRemoveStarted();
    await this.removeMayContinue;
    await super.remove(key);
  }

  continueRemove(): void {
    this.releaseRemove();
  }
}

test("persists only the successful daemon base URL and can invalidate it", async () => {
  // Break caught: retaining a failed candidate or leaving a stale URL in
  // storage makes later operations target the wrong loopback daemon.
  const storage = new MemoryStorageArea();
  const ports = new DaemonPortStore(storage);

  await expect(ports.read()).resolves.toBeNull();
  await ports.remember("http://127.0.0.1:9004");

  await expect(ports.read()).resolves.toBe("http://127.0.0.1:9004");
  expect(storage.values).toEqual({ daemonBaseUrl: "http://127.0.0.1:9004" });

  await ports.invalidate();
  await expect(ports.read()).resolves.toBeNull();
  expect(storage.values).toEqual({});
});

test("forgets corrupt and out-of-range cached base URLs", async () => {
  // Break caught: trusting arbitrary extension storage sends printer traffic
  // to an unbounded port or a non-loopback origin after storage corruption.
  const storage = new MemoryStorageArea();
  const ports = new DaemonPortStore(storage);

  storage.values.daemonBaseUrl = "https://example.test:9000";
  await expect(ports.read()).resolves.toBeNull();
  expect(storage.values).toEqual({});

  storage.values.daemonBaseUrl = "http://127.0.0.1:9010";
  await expect(ports.read()).resolves.toBeNull();
  expect(storage.values).toEqual({});
});

test("does not erase a newer remembered URL while invalidating an old one", async () => {
  // Break caught: a late failure for an old port deleting a concurrently
  // discovered healthy port makes subsequent operations rediscover needlessly.
  const storage = new PausedRemoveStorageArea();
  storage.values.daemonBaseUrl = "http://127.0.0.1:9000";
  const ports = new DaemonPortStore(storage);

  const staleFailure = ports.invalidate("http://127.0.0.1:9000");
  await storage.removeStarted;
  const newlyDiscovered = ports.remember("http://127.0.0.1:9001");
  storage.continueRemove();
  await Promise.all([staleFailure, newlyDiscovered]);

  await expect(ports.read()).resolves.toBe("http://127.0.0.1:9001");
});
