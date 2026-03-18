import { createEnv } from "@uln/env";
import { z } from "zod";

export const env = createEnv({
  SHELL: { type: z.string(), fallback: "/bin/bash" },
  EDITOR: { type: z.string(), fallback: "cursor" },
});

/** User's default shell, falls back to /bin/bash. */
export function getShell(): string {
  return env["SHELL"] as string;
}

/** User's preferred editor, falls back to cursor. */
export function getEditor(): string {
  return env["EDITOR"] as string;
}
