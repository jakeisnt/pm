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
