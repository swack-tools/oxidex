# Claude adapter

## Shared policy

Read `AGENTS.md` for shared repository policy. It is authoritative; this
adapter does not import or duplicate those rules and may not override them.

## Skills

Use project-specific Claude skills from `.claude/skills`.

## Model routing

Use Opus for architecture, release-promotion judgment, security-sensitive work,
and final broad reviews. Use Sonnet for bounded implementation and routine
review. Use Haiku only for low-risk, read-only inventory or summarization.

Prefer fast mode for delegated Claude work when it is available.
