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
