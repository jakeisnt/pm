import { getDb } from "./database.ts";

export interface Org {
  name: string;
  hidden: boolean;
}

export const LOCAL_ORG = "_local";

export function extractOrgName(githubFullName: string | null | undefined): string {
  if (!githubFullName) return LOCAL_ORG;
  const org = githubFullName.split("/")[0];
  return org ? org.toLowerCase() : LOCAL_ORG;
}

export async function getOrgs(): Promise<Org[]> {
  const rows = await getDb()
    .selectFrom("orgs")
    .select(["name", "hidden"])
    .where("deleted_at", "is", null)
    .orderBy("name", "asc")
    .execute();
  return rows.map((r) => ({ name: r.name, hidden: r.hidden === 1 }));
}

export async function ensureOrg(name: string): Promise<void> {
  await getDb()
    .insertInto("orgs")
    .values({ name: name.toLowerCase() })
    .onConflict((oc) =>
      oc.column("name").doUpdateSet({
        deleted_at: null,
        updated_at: Date.now(),
      }),
    )
    .execute();
}

export async function setOrgHidden(name: string, hidden: boolean): Promise<boolean> {
  const normalized = name.toLowerCase();
  // Ensure org exists first
  await ensureOrg(normalized);
  const result = await getDb()
    .updateTable("orgs")
    .set({ hidden: hidden ? 1 : 0, updated_at: Date.now() })
    .where("name", "=", normalized)
    .where("deleted_at", "is", null)
    .execute();
  return result.length > 0 && Number(result[0]?.numUpdatedRows ?? 0) > 0;
}

export async function removeOrg(name: string): Promise<boolean> {
  const normalized = name.toLowerCase();
  const result = await getDb()
    .updateTable("orgs")
    .set({ deleted_at: Date.now() })
    .where("name", "=", normalized)
    .where("deleted_at", "is", null)
    .execute();
  return result.length > 0 && Number(result[0]?.numUpdatedRows ?? 0) > 0;
}
