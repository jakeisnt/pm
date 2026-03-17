import type { ProjectScope } from "../types.ts";

/** GitHub orgs whose projects are considered "work" scope. */
const WORK_ORGS = ["improvin"];

/** Path prefixes that indicate work scope. */
const WORK_PATH_PREFIXES = ["/Users/jake/Documents/improvin"];

export function inferProjectScope(opts: { githubFullName?: string; path?: string }): ProjectScope {
  if (opts.githubFullName) {
    const org = opts.githubFullName.split("/")[0]?.toLowerCase();
    if (org && WORK_ORGS.includes(org)) return "work";
  }
  if (opts.path) {
    for (const prefix of WORK_PATH_PREFIXES) {
      if (opts.path.startsWith(prefix)) return "work";
    }
  }
  return "personal";
}
