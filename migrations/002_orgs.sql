-- Orgs: tracked GitHub organizations / project owners
CREATE TABLE IF NOT EXISTS orgs (
  name TEXT PRIMARY KEY,
  hidden INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL DEFAULT (unixepoch() * 1000),
  updated_at INTEGER NOT NULL DEFAULT (unixepoch() * 1000),
  deleted_at INTEGER
);

-- Add org column to projects (no FK constraint — SQLite ALTER TABLE doesn't support it)
ALTER TABLE projects ADD COLUMN org_name TEXT NOT NULL DEFAULT '_local';

CREATE INDEX IF NOT EXISTS idx_projects_org_name ON projects(org_name);

-- Seed the _local org for projects without a GitHub full name
INSERT OR IGNORE INTO orgs (name) VALUES ('_local');

-- Backfill: create org rows from existing github_full_name values
INSERT OR IGNORE INTO orgs (name)
  SELECT DISTINCT lower(substr(github_full_name, 1, instr(github_full_name, '/') - 1))
  FROM projects
  WHERE github_full_name IS NOT NULL
    AND instr(github_full_name, '/') > 0
    AND deleted_at IS NULL;

-- Backfill: set org_name on existing projects that have a github_full_name
UPDATE projects
  SET org_name = lower(substr(github_full_name, 1, instr(github_full_name, '/') - 1))
  WHERE github_full_name IS NOT NULL
    AND instr(github_full_name, '/') > 0;
