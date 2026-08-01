#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.9"
# dependencies = []
# ///
"""Find and group oxidex/ExifTool tag-coverage gaps by format.

Wraps `just compare-exiftool-full` (full corpus) or a direct
`tag-comparison --format` re-run (fast, single-format), then groups the
resulting report's missing_in_oxidex + value_differences by format,
sorted by gap count descending.

Usage:
    uv run scripts/find_tag_gaps.py [--output gaps.json] [--only-format NAME]
                                     [--cache-dir DIR]
"""
import argparse
import contextlib
import fcntl
import json
import os
import shutil
import subprocess  # nosec B404 -- list-argv only, no shell=True anywhere below
import sys
import tempfile
import threading
import time
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent

# Fixed, worktree-independent home for durable state (logs, persistent
# worker worktrees) written by the model-fix/tag-fix loops. Deliberately
# NOT REPO_ROOT-relative: these scripts are routinely run from many
# different git worktrees of this repo, and REPO_ROOT-relative paths used
# to scatter a single logical run's logs across whichever worktree
# happened to be cwd when it was launched.
OXIDEX_HOME = Path(os.environ.get("OXIDEX_HOME", str(Path.home() / ".oxidex")))

# ---------------------------------------------------------------------------
# Build semaphore -- spec section 5 ("a build semaphore, flock,
# build_semaphore = 5 ... caps concurrent cargo build/test across all
# workers+mergers so 10 cores are never oversubscribed by linking").
#
# Lives here (not model_fix_loop.py, where every other cross-process store
# in this fleet -- _governor_locked, _state_locked -- is defined) purely to
# avoid a circular import: model_fix_loop.py already does
# `from find_tag_gaps import (OXIDEX_HOME, REPO_ROOT, ...)` at module load
# time, and find_tag_gaps.ensure_tag_comparison_built is itself one of the
# call sites this semaphore wraps, so the semaphore has to be importable
# from here without find_tag_gaps needing anything back from
# model_fix_loop. model_fix_loop.py imports build_semaphore/its defaults
# from here alongside its other find_tag_gaps imports.
#
# Design choice (mirrors _governor_locked's own doc comment style): a
# single JSON state file + flock, exactly like rate-governor.json and
# model-fix-tag-state.json, rather than N pre-created lock files in a
# directory -- one file keeps this consistent with every other
# cross-process store in the fleet instead of introducing a new shape, and
# a per-holder heartbeat timestamp (not just a bare counter) gives free
# stale-holder recovery: a worker that crashes mid-cargo-build without
# releasing its slot is simply evicted once its heartbeat goes stale,
# exactly like model_fix_loop.py's tag-claim heartbeat.
DEFAULT_BUILD_SEMAPHORE_PATH = OXIDEX_HOME / "logs" / "build-semaphore.json"
DEFAULT_BUILD_SEMAPHORE_MAX_HOLDERS = 5
DEFAULT_BUILD_SEMAPHORE_STALE_SECONDS = 900  # a cargo build hung 15+ min is presumed a dead holder
# Cadence at which a held slot's heartbeat is re-stamped WHILE the
# protected build/test call is still in flight -- same value as
# model_fix_loop.py's own DEFAULT_HEARTBEAT_SECONDS (the tag-claim
# heartbeat this is meant to have parity with), and a healthy margin
# under DEFAULT_BUILD_SEMAPHORE_STALE_SECONDS (15 beats before a live
# holder could ever be mistaken for stale).
DEFAULT_BUILD_SEMAPHORE_HEARTBEAT_SECONDS = 60


def _pid_is_alive(pid):
    """Best-effort local-process liveness check for a semaphore holder.

    Unknown or inaccessible PIDs stay live until the heartbeat timeout; only
    an authoritative ProcessLookupError permits immediate eviction.
    """
    try:
        pid = int(pid)
    except (TypeError, ValueError):
        return True
    if pid <= 0:
        return True
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except (PermissionError, OSError):
        return True
    return True


