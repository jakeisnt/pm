import { describe, expect, test } from "bun:test";
import { inferProjectScope } from "./scope.ts";

describe("inferProjectScope", () => {
  test("returns 'personal' with no inputs", () => {
    expect(inferProjectScope({})).toBe("personal");
  });

  test("returns 'personal' for any org (no work orgs configured)", () => {
    expect(inferProjectScope({ githubFullName: "acme/some-repo" })).toBe("personal");
  });

  test("returns 'personal' for any path (no work path prefixes configured)", () => {
    expect(inferProjectScope({ path: "/home/user/projects/some-project" })).toBe("personal");
  });
});
