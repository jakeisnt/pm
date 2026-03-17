import type { Generated } from "kysely";

export interface SystemsTable {
  id: string;
  hostname: string;
  created_at: Generated<number>;
  updated_at: Generated<number>;
  deleted_at: number | null;
}

export interface SettingsTable {
  key: string;
  value: string;
  created_at: Generated<number>;
  updated_at: Generated<number>;
  deleted_at: number | null;
  system_id: string;
}

export interface ProjectTable {
  id: string;
  path: string;
  name: string;
  last_scanned: number;
  last_modified: number;
  is_git_repo: number;
  file_count: number | null;
  size_bytes: number | null;
  created_at: Generated<number>;
  updated_at: Generated<number>;
  deleted_at: number | null;
  source: Generated<string>;
  github_full_name: string | null;
  scope: Generated<string>;
  org_name: Generated<string>;
  system_id: string;
}

export interface ProjectHistoryTable {
  id: string;
  project_id: string;
  opened_at: number;
  created_at: Generated<number>;
  updated_at: Generated<number>;
  deleted_at: number | null;
  system_id: string;
}

export interface ProjectDevConfigTable {
  project_path: string;
  package_manager: string;
  dev_command: string;
  created_at: Generated<number>;
  updated_at: Generated<number>;
  deleted_at: number | null;
  system_id: string;
}

export interface OrgsTable {
  name: string;
  hidden: Generated<number>;
  created_at: Generated<number>;
  updated_at: Generated<number>;
  deleted_at: number | null;
}

export interface DB {
  systems: SystemsTable;
  settings: SettingsTable;
  projects: ProjectTable;
  project_history: ProjectHistoryTable;
  project_dev_config: ProjectDevConfigTable;
  orgs: OrgsTable;
}