def _semaphore_locked(path, mutate_fn):
    """Run mutate_fn(state) -> (new_state, result) under an exclusive
    flock on path's sibling .lock file -- the build-semaphore twin of
    model_fix_loop.py's _governor_locked/_state_locked. A missing or
    corrupt state file becomes a fresh empty-holders state (like the
    governor, this bookkeeping must never brick a build over its own
    corruption); saved via tempfile+os.replace (like every other shared
    store in this fleet) so a reader can never observe a half-written
    file."""
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    lock_path = path.with_suffix(".lock")
    with open(lock_path, "w") as lock_f:
        fcntl.flock(lock_f, fcntl.LOCK_EX)
        try:
            state = json.loads(path.read_text())
            if not isinstance(state, dict):
                raise ValueError("state is not a dict")
        except (OSError, ValueError, json.JSONDecodeError):
            state = {}
        state.setdefault("holders", {})
        new_state, result = mutate_fn(state)
        tmp = tempfile.NamedTemporaryFile(
            "w", dir=path.parent, prefix=path.name + ".", suffix=".tmp", delete=False,
        )
        with tmp:
            tmp.write(json.dumps(new_state))
        os.replace(tmp.name, path)
        return result


def _try_acquire_build_slot(path, max_holders, stale_seconds, now_fn, holder_id):
    """One non-blocking attempt to claim (or refresh) holder_id's slot.
    Live holders (heartbeat within stale_seconds) other than holder_id
    count against max_holders; a stale holder is dropped as part of the
    same locked read-modify-write, so eviction and (re-)acquisition can
    never race each other."""
    def mutate(state):
        now = now_fn()
        live = {
            slot: h for slot, h in state["holders"].items()
            if (now - h.get("heartbeat", 0) < stale_seconds
                and _pid_is_alive(h.get("pid")))
        }
        if holder_id in live:
            live[holder_id]["heartbeat"] = now
            state["holders"] = live
            return state, True
        if len(live) >= max_holders:
            state["holders"] = live
            return state, False
        live[holder_id] = {"pid": os.getpid(), "heartbeat": now}
        state["holders"] = live
        return state, True
    return _semaphore_locked(path, mutate)


def _release_build_slot(path, holder_id):
    def mutate(state):
        state["holders"].pop(holder_id, None)
        return state, None
    _semaphore_locked(path, mutate)


@contextlib.contextmanager
def build_semaphore(path=None, max_holders=DEFAULT_BUILD_SEMAPHORE_MAX_HOLDERS,
                     stale_seconds=DEFAULT_BUILD_SEMAPHORE_STALE_SECONDS, poll_seconds=2.0,
                     now_fn=time.time, sleep_fn=time.sleep, holder_id=None,
                     heartbeat_seconds=DEFAULT_BUILD_SEMAPHORE_HEARTBEAT_SECONDS):
    """Block until one of at most max_holders concurrent cargo build/test
    slots is free, yield, then release on the way out (success or
    exception alike -- a `finally`, never leaking a held slot).

    path=None (the default) disables the semaphore entirely -- a plain
    no-op contextmanager -- so every existing call site that doesn't
    explicitly opt in (by passing a real path) behaves exactly as before
    this feature existed; hermetic tests that never pass a path never
    touch a lock file, real or fake. Real callers (model_fix_loop.py's
    main(), squad_merge_loop.py) pass DEFAULT_BUILD_SEMAPHORE_PATH
    explicitly.

    holder_id defaults to "<pid>-<thread-ident>", unique enough that two
    threads in the same process (or two processes) never collide on one
    slot identity.

    heartbeat_seconds (0/None disables): a background daemon thread
    re-stamps this holder's heartbeat every heartbeat_seconds WHILE the
    protected build/test call is in flight -- without this, a single
    call that runs longer than stale_seconds (e.g. a slow `cargo test
    --workspace` under contention from the very other capped builds this
    semaphore exists to bound) would itself look stale to any concurrent
    waiter's _try_acquire_build_slot check, letting that waiter acquire a
    slot that is, in fact, still actively held -- oversubscribing builds
    beyond max_holders even though nothing crashed. This is the same
    fix shape as model_fix_loop.py's own tag-claim heartbeat (a
    stop-event-driven daemon thread touching the shared state on a
    cadence, joined before this slot is released) -- the parity this
    module's own design comment above already claims.
    """
    if path is None:
        yield
        return
    slot_id = holder_id or f"{os.getpid()}-{threading.get_ident()}"
    while not _try_acquire_build_slot(path, max_holders, stale_seconds, now_fn, slot_id):
        sleep_fn(poll_seconds)

    stop_heartbeat = threading.Event()

    def heartbeat_loop():
        # Event.wait doubles as the sleep: it returns True the moment
        # the protected call ends and the event is set, so this thread
        # never outlives the call by more than one (fast, local) state
        # touch -- mirrors model_fix_loop.py's own heartbeat_loop.
        while not stop_heartbeat.wait(heartbeat_seconds):
            try:
                _try_acquire_build_slot(path, max_holders, stale_seconds, now_fn, slot_id)
            except Exception:
                # A transient touch failure (ENOSPC/EACCES, a torn read
                # racing another holder's write, ...) must not kill this
                # thread or the protected build/test call -- an unhandled
                # raise here would otherwise die silently via threading's
                # default excepthook (stderr only) while the build keeps
                # running for however long it takes, reverting to exactly
                # the stale-holder-eviction-of-a-live-holder failure this
                # heartbeat exists to prevent. It just risks this one beat
                # being missed; the next one (or the eventual release)
                # still runs.
                pass

    heartbeat_thread = None
    if heartbeat_seconds:
        heartbeat_thread = threading.Thread(
            target=heartbeat_loop, name=f"build-semaphore-heartbeat-{slot_id}", daemon=True,
        )
        heartbeat_thread.start()

    try:
        yield
    finally:
        stop_heartbeat.set()
        if heartbeat_thread is not None:
            heartbeat_thread.join()
        _release_build_slot(path, slot_id)


