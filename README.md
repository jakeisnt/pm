# pm

A fast project manager CLI for switching between repositories, tracking recent projects, and organizing work across GitHub orgs.

Built with [Bun](https://bun.sh) and TypeScript.

## Install

```bash
bun install -g pm
```

Or clone and link locally:

```bash
git clone https://github.com/jakeisnt/pm.git
cd pm
bun install
bun link
```

The CLI is available as `p`.

## Usage

```bash
# Interactive project switcher (builtin fuzzy search)
p

# List all projects
p list

# List as JSON (useful for scripting / Raycast)
p list --json

# Filter by source
p list --source local
p list --source github

# Reindex local projects from configured roots
p reindex

# Manage GitHub orgs
p org list
p org hide <org>
p org show <org>

# Configure settings
p config list
p config set <key> <value>
p config delete <key>
```

## Configuration

pm uses a SQLite database for all state and settings. Configure work/personal scope detection:

```bash
# Set which GitHub orgs are considered "work"
p config set work_orgs "acme,my-company"

# Set which path prefixes are considered "work"
p config set work_path_prefixes "/home/user/work,/Users/user/company"
```

## Requirements

- [Bun](https://bun.sh) runtime
- [gh](https://cli.github.com/) CLI for GitHub integration (optional)

## License

[MIT](LICENSE)
