#!/usr/bin/env bun

import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { basename, join, resolve } from "node:path";
import { Command } from "@uln/cmd";
import pc from "picocolors";
import { disableAbort, enableAbort } from "./lib/abort.ts";
import { deleteConfigValue, loadConfig, setConfigValue } from "./lib/config/index.ts";
import {
  getCachedProjects,
  getDevConfig,
  getOrgs,
  getRecentProjects,
  setOrgHidden,
  upsertDevConfig,
} from "./lib/db/index.ts";
import { git } from "./lib/github.ts";
import { runIndexing } from "./lib/indexer.ts";
import { log } from "./lib/log.ts";
import { findProjectByName, fuzzySelectProject, runProjectSelect } from "./lib/project-select.ts";
import { askLine, SelectionCancelledError } from "./lib/prompt.ts";
import { runCmd, runShellHook } from "./lib/subprocess.ts";
import type { Project, SelectOptions } from "./types.ts";

const program = new Command();

program
  .name("p")
  .description("Project manager — switch between projects quickly")
  .version("0.1.0")
  .enablePositionalOptions();

// ─── p (default): fuzzy select a project ─────────────────────────────────
program
  .argument("[name]", "project name to match directly")
  .option("-p, --path", "print selected path to stdout")
  .option("-o, --open <cmd>", "open with command (e.g. code, zed)")
  .option("-a, --app <name>", "open with app by name (e.g. Ghostty, Finder; uses `open -a` on macOS)")
  .option("-s, --silent", "select without side effects")
  .option("--clone-dir <dir>", "directory for cloning GitHub repos")
  .option("--json", "output project list as JSON")
  .action(async (name: string | undefined, opts: Record<string, unknown>) => {
    enableAbort();
    try {
      const { searchRoots: roots, searchDepth: depth } = loadConfig();
      await runProjectSelect(roots, depth, {
        name,
        printPath: Boolean(opts["path"]),
        openCmd: opts["open"] as string | undefined,
        openApp: opts["app"] as string | undefined,
        silent: Boolean(opts["silent"]),
        cloneDir: opts["cloneDir"] as string | undefined,
        json: Boolean(opts["json"]),
      } as SelectOptions & { cloneDir?: string; json?: boolean });
    } catch (err) {
      if (err instanceof SelectionCancelledError) {
        return;
      }
      throw err;
    } finally {
      disableAbort();
    }
  });

// ─── p resolve ───────────────────────────────────────────────────────────
program
  .command("resolve <name>")
  .description("Resolve a project name or GitHub full name to its local path")
  .action(async (name: string) => {
    const path = await findProjectByName(name);
    if (path) {
      process.stdout.write(path);
    } else {
      process.exitCode = 1;
    }
  });

