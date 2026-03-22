import type { Project } from "../../types.ts";
import { loadConfig } from "../config/index.ts";
import { getDb } from "./database.ts";
import { ensureOrg, extractOrgName } from "./orgs.ts";
import { getCurrentSystemId } from "./systems.ts";

export interface DevConfig {
  projectPath: string;
  packageManager: string;
  devCommand: string;
}

export async function getDevConfig(projectPath: string): Promise<DevConfig | undefined> {
  const row = await getDb()
    .selectFrom("project_dev_config")
    .select(["project_path", "package_manager", "dev_command"])
    .where("project_path", "=", projectPath)
    .where("deleted_at", "is", null)
    .executeTakeFirst();
  if (!row) return undefined;
  return {
    projectPath: row.project_path,
    packageManager: row.package_manager,
    devCommand: row.dev_command,
  };
}

export async function upsertDevConfig(config: DevConfig): Promise<void> {
  const now = Date.now();
  const systemId = getCurrentSystemId();
  await getDb()
    .insertInto("project_dev_config")
    .values({
      project_path: config.projectPath,
      package_manager: config.packageManager,
      dev_command: config.devCommand,
      system_id: systemId,
    })
    .onConflict((oc) =>
      oc.column("project_path").doUpdateSet({
        package_manager: config.packageManager,
        dev_command: config.devCommand,
        updated_at: now,
      }),
    )
    .execute();
}

function toProject(row: {
  path: string;
  name: string;
  last_scanned: number;
  source: string | null;
  github_full_name: string | null;
  scope?: string;
}): Project {
  const project: Project = {
    path: row.path,
    name: row.name,
    lastOpened: row.last_scanned,
    source: (row.source as "local" | "github") || "local",
  };
  if (row.github_full_name) project.githubFullName = row.github_full_name;
  if (row.scope) project.scope = row.scope as "personal" | "work";
  return project;
}

export async function getCachedProjects(source?: "local" | "github"): Promise<Project[]> {
  let query = getDb()
    .selectFrom("projects")
    .innerJoin("orgs", "orgs.name", "projects.org_name")
    .select(["projects.path", "projects.name", "projects.last_scanned", "projects.source", "projects.github_full_name"])
    .where("projects.is_git_repo", "=", 1)
    .where("projects.deleted_at", "is", null)
    .where("orgs.hidden", "=", 0)
    .where("orgs.deleted_at", "is", null)
    .orderBy("projects.last_scanned", "desc");

  if (source) {
    query = query.where("projects.source", "=", source);
  }

  const rows = await query.execute();
  return rows.map(toProject);
}

export async function upsertProjects(projects: Project[]): Promise<void> {
  if (projects.length === 0) return;
  const { inferProjectScope } = await import("../scope.ts");
  const db = getDb();
  const now = Date.now();
  const systemId = getCurrentSystemId();

  // Collect unique org names and ensure they all exist
  const orgNames = new Set(projects.map((p) => extractOrgName(p.githubFullName)));
  for (const orgName of orgNames) {
    await ensureOrg(orgName);
  }

  const BATCH_SIZE = 50;
  for (let i = 0; i < projects.length; i += BATCH_SIZE) {
    const batch = projects.slice(i, i + BATCH_SIZE);
    const values = batch.map((p) => {
      const inferOpts: { githubFullName?: string; path?: string } = { path: p.path };
      if (p.githubFullName) inferOpts.githubFullName = p.githubFullName;
      const scope = p.scope ?? inferProjectScope(inferOpts);
      return {
        id: crypto.randomUUID(),
        path: p.path,
        name: p.name,
        last_scanned: now,
        last_modified: now,
        is_git_repo: 1,
        source: p.source || "local",
        github_full_name: p.githubFullName || null,
        scope,
        org_name: extractOrgName(p.githubFullName),
        system_id: systemId,
      };
    });
    await db
      .insertInto("projects")
      .values(values)
      .onConflict((oc) =>
        oc.column("path").doUpdateSet({
          name: (eb) => eb.ref("excluded.name"),
          last_scanned: now,
          deleted_at: null,
          source: (eb) => eb.ref("excluded.source"),
          // Prefer non-null github_full_name: keep existing value when new scan returns null
          github_full_name: (eb) =>
            eb.fn.coalesce(eb.ref("excluded.github_full_name"), eb.ref("projects.github_full_name")),
          scope: (eb) => eb.ref("excluded.scope"),
          // Prefer real org over _local: keep existing org when new scan falls back to _local
          org_name: (eb) =>
            eb
              .case()
              .when("excluded.org_name", "=", "_local")
              .then(eb.ref("projects.org_name"))
              .else(eb.ref("excluded.org_name"))
              .end(),
        }),
      )
      .execute();
  }
}

