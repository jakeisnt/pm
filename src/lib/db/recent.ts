import { sql } from "kysely";
import type { RecentEntry } from "../../types.ts";
import { getMaxRecent } from "../settings.ts";
import { getDb } from "./database.ts";
import { getCurrentSystemId } from "./systems.ts";
import { generateId } from "./uuid.ts";

export async function getRecentProjects(): Promise<RecentEntry[]> {
  const latestHistory = getDb()
    .selectFrom("project_history")
    .select(["project_id", sql<number>`max(opened_at)`.as("last_opened")])
    .where("deleted_at", "is", null)
    .groupBy("project_id")
    .as("latest_history");

  const rows = await getDb()
    .selectFrom(latestHistory)
    .innerJoin("projects", "projects.id", "latest_history.project_id")
    .select(["projects.path as path", "projects.name as name", "latest_history.last_opened as last_opened"])
    .where("projects.deleted_at", "is", null)
    .orderBy("latest_history.last_opened", "desc")
    .limit(getMaxRecent())
    .execute();
  return rows.map((r) => ({ path: r.path, name: r.name, lastOpened: r.last_opened }));
}

export async function touchRecentProject(path: string, name: string, openedAt = Date.now()): Promise<void> {
  const db = getDb();
  const systemId = getCurrentSystemId();
  let project = await db
    .selectFrom("projects")
    .select(["id", "name"])
    .where("path", "=", path)
    .where("deleted_at", "is", null)
    .executeTakeFirst();

  if (!project) {
    await db
      .insertInto("projects")
      .values({
        id: generateId(),
        path,
        name,
        last_scanned: openedAt,
        last_modified: openedAt,
        is_git_repo: 1,
        source: "local",
        scope: "personal",
        system_id: systemId,
      })
      .execute();
    project = await db
      .selectFrom("projects")
      .select(["id", "name"])
      .where("path", "=", path)
      .where("deleted_at", "is", null)
      .executeTakeFirstOrThrow();
  } else if (project.name !== name) {
    await db.updateTable("projects").set({ name }).where("id", "=", project.id).execute();
  }

  await db
    .insertInto("project_history")
    .values({ id: generateId(), project_id: project.id, opened_at: openedAt, system_id: systemId })
    .execute();
}
