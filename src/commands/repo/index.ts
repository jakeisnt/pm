import { basename, resolve } from "node:path";
import pc from "picocolors";
import { getCachedProjects, getRecentProjects, removeProject, shortId } from "../../lib/db/index.ts";
import { fuzzySelectProject } from "../../lib/fuzzy.ts";
import { git } from "../../lib/github.ts";
import { log } from "../../lib/log.ts";
import { askLine } from "../../lib/prompt.ts";
import {
  addRepoMemory,
  clearRepoMemories,
  formatRepoMemoriesForPrompt,
  getRepoMemory,
  listRepoMemories,
  type RepoMemoryInput,
  removeRepoMemory,
} from "../../lib/repo-memories.ts";
import type { Project } from "../../types.ts";

function resolveGitRepoRoot(dir: string): string {
  const result = git(["rev-parse", "--show-toplevel"], dir);
  if (!result.ok) throw new Error(`Not inside a git repository: ${dir}`);
  return result.stdout;
}

function resolveRepo(path?: string): { repoPath: string; repoName: string } {
  const target = path ?? process.cwd();
  try {
    const repoPath = resolveGitRepoRoot(target);
    const repoName = basename(repoPath);
    return { repoPath, repoName };
  } catch {
    log.fail(`Not inside a git repository: ${target}`);
    log.dim("Run this command from within a git repo, or pass --path <repo>.");
    throw new Error(`Not inside a git repository: ${target}`);
  }
}

function buildFilter(opts: Record<string, string | undefined>): Record<string, string> {
  const filter: Record<string, string> = {};
  for (const [k, v] of Object.entries(opts)) {
    if (v !== undefined) filter[k] = v;
  }
  return filter;
}

