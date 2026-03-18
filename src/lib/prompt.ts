import type { FzfSelectOptions } from "@uln/prompts";
import { askLine, fzfSelect as fzfSelectBase, SelectionCancelledError } from "@uln/prompts";
import { suspendAbort } from "./abort.ts";

export { askLine, SelectionCancelledError };

export async function fzfSelect<T>(items: T[], opts: Omit<FzfSelectOptions<T>, "onActivate">): Promise<T> {
  return fzfSelectBase(items, {
    ...opts,
    onActivate: suspendAbort,
  });
}
