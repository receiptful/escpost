import type { MutableOriginGrants } from "../grants";

const grantKeyPrefix = "escpost-origin:";

type StorageArea = {
  get(key: string): Promise<Record<string, unknown>>;
  set(items: Record<string, unknown>): Promise<void>;
  remove(key: string): Promise<void>;
};

type StorageChanges = {
  addListener(listener: (
    changes: Record<string, { oldValue?: unknown; newValue?: unknown }>,
    areaName: string,
  ) => void): void;
};

export class ChromeOriginGrants implements MutableOriginGrants {
  private readonly removedListeners = new Set<(patterns: string[]) => void>();

  constructor(
    private readonly storage: StorageArea,
    changes: StorageChanges,
  ) {
    changes.addListener((entries, areaName) => {
      if (areaName !== "local") return;
      const removed = Object.entries(entries).flatMap(([key, change]) => {
        if (!key.startsWith(grantKeyPrefix) || change.oldValue !== true || change.newValue === true) return [];
        try {
          return [decodeURIComponent(key.slice(grantKeyPrefix.length))];
        } catch {
          return [];
        }
      });
      if (removed.length === 0) return;
      for (const listener of this.removedListeners) listener(removed);
    });
  }

  async contains(pattern: string): Promise<boolean> {
    const key = grantKey(pattern);
    const values = await this.storage.get(key);
    return values[key] === true;
  }

  async request(pattern: string): Promise<boolean> {
    if (await this.contains(pattern)) return false;
    await this.storage.set({ [grantKey(pattern)]: true });
    return true;
  }

  async remove(pattern: string): Promise<boolean> {
    if (!await this.contains(pattern)) return false;
    await this.storage.remove(grantKey(pattern));
    return true;
  }

  onRemoved(listener: (patterns: string[]) => void): void {
    this.removedListeners.add(listener);
  }
}

function grantKey(pattern: string): string {
  return `${grantKeyPrefix}${encodeURIComponent(pattern)}`;
}
