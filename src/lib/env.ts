import { env } from "./env-schema.ts";

/** User's default shell, falls back to /bin/bash. */
export function getShell(): string {
  return env.SHELL;
}

/** User's preferred editor, falls back to cursor. */
export function getEditor(): string {
  return env.EDITOR;
}
