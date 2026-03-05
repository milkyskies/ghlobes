ghlobes (glb) — Plan

  What it does

  A Rust CLI that wraps gh CLI + GitHub GraphQL API to give you beads-like workflow on top of GitHub Issues + Projects.

  Data model

- Issues = GitHub Issues (title, body, labels for type/priority)
- Dependencies = GitHub native blockedBy/blocking (GraphQL API, GA Aug 2025)
- Status = GitHub Projects single-select field: open, in_progress, blocked, closed
- Priority = GitHub Projects single-select field: P0–P4
- Type = GitHub Labels: bug, feature, task, epic, chore

  Commands (mirrors beads)

  ┌─────────────────────┬──────────────────────────────────┬────────────────────────────────────────────────────────┐
  │       Command       │           What it does           │                     Under the hood                     │
  ├─────────────────────┼──────────────────────────────────┼────────────────────────────────────────────────────────┤
  │ glb init            │ Detect project + write config    │ gh api to find project number, write .ghlobes.toml     │
  ├─────────────────────┼──────────────────────────────────┼────────────────────────────────────────────────────────┤
  │ glb ready           │ Show unblocked open issues       │ Query issues, filter out ones with open blockedBy      │
  ├─────────────────────┼──────────────────────────────────┼────────────────────────────────────────────────────────┤
  │ glb list            │ List issues with filters         │ gh issue list + project field queries                  │
  ├─────────────────────┼──────────────────────────────────┼────────────────────────────────────────────────────────┤
  │ glb show <num>      │ Show issue + deps + status       │ GraphQL: issue + blockedBy + blocking + project fields │
  ├─────────────────────┼──────────────────────────────────┼────────────────────────────────────────────────────────┤
  │ glb create          │ Create issue with labels/project │ gh issue create + add to project + set fields          │
  ├─────────────────────┼──────────────────────────────────┼────────────────────────────────────────────────────────┤
  │ glb update <num>    │ Update status/priority/assignee  │ GraphQL mutations on project fields                    │
  ├─────────────────────┼──────────────────────────────────┼────────────────────────────────────────────────────────┤
  │ glb close <num>     │ Close issue                      │ gh issue close                                         │
  ├─────────────────────┼──────────────────────────────────┼────────────────────────────────────────────────────────┤
  │ glb dep add <a> <b> │ A is blocked by B                │ addBlockedByRelation GraphQL mutation                  │
  ├─────────────────────┼──────────────────────────────────┼────────────────────────────────────────────────────────┤
  │ glb blocked         │ Show all blocked issues          │ Query all open issues, filter by open blockedBy        │
  ├─────────────────────┼──────────────────────────────────┼────────────────────────────────────────────────────────┤
  │ glb stats           │ Open/closed/blocked counts       │ Aggregate query                                        │
  └─────────────────────┴──────────────────────────────────┴────────────────────────────────────────────────────────┘

  Key decisions

- No local database — all state lives in GitHub. No sync issues, works on any machine instantly
- Shells out to gh for auth — no token management, uses whatever gh auth is configured
- GraphQL for deps — gh CLI doesn't support deps natively, so we hit the API directly via gh api graphql
- Project fields for status/priority — one-time setup: create the project + fields, then glb manages them

  One-time setup

  1. Create a GitHub Project on the repo
  2. Add custom fields: Status (single-select), Priority (single-select)
  3. Run glb init which auto-detects project number and writes .ghlobes.toml
