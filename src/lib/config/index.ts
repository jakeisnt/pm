// ─── Config module public API ───────────────────────────────────────────
//
// Single entry point for all configuration: hardcoded defaults, env vars,
// and file-based config (config.json).

// Config file (config.json with hardcoded fallbacks)
export type { PmConfig } from "./config-file.ts";
export {
  deleteConfigValue,
  getConfigPath,
  loadConfig,
  setConfigValue,
} from "./config-file.ts";
// Hardcoded defaults (internal constants not in config.json)
export {
  ESC_DISAMBIGUATE_MS,
  EXIT_ABORT,
  GH_REPO_LIST_LIMIT,
  getAppDir,
  getDbPath,
} from "./defaults.ts";

// Environment variables
export { env, getEditor, getShell } from "./env.ts";
