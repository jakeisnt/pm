import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import {
  DEFAULT_DEPTH,
  DEFAULT_ON_ERROR,
  DEFAULT_ON_MISSING_COMMAND,
  DEFAULT_ROOTS,
  GITHUB_REINDEX_INTERVAL_MS,
  MAX_RECENT,
  REINDEX_INTERVAL_MS,
} from "./defaults.ts";

export interface PmConfig {
  searchRoots: string[];
  searchDepth: number;
  reindexIntervalMs: number;
  githubReindexIntervalMs: number;
  maxRecent: number;
  workOrgs: string[];
  workPathPrefixes: string[];
  onMissingCommand: string;
  onError: string;
}

const CONFIG_PATH = join(import.meta.dirname, "../../../config.json");

const DEFAULTS: PmConfig = {
  searchRoots: DEFAULT_ROOTS,
  searchDepth: DEFAULT_DEPTH,
  reindexIntervalMs: REINDEX_INTERVAL_MS,
  githubReindexIntervalMs: GITHUB_REINDEX_INTERVAL_MS,
  maxRecent: MAX_RECENT,
  workOrgs: [],
  workPathPrefixes: [],
  onMissingCommand: DEFAULT_ON_MISSING_COMMAND,
  onError: DEFAULT_ON_ERROR,
};

let cached: PmConfig | undefined;

function readConfigFile(): Partial<PmConfig> {
  try {
    return JSON.parse(readFileSync(CONFIG_PATH, "utf-8")) as Partial<PmConfig>;
  } catch {
    return {};
  }
}

export function loadConfig(): PmConfig {
  if (cached) return cached;
  const raw = readConfigFile();
  cached = { ...DEFAULTS, ...raw };
  return cached;
}

/** Reset cached config (for use after writes). */
export function invalidateConfig(): void {
  cached = undefined;
}

export function setConfigValue(key: string, value: string): void {
  const raw = readConfigFile();
  const def = DEFAULTS[key as keyof PmConfig];
  if (def === undefined) {
    (raw as Record<string, unknown>)[key] = value;
  } else if (typeof def === "number") {
    (raw as Record<string, unknown>)[key] = Number(value);
  } else if (Array.isArray(def)) {
    (raw as Record<string, unknown>)[key] = JSON.parse(value) as unknown;
  } else {
    (raw as Record<string, unknown>)[key] = value;
  }
  writeFileSync(CONFIG_PATH, `${JSON.stringify(raw, null, 2)}\n`);
  invalidateConfig();
}

export function deleteConfigValue(key: string): void {
  const raw = readConfigFile();
  delete (raw as Record<string, unknown>)[key];
  writeFileSync(CONFIG_PATH, `${JSON.stringify(raw, null, 2)}\n`);
  invalidateConfig();
}

export function getConfigPath(): string {
  return CONFIG_PATH;
}
