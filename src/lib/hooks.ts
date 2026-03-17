import { spawnSync } from "node:child_process";
import { getShell } from "./config/index.ts";

/** Run a shell hook command with extra environment variables. */
export const runShellHook = (command: string, env: Record<string, string>): void => {
  const shell = getShell();
  spawnSync(shell, ["-c", command], {
    stdio: "inherit",
    env: { ...process.env, ...env },
  });
};