async function selectRepoToRemove(pathArg?: string): Promise<{ path: string; name: string }> {
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

export async function runRepoRemove(pathArg?: string, opts?: { force?: boolean }): Promise<void> {
  const { path: target, name: repoName } = await selectRepoToRemove(pathArg);

  log.phase(`Untrack repo: ${repoName}`);
  log.item(`Path: ${pc.dim(target)}`);

  const memories = await listRepoMemories({ repoPath: target });
  if (memories.length > 0) {
    log.item(`Repo memories kept: ${memories.length}`);
  }

  if (!opts?.force) {
    const answer = await askLine(`\nThis will remove the local project reference only. Continue? [y/N] `);
    if (answer.toLowerCase() !== "y") {
      log.dim("Aborted.");
      return;
    }
  }

  const removedProject = await removeProject(target);
  if (removedProject) {
    log.item("Removed project record");
  } else {
    log.dim("No project record found; nothing was removed from the index.");
  }

  log.success(`Untracked ${pc.cyan(repoName)} (repo files and memories were preserved)`);
}

export async function runRepoList(opts: { source?: string; scope?: string; json?: boolean }): Promise<void> {
  const sourceFilter = opts.source as "local" | "github" | undefined;
  const projects = await getCachedProjects(sourceFilter);

  const filtered = opts.scope ? projects.filter((p) => p.scope === opts.scope) : projects;

  if (filtered.length === 0) {
    log.dim("No tracked repos found.");
    return;
  }

  if (opts.json) {
    log.raw(JSON.stringify(filtered, null, 2));
    return;
  }

  const allMemories = await listRepoMemories();
  const memoryCounts = new Map<string, number>();
  for (const m of allMemories) {
    memoryCounts.set(m.repoPath, (memoryCounts.get(m.repoPath) ?? 0) + 1);
  }

  const byScope = new Map<string, Project[]>();
  for (const p of filtered) {
    const scope = p.scope ?? "unscoped";
    const list = byScope.get(scope) ?? [];
    list.push(p);
    byScope.set(scope, list);
  }

  log.blank();
  log.phase(`Tracked Repos (${filtered.length})`);

  for (const [scope, scopeProjects] of byScope) {
    log.blank();
    log.item(pc.bold(scope.charAt(0).toUpperCase() + scope.slice(1)));

    for (const p of scopeProjects) {
      const isLocal = p.source === "local";
      const icon = isLocal ? pc.green("●") : pc.blue("☁");
      const memCount = memoryCounts.get(p.path);
      const memLabel = memCount ? pc.dim(` [${memCount} memories]`) : "";
      const ghLabel = p.githubFullName ? pc.dim(` (${p.githubFullName})`) : "";
      const pathLabel = isLocal ? pc.dim(` ${p.path}`) : "";
      log.detail(`${icon} ${pc.cyan(p.name)}${ghLabel}${pathLabel}${memLabel}`);
    }
  }

  log.blank();
  const localCount = filtered.filter((p) => p.source === "local").length;
  const ghCount = filtered.filter((p) => p.source === "github").length;
  log.dim(`${localCount} local, ${ghCount} GitHub-only`);
}

export async function runMemoryOverview(opts: { json?: boolean }): Promise<void> {
  const allMemories = await listRepoMemories();

  if (allMemories.length === 0) {
    log.dim("No repo memories stored yet.");
    log.dim(`Run ${pc.cyan("p repo memory add")} to add manually.`);
    return;
  }

  if (opts.json) {
    log.raw(JSON.stringify(allMemories, null, 2));
    return;
  }

  const byRepo = new Map<string, { name: string; count: number; categories: Map<string, number> }>();
  for (const m of allMemories) {
    const existing = byRepo.get(m.repoPath);
    if (existing) {
      existing.count++;
      existing.categories.set(m.category, (existing.categories.get(m.category) ?? 0) + 1);
    } else {
      const cats = new Map<string, number>();
      cats.set(m.category, 1);
      byRepo.set(m.repoPath, { name: m.repoName, count: 1, categories: cats });
    }
  }

  for (const entry of byRepo.values()) {
    let total = 0;
    for (const c of entry.categories.values()) total += c;
    entry.count = total;
  }

  const sorted = [...byRepo.entries()].sort((a, b) => b[1].count - a[1].count);

  log.blank();
  log.phase(`Repo Memories (${allMemories.length} across ${sorted.length} repos)`);

  for (const [repoPath, info] of sorted) {
    log.blank();
    log.item(`${pc.cyan(pc.bold(info.name))} ${pc.dim(`(${info.count})`)} ${pc.dim(repoPath)}`);
    const catParts: string[] = [];
    for (const [cat, count] of info.categories) {
      catParts.push(`${cat}: ${count}`);
    }
    log.detail(pc.dim(catParts.join(", ")));
  }

  log.blank();
}

export async function runRepoStatus(opts: { path?: string; json?: boolean }): Promise<void> {
  const { repoPath, repoName } = resolveRepo(opts.path);

  const memories = await listRepoMemories({ repoPath });
  if (memories.length === 0) {
    log.dim(`No knowledge stored for ${pc.cyan(repoName)} yet.`);
    log.dim(`Run ${pc.cyan("p repo memory add")} to add manually.`);
    return;
  }

  if (opts.json) {
    log.raw(JSON.stringify(memories, null, 2));
    return;
  }

  log.phase(`Repo Knowledge: ${repoName}`);
  log.dim(repoPath);
  log.blank();

  const grouped = new Map<string, typeof memories>();
  for (const m of memories) {
    const existing = grouped.get(m.category) ?? [];
    existing.push(m);
    grouped.set(m.category, existing);
  }

  for (const [category, items] of grouped) {
    log.item(`${pc.bold(category.charAt(0).toUpperCase() + category.slice(1))} ${pc.dim(`(${items.length})`)}`);
    for (const item of items) {
      const tagStr = item.tags.length > 0 ? ` ${pc.dim(`[${item.tags.join(", ")}]`)}` : "";
      log.detail(`${pc.dim(`#${shortId(item.id)}`)} ${pc.bold(item.key)}: ${item.value}${tagStr}`);
    }
    log.blank();
  }

  log.dim(`${memories.length} memor${memories.length === 1 ? "y" : "ies"} total`);
}

export async function runRepoMemoryList(opts: {
  path?: string;
  category?: string;
  source?: string;
  search?: string;
  tag?: string;
  json?: boolean;
}): Promise<void> {
  const { repoPath, repoName } = resolveRepo(opts.path);

  const memories = await listRepoMemories({
    repoPath,
    ...buildFilter({
      category: opts.category,
      source: opts.source,
      search: opts.search,
      tag: opts.tag,
    }),
  });

  if (memories.length === 0) {
    log.dim(`No memories found for ${pc.cyan(repoName)}.`);
    return;
  }

  if (opts.json) {
    log.raw(JSON.stringify(memories, null, 2));
    return;
  }

  const maxKey = Math.max(...memories.map((m) => m.key.length));
  for (const m of memories) {
    const tags = m.tags.length > 0 ? ` ${pc.dim(`[${m.tags.join(", ")}]`)}` : "";
    const src = m.source !== "manual" ? ` ${pc.dim(`(${m.source})`)}` : "";
    log.item(
      `${pc.dim(`#${shortId(m.id)}`)} ${pc.dim(m.category.padEnd(12))} ${pc.bold(m.key.padEnd(maxKey))}  ${m.value}${tags}${src}`,
    );
  }
  log.blank();
  log.dim(`${memories.length} memor${memories.length === 1 ? "y" : "ies"}`);
}

export async function runRepoMemoryShow(id: string): Promise<void> {
  const m = await getRepoMemory(id);
  if (!m) {
    log.fail(`Memory #${id} not found`);
    return;
  }

  log.phase(`Memory #${shortId(m.id)}`);
  log.item(`${pc.dim("Repo:")}     ${pc.cyan(m.repoName)} ${pc.dim(`(${m.repoPath})`)}`);
  log.item(`${pc.dim("Category:")} ${m.category}`);
  log.item(`${pc.dim("Key:")}      ${pc.bold(m.key)}`);
  log.item(`${pc.dim("Value:")}    ${m.value}`);
  log.item(`${pc.dim("Source:")}   ${m.source}${m.sourceRef ? ` (${m.sourceRef})` : ""}`);
  if (m.tags.length > 0) {
    log.item(`${pc.dim("Tags:")}     ${m.tags.join(", ")}`);
  }
  log.item(`${pc.dim("Created:")}  ${new Date(m.createdAt).toISOString()}`);
  log.item(`${pc.dim("Updated:")}  ${new Date(m.updatedAt).toISOString()}`);
}

export async function runRepoMemoryAdd(
  category: string,
  key: string,
  value: string,
  opts: {
    path?: string;
    tags?: string;
    source?: string;
    sourceRef?: string;
  },
): Promise<void> {
  const { repoPath, repoName } = resolveRepo(opts.path);

  const input: RepoMemoryInput = {
    repoPath,
    repoName,
    category,
    key,
    value,
    source: opts.source ?? "manual",
    ...(opts.sourceRef ? { sourceRef: opts.sourceRef } : {}),
    ...(opts.tags ? { tags: opts.tags.split(",").map((t) => t.trim()) } : {}),
  };

  const mem = await addRepoMemory(input);
  log.success(`Added memory #${shortId(mem.id)}: ${pc.bold(mem.key)}`);
}

export async function runRepoMemoryRemove(id: string): Promise<void> {
  const removed = await removeRepoMemory(id);
  if (removed) {
    log.success(`Removed memory #${id}`);
  } else {
    log.fail(`Memory #${id} not found`);
  }
}

export async function runRepoMemoryClear(opts: { path?: string; category?: string; source?: string }): Promise<void> {
  const { repoPath } = resolveRepo(opts.path);
  const count = await clearRepoMemories({
    repoPath,
    ...buildFilter({ category: opts.category, source: opts.source }),
  });
  log.success(`Cleared ${count} memor${count === 1 ? "y" : "ies"}`);
}

export async function runRepoMemoryPrompt(opts: { path?: string; copy?: boolean }): Promise<void> {
  const { repoPath } = resolveRepo(opts.path);
  const prompt = await formatRepoMemoriesForPrompt(repoPath);
  if (!prompt) {
    log.dim("No repo memories to output.");
    return;
  }

  if (opts.copy) {
    const proc = Bun.spawn(["pbcopy"], { stdin: "pipe" });
    proc.stdin.write(prompt);
    proc.stdin.end();
    await proc.exited;
    log.success("Copied to clipboard");
    return;
  }

  log.raw(prompt);
}
