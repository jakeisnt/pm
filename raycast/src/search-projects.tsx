import { execSync } from "node:child_process";
import { homedir } from "node:os";
import { join } from "node:path";
import { Action, ActionPanel, Icon, List } from "@raycast/api";
import { useExec } from "@raycast/utils";

interface Project {
  path: string;
  name: string;
  source: "local" | "github";
  githubFullName?: string;
  scope?: "personal" | "work" | "global";
}

// Resolve absolute paths — Raycast's env may not include ~/.bun/bin in PATH
const BUN_BIN = join(homedir(), ".bun", "bin", "bun");
const P_BIN = join(homedir(), ".bun", "bin", "p");

// biome-ignore lint/style/noDefaultExport: Raycast requires default export for commands
export default function SearchProjects() {
  const { data, isLoading } = useExec(BUN_BIN, ["run", P_BIN, "list", "--json"], {
    parseOutput: ({ stdout }) => {
      const trimmed = stdout.trim();
      if (!trimmed) return [];
      return JSON.parse(trimmed) as Project[];
    },
  });

  const projects = data ?? [];

  return (
    <List isLoading={isLoading} searchBarPlaceholder="Search projects…" filtering>
      <List.Section title="Local" subtitle={`${projects.filter((p) => p.source === "local").length}`}>
        {projects
          .filter((p) => p.source === "local")
          .map((project) => (
            <ProjectItem key={project.path} project={project} />
          ))}
      </List.Section>
      <List.Section title="GitHub" subtitle={`${projects.filter((p) => p.source === "github").length}`}>
        {projects
          .filter((p) => p.source === "github")
          .map((project) => (
            <ProjectItem key={project.githubFullName ?? project.name} project={project} />
          ))}
      </List.Section>
    </List>
  );
}

function ProjectItem({ project }: { project: Project }) {
  const isLocal = project.source === "local";
  const subtitle = isLocal ? project.path.replace(homedir(), "~") : (project.githubFullName ?? "");

  return (
    <List.Item
      title={project.name}
      subtitle={subtitle}
      icon={isLocal ? Icon.Folder : Icon.Globe}
      accessories={[{ tag: project.scope ?? "personal" }]}
      keywords={[project.name, project.githubFullName ?? "", project.path].filter(Boolean)}
      actions={
        <ActionPanel>
          {isLocal && (
            <>
              <Action.Open title="Open in Terminal" target={project.path} application="Ghostty" />
              <Action.Open title="Open in VS Code" target={project.path} application="Visual Studio Code" />
              <Action.Open title="Open in Finder" target={project.path} application="Finder" />
            </>
          )}
          {!isLocal && project.githubFullName && (
            <Action.OpenInBrowser title="Open on GitHub" url={`https://github.com/${project.githubFullName}`} />
          )}
          <Action.CopyToClipboard title="Copy Path" content={project.path} />
          {isLocal && (
            <Action
              title="Record in History"
              icon={Icon.Clock}
              onAction={() => {
                execSync(`${BUN_BIN} run ${P_BIN} ${JSON.stringify(project.name)} --silent`);
              }}
            />
          )}
        </ActionPanel>
      }
    />
  );
}
