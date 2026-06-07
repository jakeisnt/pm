import { createEnv } from "@uln/env";
import { z } from "zod";

export const env = createEnv({
  SHELL: { schema: z.string(), fallback: "/bin/bash" },
  EDITOR: { schema: z.string(), fallback: "cursor" },
});

/** User's default shell, falls back to /bin/bash. */
export function getShell(): string {
  return env.SHELL;
}

/** User's preferred editor, falls back to cursor. */
export function getEditor(): string {
  return env.EDITOR;
}
