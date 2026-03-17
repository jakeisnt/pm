import {
  DEFAULT_DEPTH,
  DEFAULT_ROOTS,
  GITHUB_REINDEX_INTERVAL_MS,
  MAX_RECENT,
  REINDEX_INTERVAL_MS,
} from "./constants.ts";
import { getSetting } from "./db/index.ts";

export interface SettingDef {
  description: string;
  default: string;
  parse: (v: string) => unknown;
  deviceLocal?: boolean;
}

export const SETTING_DEFS: Record<string, SettingDef> = {
  search_roots: {
    description: "Directories to scan for projects (JSON array)",
    default: JSON.stringify(DEFAULT_ROOTS),
    parse: (v) => JSON.parse(v) as string[],
    deviceLocal: true,
  },
  search_depth: {
    description: "Max depth for recursive project discovery",
    default: String(DEFAULT_DEPTH),
    parse: (v) => Number(v),
  },
  reindex_interval_ms: {
    description: "How often to reindex local projects (ms)",
    default: String(REINDEX_INTERVAL_MS),
    parse: (v) => Number(v),
  },
  github_reindex_interval_ms: {
    description: "How often to reindex GitHub projects (ms)",
    default: String(GITHUB_REINDEX_INTERVAL_MS),
    parse: (v) => Number(v),
  },
  max_recent: {
    description: "How many recent projects to show by default",
    default: String(MAX_RECENT),
    parse: (v) => Number(v),
  },
};

export function getSearchRoots(): string[] {
  const raw = getSetting("search_roots");
  return raw ? (JSON.parse(raw) as string[]) : DEFAULT_ROOTS;
}

export function getSearchDepth(): number {
  const raw = getSetting("search_depth");
  return raw ? Number(raw) : DEFAULT_DEPTH;
}

export function getReindexInterval(): number {
  const raw = getSetting("reindex_interval_ms");
  return raw ? Number(raw) : REINDEX_INTERVAL_MS;
}

export function getGithubReindexInterval(): number {
  const raw = getSetting("github_reindex_interval_ms");
  return raw ? Number(raw) : GITHUB_REINDEX_INTERVAL_MS;
}

export function getMaxRecent(): number {
  const raw = getSetting("max_recent");
  return raw ? Number(raw) : MAX_RECENT;
}
