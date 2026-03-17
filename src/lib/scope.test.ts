import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { join } from "node:path";
import { _resetForTesting } from "./db/database.ts";
import { setSetting } from "./db/settings.ts";
import { inferProjectScope } from "./scope.ts";

const MIGRATIONS_DIR = join(import.meta.dir, "..", "..", "migrations");

describe("inferProjectScope (no defaults configured)", () => {
  beforeEach(() => {
    _resetForTesting(MIGRATIONS_DIR);
  });

  test("returns 'personal' with no settings and no inputs", () => {
    expect(inferProjectScope({})).toBe("personal");
  });

  test("returns 'personal' for any org when no work_orgs configured", () => {
    expect(inferProjectScope({ githubFullName: "acme/some-repo" })).toBe("personal");
  });

  test("returns 'personal' for any path when no work_path_prefixes configured", () => {
    expect(inferProjectScope({ path: "/home/user/projects/some-project" })).toBe("personal");
  });
});

describe("inferProjectScope (settings-driven)", () => {
  beforeEach(() => {
    _resetForTesting(MIGRATIONS_DIR);
  });

  afterEach(() => {
    // Each test gets a fresh in-memory DB via beforeEach, no cleanup needed
  });

  test("work_orgs setting overrides default org list", () => {
    setSetting("work_orgs", "acme");
    expect(inferProjectScope({ githubFullName: "acme/tool" })).toBe("work");
    expect(inferProjectScope({ githubFullName: "other/tool" })).toBe("personal");
  });

  test("work_orgs setting supports multiple comma-separated values", () => {
    setSetting("work_orgs", "acme, widgets, corp");
    expect(inferProjectScope({ githubFullName: "acme/x" })).toBe("work");
    expect(inferProjectScope({ githubFullName: "widgets/x" })).toBe("work");
    expect(inferProjectScope({ githubFullName: "corp/x" })).toBe("work");
    expect(inferProjectScope({ githubFullName: "other/x" })).toBe("personal");
  });

  test("work_orgs setting with org names is still case-insensitive", () => {
    setSetting("work_orgs", "acme");
    expect(inferProjectScope({ githubFullName: "ACME/repo" })).toBe("work");
    expect(inferProjectScope({ githubFullName: "Acme/repo" })).toBe("work");
  });

  test("work_path_prefixes setting overrides default path list", () => {
    setSetting("work_path_prefixes", "/work/projects");
    expect(inferProjectScope({ path: "/work/projects/my-app" })).toBe("work");
    expect(inferProjectScope({ path: "/home/user/other/old" })).toBe("personal");
  });

  test("work_path_prefixes setting supports multiple comma-separated values", () => {
    setSetting("work_path_prefixes", "/work/a,/work/b");
    expect(inferProjectScope({ path: "/work/a/proj" })).toBe("work");
    expect(inferProjectScope({ path: "/work/b/proj" })).toBe("work");
    expect(inferProjectScope({ path: "/personal/proj" })).toBe("personal");
  });

  test("empty work_orgs setting makes all orgs personal", () => {
    setSetting("work_orgs", "");
    expect(inferProjectScope({ githubFullName: "other/tool" })).toBe("personal");
  });

  test("empty work_path_prefixes setting makes all paths personal", () => {
    setSetting("work_path_prefixes", "");
    expect(inferProjectScope({ path: "/home/user/work/proj" })).toBe("personal");
  });

  test("settings with extra whitespace are trimmed correctly", () => {
    setSetting("work_orgs", "  acme  ,  widgets  ");
    expect(inferProjectScope({ githubFullName: "acme/x" })).toBe("work");
    expect(inferProjectScope({ githubFullName: "widgets/x" })).toBe("work");
  });
});
