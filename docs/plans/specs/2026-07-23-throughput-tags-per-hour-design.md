# Throughput: More Tags Per Hour

**Date:** 2026-07-23
**Branch:** `feat/model-fix-loop-context` (PR #41)
**Files under change:** `scripts/model_fix_loop.py`, `scripts/find_tag_gaps.py` (read-only dependency), `scripts/log_sweep_review.py`, `config.toml`, `config.example.toml`, tests.

## Motivation (measured, this session)

Tags/hour = successful-calls/hour × 1/(calls per tag) × (diff→landed rate).
Today's numbers: ~70 OK calls/hour averaged over 24h — but **0/hour during the
20-worker window** (13,028 HTTP 429s in one hour, every worker backing off
independently so the account-wide limit never reset); ~37 successful calls per
landed tag (whole investigations re-paid for sibling tags like
BlackLevelRed/Green/Blue and MODE1-6); and a rejection tax where candidates
that build, test, and pass the LLM reviewer still land wrong
(`XMP:ArtworkTitle` produced `"test, verfänglich"` where real exiftool says
`"test"` — approved by the model reviewer, caught only by human sweep review).

Six features, one per lever.

## F1 — Shared rate governor (restores call supply)

A cross-process token-bucket + global-cooldown gate, shared by every worker
via a JSON state file under `OXIDEX_HOME/logs/` guarded by `fcntl.flock`.

- **State file** (`rate-governor.json`): `{"tokens": float, "last_refill":
  epoch, "cooldown_until": epoch, "consecutive_limited": int}`. Missing or
  corrupt file = fresh permissive state (the governor must never brick the
  loop).
- **`governor_acquire(path, calls_per_minute, burst, now_fn, sleep_fn)`**:
  under the lock, refill `tokens` by `calls_per_minute/60 × elapsed` (capped
  at `burst`); if `cooldown_until` is in the future or `tokens < 1`, compute
  the wait, release the lock, sleep, repeat; else decrement and proceed.
  Waits get ±20% jitter so 20 workers don't thundering-herd the same instant.
- **`governor_report(path, limited, cooldown_seconds, max_cooldown_seconds,
  now_fn)`**: on a 429/5xx outcome, under the lock set
  `consecutive_limited += 1` and `cooldown_until = now + min(cooldown_seconds
  × 2^(consecutive_limited-1), max_cooldown_seconds)` — a GLOBAL cooldown all
  workers obey, which is the entire point: today each worker backs off alone
  while the other 19 keep hammering. On success, reset
  `consecutive_limited = 0`.
- **Wiring**: `call_model` gains keyword-only `governor_path=None,
  governor_calls_per_minute, governor_burst, governor_cooldown_seconds,
  governor_max_cooldown_seconds` (None path = disabled, all existing callers
  and tests unaffected). Inside the retry loop: acquire before every attempt,
  report after every attempt (429 and 5xx count as limited). `main()` threads
  the config values into every logging closure.
- **Config** (`[worker]`, same keys honored on `[reviewer]`):
  `governor_calls_per_minute = 30`, `governor_burst = 5`,
  `governor_cooldown_seconds = 30`, `governor_max_cooldown_seconds = 300`,
  `governor_path` defaulting to `OXIDEX_HOME/logs/rate-governor.json`.

## F2 — Sibling-tag clustering (cuts calls-per-tag)

- **`cluster_key(tag_gap)`** = `(format, family, tuple(parser_files))` where
  family is the middle component of the tag key (e.g. `RW2:EXIF:BlackLevelRed`
  → `EXIF`; `JPEG:APP12:MODE3` → `APP12`).
- In `run_tag_loop`, after selecting `active[0]` (the leader), gather up to
  `max_cluster_tags - 1` further unclaimed, unblacklisted entries from
  `active` with the same cluster key and attach them as
  `leader["cluster_members"]` (a possibly-empty list). Claim every member the
  same way the leader is claimed.
- **Bookkeeping**: on `fixed` or `duplicate`, clear state for the leader AND
  every member (members that didn't actually close are re-discovered by the
  next `find_gaps_fn` run with fresh state — acceptable, they are never
  blacklist-lost). On failure, charge ONLY the leader's fail budget; members
  are released unclaimed (they'll be retried solo or in another cluster).
- **`make_cluster_gap(leader)`** (alongside `make_single_tag_gap`): a gap
  dict whose `missing_tags`/`value_differences` union the leader and all
  members, `gap_count` = member count, plus `"clustered": True`.
- `fix_gap`'s `build_prompt` call uses `max_tags = entry count` when
  `gap.get("clustered")`, else `config["max_prompt_tags"]` — so the live
  config's `max_prompt_tags = 1` doesn't truncate a cluster, and format-mode
  gaps (hundreds of entries) keep the configured cap.
- `main()`'s `real_fix_tag` recheck counts how many of the target tags are
  still open (see F3) instead of the current single-tag 0/1.
- **Config**: `max_cluster_tags = 6` (`1` disables clustering entirely).

## F3 — Differential target-tag verification (cuts the rejection tax)

The per-tag recheck in `main()` currently returns "closed" for a
kind=="missing" tag as soon as it leaves `missing_in_oxidex` — even if it
arrived in `value_differences` with a WRONG value. That is exactly the
`ArtworkTitle` escape.

- New pure helper **`tag_still_open(match, tag_gap)`** in
  `model_fix_loop.py`: a tag is open if it appears in `missing_tags`
  (family+name for kind=="missing") OR in `value_differences` (its
  `family:name` / `tag_key`), regardless of kind. Returns the open-detail
  (`None` | `"missing"` | `("value_differs", exiftool_value, oxidex_value)`)
  so failure reasons can carry the actual values.
- `main()`'s recheck closure uses it: returns the count of still-open target
  tags (leader + cluster members). When a target is open as `value_differs`,
  the reason string fed back into the retry includes both values —
  `expected (exiftool): "test" / got (oxidex): "test, verfänglich"` — which
  is precisely the evidence the next round needs.
- `fix_gap`'s gap gate stays `remaining >= gap["gap_count"]` → fail; for
  clustered gaps a partial win (some closed, none newly wrong) passes with
  `gaps_closed` = number actually closed.

## F4 — Neighbor-precedent block (kills investigation turns)

Nearly every landed fix this session was "generalize the neighboring tag"
(LightS copied ColorMode; MODE3-6 generalized MODE1/2; ZipCRC mirrored
ZipBitFlag). Give the fixer that precedent up front:

- **`build_neighbor_precedent_block(gap, repo_root, git_show_fn=None)`**: for
  single/cluster gaps, take the family prefix (`APP12:`, `ZIP:`, …), grep the
  gap's `parser_files` for already-implemented sibling tag literals
  (`"<family>:<Name>"` string occurrences whose Name is not one of the gap's
  own tags); pick the first sibling found; run
  `git log -S "<family>:<sibling>" -1 --format=%H -- <parser file>` then
  `git show <hash>` (patch, truncated to ~3,000 chars) via an injectable
  runner. Emit a section: *"How the neighboring tag `<X>` was added
  (historical commit — pattern-match this, including its test):"*.
  Empty/None on any miss (no sibling, no commit, git error) — never fatal.
- Included in `build_prompt` inside the stable per-tag section (after the
  Perl reference block). One new prompt kwarg threaded exactly like
  `perl_lib_dir`.
- The hex-window idea (targeted sample-byte windows at Perl-implied offsets)
  is **deferred** — format-specific offset inference is a rabbit hole; the
  existing exact-sample block plus line-range REQUESTs cover most of it.

## F5 — Cheaper rounds (targeted tests first, sccache)

- **`cargo_test_targeted(repo_root, filter)`** → `cargo test --lib <filter>`
  where filter = the format lowercased (best-effort name filter; zero
  matching tests is a PASS, exactly like cargo treats it). Returns
  `(success, output)` like `cargo_test_workspace`.
- `fix_gap` ordering change: after a candidate builds and closes its gap,
  run the TARGETED tests (seconds); only if those pass proceed to duplicate
  detection and review; run the FULL workspace suite once, immediately
  before `git_commit_fn` — a full-suite failure there reverts and becomes a
  `test_regressed` round exactly as today. Candidates that die at
  review/duplicate no longer pay the full-suite cost.
- **`cargo_env()`**: if `sccache` is on PATH, `RUSTC_WRAPPER` is not already
  set, and `os.environ.get("OXIDEX_USE_SCCACHE") != "0"`, set
  `RUSTC_WRAPPER=sccache` — 21 worktrees compile near-identical trees; this
  amortizes them. `main()` sets `OXIDEX_USE_SCCACHE` to `"1"`/`"0"` from the
  `use_sccache` config knob before any cargo call, so one env var carries the
  choice into every subprocess helper without threading a parameter through
  five call sites.

## F6 — Landed-tags set (stops re-deriving landed fixes)

- Append-only file `OXIDEX_HOME/logs/landed-tags.log`, one
  `<iso-ts> <tag_key>` per line.
- **`log_sweep_review.py`**: an `accepted` verdict also appends
  `<format>:<tag>` to the landed file (flag `--landed-log`, default the
  path above) — the sweep session already gets told to log verdicts; now a
  single call also feeds the skip set.
- **`load_landed_tags(path)`** in `model_fix_loop.py`; `run_tag_loop` skips
  active entries whose `tag_key` is in the set (logged once per skip:
  `"[X] skipped -- already landed via sweep"`), clearing any stale claim.
  The set is re-read each round (cheap file), so a sweep landing mid-round
  takes effect on the next selection.
- Ops note (not code): the sweep session's recurring prompt should include
  "after merging a tag fix, run `log_sweep_review.py --verdict accepted …`" —
  that single habit feeds sweep history, the landed set, and (via F6) the
  workers' skip list.

## Config defaults (both `config.toml` and `config.example.toml`)

```toml
[worker]
governor_calls_per_minute = 30
governor_burst = 5
governor_cooldown_seconds = 30
governor_max_cooldown_seconds = 300
max_cluster_tags = 6
use_sccache = true
```

(`governor_path` and the landed-log path default under `OXIDEX_HOME/logs/`
and are CLI-overridable, not TOML knobs.) `[reviewer]` inherits the governor
knobs through `_normalize_model_config` like every other shared key.

## Explicitly cut (YAGNI)

- Hex-window byte slicing at Perl-implied offsets (deferred, see F4).
- Two-tags-in-flight pipelining per worker (latency hiding) — real but
  complex; measure the above first.
- Any change to the sweep session's own automation beyond the ops note.

## Testing

Same hermetic style as everything else in these files: governor state
transitions with fake `now_fn`/`sleep_fn`/lock path in a tempdir (including
corrupt-file recovery and jitter bounds); `cluster_key`/cluster selection and
bookkeeping via `run_tag_loop` fakes (fixed clears members, failure charges
leader only); `tag_still_open` truth table incl. the wrong-value case;
neighbor-precedent block with an injectable git runner (found, no-sibling,
git-failure); `cargo_test_targeted` subprocess mock; `fix_gap` ordering
(targeted-before-review, full-suite-before-commit, full-suite failure =
test_regressed); landed-set skip in `run_tag_loop`; `log_sweep_review`
landed-log append. Full discover suite green throughout (466 baseline).

## Rollout

Land on `feat/model-fix-loop-context` → PR #41; merge to local `main`;
propagate `model_fix_loop.py`, `log_sweep_review.py`, and `config.toml` to
all worker worktrees; dispatcher restart picks everything up. The governor
file is created lazily on first use.
