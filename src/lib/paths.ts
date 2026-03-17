import { existsSync, mkdirSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

const HARDCODED_MACOS_DB_DIR = join(homedir(), "Library", "Application Support", "pm");
const HARDCODED_MACOS_DB_PATH = join(HARDCODED_MACOS_DB_DIR, "pm.db");

export function expandTilde(p: string): string {
  if (p.startsWith("~/")) return join(homedir(), p.slice(2));
  return p;
}

function getDataDir(): string {
  const platform = process.platform;
  if (platform === "darwin") return join(homedir(), "Library", "Application Support");
  if (platform === "win32") return join(homedir(), "AppData", "Roaming");
  return join(homedir(), ".local", "share");
}

export function getAppDir(): string {
  const dir = join(getDataDir(), "pm");
  if (!existsSync(dir)) {
    mkdirSync(dir, { recursive: true });
  }
  return dir;
}

export function getDbPath(): string {
  mkdirSync(HARDCODED_MACOS_DB_DIR, { recursive: true });
  return HARDCODED_MACOS_DB_PATH;
}

export function getDefaultCloneDir(): string {
  return join(homedir(), "Documents");
}
