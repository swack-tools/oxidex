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

## Documentation and comment style

Write all docs, READMEs, comments, and docstrings in the style of the
Google developer documentation style guide
(https://developers.google.com/style). Key rules:

- Address the reader as "you". Don't use "we" or "let's".
- Use active voice and present tense. Avoid "will" for current behavior.
- Keep sentences short and put one idea in each sentence.
- Put conditions before instructions: "To enable logging, set DEBUG=1."
- Use sentence case for headings.
- Use numbered lists for steps in order, bulleted lists for everything else.
- Put code elements (functions, flags, file names, values) in code font.
- Write "for example" and "that is", not "e.g." and "i.e."
- Don't use "please", "simply", "just", "easy", or "obviously".
- Use descriptive link text, never "click here".
- Use the serial comma.
- Comments explain why the code does something, not what each line does.
- Format Python docstrings per the Google Python Style Guide (Args:,
  Returns:, Raises:).
- A hook runs Vale on every file you edit. When it reports errors,
  fix them before moving on.
