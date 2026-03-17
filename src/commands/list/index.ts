import pc from "picocolors";
import { getCachedProjects } from "../../lib/db/index.ts";
import { log } from "../../lib/log.ts";
import type { Project } from "../../types.ts";

export async function runProjectList(opts: { source?: string; scope?: string; json?: boolean }): Promise<void> {
  const sourceFilter = opts.source as "local" | "github" | undefined;
  const projects = await getCachedProjects(sourceFilter);

  const filtered = opts.scope ? projects.filter((p) => p.scope === opts.scope) : projects;

  if (filtered.length === 0) {
    log.dim("No tracked projects found.");
    return;
  }

  if (opts.json) {
    log.raw(JSON.stringify(filtered, null, 2));
    return;
  }

  const byScope = new Map<string, Project[]>();
  for (const p of filtered) {
    const scope = p.scope ?? "unscoped";
    const list = byScope.get(scope) ?? [];
    list.push(p);
    byScope.set(scope, list);
  }

  log.blank();
  log.phase(`Tracked Projects (${filtered.length})`);

  for (const [scope, scopeProjects] of byScope) {
    log.blank();
    log.item(pc.bold(scope.charAt(0).toUpperCase() + scope.slice(1)));

    for (const p of scopeProjects) {
      const isLocal = p.source === "local";
      const icon = isLocal ? pc.green("●") : pc.blue("☁");
      const ghLabel = p.githubFullName ? pc.dim(` (${p.githubFullName})`) : "";
      const pathLabel = isLocal ? pc.dim(` ${p.path}`) : "";
      log.detail(`${icon} ${pc.cyan(p.name)}${ghLabel}${pathLabel}`);
    }
  }

  log.blank();
  const localCount = filtered.filter((p) => p.source === "local").length;
  const ghCount = filtered.filter((p) => p.source === "github").length;
  log.dim(`${localCount} local, ${ghCount} GitHub-only`);
}