# Best-effort format -> source directory/file map, used to hand the model
# real context (it has no file-search tool of its own -- single-shot patch
# generation only). Not authoritative; unlisted formats fall back to a
# lowercase directory guess, and finding nothing is a valid, handled
# outcome (the prompt tells the model these are "likely relevant", not
# exhaustive).
FORMAT_TO_DIR = {
    "JPEG": ["parsers/jpeg", "core"],
    "PNG": ["parsers/png", "core"],
    "TIFF": ["parsers/tiff"],
    "EXIF": ["parsers/tiff"],
    "BMP": ["parsers/image"],
    "GIF": ["parsers/image"],
    "WebP": ["parsers/image"],
    "PDF": ["parsers/pdf"],
    "QuickTime": ["parsers/quicktime"],
    "MP4": ["parsers/quicktime"],
    "MOV": ["parsers/quicktime"],
    "MKV": ["parsers/video"],
    "AVI": ["parsers/video"],
    "RIFF": ["parsers/video"],
    "PE": ["parsers/pe"],
    "ELF": ["parsers/elf"],
    "Mach-O": ["parsers/macho"],
    "ZIP": ["parsers/archive"],
    "DOCX": ["parsers/document"],
    "XLSX": ["parsers/document"],
    "TTF": ["parsers/font"],
    "OTF": ["parsers/font"],
    "DNG": ["parsers/raw"],
    "CR2": ["parsers/raw"],
    "NEF": ["parsers/raw"],
    "ARW": ["parsers/raw"],
    "RAF": ["parsers/raw"],
    "ORF": ["parsers/raw"],
    "RW2": ["parsers/raw"],
    "ICC": ["parsers/icc"],
    "XMP": ["parsers/xmp"],
    "FLAC": ["parsers/audio"],
    "MP3": ["parsers/audio"],
    "AAC": ["parsers/audio"],
    "APE": ["parsers/audio"],
    "Opus": ["parsers/audio"],
    "OGG": ["parsers/audio"],
    "WAV": ["parsers/audio"],
    # FlashPix property sets are MS-OLEPS, parsed by the OLE compound-file
    # reader in parsers/archive -- there is no separate flashpix parser.
    "FLASHPIX": ["parsers/archive"],
    "IPTC": ["parsers/jpeg"],
}


def load_comparison_report(path):
    """Load a tag-comparison ComparisonReport JSON file."""
    with open(path) as f:
        return json.load(f)


