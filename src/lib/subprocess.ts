import { checkAbort, trackProcess } from "./abort.ts";
import { getShell } from "./env.ts";

export interface RunResult {
  ok: boolean;
  stdout: string;
  stderr: string;
  exitCode: number;
}

export function run(cmd: string[], opts?: { cwd?: string }): RunResult {
  const spawnOpts: { stdout: "pipe"; stderr: "pipe"; cwd?: string } = {
    stdout: "pipe",
    stderr: "pipe",
  };
  if (opts?.cwd) spawnOpts.cwd = opts.cwd;
  const result = Bun.spawnSync(cmd, spawnOpts);
  return {
    ok: result.exitCode === 0,
    stdout: result.stdout.toString().trim(),
    stderr: result.stderr.toString().trim(),
    exitCode: result.exitCode,
  };
}

export function runCmd(
  cmd: string,
  args: string[],
  cwd: string,
  opts?: { env?: Record<string, string | undefined> },
): number {
  const env = opts?.env ? { ...process.env, ...opts.env } : undefined;
  const spawnOpts: Parameters<typeof Bun.spawnSync>[1] = {
    cwd,
    stdin: "inherit",
    stdout: "inherit",
    stderr: "inherit",
  };
  if (env) spawnOpts.env = env;
  const result = Bun.spawnSync([cmd, ...args], spawnOpts);
  return result.exitCode;
}

export async function runCmdAsync(
  cmd: string,
  args: string[],
  cwd: string,
  opts?: { env?: Record<string, string | undefined> },
): Promise<number> {
  const env = opts?.env ? { ...process.env, ...opts.env } : undefined;
  const spawnOpts: Parameters<typeof Bun.spawn>[1] = {
    cwd,
    stdin: "inherit",
    stdout: "inherit",
    stderr: "inherit",
  };
  if (env) spawnOpts.env = env;
  const proc = Bun.spawn([cmd, ...args], spawnOpts);

  trackProcess(proc);
  const exitCode = await proc.exited;
  trackProcess(null);

  return exitCode;
}

export function runAbortable(cmd: string[], opts: { cwd?: string } = {}): RunResult {
  checkAbort();
  return run(cmd, opts);
}

export function spawnShell(cwd: string): number {
  const shell = getShell();
  const result = Bun.spawnSync([shell], {
    cwd,
    stdin: "inherit",
    stdout: "inherit",
    stderr: "inherit",
  });
  return result.exitCode;
}
