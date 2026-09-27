#!/usr/bin/env bash
# Lints edited docs and comments with Vale. Exit code 2 sends errors back to Claude.
command -v vale >/dev/null || { echo "prose-lint: vale not installed, skipping" >&2; exit 0; }
command -v jq >/dev/null || { echo "prose-lint: jq not installed, skipping" >&2; exit 0; }
cd "$CLAUDE_PROJECT_DIR" || exit 0
[ -d .vale/styles/Google ] || vale sync >/dev/null 2>&1
f=$(jq -r '.tool_input.file_path // empty')
case "$f" in *.md|*.rs|*.py|*.sh|*.pl) ;; *) exit 0 ;; esac
out=$(vale --output=line "$f" 2>&1) || { echo "Fix these style errors: $out" >&2; exit 2; }
exit 0
