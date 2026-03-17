import { describe, expect, test } from "bun:test";

const CLI_CWD = import.meta.dir.replace(/\/src$/, "");

function spawnCli(args: string[]): { stdout: string; stderr: string; exitCode: number } {
  const result = Bun.spawnSync(["bun", "run", "src/index.ts", ...args], {
    cwd: CLI_CWD,
  });
  return {
    stdout: result.stdout.toString(),
    stderr: result.stderr.toString(),
    exitCode: result.exitCode ?? 0,
  };
}

type ProjectEntry = {
  path: string;
  name: string;
  source: string;
};

function isProjectEntry(value: unknown): value is ProjectEntry {
  return (
    typeof value === "object" &&
    value !== null &&
    typeof (value as Record<string, unknown>)["path"] === "string" &&
    typeof (value as Record<string, unknown>)["name"] === "string" &&
    typeof (value as Record<string, unknown>)["source"] === "string"
  );
}

describe("CLI smoke tests", () => {
  test("p list --json outputs valid JSON array with required fields", () => {
    const { stdout, exitCode } = spawnCli(["list", "--json"]);
    expect(exitCode).toBe(0);

    let parsed: unknown;
    expect(() => {
      parsed = JSON.parse(stdout);
    }).not.toThrow();

    expect(Array.isArray(parsed)).toBe(true);
    const items = parsed as unknown[];
    for (const item of items) {
      expect(isProjectEntry(item)).toBe(true);
    }
  }, 10_000);

  test("p --help outputs usage text containing 'p' and 'Project manager'", () => {
    const { stdout, exitCode } = spawnCli(["--help"]);
    expect(exitCode).toBe(0);
    expect(stdout).toContain("p");
    expect(stdout).toContain("Project manager");
  }, 10_000);

  test("p list --json --source local only returns items with source 'local'", () => {
    const { stdout, exitCode } = spawnCli(["list", "--json", "--source", "local"]);
    expect(exitCode).toBe(0);

    let parsed: unknown;
    expect(() => {
      parsed = JSON.parse(stdout);
    }).not.toThrow();

    expect(Array.isArray(parsed)).toBe(true);
    const items = parsed as unknown[];
    for (const item of items) {
      expect(isProjectEntry(item)).toBe(true);
      expect((item as ProjectEntry).source).toBe("local");
    }
  }, 10_000);

  test("p list --json --source github only returns items with source 'github'", () => {
    const { stdout, exitCode } = spawnCli(["list", "--json", "--source", "github"]);
    expect(exitCode).toBe(0);

    let parsed: unknown;
    expect(() => {
      parsed = JSON.parse(stdout);
    }).not.toThrow();

    expect(Array.isArray(parsed)).toBe(true);
    const items = parsed as unknown[];
    for (const item of items) {
      expect(isProjectEntry(item)).toBe(true);
      expect((item as ProjectEntry).source).toBe("github");
    }
  }, 10_000);

  test("p config list exits with code 0", () => {
    const { exitCode } = spawnCli(["config", "list"]);
    expect(exitCode).toBe(0);
  }, 10_000);

  test("p org list exits with code 0", () => {
    const { exitCode } = spawnCli(["org", "list"]);
    expect(exitCode).toBe(0);
  }, 10_000);
});
