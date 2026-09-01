import { DAEMON_HOST, DAEMON_PORTS } from "./config";

type StorageValues = Record<string, unknown>;

export type DaemonStorageArea = {
  get(key: string): Promise<StorageValues>;
  set(values: StorageValues): Promise<void>;
  remove(key: string): Promise<void>;
};

const daemonBaseUrlKey = "daemonBaseUrl";

export class DaemonPortStore {
  private writeQueue = Promise.resolve();

  constructor(private readonly storage: DaemonStorageArea = chrome.storage.local) {}

  async read(): Promise<string | null> {
    const values = await this.storage.get(daemonBaseUrlKey);
    const value = values[daemonBaseUrlKey];
    if (isDaemonBaseUrl(value)) return value;
    if (daemonBaseUrlKey in values) await this.removeIfCurrent(value, true);
    return null;
  }

  async remember(baseUrl: string): Promise<void> {
    await this.exclusive(() => this.storage.set({ [daemonBaseUrlKey]: baseUrl }));
  }

  async invalidate(expectedBaseUrl?: unknown): Promise<void> {
    await this.removeIfCurrent(expectedBaseUrl, expectedBaseUrl !== undefined);
  }

  private async removeIfCurrent(expectedBaseUrl: unknown, conditional: boolean): Promise<void> {
    await this.exclusive(async () => {
      const values = await this.storage.get(daemonBaseUrlKey);
      if (!conditional || values[daemonBaseUrlKey] === expectedBaseUrl) {
        await this.storage.remove(daemonBaseUrlKey);
      }
    });
  }

  private async exclusive<T>(operation: () => Promise<T>): Promise<T> {
    const previous = this.writeQueue;
    let release: () => void = () => undefined;
    this.writeQueue = new Promise<void>((resolve) => { release = resolve; });
    await previous;
    try {
      return await operation();
    } finally {
      release();
    }
  }
}

function isDaemonBaseUrl(value: unknown): value is string {
  return typeof value === "string" && DAEMON_PORTS.some((port) => value === `http://${DAEMON_HOST}:${port}`);
}
