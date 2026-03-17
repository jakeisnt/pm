import { describe, expect, test } from "bun:test";
import { mkdirSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { findProjects } from "./find-projects.ts";

function makeTempDir(): string {
  const dir = join(tmpdir(), `pm-test-${Date.now()}-${Math.random().toString(36).slice(2)}`);
  mkdirSync(dir, { recursive: true });
  return dir;
}

describe("findProjects", () => {
  test("finds git repos in root directories", () => {
    const root = makeTempDir();
    const projectDir = join(root, "my-project");
    mkdirSync(join(projectDir, ".git"), { recursive: true });

    const projects = findProjects({ roots: [root], maxDepth: 2 });
    expect(projects).toHaveLength(1);
    expect(projects[0]?.path).toBe(projectDir);
    expect(projects[0]?.name).toBe("my-project");
    expect(projects[0]?.source).toBe("local");

    rmSync(root, { recursive: true });
  });

  test("finds nested git repos up to maxDepth", () => {
    const root = makeTempDir();
    mkdirSync(join(root, "a", "b", ".git"), { recursive: true });

    const shallow = findProjects({ roots: [root], maxDepth: 1 });
    expect(shallow).toHaveLength(0);

    const deep = findProjects({ roots: [root], maxDepth: 2 });
    expect(deep).toHaveLength(1);
    expect(deep[0]?.name).toBe("b");

    rmSync(root, { recursive: true });
  });

  test("ignores node_modules directories", () => {
    const root = makeTempDir();
    mkdirSync(join(root, "node_modules", "pkg", ".git"), { recursive: true });
    mkdirSync(join(root, "real-project", ".git"), { recursive: true });

    const projects = findProjects({ roots: [root], maxDepth: 3 });
    expect(projects).toHaveLength(1);
    expect(projects[0]?.name).toBe("real-project");

    rmSync(root, { recursive: true });
  });

  test("deduplicates projects found via multiple roots", () => {
    const root = makeTempDir();
    const projectDir = join(root, "my-project");
    mkdirSync(join(projectDir, ".git"), { recursive: true });

    // Same root listed twice
    const projects = findProjects({ roots: [root, root], maxDepth: 2 });
    expect(projects).toHaveLength(1);

    rmSync(root, { recursive: true });
  });

  test("returns empty array for nonexistent roots", () => {
    const projects = findProjects({ roots: ["/nonexistent/path/xyz"], maxDepth: 2 });
    expect(projects).toHaveLength(0);
  });

  test("does not recurse into .git directory", () => {
    const root = makeTempDir();
    const projectDir = join(root, "my-project");
    mkdirSync(join(projectDir, ".git", "refs"), { recursive: true });

    const projects = findProjects({ roots: [root], maxDepth: 3 });
    expect(projects).toHaveLength(1);

    rmSync(root, { recursive: true });
  });
});
