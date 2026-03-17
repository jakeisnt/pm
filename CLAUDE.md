# pm — Project Manager

A Bun TypeScript CLI tool. `p` is the binary. Manages project switching, per-project knowledge (repo memories), and project discovery. Private project for personal use.

## Quick Start

```bash
bun install              # Install dependencies
bun run src/index.ts     # Run CLI during development
bun run lint             # Biome check (format, imports, lint)
bun run typecheck        # TypeScript strict type check
```

No tests. Type checking and linting are the verification steps — run both after every change.

After every change: rebuild and relink (`bun install && bun link`), then run `p <command>` to verify end-to-end.

## Structure

- `src/index.ts` — CLI entry point with all command definitions (commander)
- `src/types.ts` — Shared type definitions
- `src/lib/` — Infrastructure: database, subprocess helpers, settings, etc.
- `src/commands/` — One directory per command group, each with an `index.ts`

## Key Conventions

### Constants
All config values, limits, timeouts, and tunables go in `src/lib/constants.ts`.

### Environment Variables
Never use `process.env` directly. All env vars are defined in `src/lib/env-schema.ts` with Zod validation.

### Subprocess Execution
All subprocess calls go through `src/lib/subprocess.ts` (`run()`). No direct `Bun.spawnSync()`.

### CLI Tool Abstraction
`src/lib/github.ts` — `git`, `gh` CLI wrappers. All invocations go through this module.

### Output
Never use `console.log` (enforced by Biome). Use the `log` module from `src/lib/log.ts`.

### Error Handling
Throw exceptions, don't return Result types. Never `process.exit` except at top-level.

### TypeScript
- Maximum strictness (`strict: true` + all extra flags)
- ES modules with `.ts` extensions in imports
- `import type` for type-only imports
- Named exports only, no default exports
- Functional style, no classes
- No `as` casts, no `any`, no non-null assertions

### Formatting (Biome)
Spaces (2), double quotes, semicolons, trailing commas, 120 char width.

### Naming
`camelCase` for variables/functions, `PascalCase` for types, `kebab-case.ts` for files.

### Data Storage
SQLite (`pm.db`) for all application state via Kysely.

### Entity IDs (UUIDs)
All entity tables use `TEXT PRIMARY KEY` with UUID v4 identifiers. Generate via `generateId()` from `src/lib/db/index.ts`. CLI displays first 8 chars as short IDs. Prefix matching: `WHERE id LIKE ? || '%'`.

### Database Timestamps
Every table must have `created_at`, `updated_at`, and `deleted_at` columns:
- `created_at INTEGER NOT NULL DEFAULT (unixepoch() * 1000)`
- `updated_at INTEGER NOT NULL DEFAULT (unixepoch() * 1000)`
- `deleted_at INTEGER` — NULL means active; set to timestamp for soft delete
- All timestamps are milliseconds since epoch

## Adding a New Command

1. Create `src/commands/<name>/index.ts` exporting `run<CommandName>(opts)`
2. Register in `src/index.ts` with Commander
3. Run `bun run typecheck` and `bun run lint`
