import { existsSync } from "node:fs";
import { rm } from "node:fs/promises";
import { basename, resolve } from "node:path";
import pc from "picocolors";
import { getCachedProjects, getRecentProjects } from "../lib/db/index.ts";
import { git } from "../lib/github.ts";
import { log } from "../lib/log.ts";
import { fuzzySelectProject } from "../lib/project-select.ts";
import { askLine } from "../lib/prompt.ts";

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

export async function runProjectRemove(pathArg?: string, opts?: { force?: boolean }): Promise<void> {
  const { path: target, name: projectName } = await selectProjectToRemove(pathArg);

  if (existsSync(target)) {
    const staged = git(["diff", "--cached", "--name-only"], target);
    if (staged.ok && staged.stdout.trim().length > 0) {
      log.error(`Project ${pc.cyan(projectName)} has staged changes. Commit or unstage them before deleting.`);
      return;
    }
  }

  log.phase(`Delete project: ${projectName}`);
  log.item(`Path: ${pc.dim(target)}`);
  log.dim("The project will remain in the index until the next GitHub refetch.");

  if (!opts?.force) {
    const prompt = `\nThis will ${pc.red("permanently delete")} the project from disk. Continue? [y/N] `;
    const answer = await askLine(prompt);
    if (answer.toLowerCase() !== "y") {
      log.dim("Aborted.");
      return;
    }
  }

  if (existsSync(target)) {
    await rm(target, { recursive: true });
    log.success(`Deleted ${pc.cyan(target)} from disk`);
  } else {
    log.dim("Directory does not exist on disk; nothing to delete.");
  }
}
