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
