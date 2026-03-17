import { generateId, getCurrentSystemId, getDb } from "./db/index.ts";

export interface RepoMemory {
  id: string;
  repoPath: string;
  repoName: string;
  category: string;
  key: string;
  value: string;
  source: string;
  sourceRef: string | null;
  tags: string[];
  createdAt: number;
  updatedAt: number;
}

export interface RepoMemoryInput {
  repoPath: string;
  repoName: string;
  category: string;
  key: string;
  value: string;
  source?: string;
  sourceRef?: string;
  tags?: string[];
}

export const REPO_MEMORY_CATEGORIES = [
  "architecture",
  "convention",
  "gotcha",
  "pattern",
  "dependency",
  "workflow",
  "debug-tip",
  "key-file",
  "other",
] as const;

export const REPO_MEMORY_SOURCES = ["manual", "agent", "learn"] as const;

function rowToMemory(row: {
  id: string;
  repo_path: string;
  repo_name: string;
  category: string;
  key: string;
  value: string;
  source: string;
  source_ref: string | null;
  tags: string | null;
  created_at: number;
  updated_at: number;
}): RepoMemory {
  return {
    id: row.id,
    repoPath: row.repo_path,
    repoName: row.repo_name,
    category: row.category,
    key: row.key,
    value: row.value,
    source: row.source,
    sourceRef: row.source_ref,
    tags: row.tags ? row.tags.split(",").map((t) => t.trim()) : [],
    createdAt: row.created_at,
    updatedAt: row.updated_at,
  };
}

export async function listRepoMemories(opts?: {
  repoPath?: string;
  category?: string;
  source?: string;
  search?: string;
  tag?: string;
}): Promise<RepoMemory[]> {
  let query = getDb()
    .selectFrom("repo_memories")
    .selectAll()
    .where("deleted_at", "is", null)
    .orderBy("updated_at", "desc");

  if (opts?.repoPath) {
    query = query.where("repo_path", "=", opts.repoPath);
  }
  if (opts?.category) {
    query = query.where("category", "=", opts.category);
  }
  if (opts?.source) {
    query = query.where("source", "=", opts.source);
  }
  if (opts?.tag) {
    query = query.where("tags", "like", `%${opts.tag}%`);
  }
  if (opts?.search) {
    const term = `%${opts.search}%`;
    query = query.where((eb) => eb.or([eb("key", "like", term), eb("value", "like", term), eb("tags", "like", term)]));
  }

  const rows = await query.execute();
  return rows.map(rowToMemory);
}

export async function getRepoMemory(id: string): Promise<RepoMemory | null> {
  const row = await getDb()
    .selectFrom("repo_memories")
    .selectAll()
    .where("id", "like", `${id}%`)
    .where("deleted_at", "is", null)
    .executeTakeFirst();
  return row ? rowToMemory(row) : null;
}

export async function addRepoMemory(input: RepoMemoryInput): Promise<RepoMemory> {
  const now = Date.now();
  const id = generateId();
  const systemId = getCurrentSystemId();
  await getDb()
    .insertInto("repo_memories")
    .values({
      id,
      repo_path: input.repoPath,
      repo_name: input.repoName,
      category: input.category,
      key: input.key,
      value: input.value,
      source: input.source ?? "manual",
      source_ref: input.sourceRef ?? null,
      tags: input.tags?.join(",") ?? null,
      created_at: now,
      updated_at: now,
      deleted_at: null,
      system_id: systemId,
    })
    .execute();

  const mem = await getRepoMemory(id);
  if (!mem) throw new Error("Failed to retrieve newly created repo memory");
  return mem;
}

export async function addRepoMemories(inputs: RepoMemoryInput[]): Promise<number> {
  const now = Date.now();
  const systemId = getCurrentSystemId();
  let count = 0;
  for (const input of inputs) {
    const existing = await getDb()
      .selectFrom("repo_memories")
      .select(["id"])
      .where("key", "=", input.key)
      .where("category", "=", input.category)
      .where("repo_path", "=", input.repoPath)
      .where("deleted_at", "is", null)
      .executeTakeFirst();

    if (existing) {
      await getDb()
        .updateTable("repo_memories")
        .set({
          value: input.value,
          source: input.source ?? "manual",
          source_ref: input.sourceRef ?? null,
          tags: input.tags?.join(",") ?? null,
          updated_at: now,
        })
        .where("id", "=", existing.id)
        .execute();
    } else {
      await getDb()
        .insertInto("repo_memories")
        .values({
          id: generateId(),
          repo_path: input.repoPath,
          repo_name: input.repoName,
          category: input.category,
          key: input.key,
          value: input.value,
          source: input.source ?? "manual",
          source_ref: input.sourceRef ?? null,
          tags: input.tags?.join(",") ?? null,
          created_at: now,
          updated_at: now,
          deleted_at: null,
          system_id: systemId,
        })
        .execute();
    }
    count++;
  }
  return count;
}

export async function removeRepoMemory(id: string): Promise<boolean> {
  const existing = await getRepoMemory(id);
  if (!existing) return false;
  const result = await getDb()
    .updateTable("repo_memories")
    .set({ deleted_at: Date.now() })
    .where("id", "=", existing.id)
    .where("deleted_at", "is", null)
    .execute();
  return result.length > 0 && BigInt(result[0]?.numUpdatedRows ?? 0) > 0n;
}

export async function clearRepoMemories(opts?: {
  repoPath?: string;
  category?: string;
  source?: string;
}): Promise<number> {
  let query = getDb().updateTable("repo_memories").set({ deleted_at: Date.now() }).where("deleted_at", "is", null);
  if (opts?.repoPath) {
    query = query.where("repo_path", "=", opts.repoPath);
  }
  if (opts?.category) {
    query = query.where("category", "=", opts.category);
  }
  if (opts?.source) {
    query = query.where("source", "=", opts.source);
  }
  const result = await query.execute();
  return Number(result[0]?.numUpdatedRows ?? 0);
}

export async function formatRepoMemoriesForPrompt(repoPath: string): Promise<string | null> {
  const memories = await listRepoMemories({ repoPath });
  if (memories.length === 0) return null;

  const grouped = new Map<string, RepoMemory[]>();
  for (const m of memories) {
    const existing = grouped.get(m.category) ?? [];
    existing.push(m);
    grouped.set(m.category, existing);
  }

  const first = memories[0];
  if (!first) return null;
  const lines: string[] = [`# Repo Knowledge: ${first.repoName}`, ""];

  for (const [category, items] of grouped) {
    lines.push(`## ${category.charAt(0).toUpperCase() + category.slice(1)}`);
    lines.push("");
    for (const item of items) {
      const tagStr = item.tags.length > 0 ? ` [${item.tags.join(", ")}]` : "";
      lines.push(`- **${item.key}**: ${item.value}${tagStr}`);
    }
    lines.push("");
  }

  return lines.join("\n");
}

export async function getRepoMemoryStats(
  repoPath: string,
): Promise<{ total: number; byCategory: Map<string, number> }> {
  const memories = await listRepoMemories({ repoPath });
  const byCategory = new Map<string, number>();
  for (const m of memories) {
    byCategory.set(m.category, (byCategory.get(m.category) ?? 0) + 1);
  }
  return { total: memories.length, byCategory };
}
