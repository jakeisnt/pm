import type { ProjectScope } from "../types.ts";
import { getSetting } from "./db/settings.ts";

/** GitHub orgs whose projects are considered "work" scope (fallback defaults). */
const DEFAULT_WORK_ORGS: string[] = [];

/** Path prefixes that indicate work scope (fallback defaults). */
const DEFAULT_WORK_PATH_PREFIXES: string[] = [];

function parseCommaSeparated(value: string): string[] {
  return value
    .split(",")
    .map((s) => s.trim())
    .filter((s) => s.length > 0);
}

function getWorkOrgs(): string[] {
  const setting = getSetting("work_orgs");
  if (setting !== undefined) return parseCommaSeparated(setting);
  return DEFAULT_WORK_ORGS;
}

function getWorkPathPrefixes(): string[] {
  const setting = getSetting("work_path_prefixes");
  if (setting !== undefined) return parseCommaSeparated(setting);
  return DEFAULT_WORK_PATH_PREFIXES;
}

export function inferProjectScope(opts: { githubFullName?: string; path?: string }): ProjectScope {
  if (opts.githubFullName) {
    const org = opts.githubFullName.split("/")[0]?.toLowerCase();
    if (org && getWorkOrgs().includes(org)) return "work";
  }
  if (opts.path) {
    for (const prefix of getWorkPathPrefixes()) {
      if (opts.path.startsWith(prefix)) return "work";
    }
  }
  return "personal";
}