def locate_parser_files(format_name, repo_root=REPO_ROOT):
    """Best-effort list of source paths likely responsible for `format_name`.

    Not authoritative -- the model still needs to be told to double-check
    against the actual gap list, but this saves it from starting with
    nothing (it has no file-search tool of its own).
    """
    candidates = FORMAT_TO_DIR.get(format_name, [f"parsers/{format_name.lower()}"])
    found = []
    for rel in candidates:
        path = repo_root / "src" / rel
        if path.is_file():
            found.append(str(path.relative_to(repo_root)))
        elif path.is_dir():
            for rs_file in sorted(path.rglob("*.rs")):
                found.append(str(rs_file.relative_to(repo_root)))
    return found


def group_gaps_by_format(report, repo_root=REPO_ROOT):
    """Group a ComparisonReport's by_format map into a sorted gap list.

    Returns entries for formats with at least one missing_in_oxidex or
    value_differences entry, OR at least one duplicate_emissions entry,
    sorted by combined gap count descending.

    duplicate_emissions keeps a format in the list even at gap_count == 0
    (2026-07-26). Every publish gate reads duplicates through a per-format
    entry this function produced -- squad_merge_loop.real_format_match is
    `next((g for g in group_gaps_by_format(...) if g["format"] == fmt),
    None)`, and process_commit/run_batch_check then do
    `(post or {}).get("duplicate_emissions") or []`, as does
    overlord_sweep.evaluate_post_merge. Dropping a zero-gap format made
    that `None`, so the gates read `[]` and passed. A format at 100%
    parity that emits one tag twice under two keys is exactly where the
    old behavior was worst: there was no gap left to draw anyone's eye,
    and the double emission -- the thing that made GIF's gap list flip
    between runs on an unchanged tree -- was silently unreportable.

    These entries carry gap_count 0 with empty missing_tags/
    value_differences, so they sort last and contribute no work units:
    model_fix_loop.expand_gaps_to_tags iterates exactly those two lists,
    so a duplicate-only entry expands to zero tag_gaps and can never
    dispatch a worker at a format with nothing to fix.
    """
    gaps = []
    for fmt, comp in (report.get("by_format") or {}).items():
        missing = comp.get("missing_in_oxidex") or []
        diffs = comp.get("value_differences") or []
        duplicates = comp.get("duplicate_emissions") or []
        gap_count = len(missing) + len(diffs)
        if gap_count == 0 and not duplicates:
            continue
        gaps.append({
            "format": fmt,
            "missing_tags": missing,
            "value_differences": diffs,
            "gap_count": gap_count,
            "parser_files": locate_parser_files(fmt, repo_root),
            # Spec M3: threaded straight from the Rust ComparisonReport so
            # tag_still_open's duplicate_emission check and
            # new_oxidex_only_keys (model_fix_loop.py) can see them.
            "duplicate_emissions": duplicates,
            "extra_in_oxidex": comp.get("extra_in_oxidex") or [],
        })
    gaps.sort(key=lambda g: g["gap_count"], reverse=True)
    return gaps


def ensure_tag_comparison_built(repo_root=REPO_ROOT, semaphore_path=None,
                                 semaphore_max_holders=DEFAULT_BUILD_SEMAPHORE_MAX_HOLDERS):
    """Build tag-comparison under the "fixloop" profile (see Cargo.toml) --
    this runs on every single round of a fix-loop to re-check gaps, so it's
    a correctness check, not a binary anyone ships; --release's fat LTO and
    single codegen unit make every one of those rebuilds pay a compile cost
    tuned for runtime speed nobody needs here.

    List-argv only, no shell=True anywhere in this file -- repo_root is a
    local path this process already trusts.

    semaphore_path (spec section 5's build semaphore -- see
    build_semaphore above), if given, gates this cargo build behind the
    shared cross-process slot limit alongside every worker's own cargo
    build/test/check calls, so a full round of workers all re-checking
    gaps at once can't oversubscribe the host's cores by linking
    concurrently. None (the default) keeps this call ungated -- every
    existing caller/test is unaffected unless it opts in.
    """
    env = dict(os.environ)
    if shutil.which("sccache"):
        # See model_fix_loop.py's cargo_env() -- lets parallel workers
        # (each its own worktree with its own target/) share compiled
        # dependency artifacts instead of every worker cold-compiling the
        # same crates independently.
        env["RUSTC_WRAPPER"] = "sccache"
    with build_semaphore(semaphore_path, semaphore_max_holders):
        subprocess.run(  # nosec B603
            ["cargo", "build", "--profile", "fixloop", "--bin", "tag-comparison",
             "--features", "tag-comparison-binary"],
            cwd=repo_root, check=True, env=env,
        )


