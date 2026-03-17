import { describe, expect, test } from "bun:test";
import { generateId, shortId } from "./uuid.ts";

describe("generateId", () => {
  test("returns a valid UUID v4", () => {
    const id = generateId();
    expect(id).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
  });

  test("returns unique values", () => {
    const ids = new Set(Array.from({ length: 100 }, () => generateId()));
    expect(ids.size).toBe(100);
  });
});

describe("shortId", () => {
  test("returns first 8 characters", () => {
    expect(shortId("abcdefgh-1234-5678-9abc-def012345678")).toBe("abcdefgh");
  });

  test("works with generateId output", () => {
    const id = generateId();
    const short = shortId(id);
    expect(short).toHaveLength(8);
    expect(id.startsWith(short)).toBe(true);
  });
});
