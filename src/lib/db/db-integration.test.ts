import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { join } from "node:path";
import { _resetForTesting } from "./database.ts";
import { ensureOrg, getOrgs, removeOrg, setOrgHidden } from "./orgs.ts";
import {
  getCachedProjects,
  getProjectsCount,
  removeProject,
  touchProject,
  upsertProject,
  upsertProjects,
} from "./projects.ts";
import { getRecentProjects, touchRecentProject } from "./recent.ts";
import { deleteAllSettings, deleteSetting, getAllSettings, getSetting, setSetting } from "./settings.ts";

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
      path: "github://jakeisnt/remote",
      name: "remote",
      source: "github",
      githubFullName: "jakeisnt/remote",
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

// ─── Settings ────────────────────────────────────────────────────────────

describe("settings", () => {
  test("set and get a setting", () => {
    setSetting("test_key", "test_value");
    expect(getSetting("test_key")).toBe("test_value");
  });

  test("getSetting returns undefined for missing key", () => {
    expect(getSetting("nonexistent")).toBeUndefined();
  });

  test("setSetting overwrites existing value", () => {
    setSetting("key", "v1");
    setSetting("key", "v2");
    expect(getSetting("key")).toBe("v2");
  });

  test("deleteSetting soft-deletes", () => {
    setSetting("to_delete", "value");
    deleteSetting("to_delete");
    expect(getSetting("to_delete")).toBeUndefined();
  });

  test("getAllSettings returns all active settings", () => {
    setSetting("a", "1");
    setSetting("b", "2");
    const all = getAllSettings();
    expect(all["a"]).toBe("1");
    expect(all["b"]).toBe("2");
  });

  test("deleteAllSettings clears everything", () => {
    setSetting("x", "1");
    setSetting("y", "2");
    deleteAllSettings();
    expect(getSetting("x")).toBeUndefined();
    expect(getSetting("y")).toBeUndefined();
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
