import { checkAbort } from "./abort.ts";
import type { RunResult } from "./subprocess.ts";
import { run } from "./subprocess.ts";

export type { RunResult };

export function git(args: string[], cwd?: string): RunResult {
  return run(["git", ...args], cwd ? { cwd } : undefined);
}

export function gh(args: string[], cwd?: string): RunResult {
  return run(["gh", ...args], cwd ? { cwd } : undefined);
}

export function gitAbortable(args: string[], cwd?: string): RunResult {
  checkAbort();
  return git(args, cwd);
}

export function ghAbortable(args: string[], cwd?: string): RunResult {
  checkAbort();
  return gh(args, cwd);
}
