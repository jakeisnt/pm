import type { Database } from "bun:sqlite";
import { hostname } from "node:os";
import { generateId } from "./uuid.ts";

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

  const id = generateId();
  db.run("INSERT OR IGNORE INTO systems (id, hostname) VALUES (?, ?)", [id, systemName]);
  cached = { hostname: systemName, id };
  return id;
}
