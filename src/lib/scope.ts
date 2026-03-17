import type { ProjectScope } from "../types.ts";
import { loadConfig } from "./config/index.ts";

export function inferProjectScope(opts: { githubFullName?: string; path?: string }): ProjectScope {
  const config = loadConfig();
  if (opts.githubFullName) {
    const org = opts.githubFullName.split("/")[0]?.toLowerCase();
    if (org && config.workOrgs.includes(org)) return "work";
  }
  if (opts.path) {
    for (const prefix of config.workPathPrefixes) {
      if (opts.path.startsWith(prefix)) return "work";
    }
  }
  return "personal";
}
