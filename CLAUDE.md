@AGENTS.md

`AGENTS.md` is the shared guide for every coding agent in this repository
(Claude Code and Codex read it). Put project, workflow, measurement and CI rules
there. This file holds only what is specific to Claude Code.

## Claude Code specifics

- **Use `claude-mem`** alongside ripgrep and the language servers to cut down on
  repeated searching.
- **Wait with background tasks, not sleep loops.** Run long builds, corpus
  sweeps and CI polls with `run_in_background` and act on the completion
  notification. For a condition, use an `until <check>; do sleep N; done` loop.
- **Subagents report, they do not prove.** Before building on a subagent's
  result, check its worktree and rerun the tests it cites. Give each subagent
  that edits files its own worktree (`isolation: "worktree"` or an explicit
  `git worktree add`), per the one-agent-one-worktree rule in `AGENTS.md`.
- **The Bash tool's shell may be zsh.** Brace variables before a colon
  (`"${SHA}:refs/heads/x"`, `"${c}:${path}"`): zsh applies `:r`/`:s` modifiers
  to `$VAR:...`. After a pipe, read `$pipestatus` in zsh, not `${PIPESTATUS[0]}`.