// ─── p list ──────────────────────────────────────────────────────────────
program
  .command("list")
  .description("List all tracked projects")
  .option("--source <source>", "filter by source (local/github)")
  .option("--scope <scope>", "filter by scope (personal/work)")
  .option("--json", "output as JSON")
  .action(async (opts: { source?: string; scope?: string; json?: boolean }) => {
    const { searchRoots: roots, searchDepth: depth } = loadConfig();
    await runIndexing({ roots, maxDepth: depth });

    const sourceFilter = opts.source as "local" | "github" | undefined;
    const projects = await getCachedProjects(sourceFilter);

    const filtered = opts.scope ? projects.filter((p) => p.scope === opts.scope) : projects;

    if (filtered.length === 0) {
      log.dim("No tracked projects found.");
      return;
    }

    if (opts.json) {
      process.stdout.write(JSON.stringify(filtered));
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
  });

// ─── p remove ────────────────────────────────────────────────────────────
program
  .command("remove [path]")
  .description("Delete a project from disk (index entry persists until next GitHub refetch)")
  .option("-f, --force", "skip confirmation")
  .action(async (pathArg: string | undefined, opts: { force?: boolean }) => {
    let target: string;
    let projectName: string;

    if (pathArg) {
      target = resolve(pathArg);
      projectName = basename(target);
    } else {
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
      target = selected.path;
      projectName = selected.name;
    }

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

    if (!opts.force) {
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
  });

// ─── p dev ───────────────────────────────────────────────────────────────

function detectPackageManager(dir: string): string | null {
  if (existsSync(join(dir, "bun.lock")) || existsSync(join(dir, "bun.lockb"))) return "bun";
  if (existsSync(join(dir, "yarn.lock"))) return "yarn";
  if (existsSync(join(dir, "pnpm-lock.yaml"))) return "pnpm";
  if (existsSync(join(dir, "package-lock.json"))) return "npm";
  if (existsSync(join(dir, "package.json"))) return "bun";
  return null;
}

function detectDevScript(dir: string): string | null {
  const pkgPath = join(dir, "package.json");
  if (!existsSync(pkgPath)) return null;
  try {
    const pkg = JSON.parse(readFileSync(pkgPath, "utf-8"));
    if (pkg.scripts?.dev) return "dev";
    if (pkg.scripts?.start) return "start";
    return null;
  } catch {
    return null;
  }
}

program
  .command("dev")
  .description("Run the current project's dev command")
  .action(async () => {
    const cwd = process.cwd();

    let config = await getDevConfig(cwd);
    if (!config) {
      const pm = detectPackageManager(cwd);
      if (!pm) {
        log.fail("Could not detect dev command for this project.");
        return;
      }
      const script = detectDevScript(cwd);
      if (!script) {
        log.fail("Could not detect dev command for this project.");
        return;
      }

      await upsertDevConfig({ projectPath: cwd, packageManager: pm, devCommand: script });
      config = { projectPath: cwd, packageManager: pm, devCommand: script };
    }

    const cmd = config.packageManager === "bun" ? "bun" : config.packageManager;
    const args = config.packageManager === "bun" ? ["run", config.devCommand] : [config.devCommand];

    log.dim(`Running: ${cmd} ${args.join(" ")}`);
    runCmd(cmd, args, cwd);
  });

// ─── p config ────────────────────────────────────────────────────────────
const configCmd = program.command("config").description("Manage CLI settings (config.json)");

configCmd
  .command("list")
  .description("Show all settings")
  .action(() => {
    const config = loadConfig();
    for (const [key, value] of Object.entries(config)) {
      log.item(`${pc.cyan(key)}: ${pc.dim(JSON.stringify(value))}`);
    }
  });

configCmd
  .command("set <key> <value>")
  .description("Set a configuration value")
  .action((key: string, value: string) => {
    setConfigValue(key, value);
    log.success(`Set ${pc.cyan(key)} = ${pc.dim(value)}`);
  });

configCmd
  .command("delete <key>")
  .description("Delete a setting (reverts to default)")
  .action((key: string) => {
    deleteConfigValue(key);
    log.success(`Deleted ${pc.cyan(key)} (reverted to default)`);
  });

// ─── p org ───────────────────────────────────────────────────────────────
const org = program.command("org").description("Manage tracked orgs");

org
  .command("list")
  .description("List all tracked orgs")
  .action(async () => {
    const orgs = await getOrgs();
    if (orgs.length === 0) {
      log.dim("No tracked orgs.");
      return;
    }

    log.blank();
    log.phase(`Orgs (${orgs.length})`);
    log.blank();
    for (const o of orgs) {
      const icon = o.hidden ? pc.red("●") : pc.green("●");
      const label = o.hidden ? pc.dim(o.name) : pc.cyan(o.name);
      const tag = o.hidden ? pc.dim(" (hidden)") : "";
      log.item(`${icon} ${label}${tag}`);
    }
    log.blank();
  });

org
  .command("hide <name>")
  .description("Hide an org from all project listings")
  .action(async (name: string) => {
    await setOrgHidden(name, true);
    log.success(`Org "${name}" is now hidden.`);
  });

org
  .command("show <name>")
  .description("Unhide an org so its projects appear again")
  .action(async (name: string) => {
    await setOrgHidden(name, false);
    log.success(`Org "${name}" is now visible.`);
  });

// ─── Unknown command hook ────────────────────────────────────────────────
program.on("command:*", (args: string[]) => {
  const config = loadConfig();
  runShellHook(config.onMissingCommand, {
    PM_COMMAND: args.join(" "),
  });
  process.exitCode = 1;
});

// ─── Parse ───────────────────────────────────────────────────────────────
try {
  program.parse();
} catch (err) {
  const config = loadConfig();
  const errorMsg = err instanceof Error ? err.message : String(err);
  const traceFile = join(tmpdir(), `pm-error-${Date.now()}.txt`);
  const stack = err instanceof Error ? (err.stack ?? errorMsg) : errorMsg;
  writeFileSync(traceFile, stack, "utf-8");
  runShellHook(config.onError, {
    PM_ERROR: errorMsg,
    PM_ERROR_TRACE: traceFile,
    PM_REPO: process.cwd(),
  });
  process.exitCode = 1;
}
