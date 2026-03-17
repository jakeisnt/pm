import { Database } from "bun:sqlite";
import { readdirSync, readFileSync, unlinkSync } from "node:fs";
import { join } from "node:path";
import { Kysely } from "kysely";
import { BunSqliteDialect } from "kysely-bun-sqlite";
import { createSpinner } from "nanospinner";
import { getDbPath } from "../paths.ts";
import type { DB } from "./schema.ts";
import { ensureSystemId } from "./systems.ts";

const MIGRATIONS_DIR = join(import.meta.dir, "..", "..", "..", "migrations");

function throwDatabaseAccessError(dbPath: string, cause: unknown): never {
  const msg = cause instanceof Error ? cause.message : String(cause);
  const isPermission = /permission|EPERM|EACCES|readonly|not permitted|sandbox/i.test(msg);

  const lines = [`Failed to open database: ${dbPath}`, ""];
  if (isPermission) {
    lines.push(
      "This looks like a permissions issue. If you're running inside Claude Code,",
      "the sandbox may be blocking access. Add this to ~/.claude/settings.json:",
      "",
      '  "sandbox": {',
      '    "filesystem": {',
      '      "allowWrite": ["~/Library/Application Support/pm"]',
      "    }",
      "  }",
    );
  } else {
    lines.push(`Error: ${msg}`);
  }

  const error = new Error(lines.join("\n"), { cause });
  error.name = "DatabaseAccessError";
  throw error;
}

let _db: Kysely<DB> | null = null;
let _rawDb: Database | null = null;

function tableExists(db: Database, name: string): boolean {
  const row = db.query("SELECT 1 FROM sqlite_master WHERE type='table' AND name=?").get(name);
  return row !== null;
}

export function runMigrationsFromDir(db: Database, migrationsDir: string): void {
  db.run(`
    CREATE TABLE IF NOT EXISTS schema_migrations (
      id INTEGER PRIMARY KEY,
      name TEXT NOT NULL UNIQUE,
      applied_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
    )
  `);

  const applied = new Set(
    (db.query("SELECT name FROM schema_migrations").all() as { name: string }[]).map((r) => r.name),
  );

  const files = readdirSync(migrationsDir)
    .filter((f) => f.endsWith(".sql"))
    .sort();

  const pending = files.filter((f) => !applied.has(f));
  if (pending.length === 0) return;

  const spinner = createSpinner(`Running ${pending.length} migration(s)...`).start();
  for (const file of pending) {
    spinner.update({ text: `Migrating: ${file}...` });
    const sql = readFileSync(join(migrationsDir, file), "utf-8");

    db.transaction(() => {
      if (sql.trim().length > 0) {
        db.exec(sql);
      }
      db.run("INSERT INTO schema_migrations (name) VALUES (?)", [file]);
    })();
  }
  spinner.success({ text: `Applied ${pending.length} migration(s)` });
}

function runMigrations(db: Database): void {
  runMigrationsFromDir(db, MIGRATIONS_DIR);
}

function removeWalFiles(dbPath: string): void {
  try {
    unlinkSync(`${dbPath}-wal`);
  } catch {}
  try {
    unlinkSync(`${dbPath}-shm`);
  } catch {}
}

function initRawDb(): Database {
  if (_rawDb) return _rawDb;

  let dbPath: string;
  try {
    dbPath = getDbPath();
  } catch (e: unknown) {
    throwDatabaseAccessError("~/Library/Application Support/pm/pm.db", e);
  }

  try {
    _rawDb = new Database(dbPath);
    _rawDb.run("PRAGMA journal_mode=WAL");
    runMigrations(_rawDb);
    // Ensure system record exists
    if (tableExists(_rawDb, "systems")) {
      ensureSystemId(_rawDb);
    }
  } catch (e: unknown) {
    const sqliteErr = e as { code?: string; errno?: number };
    if (sqliteErr.code === "SQLITE_IOERR_SHORT_READ" || sqliteErr.errno === 522) {
      _rawDb = null;
      removeWalFiles(dbPath);
      _rawDb = new Database(dbPath);
      _rawDb.run("PRAGMA journal_mode=WAL");
      runMigrations(_rawDb);
      if (tableExists(_rawDb, "systems")) {
        ensureSystemId(_rawDb);
      }
    } else {
      throwDatabaseAccessError(dbPath, e);
    }
  }
  return _rawDb as Database;
}

export function getDb(): Kysely<DB> {
  if (_db) return _db;
  const rawDb = initRawDb();
  _db = new Kysely<DB>({
    dialect: new BunSqliteDialect({ database: rawDb }),
  });
  return _db;
}

export function getRawDb(): Database {
  return initRawDb();
}

export async function closeDatabase(): Promise<void> {
  if (_db) {
    await _db.destroy();
    _db = null;
  }
  if (_rawDb) {
    _rawDb.close();
    _rawDb = null;
  }
}

/** @internal Test-only: replace the singleton with an in-memory database. */
export function _resetForTesting(migrationsDir: string): { db: Kysely<DB>; rawDb: Database } {
  if (_db) {
    void _db.destroy();
    _db = null;
  }
  if (_rawDb) {
    _rawDb.close();
    _rawDb = null;
  }
  const rawDb = new Database(":memory:");
  rawDb.run("PRAGMA journal_mode=WAL");
  runMigrationsFromDir(rawDb, migrationsDir);
  if (tableExists(rawDb, "systems")) {
    ensureSystemId(rawDb);
  }
  _rawDb = rawDb;
  _db = new Kysely<DB>({ dialect: new BunSqliteDialect({ database: rawDb }) });
  return { db: _db, rawDb };
}
