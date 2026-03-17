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

export interface RepoMemoryTable {
  id: string;
  repo_path: string;
  repo_name: string;
  category: string;
  key: string;
  value: string;
  source: string;
  source_ref: string | null;
  tags: string | null;
  created_at: Generated<number>;
  updated_at: Generated<number>;
  deleted_at: number | null;
  system_id: string;
}

export interface RepoMemoryEloTable {
  repo_memory_id: string;
  elo_rating: number;
  matches_played: number;
  agent_elo_rating: number;
  agent_matches_played: number;
  created_at: Generated<number>;
  updated_at: number;
  deleted_at: number | null;
  system_id: string;
}

export interface RepoMemoryVoteTable {
  id: string;
  option1_repo_memory_id: string;
  option2_repo_memory_id: string;
  selected_repo_memory_id: string;
  agent: number;
  model: string;
  voted_at: number;
  created_at: Generated<number>;
  updated_at: Generated<number>;
  deleted_at: number | null;
  system_id: string;
}

export interface DB {
  systems: SystemsTable;
  settings: SettingsTable;
  projects: ProjectTable;
  project_history: ProjectHistoryTable;
  project_dev_config: ProjectDevConfigTable;
  repo_memories: RepoMemoryTable;
  repo_memory_elo: RepoMemoryEloTable;
  repo_memory_votes: RepoMemoryVoteTable;
}
