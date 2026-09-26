# Claude adapter

## Shared policy

Read `AGENTS.md` for shared repository policy. It is authoritative; this
adapter does not import or duplicate those rules and may not override them.

## Skills

Use project-specific Claude skills from `.claude/skills`.

## Model routing

Use the least usage-intensive model and effort that can do the job well.
Usage is a shared budget: an Opus agent doing Haiku work spends quota that a
later judgment call needs.

| Work | Model | Effort |
|---|---|---|
| Polling CI/PRs, watching a lock or a run, file lookups, log and output summaries, read-only inventory | Haiku | low |
| Bounded implementation, mechanical merges without semantic conflicts, running named gates and reporting them, posting drafted review replies, routine review | Sonnet | low–medium |
| Semantic conflict resolution, review-thread fixes in write paths, oracle-parity debugging, roll-up integration | Opus | medium |
| Architecture, release-promotion judgment, security-sensitive work, final broad reviews, the controller's independent verification of a central claim | Opus | high (sparingly) |

- Name the model on every delegated launch; never let a subagent inherit Opus by
  default. Pick effort the same way where the launcher allows it.
- An agent's model is fixed once it starts. When its remaining work drops to a
  cheaper tier (for example, only drafting or posting replies is left), finish it
  with a fresh agent on the cheaper model that reads the first agent's handoff
  notes, rather than resuming the expensive one.
- Waiting is not work: an agent blocked on a lock or a long run should end its
  turn with its state saved, not poll.
- Escalate a tier only for a concrete reason, such as a failed attempt or a
  finding the cheaper model could not assess, and say why.

Do not use fast mode for delegated Claude work.
