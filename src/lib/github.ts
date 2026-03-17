import type { ExecResult } from "@uln/cmd";
import { exec } from "@uln/cmd";
import { checkAbort } from "./abort.ts";

export type RunResult = ExecResult;

export function git(args: string[], cwd?: string): ExecResult {
  return exec(["git", ...args], cwd ? { cwd } : undefined);
}

export function gh(args: string[], cwd?: string): ExecResult {
  return exec(["gh", ...args], cwd ? { cwd } : undefined);
}

export function gitAbortable(args: string[], cwd?: string): ExecResult {
  checkAbort();
  return git(args, cwd);
}

export function ghAbortable(args: string[], cwd?: string): ExecResult {
  checkAbort();
  return gh(args, cwd);
}
