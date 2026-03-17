import { homedir } from "node:os";
import { join } from "node:path";

export function expandTilde(p: string): string {
  if (p.startsWith("~/")) return join(homedir(), p.slice(2));
  return p;
}
