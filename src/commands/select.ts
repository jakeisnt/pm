import { runProjectSelect } from "../lib/project-select.ts";
import { getSearchDepth, getSearchRoots } from "../lib/settings.ts";
import type { SelectOptions } from "../types.ts";

export async function runSelect(
  options: SelectOptions & { cloneDir?: string | undefined; json?: boolean | undefined },
): Promise<string> {
  const roots = getSearchRoots();
  const depth = getSearchDepth();
  return runProjectSelect(roots, depth, options);
}
