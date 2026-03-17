import type { ProjectScope } from "../types.ts";
import { getDb } from "./db/index.ts";

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

export async function resolveScope(cwd?: string): Promise<ProjectScope> {
  const dir = cwd ?? process.cwd();

  const projects = await getDb()
    .selectFrom("projects")
    .select(["path", "scope"])
    .where("source", "=", "local")
    .where("deleted_at", "is", null)
    .execute();

  let bestScope: string | undefined;
  let bestLen = 0;
  for (const p of projects) {
    if (dir.startsWith(p.path) && p.path.length > bestLen) {
      bestScope = p.scope;
      bestLen = p.path.length;
    }
  }

  if (bestScope) {
    return bestScope as ProjectScope;
  }

  return inferProjectScope({ path: dir });
}
