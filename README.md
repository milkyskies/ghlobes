# ghlobes (glb)

A Rust CLI that wraps gh CLI + GitHub GraphQL API to give you beads-like workflow on top of GitHub Issues + Projects.

## Data model

- **Issues** = GitHub Issues (title, body, labels for type/priority)
- **Dependencies** = GitHub native blockedBy/blocking (GraphQL API, GA Aug 2025)
- **Status** = GitHub Projects single-select field: open, in_progress, blocked, closed
- **Priority** = GitHub Projects single-select field: P0–P4
- **Type** = GitHub Labels: bug, feature, task, epic, chore

## Commands (mirrors beads)

| Command | What it does | Under the hood |
|---|---|---|
| `glb init` | Detect project + write config | gh api to find project number, write .ghlobes.toml |
| `glb ready` | Show unblocked open issues | Query issues, filter out ones with open blockedBy |
| `glb list` | List issues with filters | gh issue list + project field queries |
| `glb show <num>` | Show issue + deps + status | GraphQL: issue + blockedBy + blocking + project fields |
| `glb create` | Create issue with labels/project | gh issue create + add to project + set fields |
| `glb update <num>` | Update status/priority/assignee | GraphQL mutations on project fields |
| `glb close <num>` | Close issue | gh issue close |
| `glb dep add <a> <b>` | A is blocked by B | addBlockedByRelation GraphQL mutation |
| `glb blocked` | Show all blocked issues | Query all open issues, filter by open blockedBy |
| `glb stuck` | Top blockers + per-epic stuck counts | Rank issues by direct dependents |
| `glb tree <num>` | Recursive sub-issue tree with status | Walks subIssues recursively |
| `glb deps <num>` | Transitive upstream/downstream dep tree | Walks blockedBy/blocking from in-memory graph |
| `glb closed --since 7d` | Recently closed (date or `--in-epic <num>`) | gh search with `closed:>=` filter |
| `glb done <num>` | Close + show newly unblocked + suggest next | Snapshot graph, close, diff |
| `glb path` | Critical path + high-leverage issues. `--epic`, `--explain` | Build dep graph, longest-path DP weighted by points |
| `glb next` | Recommend next batch. `--diverse`, `--reason`, `--exclude`, `--epic`, `--track` | Score ready issues + sub-of-epic credit, anti-conflict greedy pick |
| `glb stats` | Open/closed/blocked counts | Aggregate query |

## Key decisions

- **No local database** — all state lives in GitHub. No sync issues, works on any machine instantly.
- **Shells out to gh for auth** — no token management, uses whatever gh auth is configured.
- **GraphQL for deps** — gh CLI doesn't support deps natively, so we hit the API directly via `gh api graphql`.
- **Project fields for status/priority** — one-time setup: create the project + fields, then glb manages them.

## Writing issue bodies

`glb create` takes freeform Markdown via `--body` and imposes no structure — but issues should be self-contained so any agent can pick them up cold. The house template:

- `## Problem` / `## Goal` — what's missing and why it matters (lead with the problem; a concrete scenario helps)
- Key insight or rejected approach — the non-obvious reasoning (optional but valued)
- `## What this issue does` — mechanics broken into named sub-behaviors, with code/data snippets
- `## Acceptance criteria` — checkable outcomes
- `## Tests` — the tests that verify the work (required, except bug reports that reference a failing test, or no-behavior chores)

Titles are one concise clause with no em or en dashes (`glb create` rejects them — use `-` or `:`).

The canonical, always-current version of this template lives with the agent instructions in [milky-kit](https://github.com/milkyskies/milky-kit) at `modules/ghlobes/rules/glb.md`, which projects symlink into `.claude/rules/`.

## One-time setup

1. Run `glb init`
