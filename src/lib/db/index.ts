// ─── Database module public API ─────────────────────────────────────────
//
// External code should import from this file only.

export { _resetForTesting, closeDatabase, getDb, getRawDb, runMigrationsFromDir } from "./database.ts";
export type { Org } from "./orgs.ts";
export { ensureOrg, extractOrgName, getOrgs, LOCAL_ORG, removeOrg, setOrgHidden } from "./orgs.ts";
export type { DevConfig } from "./projects.ts";
export {
  cleanupNotOnGithub,
  getCachedProjects,
  getDevConfig,
  getLocalProjectByGithubName,
  getProjectsCount,
  needsGithubReindex,
  needsReindex,
  promoteToLocal,
  removeProject,
  touchProject,
  upsertDevConfig,
  upsertProject,
  upsertProjects,
} from "./projects.ts";
export { getRecentProjects, touchRecentProject } from "./recent.ts";
export type { DB } from "./schema.ts";
export { getCurrentSystemId, getCurrentSystemName, listSystems } from "./systems.ts";
