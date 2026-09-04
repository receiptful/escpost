import type { MutableOriginGrants } from "../grants";

type Permissions = {
  contains(details: { origins: string[] }): Promise<boolean>;
  request(details: { origins: string[] }): Promise<boolean>;
  remove(details: { origins: string[] }): Promise<boolean>;
  onRemoved: { addListener(listener: (details: { origins?: string[] }) => void): void };
};

export class FirefoxOriginGrants implements MutableOriginGrants {
  constructor(private readonly permissions: Permissions) {}

  contains(pattern: string): Promise<boolean> {
    return this.permissions.contains({ origins: [pattern] });
  }

  request(pattern: string): Promise<boolean> {
    return this.permissions.request({ origins: [pattern] });
  }

  remove(pattern: string): Promise<boolean> {
    return this.permissions.remove({ origins: [pattern] });
  }

  onRemoved(listener: (patterns: string[]) => void): void {
    this.permissions.onRemoved.addListener((details) => listener(details.origins ?? []));
  }
}
