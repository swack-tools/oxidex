# Project prose lint

Claude Code and Codex share `tools/prose_lint.py` and the root `.vale.ini`.
The hook checks Markdown and prose in Rust, Python, shell, and Perl files.
Install Python 3.9 or newer and Vale, then run `vale --config .vale.ini sync`.

Codex configuration lives in `.codex/config.toml` and `.codex/hooks.json`.
Open the repository as a trusted project, restart its Codex session, and use
`/hooks` to inspect and trust the project hook when prompted. A linked worktree
may require its own project trust. These files do not install a user hook.
Trust and discovery must succeed before automatic feedback is active.

Claude uses `.claude/hooks/prose-lint.sh` through its existing project settings.
Both adapters return exit code 2 with Vale diagnostics when lint fails.
A missing Vale executable reports a skip; a missing configuration, failed
style download, or timeout reports an error. The first check downloads the
Google style package if it is absent.

Codex patch edits check the paths named in the patch. Shell completions check
tracked changes and untracked prose files. Deleted files and paths outside the
repository are excluded. Read-only shell commands skip files whose contents
already passed lint in that session; the cache lives in the worktree's Git
metadata. Configuration or style changes invalidate the cache. Failed checks
are never cached. Explicit file and patch edits always check again.

Run the protocol tests with:

```bash
python3 -m unittest tools.ci.test_prose_lint
```
