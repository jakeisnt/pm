import { fetchGithubRepos, getOctokit, toGithubPlaceholder } from "@uln/repo";
import type { RootsConfig } from "../types.ts";
import { GH_REPO_LIST_LIMIT } from "./constants.ts";
import { cleanupNotOnGithub, getDb, needsGithubReindex, needsReindex, upsertProjects } from "./db/index.ts";
import { findProjects } from "./find-projects.ts";

export async function runIndexing(config: RootsConfig): Promise<void> {
  getDb(); // ensure initialized
  if (!(await needsReindex())) return;
  await forceReindex(config);
}

export async function forceReindex(config: RootsConfig): Promise<void> {
  const projects = findProjects(config);
  await upsertProjects(projects);
}

export async function indexGithubRepos(): Promise<void> {
  getDb();
  if (!(await needsGithubReindex())) return;
  await forceGithubReindex();
}

export async function forceGithubReindex(): Promise<void> {
  const octokit = getOctokit();
  const repoEntries = await fetchGithubRepos(octokit, Number(GH_REPO_LIST_LIMIT));
  if (repoEntries.length > 0) {
    const githubFullNames = new Set(repoEntries.map((r) => r.nameWithOwner));
    const projects = repoEntries.map((r) => ({
      path: toGithubPlaceholder(r.nameWithOwner),
      name: r.name,
      source: "github" as const,
      githubFullName: r.nameWithOwner,
    }));
    await upsertProjects(projects);
    await cleanupNotOnGithub(githubFullNames);
  }
}
