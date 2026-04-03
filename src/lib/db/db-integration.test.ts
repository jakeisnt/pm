import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { join } from "node:path";
import { _resetForTesting } from "./database.ts";
import { ensureOrg, getOrgs, removeOrg, setOrgHidden } from "./orgs.ts";
import {
  cleanupNotOnGithub,
  getCachedProjects,
  getProjectsCount,
  removeProject,
  touchProject,
  upsertProject,
  upsertProjects,
} from "./projects.ts";
import { getRecentProjects, touchRecentProject } from "./recent.ts";

const MIGRATIONS_DIR = join(import.meta.dir, "..", "..", "..", "migrations");

beforeEach(() => {
  _resetForTesting(MIGRATIONS_DIR);
});

afterEach(() => {
  // Each test gets a fresh in-memory DB via beforeEach, no cleanup needed
});

// ─── Orgs ────────────────────────────────────────────────────────────────

describe("orgs", () => {
  test("ensureOrg creates an org", async () => {
    await ensureOrg("testorg");
    const orgs = await getOrgs();
    const found = orgs.find((o) => o.name === "testorg");
    expect(found).toBeDefined();
    expect(found?.hidden).toBe(false);
  });

  test("ensureOrg is idempotent", async () => {
    await ensureOrg("testorg");
    await ensureOrg("testorg");
    const orgs = await getOrgs();
    const matches = orgs.filter((o) => o.name === "testorg");
    expect(matches).toHaveLength(1);
  });

  test("setOrgHidden hides and shows an org", async () => {
    await ensureOrg("myorg");
    await setOrgHidden("myorg", true);
    let orgs = await getOrgs();
    expect(orgs.find((o) => o.name === "myorg")?.hidden).toBe(true);

    await setOrgHidden("myorg", false);
    orgs = await getOrgs();
    expect(orgs.find((o) => o.name === "myorg")?.hidden).toBe(false);
  });

  test("removeOrg soft-deletes an org", async () => {
    await ensureOrg("deadorg");
    const removed = await removeOrg("deadorg");
    expect(removed).toBe(true);
    const orgs = await getOrgs();
    expect(orgs.find((o) => o.name === "deadorg")).toBeUndefined();
  });

  test("removeOrg returns false for nonexistent org", async () => {
    const removed = await removeOrg("nope");
    expect(removed).toBe(false);
  });
});

// ─── Projects ────────────────────────────────────────────────────────────

