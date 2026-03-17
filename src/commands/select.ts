import { getSearchDepth, getSearchRoots } from "../lib/config/index.ts";
import { runProjectSelect } from "../lib/project-select.ts";
import type { SelectOptions } from "../types.ts";

export async function runSelect(
  options: SelectOptions & { cloneDir?: string | undefined; json?: boolean | undefined },
): Promise<string> {
  const roots = getSearchRoots();
  const depth = getSearchDepth();
  return runProjectSelect(roots, depth, options);
}
