import type { Database } from "bun:sqlite";
import { hostname } from "node:os";
import { getRawDb } from "./database.ts";

export interface SystemRecord {
  id: string;
  hostname: string;
  created_at: number;
  updated_at: number;
  deleted_at: number | null;
}

let cached: { hostname: string; id: string } | null = null;

export function getCurrentSystemName(): string {
  const full = hostname();
  return full.split(".")[0] ?? full;
}

export function ensureSystemId(db: Database): string {
  const systemName = getCurrentSystemName();
  if (cached && cached.hostname === systemName) {
    return cached.id;
  }

  const existing = db
    .query("SELECT id FROM systems WHERE hostname = ? ORDER BY created_at DESC LIMIT 1")
    .get(systemName) as {
    id: string;
  } | null;
  if (existing?.id) {
    cached = { hostname: systemName, id: existing.id };
    return existing.id;
  }

  const id = crypto.randomUUID();
  db.run("INSERT OR IGNORE INTO systems (id, hostname) VALUES (?, ?)", [id, systemName]);
  cached = { hostname: systemName, id };
  return id;
}

export function getCurrentSystemId(): string {
  return ensureSystemId(getRawDb());
}

export function listSystems(): SystemRecord[] {
  return getRawDb()
    .query("SELECT id, hostname, created_at, updated_at, deleted_at FROM systems ORDER BY hostname")
    .all() as SystemRecord[];
}
