import type { Project } from "../types.ts";
import { fzfSelect } from "./prompt.ts";

function formatProject(p: Project): string {
  if (p.source === "github") {
    return `\x1b[36m☁\x1b[0m ${p.githubFullName || p.name}`;
  }
  return `\x1b[32m●\x1b[0m ${p.name} \x1b[2m(${p.path})\x1b[0m`;
}

export async function fuzzySelectProject(projects: Project[]): Promise<Project> {
  return fzfSelect(projects, {
    format: (p) => formatProject(p),
    searchKey: (p) => p.name,
    noMatchError: "No projects found",
  });
}
