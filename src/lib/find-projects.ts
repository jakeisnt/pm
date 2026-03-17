import { existsSync, readdirSync, readFileSync } from "node:fs";
import { basename, join } from "node:path";
import type { Project, RootsConfig } from "../types.ts";
import { expandTilde } from "./paths.ts";

const IGNORED_DIRS = new Set([
  "node_modules",
  "build",
  "dist",
  ".next",
  ".cache",
  "target",
  "vendor",
  ".vscode",
  ".idea",
]);

export function findProjects(config: RootsConfig): Project[] {
  const seen = new Set<string>();
  const projects: Project[] = [];

  for (const root of config.roots) {
    const expanded = expandTilde(root);
    if (!existsSync(expanded)) continue;
    walk(expanded, 0, config.maxDepth, seen, projects);
  }

  return projects;
}

function extractGithubFullName(dir: string): string | undefined {
  try {
    const configPath = join(dir, ".git", "config");
    if (!existsSync(configPath)) return undefined;
    const content = readFileSync(configPath, "utf-8");
    const match = content.match(/github\.com[:/]([^/\s]+\/[^/\s]+?)(?:\.git)?(?:\s|$)/);
    return match?.[1];
  } catch {
    return undefined;
  }
}

function walk(dir: string, depth: number, maxDepth: number, seen: Set<string>, projects: Project[]): void {
  if (depth > maxDepth) return;

  let entries: import("node:fs").Dirent[];
  try {
    entries = readdirSync(dir, { withFileTypes: true });
  } catch {
    return;
  }

  for (const entry of entries) {
    if (!entry.isDirectory()) continue;
    if (IGNORED_DIRS.has(entry.name)) continue;

    const full = join(dir, entry.name);

    if (entry.name === ".git") {
      if (!seen.has(dir)) {
        seen.add(dir);
        const githubFullName = extractGithubFullName(dir);
        projects.push({
          path: dir,
          name: basename(dir),
          source: "local",
          ...(githubFullName ? { githubFullName } : {}),
        });
      }
      return;
    }

    walk(full, depth + 1, maxDepth, seen, projects);
  }
}
