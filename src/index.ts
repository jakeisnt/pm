#!/usr/bin/env bun

import { writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Command } from "@uln/cmd";
import { disableAbort, enableAbort } from "./lib/abort.ts";
import { loadConfig } from "./lib/config/index.ts";
import { runShellHook } from "./lib/hooks.ts";
import { SelectionCancelledError } from "./lib/prompt.ts";

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
  .option("-s, --silent", "select without side effects")
  .option("--clone-dir <dir>", "directory for cloning GitHub repos")
  .option("--json", "output project list as JSON")
  .action(async (name: string | undefined, opts: Record<string, unknown>) => {
    enableAbort();
    try {
      const { runSelect } = await import("./commands/select.ts");
      await runSelect({
        name,
        printPath: Boolean(opts["path"]),
        openCmd: opts["open"] as string | undefined,
        silent: Boolean(opts["silent"]),
        cloneDir: opts["cloneDir"] as string | undefined,
        json: Boolean(opts["json"]),
      });
    } catch (err) {
      if (err instanceof SelectionCancelledError) {
        // User pressed Escape or Ctrl-C in selector — exit silently
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
    const { findProjectByName } = await import("./lib/project-select.ts");
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
    const { runProjectList } = await import("./commands/list.ts");
    await runProjectList(opts);
  });

// ─── p remove ────────────────────────────────────────────────────────────
program
  .command("remove [path]")
  .description("Delete a project from disk (index entry persists until next GitHub refetch)")
  .option("-f, --force", "skip confirmation")
  .action(async (path: string | undefined, opts: { force?: boolean }) => {
    const { runProjectRemove } = await import("./commands/remove.ts");
    await runProjectRemove(path, opts);
  });

// ─── p dev ───────────────────────────────────────────────────────────────
program
  .command("dev")
  .description("Run the current project's dev command")
  .action(async () => {
    const { runProjectDev } = await import("./commands/dev.ts");
    await runProjectDev();
  });

// ─── p config ────────────────────────────────────────────────────────────
const config = program.command("config").description("Manage CLI settings");

config
  .command("list")
  .description("Show all settings")
  .action(async () => {
    const { runConfigList } = await import("./commands/config.ts");
    runConfigList();
  });

config
  .command("set <key> <value>")
  .description("Set a configuration value")
  .option("-d, --device", "scope to this device only")
  .action(async (key: string, value: string, opts: { device?: boolean }) => {
    const { runConfigSet } = await import("./commands/config.ts");
    runConfigSet(key, value, opts);
  });

config
  .command("delete <key>")
  .description("Delete a setting")
  .action(async (key: string) => {
    const { runConfigDelete } = await import("./commands/config.ts");
    runConfigDelete(key);
  });

// ─── p org ───────────────────────────────────────────────────────────────
const org = program.command("org").description("Manage tracked orgs");

org
  .command("list")
  .description("List all tracked orgs")
  .action(async () => {
    const { runOrgList } = await import("./commands/org.ts");
    await runOrgList();
  });

org
  .command("hide <name>")
  .description("Hide an org from all project listings")
  .action(async (name: string) => {
    const { runOrgHide } = await import("./commands/org.ts");
    await runOrgHide(name);
  });

org
  .command("show <name>")
  .description("Unhide an org so its projects appear again")
  .action(async (name: string) => {
    const { runOrgShow } = await import("./commands/org.ts");
    await runOrgShow(name);
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
