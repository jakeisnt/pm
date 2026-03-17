-- Systems: multi-device support
CREATE TABLE IF NOT EXISTS systems (
  id TEXT PRIMARY KEY,
  hostname TEXT NOT NULL,
  created_at INTEGER NOT NULL DEFAULT (unixepoch() * 1000),
  updated_at INTEGER NOT NULL DEFAULT (unixepoch() * 1000),
  deleted_at INTEGER
);

CREATE INDEX IF NOT EXISTS idx_systems_hostname ON systems(hostname);

-- Settings: key-value configuration store
CREATE TABLE IF NOT EXISTS settings (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL,
  created_at INTEGER NOT NULL DEFAULT (unixepoch() * 1000),
  updated_at INTEGER NOT NULL DEFAULT (unixepoch() * 1000),
  deleted_at INTEGER,
  system_id TEXT NOT NULL DEFAULT ''
);

-- Projects: indexed project directory
CREATE TABLE IF NOT EXISTS projects (
  id TEXT PRIMARY KEY,
  path TEXT UNIQUE NOT NULL,
  name TEXT NOT NULL,
  last_scanned INTEGER NOT NULL,
  last_modified INTEGER NOT NULL,
  is_git_repo INTEGER NOT NULL DEFAULT 1,
  file_count INTEGER,
  size_bytes INTEGER,
  created_at INTEGER NOT NULL DEFAULT (unixepoch() * 1000),
  updated_at INTEGER NOT NULL DEFAULT (unixepoch() * 1000),
  deleted_at INTEGER,
  source TEXT DEFAULT 'local',
  github_full_name TEXT,
  scope TEXT NOT NULL DEFAULT 'personal',
  system_id TEXT NOT NULL DEFAULT ''
);

CREATE INDEX IF NOT EXISTS idx_projects_path ON projects(path);
CREATE INDEX IF NOT EXISTS idx_projects_last_scanned ON projects(last_scanned);
CREATE INDEX IF NOT EXISTS idx_projects_source ON projects(source);

-- Project history: track recent opens
CREATE TABLE IF NOT EXISTS project_history (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id),
  opened_at INTEGER NOT NULL,
  created_at INTEGER NOT NULL DEFAULT (unixepoch() * 1000),
  updated_at INTEGER NOT NULL DEFAULT (unixepoch() * 1000),
  deleted_at INTEGER,
  system_id TEXT NOT NULL DEFAULT ''
);

CREATE INDEX IF NOT EXISTS idx_project_history_project_id ON project_history(project_id);
CREATE INDEX IF NOT EXISTS idx_project_history_opened_at ON project_history(opened_at);

-- Project dev config: cached dev commands
CREATE TABLE IF NOT EXISTS project_dev_config (
  project_path TEXT PRIMARY KEY,
  package_manager TEXT NOT NULL,
  dev_command TEXT NOT NULL,
  created_at INTEGER NOT NULL DEFAULT (unixepoch() * 1000),
  updated_at INTEGER NOT NULL DEFAULT (unixepoch() * 1000),
  deleted_at INTEGER,
  system_id TEXT NOT NULL DEFAULT ''
);

