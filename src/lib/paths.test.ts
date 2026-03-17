import { describe, expect, test } from "bun:test";
import { homedir } from "node:os";
import { join } from "node:path";
import { expandTilde } from "./paths.ts";

describe("expandTilde", () => {
  test("expands ~/ to home directory", () => {
    expect(expandTilde("~/Documents")).toBe(join(homedir(), "Documents"));
  });

  test("expands nested ~/ path", () => {
    expect(expandTilde("~/a/b/c")).toBe(join(homedir(), "a", "b", "c"));
  });

  test("does not expand paths without ~/", () => {
    expect(expandTilde("/usr/local/bin")).toBe("/usr/local/bin");
  });

  test("does not expand bare tilde", () => {
    expect(expandTilde("~")).toBe("~");
  });

  test("does not expand tilde in middle of path", () => {
    expect(expandTilde("/foo/~/bar")).toBe("/foo/~/bar");
  });
});
