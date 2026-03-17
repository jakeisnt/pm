import { existsSync } from "node:fs";
import { rm } from "node:fs/promises";
import { basename, resolve } from "node:path";
import pc from "picocolors";
import { getCachedProjects, getRecentProjects, removeProject } from "../../lib/db/index.ts";
import { fuzzySelectProject } from "../../lib/fuzzy.ts";
import { log } from "../../lib/log.ts";
import { askLine } from "../../lib/prompt.ts";

async function selectProjectToRemove(pathArg?: string): Promise<{ path: string; name: string }> {
  if (pathArg) {
    const resolved = resolve(pathArg);
    return { path: resolved, name: basename(resolved) };
  }

  const localProjects = await getCachedProjects("local");
  if (localProjects.length === 0) {
    throw new Error("No indexed projects found. Pass a path explicitly.");
  }

  const recentEntries = await getRecentProjects();
  const recentMap = new Map(recentEntries.map((e) => [e.path, e.lastOpened]));
  const sorted = [...localProjects].sort((a, b) => {
    const aRecent = recentMap.get(a.path) ?? 0;
    const bRecent = recentMap.get(b.path) ?? 0;
    if (aRecent !== bRecent) return bRecent - aRecent;
    return a.name.localeCompare(b.name);
  });

  const selected = await fuzzySelectProject(sorted);
  return { path: selected.path, name: selected.name };
}

export async function runProjectRemove(pathArg?: string, opts?: { force?: boolean; delete?: boolean }): Promise<void> {
  const { path: target, name: projectName } = await selectProjectToRemove(pathArg);
  const deleteFromDisk = opts?.delete ?? false;

  log.phase(`${deleteFromDisk ? "Delete" : "Untrack"} project: ${projectName}`);
  log.item(`Path: ${pc.dim(target)}`);

  if (!opts?.force) {
    const prompt = deleteFromDisk
      ? `\nThis will ${pc.red("permanently delete")} the project from disk and remove it from the index. Continue? [y/N] `
      : `\nThis will remove the project from the index. Continue? [y/N] `;
    const answer = await askLine(prompt);
    if (answer.toLowerCase() !== "y") {
      log.dim("Aborted.");
      return;
    }
  }

  const removed = await removeProject(target);
  if (removed) {
    log.success(`Untracked ${pc.cyan(projectName)}`);
  } else {
    log.dim("No project record found; nothing was removed from the index.");
  }

  if (deleteFromDisk) {
    if (existsSync(target)) {
      await rm(target, { recursive: true });
      log.success(`Deleted ${pc.cyan(target)} from disk`);
    } else {
      log.dim("Directory does not exist on disk; nothing to delete.");
    }
  }
}
