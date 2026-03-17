export type ProjectSource = "local" | "github";
export type ProjectScope = "personal" | "work" | "global";

export interface Project {
  path: string;
  name: string;
  lastOpened?: number; // unix ms
  source: ProjectSource;
  githubFullName?: string; // e.g. "owner/repo"
  scope?: ProjectScope;
}

export interface RootsConfig {
  roots: string[];
  maxDepth: number;
}

export interface RecentEntry {
  path: string;
  name: string;
  lastOpened: number; // unix ms
}

export interface SelectOptions {
  openCmd?: string | undefined;
  printPath: boolean;
  silent?: boolean | undefined;
  name?: string | undefined;
}
