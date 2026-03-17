// ─── Config module public API ───────────────────────────────────────────
//
// Single entry point for all configuration: hardcoded defaults, env vars,
// DB-backed settings, and file-based config.

// Hardcoded defaults
export {
  DEFAULT_DEPTH,
  DEFAULT_ON_ERROR,
  DEFAULT_ON_MISSING_COMMAND,
  DEFAULT_ROOTS,
  ESC_DISAMBIGUATE_MS,
  EXIT_ABORT,
  GH_REPO_LIST_LIMIT,
  GITHUB_REINDEX_INTERVAL_MS,
  MAX_RECENT,
  REINDEX_INTERVAL_MS,
} from "./defaults.ts";

// Environment variables
export { env, getEditor, getShell } from "./env.ts";
// File-based config (config.json)
export { loadConfig } from "./file-config.ts";
// DB-backed settings
export type { SettingDef } from "./settings.ts";
export {
  getGithubReindexInterval,
  getMaxRecent,
  getReindexInterval,
  getSearchDepth,
  getSearchRoots,
  SETTING_DEFS,
} from "./settings.ts";
