# ghlobes (glb)

Rust CLI that wraps `gh` + GitHub GraphQL API to give beads-like workflow on top of GitHub Issues + Projects.

Rules load from `.claude/rules/`, which holds symlinks into `~/.claude/kit/`. See milky-kit's README for the full set.

## Project-specific

- This is the canonical home of the `glb` CLI. The agent rule for using it lives in milky-kit at `modules/ghlobes/rules/glb.md`; consuming projects symlink it into their own `.claude/rules/glb.md` via `/milky-kit:retrofit`. ghlobes itself does not write or manage CLAUDE.md content — that's milky-kit's job.
- Worktrees not wired up — small repo, work on main.
