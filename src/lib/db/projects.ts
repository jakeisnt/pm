import { existsSync } from "node:fs";
import type { Project } from "../../types.ts";
import { getGithubReindexInterval, getReindexInterval } from "../settings.ts";
import { getDb } from "./database.ts";
import { getCurrentSystemId } from "./systems.ts";
import { generateId } from "./uuid.ts";

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
    .select(["path", "name", "last_scanned", "source", "github_full_name"])
    .where("is_git_repo", "=", 1)
    .where("deleted_at", "is", null)
    .orderBy("last_scanned", "desc");

  if (source) {
    query = query.where("source", "=", source);
  }

  const rows = await query.execute();
  return rows.map(toProject);
}

export async function upsertProjects(projects: Project[]): Promise<void> {
  const { inferProjectScope } = await import("../scope.ts");
  const db = getDb();
  const now = Date.now();
  const systemId = getCurrentSystemId();
  for (const p of projects) {
    const inferOpts: { githubFullName?: string; path?: string } = { path: p.path };
    if (p.githubFullName) inferOpts.githubFullName = p.githubFullName;
    const scope = p.scope ?? inferProjectScope(inferOpts);
    await db
      .insertInto("projects")
      .values({
        id: generateId(),
        path: p.path,
        name: p.name,
        last_scanned: now,
        last_modified: now,
        is_git_repo: 1,
        source: p.source || "local",
        github_full_name: p.githubFullName || null,
        scope,
        system_id: systemId,
      })
      .onConflict((oc) =>
        oc.column("path").doUpdateSet({
          name: p.name,
          last_scanned: now,
          source: p.source || "local",
          github_full_name: p.githubFullName || null,
          scope,
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
  return Date.now() - (row.latest as number) > getGithubReindexInterval();
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

  await db
    .insertInto("projects")
    .values({
      id: generateId(),
      path: localPath,
      name: githubFullName.split("/").pop() ?? githubFullName,
      last_scanned: now,
      last_modified: now,
      is_git_repo: 1,
      source: "local",
      github_full_name: githubFullName,
      scope,
      system_id: systemId,
    })
    .onConflict((oc) =>
      oc.column("path").doUpdateSet({
        source: "local",
        github_full_name: githubFullName,
        last_scanned: now,
        scope,
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

export async function cleanupMissing(): Promise<void> {
  const db = getDb();
  const now = Date.now();
  const rows = await db
    .selectFrom("projects")
    .select(["id", "path"])
    .where("source", "=", "local")
    .where("deleted_at", "is", null)
    .execute();
  const toDelete = rows.filter((r) => !existsSync(r.path));
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
  return Date.now() - (row.latest as number) > getReindexInterval();
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
