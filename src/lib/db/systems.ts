import { getRawDb } from "./database.ts";
import { ensureSystemId, getCurrentSystemName } from "./system-tag.ts";

export interface SystemRecord {
  id: string;
  hostname: string;
  created_at: number;
  updated_at: number;
  deleted_at: number | null;
}

export function getCurrentSystemNameDb(): string {
  return getCurrentSystemName();
}

export function getCurrentSystemId(): string {
  return ensureSystemId(getRawDb());
}

export function listSystems(): SystemRecord[] {
  return getRawDb()
    .query("SELECT id, hostname, created_at, updated_at, deleted_at FROM systems ORDER BY hostname")
    .all() as SystemRecord[];
}
