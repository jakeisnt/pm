import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { getDevConfig, upsertDevConfig } from "../lib/db/index.ts";
import { log } from "../lib/log.ts";
import { runCmd } from "../lib/subprocess.ts";

function detectPackageManager(dir: string): string | null {
  if (existsSync(join(dir, "bun.lock")) || existsSync(join(dir, "bun.lockb"))) return "bun";
  if (existsSync(join(dir, "yarn.lock"))) return "yarn";
  if (existsSync(join(dir, "pnpm-lock.yaml"))) return "pnpm";
  if (existsSync(join(dir, "package-lock.json"))) return "npm";
  if (existsSync(join(dir, "package.json"))) return "bun"; // default to bun
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

export async function detectAndSaveDevConfig(dir: string): Promise<{ pm: string; script: string } | null> {
  const pm = detectPackageManager(dir);
  if (!pm) return null;
  const script = detectDevScript(dir);
  if (!script) return null;

  await upsertDevConfig({ projectPath: dir, packageManager: pm, devCommand: script });
  return { pm, script };
}

export async function runProjectDev(dir?: string): Promise<void> {
  const cwd = dir ?? process.cwd();

  let config = await getDevConfig(cwd);
  if (!config) {
    const detected = await detectAndSaveDevConfig(cwd);
    if (!detected) {
      log.fail("Could not detect dev command for this project.");
      return;
    }
    config = { projectPath: cwd, packageManager: detected.pm, devCommand: detected.script };
  }

  const cmd = config.packageManager === "bun" ? "bun" : config.packageManager;
  const args = config.packageManager === "bun" ? ["run", config.devCommand] : [config.devCommand];

  log.dim(`Running: ${cmd} ${args.join(" ")}`);
  runCmd(cmd, args, cwd);
}
