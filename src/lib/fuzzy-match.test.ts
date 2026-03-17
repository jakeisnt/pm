import { describe, expect, test } from "bun:test";
import { fuzzyFilter } from "./fuzzy-match.ts";

const identity = (s: string) => s;

describe("fuzzyFilter", () => {
  test("returns all items when query is empty", () => {
    const items = ["alpha", "beta", "gamma"];
    const results = fuzzyFilter(items, "", identity);
    expect(results.map((r) => r.item)).toEqual(items);
    expect(results.every((r) => r.score === 0)).toBe(true);
  });

  test("filters items that don't match", () => {
    const items = ["foo", "bar", "baz"];
    const results = fuzzyFilter(items, "fo", identity);
    expect(results.map((r) => r.item)).toEqual(["foo"]);
  });

  test("matches case-insensitively", () => {
    const items = ["FooBar", "foobar", "FOOBAR"];
    const results = fuzzyFilter(items, "foo", identity);
    expect(results).toHaveLength(3);
  });

  test("scores prefix matches higher", () => {
    const items = ["my-zoo", "zoo-keeper"];
    const results = fuzzyFilter(items, "zoo", identity);
    // "zoo-keeper" starts with "zoo" so should rank higher
    expect(results[0]?.item).toBe("zoo-keeper");
  });

  test("scores consecutive matches higher", () => {
    const items = ["a-b-c-d", "abcd"];
    const results = fuzzyFilter(items, "abcd", identity);
    // "abcd" has all consecutive matches
    expect(results[0]?.item).toBe("abcd");
  });

  test("scores word-boundary matches higher", () => {
    const items = ["xyzproject-manager", "project-manager"];
    const results = fuzzyFilter(items, "pm", identity);
    // "project-manager" has p and m at word boundaries
    expect(results[0]?.item).toBe("project-manager");
  });

  test("returns empty array when nothing matches", () => {
    const items = ["foo", "bar"];
    const results = fuzzyFilter(items, "xyz", identity);
    expect(results).toHaveLength(0);
  });

  test("works with custom key function", () => {
    const items = [{ name: "alpha" }, { name: "beta" }];
    const results = fuzzyFilter(items, "al", (item) => item.name);
    expect(results).toHaveLength(1);
    expect(results[0]?.item.name).toBe("alpha");
  });

  test("handles single-character query", () => {
    const items = ["abc", "def", "ghi"];
    const results = fuzzyFilter(items, "a", identity);
    expect(results.map((r) => r.item)).toEqual(["abc"]);
  });

  test("handles query longer than text", () => {
    const items = ["ab"];
    const results = fuzzyFilter(items, "abcdef", identity);
    expect(results).toHaveLength(0);
  });

  test("provides match indices", () => {
    const results = fuzzyFilter(["foobar"], "fb", identity);
    expect(results).toHaveLength(1);
    expect(results[0]?.indices).toBeDefined();
    expect(results[0]?.indices.length).toBe(2);
  });
});
