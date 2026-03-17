import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { join } from "node:path";
import { _resetForTesting } from "./db/database.ts";
import { setSetting } from "./db/settings.ts";
import { inferProjectScope } from "./scope.ts";

const MIGRATIONS_DIR = join(import.meta.dir, "..", "..", "migrations");

describe("inferProjectScope (hardcoded defaults)", () => {
  beforeEach(() => {
    _resetForTesting(MIGRATIONS_DIR);
  });

  test("returns 'work' for work org in githubFullName", () => {
    expect(inferProjectScope({ githubFullName: "improvin/some-repo" })).toBe("work");
  });

  test("returns 'work' case-insensitively for org name", () => {
    expect(inferProjectScope({ githubFullName: "Improvin/some-repo" })).toBe("work");
  });

  test("returns 'personal' for non-work org", () => {
    expect(inferProjectScope({ githubFullName: "jakeisnt/pm" })).toBe("personal");
  });

  test("returns 'work' for work path prefix", () => {
    expect(inferProjectScope({ path: "/Users/jake/Documents/improvin/some-project" })).toBe("work");
  });

  test("returns 'personal' for non-work path", () => {
    expect(inferProjectScope({ path: "/Users/jake/Documents/personal/my-project" })).toBe("personal");
  });

  test("returns 'personal' with no inputs", () => {
    expect(inferProjectScope({})).toBe("personal");
  });

  test("githubFullName takes precedence over path", () => {
    // work org, personal path — org wins since it's checked first
    expect(
      inferProjectScope({
        githubFullName: "improvin/tool",
        path: "/Users/jake/Documents/personal/tool",
      }),
    ).toBe("work");
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
    // acme is now a work org, improvin is no longer
    expect(inferProjectScope({ githubFullName: "acme/tool" })).toBe("work");
    expect(inferProjectScope({ githubFullName: "improvin/tool" })).toBe("personal");
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
    expect(inferProjectScope({ path: "/Users/jake/Documents/improvin/old" })).toBe("personal");
  });

  test("work_path_prefixes setting supports multiple comma-separated values", () => {
    setSetting("work_path_prefixes", "/work/a,/work/b");
    expect(inferProjectScope({ path: "/work/a/proj" })).toBe("work");
    expect(inferProjectScope({ path: "/work/b/proj" })).toBe("work");
    expect(inferProjectScope({ path: "/personal/proj" })).toBe("personal");
  });

  test("empty work_orgs setting makes all orgs personal", () => {
    setSetting("work_orgs", "");
    expect(inferProjectScope({ githubFullName: "improvin/tool" })).toBe("personal");
  });

  test("empty work_path_prefixes setting makes all paths personal", () => {
    setSetting("work_path_prefixes", "");
    expect(inferProjectScope({ path: "/Users/jake/Documents/improvin/proj" })).toBe("personal");
  });

  test("settings with extra whitespace are trimmed correctly", () => {
    setSetting("work_orgs", "  acme  ,  widgets  ");
    expect(inferProjectScope({ githubFullName: "acme/x" })).toBe("work");
    expect(inferProjectScope({ githubFullName: "widgets/x" })).toBe("work");
  });
});
