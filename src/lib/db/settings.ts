import { hostname } from "node:os";
import { getRawDb } from "./database.ts";
import { getCurrentSystemId } from "./systems.ts";

export function getDeviceName(): string {
  return hostname().split(".")[0] ?? hostname();
}

function deviceKey(key: string): string {
  return `${getDeviceName()}:${key}`;
}

export function getSetting(key: string): string | undefined {
  const db = getRawDb();
  const deviceRow = db.query("SELECT value FROM settings WHERE key = ? AND deleted_at IS NULL").get(deviceKey(key)) as {
    value: string;
  } | null;
  if (deviceRow) return deviceRow.value;
  const row = db.query("SELECT value FROM settings WHERE key = ? AND deleted_at IS NULL").get(key) as {
    value: string;
  } | null;
  return row?.value;
}

export function setSetting(key: string, value: string, device?: boolean): void {
  const db = getRawDb();
  const storeKey = device ? deviceKey(key) : key;
  db.run("INSERT OR REPLACE INTO settings (key, value, system_id) VALUES (?, ?, ?)", [
    storeKey,
    value,
    getCurrentSystemId(),
  ]);
}

export function deleteSetting(key: string): boolean {
  const db = getRawDb();
  db.run("UPDATE settings SET deleted_at = unixepoch() * 1000 WHERE (key = ? OR key = ?) AND deleted_at IS NULL", [
    key,
    deviceKey(key),
  ]);
  return db.query("SELECT changes() as c").get() !== null;
}

export function getAllSettings(): Record<string, string> {
  const db = getRawDb();
  const rows = db.query("SELECT key, value FROM settings WHERE deleted_at IS NULL").all() as {
    key: string;
    value: string;
  }[];
  const device = getDeviceName();
  const result: Record<string, string> = {};
  for (const row of rows) {
    if (!row.key.includes(":")) {
      result[row.key] = row.value;
    }
  }
  const prefix = `${device}:`;
  for (const row of rows) {
    if (row.key.startsWith(prefix)) {
      result[row.key.slice(prefix.length)] = row.value;
    }
  }
  return result;
}

export function getAllSettingsRaw(): { key: string; value: string }[] {
  const db = getRawDb();
  return db.query("SELECT key, value FROM settings WHERE deleted_at IS NULL ORDER BY key").all() as {
    key: string;
    value: string;
  }[];
}

export function deleteAllSettings(): void {
  const db = getRawDb();
  db.run("UPDATE settings SET deleted_at = unixepoch() * 1000 WHERE deleted_at IS NULL");
}
