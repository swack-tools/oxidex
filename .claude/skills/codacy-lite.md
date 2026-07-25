---
name: codacy-lite
description: Use when you need code quality metrics (duplication, complexity, stats) with minimal tokens - runs jscpd, scc, and tokei in parallel, returns compact summary
---

# codacy-lite

Single-command code quality check. Runs tools in parallel, returns structured summary.

## When to Use

- Quick quality check before PR
- Find duplicate code blocks
- Identify high-complexity files
- Get codebase stats

## Dependencies

```bash
npm install -g jscpd
brew install scc        # or: go install github.com/boyter/scc/v3@latest
cargo install tokei     # or: brew install tokei
```

## Execution

Run all tools in parallel, capture JSON, summarize:

```bash
# Stats
tokei --output json . 2>/dev/null

# Duplication (min 20 lines, JSON output, exclude node_modules/target/dist)
jscpd --min-lines 20 --ignore "**/node_modules/**,**/target/**,**/dist/**" --reporters json --output /tmp/jscpd/ . 2>/dev/null

# Complexity per file (JSON output)
scc --by-file --format json . 2>/dev/null
```

Or single parallel command:
```bash
tokei --output json . > /tmp/tokei.json 2>/dev/null & \
jscpd --min-lines 20 --ignore "**/node_modules/**,**/target/**,**/dist/**" --reporters json --output /tmp/jscpd/ . 2>/dev/null & \
scc --by-file --format json . > /tmp/scc.json 2>/dev/null & \
wait
```

## Output Format

Present results in this exact format:

```
## Stats
[Language]: [total] lines ([code] code, [comments] comments, [blank] blank) across [files] files

## Duplication
Total: [percentage]% ([lines] lines in [clones] clones)

Top clones:
- [file1]:[start]-[end] ↔ [file2]:[start]-[end] ([lines] lines)
[max 5 entries]

## Complexity
Avg cyclomatic: [avg] | Max: [max]

High complexity (>10):
- [file] ([score])
[max 5 entries, only show if score > 10]
```

## Thresholds

| Metric | Threshold | Action |
|--------|-----------|--------|
| Complexity | >10 | Show in report |
| Clone size | >20 lines | Include |
| Max items | 5 | Per section |

## Parsing Notes

**tokei JSON:** `.[language].code`, `.[language].comments`, `.[language].blanks`

**jscpd JSON:** Look in `/tmp/jscpd/jscpd-report.json` for `.statistics.total.percentage`, `.duplicates[]` with `firstFile`, `secondFile`, `lines`

**scc JSON:** Array of languages, each with `Files[]` containing `Location` and `Complexity`. Sort files by complexity descending, report top 5 with score >10