def run_full_comparison(cache_dir, repo_root=REPO_ROOT):
    """Run `just compare-exiftool-full` and return the path to comparison.json."""
    subprocess.run(  # nosec B603
        ["just", "compare-exiftool-full"],
        cwd=repo_root,
        env={**os.environ, "EXIFTOOL_CACHE_DIR": str(cache_dir)},
        check=True,
    )
    return repo_root / "comparison.json"


def run_format_comparison(format_name, cache_dir, repo_root=REPO_ROOT, out_suffix="",
                           semaphore_path=None, semaphore_max_holders=DEFAULT_BUILD_SEMAPHORE_MAX_HOLDERS):
    """Re-run tag-comparison for a single format against the cached samples.

    Requires run_full_comparison to have populated cache_dir at least once
    (this does not download or build the combined samples itself).

    out_suffix, when non-empty, namespaces the output paths per caller:
    /tmp/tagcmp-<FMT>-<suffix>.json plus a matching -md dir. Every
    concurrent process re-checking the same format (each model-fix
    worker passes its worker id) gets its own report file, ending the
    shared-fixed-path race where two same-format workers overwrote each
    other's report mid-recheck and corrupted tag_still_open verdicts.
    Empty (the default) keeps the legacy un-suffixed path for
    single-process/manual use.

    semaphore_path/semaphore_max_holders, if given, are passed straight
    through to ensure_tag_comparison_built (spec section 5's build
    semaphore); None (the default) keeps this call's cargo build
    ungated, same as before this feature existed.
    """
    ensure_tag_comparison_built(repo_root, semaphore_path=semaphore_path,
                                 semaphore_max_holders=semaphore_max_holders)
    # Fixed /tmp paths are a race-condition concern on shared multi-user
    # systems; this is a single-developer local CLI tool.
    suffix = f"-{out_suffix}" if out_suffix else ""
    output = Path(f"/tmp/tagcmp-{format_name}{suffix}.json")  # nosec B108
    subprocess.run(  # nosec B603 # nosemgrep: python.lang.security.audit.dangerous-subprocess-use-audit.dangerous-subprocess-use-audit
        [
            str(repo_root / "target/fixloop/tag-comparison"),
            "--exiftool", f"{cache_dir}/exiftool/exiftool",
            "--samples", f"{cache_dir}/combined-samples",
            "--format", format_name,
            "-o", str(output),
            "--markdown-dir", f"/tmp/tagcmp-{format_name}{suffix}-md",  # nosec B108
        ],
        cwd=repo_root, check=True,
    )
    return output


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", default="gaps.json")
    parser.add_argument("--only-format")
    # A fixed /tmp default is a race-condition concern on shared multi-user
    # systems; this is a single-developer local CLI tool, and the value is
    # always overridable via EXIFTOOL_CACHE_DIR/--cache-dir.
    parser.add_argument(
        "--cache-dir",
        default=os.environ.get("EXIFTOOL_CACHE_DIR", "/tmp/oxidex-exiftool-cache"),  # nosec B108
    )
    args = parser.parse_args(argv)

    if args.only_format:
        report_path = run_format_comparison(args.only_format, args.cache_dir)
    else:
        report_path = run_full_comparison(args.cache_dir)

    report = load_comparison_report(report_path)
    gaps = group_gaps_by_format(report)
    if args.only_format:
        gaps = [g for g in gaps if g["format"] == args.only_format]

    with open(args.output, "w") as f:
        json.dump(gaps, f, indent=2)

    total = sum(g["gap_count"] for g in gaps)
    print(f"{len(gaps)} formats with gaps, {total} total gaps -> {args.output}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
