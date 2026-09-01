type StorageValues = Record<string, unknown>;

export type DaemonStorageArea = {
  get(key: string): Promise<StorageValues>;
  set(values: StorageValues): Promise<void>;
  remove(key: string): Promise<void>;
};

const daemonBaseUrlKey = "daemonBaseUrl";

export class DaemonPortStore {
  constructor(private readonly storage: DaemonStorageArea = chrome.storage.local) {}

  async read(): Promise<string | null> {
    const values = await this.storage.get(daemonBaseUrlKey);
    const value = values[daemonBaseUrlKey];
    return typeof value === "string" ? value : null;
  }

  async remember(baseUrl: string): Promise<void> {
    await this.storage.set({ [daemonBaseUrlKey]: baseUrl });
  }

  async invalidate(): Promise<void> {
    await this.storage.remove(daemonBaseUrlKey);
  }
}
