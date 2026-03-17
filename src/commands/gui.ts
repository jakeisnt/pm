import { existsSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { cloneGithubRepo, isGithubPlaceholder } from "@uln/repo";
import { createSpinner } from "nanospinner";
import {
  getCachedProjects,
  getLocalProjectByGithubName,
  getRecentProjects,
  promoteToLocal,
  touchProject,
  touchRecentProject,
  upsertProject,
} from "../lib/db/index.ts";
import { findProjects } from "../lib/find-projects.ts";
import { fuzzyFilter } from "../lib/fuzzy-match.ts";
import { forceReindex, indexGithubRepos } from "../lib/indexer.ts";
import { log } from "../lib/log.ts";
import { SelectionCancelledError } from "../lib/prompt.ts";
import { getSearchDepth, getSearchRoots } from "../lib/settings.ts";
import { runCmd, spawnShell } from "../lib/subprocess.ts";
import type { TuiListItem } from "../lib/tui.ts";
import { tuiSelect } from "../lib/tui.ts";
import type { Project, RootsConfig, SelectOptions } from "../types.ts";

function shortenPath(path: string): string {
  const home = homedir();
  if (path.startsWith(home)) return `~${path.slice(home.length)}`;
  return path;
}

function projectToListItem(project: Project, _query: string): TuiListItem {
  if (project.source === "github") {
    return {
      icon: "☁",
      iconColor: "cyan",
      label: project.githubFullName ?? project.name,
      detail: "",
      highlights: [],
    };
  }
  return {
    icon: "●",
    iconColor: "green",
    label: project.name,
    detail: shortenPath(project.path),
    highlights: [],
  };
}

function filterProjects(projects: Project[], query: string): { item: Project; highlights: number[] }[] {
  const results = fuzzyFilter(projects, query, (p) => (p.source === "github" ? (p.githubFullName ?? p.name) : p.name));
  return results.map((r) => ({ item: r.item, highlights: r.indices }));
}

export async function runGui(options: SelectOptions & { cloneDir?: string | undefined }): Promise<string> {
  const roots = getSearchRoots();
  const depth = getSearchDepth();
  const config: RootsConfig = { roots, maxDepth: depth };

  // Load projects (shared with CLI select)
  let localProjects = await getCachedProjects("local");
  if (localProjects.length === 0) {
    localProjects = findProjects(config).map((p) => ({ ...p, source: "local" as const }));
  }

  const githubProjects = await getCachedProjects("github");
  const localGithubNames = new Set(
    localProjects.filter((p) => p.githubFullName).map((p) => p.githubFullName as string),
  );
  const remoteOnly = githubProjects.filter((p) => p.githubFullName && !localGithubNames.has(p.githubFullName));
  const allProjects = [...localProjects, ...remoteOnly];

  // Sort by recency (shared logic)
  const recentEntries = await getRecentProjects();
  const recentMap = new Map(recentEntries.map((e) => [e.path, e.lastOpened]));
  const merged = allProjects.sort((a, b) => {
    if (a.source !== b.source) return a.source === "local" ? -1 : 1;
    const aRecent = recentMap.get(a.path) ?? 0;
    const bRecent = recentMap.get(b.path) ?? 0;
    if (aRecent !== bRecent) return bRecent - aRecent;
    return a.name.localeCompare(b.name);
  });

  if (merged.length === 0) {
    log.warn("No projects found. Run `p` first to index projects.");
    return "";
  }

  // TUI fuzzy select
  const result = await tuiSelect(merged, {
    toListItem: projectToListItem,
    filter: filterProjects,
    placeholder: "Search projects…",
    emptyMessage: "No matching projects",
  });

  if (result.cancelled) {
    throw new SelectionCancelledError();
  }

  const selected = result.item;
  let targetPath = selected.path;

  // Handle GitHub placeholder clone (shared with CLI)
  if (selected.source === "github" && isGithubPlaceholder(selected.path)) {
    const fullName = selected.githubFullName ?? selected.name;

    const existing = await getLocalProjectByGithubName(fullName);
    if (existing && existsSync(existing.path)) {
      targetPath = existing.path;
    } else {
      const cloneDir = options.cloneDir ?? roots[0];
      if (!cloneDir) throw new Error("No clone directory configured");
      const [owner, repo] = fullName.split("/");
      if (!owner || !repo) throw new Error(`Invalid GitHub full name: ${fullName}`);
      targetPath = join(cloneDir, owner, repo);
      await promoteToLocal(fullName, targetPath);
      const cloneSpinner = createSpinner(`Cloning ${fullName}...`).start();
      const cloneResult = cloneGithubRepo(fullName, cloneDir);
      cloneSpinner.success({
        text: cloneResult.alreadyExisted ? `Found local clone of ${fullName}` : `Cloned ${fullName}`,
      });
    }
  }

  // Record selection (shared)
  await touchRecentProject(targetPath, selected.name);
  await touchProject(targetPath);
  await upsertProject({ ...selected, path: targetPath, source: "local" });

  // Execute side effect
  if (options.silent) {
    // No side effects
  } else if (options.printPath) {
    process.stdout.write(targetPath);
  } else if (options.openCmd) {
    runCmd(options.openCmd, [targetPath], targetPath);
  } else {
    spawnShell(targetPath);
  }

  // Background reindex (shared)
  forceReindex(config).catch(() => {});
  indexGithubRepos().catch(() => {});

  return targetPath;
}
