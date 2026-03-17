import { existsSync } from "node:fs";
import { cloneGithubRepo, isGithubPlaceholder } from "@uln/repo";
import { createSpinner } from "nanospinner";
import type { Project, RootsConfig, SelectOptions } from "../types.ts";
import {
  getCachedProjects,
  getLocalProjectByGithubName,
  getRecentProjects,
  promoteToLocal,
  touchProject,
  touchRecentProject,
  upsertProject,
} from "./db/index.ts";
import { findProjects } from "./find-projects.ts";
import { fuzzySelectProject } from "./fuzzy.ts";
import { forceReindex, indexGithubRepos } from "./indexer.ts";
import { log } from "./log.ts";
import { runCmd, spawnShell } from "./subprocess.ts";

export async function runProjectSelect(
  roots: string[],
  depth: number,
  options: SelectOptions & { cloneDir?: string | undefined; json?: boolean | undefined },
): Promise<string> {
  const config: RootsConfig = { roots, maxDepth: depth };

  if (options.json) {
    let localProjects = await getCachedProjects("local");
    if (localProjects.length === 0) {
      localProjects = findProjects(config).map((p) => ({ ...p, source: "local" as const }));
    }
    const githubProjects = await getCachedProjects("github");
    const allProjects = [...localProjects, ...githubProjects];
    const data = {
      projects: allProjects.map((p) => ({
        path: p.path,
        name: p.name,
        source: p.source,
        ...(p.githubFullName ? { githubFullName: p.githubFullName } : {}),
      })),
    };
    process.stdout.write(`${JSON.stringify(data, null, 2)}\n`);
    return "";
  }

  // Load cached projects first — reindex happens after selection to avoid
  // blocking on the synchronous filesystem walk in findProjects()
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

  const recentEntries = await getRecentProjects();
  const recentMap = new Map(recentEntries.map((e) => [e.path, e.lastOpened]));
  const merged = allProjects.sort((a, b) => {
    if (a.source !== b.source) return a.source === "local" ? -1 : 1;
    const aRecent = recentMap.get(a.path) ?? 0;
    const bRecent = recentMap.get(b.path) ?? 0;
    if (aRecent !== bRecent) return bRecent - aRecent;
    return a.name.localeCompare(b.name);
  });

  let selected: Project | undefined;
  if (options.name) {
    const match = await findProjectByName(options.name);
    if (match) {
      const proj = allProjects.find((p) => p.path === match);
      if (proj) {
        selected = proj;
      }
    }
    if (!selected) {
      log.warn(`No project matching "${options.name}" found. Falling back to fuzzy select.`);
    }
  }
  if (!selected) {
    selected = await fuzzySelectProject(merged);
  }

  let targetPath = selected.path;

  if (selected.source === "github" && isGithubPlaceholder(selected.path)) {
    const fullName = selected.githubFullName ?? selected.name;

    const existing = await getLocalProjectByGithubName(fullName);
    if (existing && existsSync(existing.path)) {
      targetPath = existing.path;
    } else {
      const cloneSpinner = createSpinner(`Cloning ${fullName}...`).start();
      const cloneDir = options.cloneDir || roots[0];
      if (!cloneDir) throw new Error("No clone directory configured");
      const cloneResult = cloneGithubRepo(fullName, cloneDir);
      targetPath = cloneResult.path;
      await promoteToLocal(fullName, targetPath);
      cloneSpinner.success({
        text: cloneResult.alreadyExisted ? `Found local clone of ${fullName}` : `Cloned ${fullName}`,
      });
    }
  }

  await touchRecentProject(targetPath, selected.name);
  await touchProject(targetPath);
  await upsertProject({ ...selected, path: targetPath, source: "local" });

  if (options.silent) {
    // Return path without any side effects
  } else if (options.printPath) {
    process.stdout.write(targetPath);
  } else if (options.openCmd) {
    runCmd(options.openCmd, [targetPath], targetPath);
  } else {
    spawnShell(targetPath);
  }

  // Reindex in background after selection — forceReindex uses sync FS APIs
  // (readdirSync/readFileSync) which block the event loop before the first await,
  // so calling it before selection would delay fzf from appearing.
  forceReindex(config).catch(() => {});
  indexGithubRepos().catch(() => {});

  return targetPath;
}

export async function findProjectByName(name: string): Promise<string | null> {
  const projects = await getCachedProjects();
  const lower = name.toLowerCase();

  const exact = projects.find((p) => p.name.toLowerCase() === lower);
  if (exact) return exact.path;

  const matches = projects.filter((p) => p.name.toLowerCase().includes(lower));
  if (matches.length === 1) return matches[0]?.path ?? null;

  const ghMatch = projects.find((p) => p.githubFullName?.toLowerCase() === lower);
  if (ghMatch) return ghMatch.path;

  return null;
}
