import { describe, expect, test } from "bun:test";
import { extractOrgName, LOCAL_ORG } from "./orgs.ts";

describe("extractOrgName", () => {
  test("returns org name from github full name", () => {
    expect(extractOrgName("jakeisnt/pm")).toBe("jakeisnt");
  });

  test("lowercases org name", () => {
    expect(extractOrgName("Improvin/some-repo")).toBe("improvin");
  });

  test("returns LOCAL_ORG for null", () => {
    expect(extractOrgName(null)).toBe(LOCAL_ORG);
  });

  test("returns LOCAL_ORG for undefined", () => {
    expect(extractOrgName(undefined)).toBe(LOCAL_ORG);
  });

  test("returns LOCAL_ORG for empty string", () => {
    expect(extractOrgName("")).toBe(LOCAL_ORG);
  });

  test("handles name without slash", () => {
    // No slash means split()[0] is the whole string
    expect(extractOrgName("solo")).toBe("solo");
  });
});
