import { spawnSync } from "node:child_process";
import type { ExecResult } from "@uln/cmd";
import { exec, execInherit, execInheritAsync, execShell } from "@uln/cmd";
import { checkAbort, trackProcess } from "./abort.ts";
import { getShell } from "./config/index.ts";

export type RunResult = ExecResult;

export { exec as run };

function mergeEnv(extra: Record<string, string | undefined>): Record<string, string> {
  const merged: Record<string, string> = {};
  for (const [k, v] of Object.entries(process.env)) {
    if (v !== undefined) merged[k] = v;
  }
  for (const [k, v] of Object.entries(extra)) {
    if (v !== undefined) merged[k] = v;
  }
  return merged;
}

export function runCmd(
  cmd: string,
  args: string[],
  cwd: string,
  opts?: { env?: Record<string, string | undefined> },
): number {
  const env = opts?.env ? mergeEnv(opts.env) : undefined;
  return execInherit([cmd, ...args], { cwd, env });
}

export async function runCmdAsync(
  cmd: string,
  args: string[],
  cwd: string,
  opts?: { env?: Record<string, string | undefined> },
): Promise<number> {
  const env = opts?.env ? mergeEnv(opts.env) : undefined;
  return execInheritAsync([cmd, ...args], {
    cwd,
    env,
    onProcess: (proc) => trackProcess(proc),
  });
}

export function runAbortable(cmd: string[], opts: { cwd?: string } = {}): ExecResult {
  checkAbort();
  return exec(cmd, opts);
}

export function spawnShell(cwd: string): number {
  return execShell(cwd, getShell());
}

/** Run a shell hook command with extra environment variables. */
export function runShellHook(command: string, env: Record<string, string>): void {
  const shell = getShell();
  spawnSync(shell, ["-c", command], {
    stdio: "inherit",
    env: { ...process.env, ...env },
  });
}