describe("projects", () => {
  test("upsertProject inserts and retrieves a project", async () => {
    await upsertProject({
      path: "/tmp/test-project",
      name: "test-project",
      source: "local",
    });
    const projects = await getCachedProjects();
    const found = projects.find((p) => p.path === "/tmp/test-project");
    expect(found).toBeDefined();
    expect(found?.name).toBe("test-project");
    expect(found?.source).toBe("local");
  });

  test("upsertProject updates on conflict", async () => {
    await upsertProject({
      path: "/tmp/test-project",
      name: "old-name",
      source: "local",
    });
    await upsertProject({
      path: "/tmp/test-project",
      name: "new-name",
      source: "local",
    });
    const projects = await getCachedProjects();
    const matches = projects.filter((p) => p.path === "/tmp/test-project");
    expect(matches).toHaveLength(1);
    expect(matches[0]?.name).toBe("new-name");
  });

  test("upsertProjects handles batch insert", async () => {
    const batch = Array.from({ length: 75 }, (_, i) => ({
      path: `/tmp/project-${i}`,
      name: `project-${i}`,
      source: "local" as const,
    }));
    await upsertProjects(batch);
    const count = await getProjectsCount();
    expect(count).toBe(75);
  });

  test("getCachedProjects filters by source", async () => {
    await upsertProject({ path: "/tmp/local-proj", name: "local-proj", source: "local" });
    await upsertProject({
      path: "github://example-user/remote",
      name: "remote",
      source: "github",
      githubFullName: "example-user/remote",
    });

    const local = await getCachedProjects("local");
    const github = await getCachedProjects("github");
    expect(local.every((p) => p.source === "local")).toBe(true);
    expect(github.every((p) => p.source === "github")).toBe(true);
  });

  test("getCachedProjects excludes hidden orgs", async () => {
    await upsertProject({
      path: "github://hiddenorg/repo",
      name: "repo",
      source: "github",
      githubFullName: "hiddenorg/repo",
    });
    await setOrgHidden("hiddenorg", true);

    const projects = await getCachedProjects();
    expect(projects.find((p) => p.path === "github://hiddenorg/repo")).toBeUndefined();
  });

  test("removeProject soft-deletes a project", async () => {
    await upsertProject({ path: "/tmp/doomed", name: "doomed", source: "local" });
    const removed = await removeProject("/tmp/doomed");
    expect(removed).toBe(true);
    const projects = await getCachedProjects();
    expect(projects.find((p) => p.path === "/tmp/doomed")).toBeUndefined();
  });

  test("removeProject returns false for nonexistent", async () => {
    const removed = await removeProject("/tmp/nonexistent");
    expect(removed).toBe(false);
  });

  test("touchProject updates last_scanned", async () => {
    await upsertProject({ path: "/tmp/touch-test", name: "touch-test", source: "local" });
    const before = (await getCachedProjects()).find((p) => p.path === "/tmp/touch-test")?.lastOpened;
    // Small delay to ensure timestamp differs
    await new Promise((r) => setTimeout(r, 5));
    await touchProject("/tmp/touch-test");
    const after = (await getCachedProjects()).find((p) => p.path === "/tmp/touch-test")?.lastOpened;
    expect(after).toBeDefined();
    expect(before).toBeDefined();
    if (after !== undefined && before !== undefined) {
      expect(after).toBeGreaterThanOrEqual(before);
    }
  });

  test("getProjectsCount counts active projects", async () => {
    await upsertProject({ path: "/tmp/a", name: "a", source: "local" });
    await upsertProject({ path: "/tmp/b", name: "b", source: "local" });
    expect(await getProjectsCount()).toBe(2);
    await removeProject("/tmp/a");
    expect(await getProjectsCount()).toBe(1);
  });
});

// ─── cleanupNotOnGithub ──────────────────────────────────────────────────

describe("cleanupNotOnGithub", () => {
  test("soft-deletes github-only projects not in the provided set", async () => {
    await upsertProject({
      path: "github://owner/kept",
      name: "kept",
      source: "github",
      githubFullName: "owner/kept",
    });
    await upsertProject({
      path: "github://owner/removed",
      name: "removed",
      source: "github",
      githubFullName: "owner/removed",
    });

    await cleanupNotOnGithub(new Set(["owner/kept"]));

    const projects = await getCachedProjects();
    expect(projects.find((p) => p.name === "kept")).toBeDefined();
    expect(projects.find((p) => p.name === "removed")).toBeUndefined();
  });

  test("does NOT delete local projects whose github_full_name is missing from the set", async () => {
    // Regression: cleanupNotOnGithub previously queried all projects with
    // a github_full_name, including local clones. Local clones of repos
    // not owned by the user would get soft-deleted during GitHub reindex.
    await upsertProject({
      path: "/home/user/projects/someone-elses-repo",
      name: "someone-elses-repo",
      source: "local",
      githubFullName: "other-owner/someone-elses-repo",
    });
    await upsertProject({
      path: "github://myorg/my-repo",
      name: "my-repo",
      source: "github",
      githubFullName: "myorg/my-repo",
    });

    // GitHub API only returns myorg/my-repo — other-owner/someone-elses-repo is NOT in the set
    await cleanupNotOnGithub(new Set(["myorg/my-repo"]));

    const projects = await getCachedProjects();
    const local = projects.find((p) => p.path === "/home/user/projects/someone-elses-repo");
    expect(local).toBeDefined();
    expect(local?.source).toBe("local");
  });

  test("does NOT delete local projects even when github set is empty", async () => {
    await upsertProject({
      path: "/home/user/projects/forked-repo",
      name: "forked-repo",
      source: "local",
      githubFullName: "upstream/forked-repo",
    });

    await cleanupNotOnGithub(new Set());

    const projects = await getCachedProjects();
    expect(projects.find((p) => p.name === "forked-repo")).toBeDefined();
  });

  test("only deletes github source projects when both local and github entries share the same github_full_name", async () => {
    // A project can exist as both a github placeholder and a local clone
    await upsertProject({
      path: "github://myorg/dual-repo",
      name: "dual-repo",
      source: "github",
      githubFullName: "myorg/dual-repo",
    });
    await upsertProject({
      path: "/home/user/projects/dual-repo",
      name: "dual-repo",
      source: "local",
      githubFullName: "myorg/dual-repo",
    });

    // Simulate: myorg/dual-repo is no longer on GitHub
    await cleanupNotOnGithub(new Set());

    const projects = await getCachedProjects();
    const local = projects.find((p) => p.path === "/home/user/projects/dual-repo");
    const github = projects.find((p) => p.path === "github://myorg/dual-repo");
    expect(local).toBeDefined();
    expect(github).toBeUndefined();
  });
});

