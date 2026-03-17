import { describe, expect, test } from "bun:test";
import { inferProjectScope } from "./scope.ts";

describe("inferProjectScope", () => {
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
