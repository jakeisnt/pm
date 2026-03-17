import { existsSync, mkdirSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

// ─── Time intervals ──────────────────────────────────────────────────────
/** Reindex local projects after 1 hour. */
export const REINDEX_INTERVAL_MS = 3_600_000;
/** Reindex GitHub projects after 4 hours. */
export const GITHUB_REINDEX_INTERVAL_MS = 3_600_000 * 4;

// ─── Abort / signals ─────────────────────────────────────────────────────
/** Milliseconds to wait before treating a lone ESC as a bare keypress. */
export const ESC_DISAMBIGUATE_MS = 80;
/** Exit code for user-initiated abort (SIGINT convention). */
export const EXIT_ABORT = 130;

// ─── GitHub / API limits ─────────────────────────────────────────────────
/** Maximum repos to fetch from the GitHub API. */
export const GH_REPO_LIST_LIMIT = "1000";

// ─── Recent projects ─────────────────────────────────────────────────────
/** Default number of recent projects to return from queries. */
export const MAX_RECENT = 100;

// ─── Project discovery ───────────────────────────────────────────────────
/** Default root directories for project scanning. */
export const DEFAULT_ROOTS = [join(homedir(), "Documents")];
/** Default max depth for recursive project discovery. */
export const DEFAULT_DEPTH = 2;
/** Default directory to clone GitHub repos into. */
export const DEFAULT_CLONE_DIR = join(homedir(), "Documents");

// ─── Failure hooks ──────────────────────────────────────────────────────
/** Default CLI to run when a command is not found. */
export const DEFAULT_ON_MISSING_COMMAND = 'echo "could not find command"';
/** Default CLI to run on unhandled error. Receives PM_ERROR, PM_ERROR_TRACE, PM_REPO env vars. */
export const DEFAULT_ON_ERROR = 'echo "error in $PM_REPO: $PM_ERROR"';

// ─── Application paths ─────────────────────────────────────────────────
function getDataDir(): string {
  const platform = process.platform;
  if (platform === "darwin") return join(homedir(), "Library", "Application Support");
  if (platform === "win32") return join(homedir(), "AppData", "Roaming");
  return join(homedir(), ".local", "share");
}

/** Ensure the app data directory exists and return its path. */
export function getAppDir(): string {
  const dir = join(getDataDir(), "pm");
  if (!existsSync(dir)) {
    mkdirSync(dir, { recursive: true });
  }
  return dir;
}

/** Ensure the database directory exists and return the database file path. */
export function getDbPath(): string {
  const dir = join(homedir(), "Library", "Application Support", "pm");
  mkdirSync(dir, { recursive: true });
  return join(dir, "pm.db");
}