// ─── upsert resilience ──────────────────────────────────────────────────

describe("upsert resilience", () => {
  test("upsert revives a soft-deleted project found again on disk", async () => {
    await upsertProject({ path: "/tmp/revive", name: "revive", source: "local" });
    await removeProject("/tmp/revive");
    expect((await getCachedProjects()).find((p) => p.path === "/tmp/revive")).toBeUndefined();

    // Re-discovered during filesystem scan
    await upsertProject({ path: "/tmp/revive", name: "revive", source: "local" });
    expect((await getCachedProjects()).find((p) => p.path === "/tmp/revive")).toBeDefined();
  });

  test("upsert does not downgrade github_full_name to null", async () => {
    await upsertProject({
      path: "/tmp/gh-proj",
      name: "gh-proj",
      source: "local",
      githubFullName: "owner/gh-proj",
    });

    // Rescan finds the project but fails to extract github name
    await upsertProject({ path: "/tmp/gh-proj", name: "gh-proj", source: "local" });

    const projects = await getCachedProjects();
    const found = projects.find((p) => p.path === "/tmp/gh-proj");
    expect(found?.githubFullName).toBe("owner/gh-proj");
  });

  test("upsert does not downgrade org_name to _local", async () => {
    await upsertProject({
      path: "/tmp/org-proj",
      name: "org-proj",
      source: "local",
      githubFullName: "myorg/org-proj",
    });

    // Rescan finds the project but fails to extract github name → org falls back to _local
    await upsertProject({ path: "/tmp/org-proj", name: "org-proj", source: "local" });

    // Project should still be visible (not silently moved to _local)
    const projects = await getCachedProjects();
    const found = projects.find((p) => p.path === "/tmp/org-proj");
    expect(found).toBeDefined();
  });

  test("setOrgHidden refuses to hide _local org", async () => {
    const result = await setOrgHidden("_local", true);
    expect(result).toBe(false);
    const orgs = await getOrgs();
    expect(orgs.find((o) => o.name === "_local")?.hidden).toBe(false);
  });
});

// ─── Recent ──────────────────────────────────────────────────────────────

describe("recent", () => {
  test("touchRecentProject records and retrieves history", async () => {
    await touchRecentProject("/tmp/recent-test", "recent-test");
    const recent = await getRecentProjects();
    expect(recent.find((r) => r.path === "/tmp/recent-test")).toBeDefined();
  });

  test("recent projects are ordered by most recent first", async () => {
    await touchRecentProject("/tmp/old", "old", 1000);
    await touchRecentProject("/tmp/new", "new", 2000);
    const recent = await getRecentProjects();
    const oldIdx = recent.findIndex((r) => r.path === "/tmp/old");
    const newIdx = recent.findIndex((r) => r.path === "/tmp/new");
    expect(newIdx).toBeLessThan(oldIdx);
  });

  test("touchRecentProject creates project if it doesn't exist", async () => {
    await touchRecentProject("/tmp/auto-created", "auto-created");
    const projects = await getCachedProjects();
    expect(projects.find((p) => p.path === "/tmp/auto-created")).toBeDefined();
  });
});
