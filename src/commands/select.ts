import { loadConfig } from "../lib/config/index.ts";
import { runProjectSelect } from "../lib/project-select.ts";
import type { SelectOptions } from "../types.ts";

export async function runSelect(
  options: SelectOptions & { cloneDir?: string | undefined; json?: boolean | undefined },
): Promise<string> {
  const { searchRoots: roots, searchDepth: depth } = loadConfig();
  return runProjectSelect(roots, depth, options);
}
