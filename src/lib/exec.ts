import { spawnSync } from "node:child_process";

export interface ExecResult {
  stdout: string;
  stderr: string;
  exitCode: number;
  ok: boolean;
}

export interface ExecOptions {
  cwd?: string | undefined;
  env?: Record<string, string> | undefined;
}

export function exec(cmd: string[], opts?: ExecOptions): ExecResult {
  const bin = cmd[0] ?? "";
  const args = cmd.slice(1);
  const result = spawnSync(bin, args, {
    cwd: opts?.cwd,
    env: opts?.env ? { ...process.env, ...opts.env } : undefined,
    encoding: "utf-8",
    maxBuffer: 50 * 1024 * 1024,
  });
  const exitCode = result.status ?? 1;
  return {
    stdout: result.stdout ?? "",
    stderr: result.stderr ?? "",
    exitCode,
    ok: exitCode === 0,
  };
}

export function execInherit(cmd: string[], opts?: ExecOptions): number {
  const bin = cmd[0] ?? "";
  const args = cmd.slice(1);
  const result = spawnSync(bin, args, {
    cwd: opts?.cwd,
    env: opts?.env ? { ...process.env, ...opts.env } : undefined,
    stdio: "inherit",
  });
  return result.status ?? 1;
}

export async function execInheritAsync(
  cmd: string[],
  opts?: ExecOptions & { onProcess?: (proc: { kill(sig?: number | string): void }) => void },
): Promise<number> {
  const bin = cmd[0] ?? "";
  const args = cmd.slice(1);
  const spawnEnv: Record<string, string> = {};
  if (opts?.env) {
    for (const [k, v] of Object.entries(process.env)) {
      if (v !== undefined) spawnEnv[k] = v;
    }
    for (const [k, v] of Object.entries(opts.env)) {
      spawnEnv[k] = v;
    }
  }
  const spawnOpts: { cwd?: string; env?: Record<string, string>; stdio: ["inherit", "inherit", "inherit"] } = {
    stdio: ["inherit", "inherit", "inherit"],
  };
  if (opts?.cwd) spawnOpts.cwd = opts.cwd;
  if (opts?.env) spawnOpts.env = spawnEnv;
  const proc = Bun.spawn([bin, ...args], spawnOpts);
  opts?.onProcess?.(proc);
  await proc.exited;
  return proc.exitCode ?? 1;
}

export function execShell(cwd: string, shell: string): number {
  const result = spawnSync(shell, [], {
    cwd,
    stdio: "inherit",
  });
  return result.status ?? 1;
}
