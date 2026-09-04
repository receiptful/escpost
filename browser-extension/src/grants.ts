export type OriginGrants = {
  contains(pattern: string): Promise<boolean>;
  onRemoved(listener: (patterns: string[]) => void): void;
};

export type MutableOriginGrants = OriginGrants & {
  request(pattern: string): Promise<boolean>;
  remove(pattern: string): Promise<boolean>;
};
