import { expect, test, vi } from "vitest";
import { ChromeOriginGrants } from "../src/chrome/grants";

type Change = { oldValue?: unknown; newValue?: unknown };

class FakeStorageArea {
  readonly values = new Map<string, unknown>();

  async get(key: string): Promise<Record<string, unknown>> {
    return this.values.has(key) ? { [key]: this.values.get(key) } : {};
  }

  async set(items: Record<string, unknown>): Promise<void> {
    for (const [key, value] of Object.entries(items)) this.values.set(key, value);
  }

  async remove(key: string): Promise<void> {
    this.values.delete(key);
  }
}

class FakeStorageChanges {
  listener?: (changes: Record<string, Change>, areaName: string) => void;

  addListener(listener: (changes: Record<string, Change>, areaName: string) => void): void {
    this.listener = listener;
  }
}

test("stores independent exact-origin grants without browser host permissions", async () => {
  const storage = new FakeStorageArea();
  const changes = new FakeStorageChanges();
  const grants = new ChromeOriginGrants(storage, changes);

  await expect(grants.request("https://shop.example/*")).resolves.toBe(true);
  await expect(grants.request("http://127.0.0.1:8081/*")).resolves.toBe(true);
  await expect(grants.contains("https://shop.example/*")).resolves.toBe(true);
  await expect(grants.contains("http://127.0.0.1:8081/*")).resolves.toBe(true);
  expect(storage.values.size).toBe(2);
});

test("fails closed for absent and malformed stored grants", async () => {
  const storage = new FakeStorageArea();
  const grants = new ChromeOriginGrants(storage, new FakeStorageChanges());

  await expect(grants.contains("https://missing.example/*")).resolves.toBe(false);
  await grants.request("https://shop.example/*");
  const [key] = [...storage.values.keys()];
  storage.values.set(key!, "true");
  await expect(grants.contains("https://shop.example/*")).resolves.toBe(false);
});

test("removes one origin and reports only that revoked pattern", async () => {
  const storage = new FakeStorageArea();
  const changes = new FakeStorageChanges();
  const grants = new ChromeOriginGrants(storage, changes);
  const removed = vi.fn();
  grants.onRemoved(removed);
  await grants.request("https://shop.example/*");
  await grants.request("https://office.example/*");
  const [shopKey] = [...storage.values.keys()].filter((key) => key.includes(encodeURIComponent("https://shop.example/*")));

  await expect(grants.remove("https://shop.example/*")).resolves.toBe(true);
  changes.listener?.({ [shopKey!]: { oldValue: true } }, "local");

  await expect(grants.contains("https://shop.example/*")).resolves.toBe(false);
  await expect(grants.contains("https://office.example/*")).resolves.toBe(true);
  expect(removed).toHaveBeenCalledWith(["https://shop.example/*"]);
});

test("ignores unrelated storage areas and non-grant keys", () => {
  const changes = new FakeStorageChanges();
  const grants = new ChromeOriginGrants(new FakeStorageArea(), changes);
  const removed = vi.fn();
  grants.onRemoved(removed);

  changes.listener?.({ "escpost-origin:https%3A%2F%2Fshop.example%2F*": { oldValue: true } }, "sync");
  changes.listener?.({ "escpost-daemon-port": { oldValue: 9000 } }, "local");

  expect(removed).not.toHaveBeenCalled();
});
