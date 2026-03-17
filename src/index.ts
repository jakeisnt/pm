#!/usr/bin/env bun

import { Command } from "commander";
import { disableAbort, enableAbort } from "./lib/abort.ts";
import { REPO_MEMORY_CATEGORIES, REPO_MEMORY_SOURCES } from "./lib/repo-memories.ts";

const program = new Command();

program.name("p").description("Project manager — switch projects, manage per-project knowledge").version("0.1.0");

// ─── p (default): fuzzy select a project ─────────────────────────────────
program
  .argument("[name]", "project name to match directly")
  .option("-p, --path", "print selected path to stdout")
  .option("-o, --open <cmd>", "open with command (e.g. code, zed)")
  .option("-s, --silent", "select without side effects")
  .option("--clone-dir <dir>", "directory for cloning GitHub repos")
  .option("--json", "output project list as JSON")
  .action(async (name: string | undefined, opts: Record<string, unknown>) => {
    enableAbort();
    try {
      const { runSelect } = await import("./commands/select/index.ts");
      await runSelect({
        name,
        printPath: Boolean(opts["path"]),
        openCmd: opts["open"] as string | undefined,
        silent: Boolean(opts["silent"]),
        cloneDir: opts["cloneDir"] as string | undefined,
        json: Boolean(opts["json"]),
      });
    } finally {
      disableAbort();
    }
  });

// ─── p repo ──────────────────────────────────────────────────────────────
const repo = program.command("repo").description("Repository knowledge and management");

repo
  .command("status")
  .description("Show what's known about the current repo")
  .option("--path <path>", "repo path")
  .option("--json", "output as JSON")
  .action(async (opts: { path?: string; json?: boolean }) => {
    const { runRepoStatus } = await import("./commands/repo/index.ts");
    await runRepoStatus(opts);
  });

repo
  .command("list")
  .description("List all tracked repos")
  .option("--source <source>", "filter by source (local/github)")
  .option("--scope <scope>", "filter by scope (personal/work)")
  .option("--json", "output as JSON")
  .action(async (opts: { source?: string; scope?: string; json?: boolean }) => {
    const { runRepoList } = await import("./commands/repo/index.ts");
    await runRepoList(opts);
  });

repo
  .command("remove [path]")
  .description("Untrack a repo (preserves memories)")
  .option("-f, --force", "skip confirmation")
  .action(async (path: string | undefined, opts: { force?: boolean }) => {
    const { runRepoRemove } = await import("./commands/repo/index.ts");
    await runRepoRemove(path, opts);
  });

// ─── p repo memory ───────────────────────────────────────────────────────
const memory = repo.command("memory").description("Manage per-repo knowledge entries");

memory
  .command("list")
  .description("List memories for the current repo")
  .option("--path <path>", "repo path")
  .option("--category <cat>", `filter by category (${REPO_MEMORY_CATEGORIES.join(", ")})`)
  .option("--source <src>", `filter by source (${REPO_MEMORY_SOURCES.join(", ")})`)
  .option("--search <term>", "search key/value/tags")
  .option("--tag <tag>", "filter by tag")
  .option("--json", "output as JSON")
  .action(
    async (opts: {
      path?: string;
      category?: string;
      source?: string;
      search?: string;
      tag?: string;
      json?: boolean;
    }) => {
      const { runRepoMemoryList } = await import("./commands/repo/index.ts");
      await runRepoMemoryList(opts);
    },
  );

memory
  .command("add <category> <key> <value>")
  .description("Add a memory to the current repo")
  .option("--path <path>", "repo path")
  .option("--tags <tags>", "comma-separated tags")
  .option("--source <src>", "source (manual/agent/learn)")
  .option("--source-ref <ref>", "source reference")
  .action(
    async (
      category: string,
      key: string,
      value: string,
      opts: { path?: string; tags?: string; source?: string; sourceRef?: string },
    ) => {
      const { runRepoMemoryAdd } = await import("./commands/repo/index.ts");
      await runRepoMemoryAdd(category, key, value, opts);
    },
  );

memory
  .command("show <id>")
  .description("Show details of a specific memory")
  .action(async (id: string) => {
    const { runRepoMemoryShow } = await import("./commands/repo/index.ts");
    await runRepoMemoryShow(id);
  });

memory
  .command("rm <id>")
  .description("Remove a memory")
  .action(async (id: string) => {
    const { runRepoMemoryRemove } = await import("./commands/repo/index.ts");
    await runRepoMemoryRemove(id);
  });

memory
  .command("clear")
  .description("Clear all memories for the current repo")
  .option("--path <path>", "repo path")
  .option("--category <cat>", "clear only this category")
  .option("--source <src>", "clear only this source")
  .action(async (opts: { path?: string; category?: string; source?: string }) => {
    const { runRepoMemoryClear } = await import("./commands/repo/index.ts");
    await runRepoMemoryClear(opts);
  });

memory
  .command("prompt")
  .description("Output memories as markdown for agent injection")
  .option("--path <path>", "repo path")
  .option("--copy", "copy to clipboard")
  .action(async (opts: { path?: string; copy?: boolean }) => {
    const { runRepoMemoryPrompt } = await import("./commands/repo/index.ts");
    await runRepoMemoryPrompt(opts);
  });

// ─── p memory (overview) ─────────────────────────────────────────────────
program
  .command("memory")
  .description("Overview of repo memories across all repos")
  .option("--json", "output as JSON")
  .action(async (opts: { json?: boolean }) => {
    const { runMemoryOverview } = await import("./commands/repo/index.ts");
    await runMemoryOverview(opts);
  });

// ─── p dev ───────────────────────────────────────────────────────────────
program
  .command("dev")
  .description("Run the current project's dev command")
  .action(async () => {
    const { runProjectDev } = await import("./commands/dev/index.ts");
    await runProjectDev();
  });

// ─── p config ────────────────────────────────────────────────────────────
const config = program.command("config").description("Manage CLI settings");

config
  .command("list")
  .description("Show all settings")
  .action(async () => {
    const { runConfigList } = await import("./commands/config/index.ts");
    runConfigList();
  });

config
  .command("set <key> <value>")
  .description("Set a configuration value")
  .option("-d, --device", "scope to this device only")
  .action(async (key: string, value: string, opts: { device?: boolean }) => {
    const { runConfigSet } = await import("./commands/config/index.ts");
    runConfigSet(key, value, opts);
  });

config
  .command("delete <key>")
  .description("Delete a setting")
  .action(async (key: string) => {
    const { runConfigDelete } = await import("./commands/config/index.ts");
    runConfigDelete(key);
  });

// ─── Parse ───────────────────────────────────────────────────────────────
program.parse();