export async function upsertProject(project: Project): Promise<void> {
  await upsertProjects([project]);
}

export async function needsGithubReindex(): Promise<boolean> {
  const row = await getDb()
    .selectFrom("projects")
    .select((eb) => eb.fn.max("last_scanned").as("latest"))
    .where("source", "=", "github")
    .where("deleted_at", "is", null)
    .executeTakeFirst();
  if (!row?.latest) return true;
  return Date.now() - (row.latest as number) > loadConfig().githubReindexIntervalMs;
}

export async function getLocalProjectByGithubName(fullName: string): Promise<Project | null> {
  const row = await getDb()
    .selectFrom("projects")
    .select(["path", "name", "last_scanned", "source", "github_full_name"])
    .where("github_full_name", "=", fullName)
    .where("source", "=", "local")
    .where("deleted_at", "is", null)
    .executeTakeFirst();
  if (!row) return null;
  return toProject(row);
}

export async function promoteToLocal(githubFullName: string, localPath: string): Promise<void> {
  const { inferProjectScope } = await import("../scope.ts");
  const db = getDb();
  const now = Date.now();
  const scope = inferProjectScope({ githubFullName, path: localPath });
  const systemId = getCurrentSystemId();
  const orgName = extractOrgName(githubFullName);
  await ensureOrg(orgName);

  await db
    .insertInto("projects")
    .values({
      id: crypto.randomUUID(),
      path: localPath,
      name: githubFullName.split("/").pop() ?? githubFullName,
      last_scanned: now,
      last_modified: now,
      is_git_repo: 1,
      source: "local",
      github_full_name: githubFullName,
      scope,
      org_name: orgName,
      system_id: systemId,
    })
    .onConflict((oc) =>
      oc.column("path").doUpdateSet({
        source: "local",
        github_full_name: githubFullName,
        last_scanned: now,
        scope,
        org_name: orgName,
      }),
    )
    .execute();

  const placeholder = `github://${githubFullName}`;
  await db
    .updateTable("projects")
    .set({ deleted_at: now })
    .where("path", "=", placeholder)
    .where("deleted_at", "is", null)
    .execute();
}

export async function cleanupNotOnGithub(githubFullNames: Set<string>): Promise<void> {
  const db = getDb();
  const now = Date.now();
  const rows = await db
    .selectFrom("projects")
    .select(["id", "github_full_name"])
    .where("deleted_at", "is", null)
    .where("source", "=", "github")
    .where("github_full_name", "is not", null)
    .execute();
  const toDelete = rows.filter((r) => !githubFullNames.has(r.github_full_name as string));
  if (toDelete.length === 0) return;
  for (const project of toDelete) {
    await db
      .updateTable("project_history")
      .set({ deleted_at: now })
      .where("project_id", "=", project.id)
      .where("deleted_at", "is", null)
      .execute();
    await db.updateTable("projects").set({ deleted_at: now }).where("id", "=", project.id).execute();
  }
}

export async function needsReindex(): Promise<boolean> {
  const row = await getDb()
    .selectFrom("projects")
    .select((eb) => eb.fn.max("last_scanned").as("latest"))
    .where("deleted_at", "is", null)
    .executeTakeFirst();
  if (!row?.latest) return true;
  return Date.now() - (row.latest as number) > loadConfig().reindexIntervalMs;
}

export async function touchProject(path: string): Promise<void> {
  await getDb().updateTable("projects").set({ last_scanned: Date.now() }).where("path", "=", path).execute();
}

export async function removeProject(path: string): Promise<boolean> {
  const db = getDb();
  const now = Date.now();
  const project = await db
    .selectFrom("projects")
    .select(["id"])
    .where("path", "=", path)
    .where("deleted_at", "is", null)
    .executeTakeFirst();
  if (!project) return false;

  await db
    .updateTable("project_history")
    .set({ deleted_at: now })
    .where("project_id", "=", project.id)
    .where("deleted_at", "is", null)
    .execute();
  await db.updateTable("projects").set({ deleted_at: now }).where("id", "=", project.id).execute();
  return true;
}

export async function getProjectsCount(): Promise<number> {
  const row = await getDb()
    .selectFrom("projects")
    .select((eb) => eb.fn.countAll().as("count"))
    .where("is_git_repo", "=", 1)
    .where("deleted_at", "is", null)
    .executeTakeFirstOrThrow();
  return Number(row.count);
}
