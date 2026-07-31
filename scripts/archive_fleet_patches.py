#!/usr/bin/env python3
"""Back up every unlanded fleet fix commit as a re-appliable patch file.

Fleet work lives in four places that each get destroyed for a different,
routine reason:

  * worker worktrees (~/.oxidex/worktrees/parallel-fix/*) -- reset to
    origin/main on every sync, which is a normal part of the loop
  * squad staging branches -- re-cut from origin/main when stale (spec M5),
    and a re-cut that hits a conflict can strand commits
  * salvage/* branches -- ad-hoc, easy to prune by accident
  * the quarantine ledger -- records a sha, but nothing pins that sha, and
    an unreferenced commit is gc-able

None of those is a backup. On 2026-07-27 a recut correctly ABORTED rather
than discard two real fixes (a CR2 2-tag and a JPEG 5-tag commit) -- the
no-discard invariant caught it, but only because someone was watching. This
script makes that safety unconditional: run it before any destructive fleet
operation and the work survives regardless.

Identity is the PATCH-ID, not the sha. Cherry-picks and re-cuts rewrite shas
freely while preserving content, so the same fix appears under many shas
across the four sources; patch-id collapses them to one archived file and is
also how we tell "already landed on main" from "still outstanding".

Output layout:

    <archive>/<run>/patches/<format>-<patch-id[:12]>.patch
    <archive>/<run>/manifest.json
    <archive>/<run>/RESTORE.md

Every patch is `git format-patch` output, so it carries the full commit
message including the evidence trailers -- restoring is `git am`, and the
validator sees exactly what it saw originally.
"""

import argparse
import json
import subprocess  # nosec B404 -- list-argv only, no shell=True anywhere below
import sys
from datetime import datetime, timezone
from pathlib import Path

DEFAULT_ARCHIVE = Path.home() / ".oxidex" / "patch-archive"
DEFAULT_REPO = Path("/Users/allen/git/oxidex")
WORKER_GLOB = Path.home() / ".oxidex" / "worktrees" / "parallel-fix"
SQUAD_GLOB = Path.home() / ".oxidex" / "worktrees" / "squad-staging"
QUARANTINE = Path.home() / ".oxidex" / "logs" / "quarantine.jsonl"

#: Subjects worth archiving. Sync/merge noise on a worker branch is already
#: on main by definition and needs no backup.
#:
#: `tuning:` and `chore(` are here because of a near-miss on 2026-07-27: the
#: first version of this script matched only fix(/feat(, and silently skipped
#: `tuning: nudge fixer toward earlier patch attempts` -- a commit that was
#: DANGLING (on no branch at all, alive only on git's grace period) and would
#: have been lost at the next gc. An archive that quietly declines to archive
#: things is worse than no archive, because it is trusted.
#:
#: The prefix list is a heuristic, so `--all-unlanded` overrides it entirely
#: and takes everything reachable in the ranges scanned.
FIX_SUBJECT_HINTS = ("fix(", "feat(", "tuning:", "chore(", "perf(", "test(", "refactor(")


def git(repo, *args, check=False):
    """Run git in `repo`; return stdout (stripped). Never raises unless
    check=True -- a missing worktree or an unborn branch is an expected,
    skippable condition here, not an error worth aborting a backup over."""
    proc = subprocess.run(  # nosec B603
        ["git", *args], cwd=str(repo), capture_output=True, text=True,
    )
    if check and proc.returncode != 0:
        raise RuntimeError(f"git {' '.join(args)} failed: {proc.stderr.strip()}")
    return proc.stdout.strip()


def patch_id(repo, sha):
    """Content identity, stable across cherry-pick and rebase.

    --stable so the id does not shift with hunk ordering; that is the whole
    point of using it as the dedup key across four sources that each rewrite
    shas."""
    show = subprocess.run(  # nosec B603
        ["git", "show", sha], cwd=str(repo), capture_output=True, text=True,
    )
    if show.returncode != 0:
        return None
    pid = subprocess.run(  # nosec B603
        ["git", "patch-id", "--stable"], cwd=str(repo),
        input=show.stdout, capture_output=True, text=True,
    )
    out = pid.stdout.split()
    return out[0] if out else None


def landed_patch_ids(repo, base_ref, since):
    """Patch-ids already on base_ref, so the archive can label what is
    outstanding versus what merely has not been garbage-collected yet."""
    log = subprocess.run(  # nosec B603
        ["git", "log", "--format=%H", f"--since={since}", base_ref],
        cwd=str(repo), capture_output=True, text=True,
    )
    ids = {}
    for sha in log.stdout.split():
        pid = patch_id(repo, sha)
        if pid:
            ids[pid] = sha
    return ids


def commit_meta(repo, sha):
    body = git(repo, "log", "-1", "--format=%B", sha)
    subject = git(repo, "log", "-1", "--format=%s", sha)
    tags, fmt, verified = [], "", ""
    for line in body.splitlines():
        if line.startswith("Tag:"):
            tags.append(line.split(":", 1)[1].strip())
        elif line.startswith("Format:"):
            fmt = line.split(":", 1)[1].strip()
        elif line.startswith("Verified:"):
            verified = line.split(":", 1)[1].strip()
    return {
        "subject": subject,
        "format": fmt,
        "tags": tags,
        "tag_count": len(tags),
        "verified": verified,
        "date": git(repo, "log", "-1", "--format=%cI", sha),
    }


def collect_sources(repo, base_ref):
    """(sha, source_label) for every candidate commit in all four places.

    Worker/squad ranges use the MERGE-BASE, never `git diff branch main`:
    diffing against the tip measures total divergence and happily reports a
    branch that is merely BEHIND as if it carried unique work."""
    found = []

    for wt in sorted(SQUAD_GLOB.glob("*")):
        if not (wt / ".git").exists():
            continue
        mb = git(wt, "merge-base", "HEAD", base_ref)
        if not mb:
            continue
        for sha in git(wt, "log", "--format=%H", f"{mb}..HEAD").split():
            found.append((sha, f"squad/{wt.name}"))

    for wt in sorted(WORKER_GLOB.glob("*")):
        if not (wt / ".git").exists():
            continue
        mb = git(wt, "merge-base", "HEAD", base_ref)
        if not mb:
            continue
        for sha in git(wt, "log", "--format=%H", f"{mb}..HEAD").split():
            found.append((sha, f"worker/{wt.name}"))

    for line in git(repo, "branch", "--list", "salvage/*", "--format=%(refname:short)").splitlines():
        br = line.strip()
        if not br:
            continue
        mb = git(repo, "merge-base", br, base_ref)
        if not mb:
            continue
        for sha in git(repo, "log", "--format=%H", f"{mb}..{br}").split():
            found.append((sha, f"salvage/{br.split('/', 1)[-1]}"))

    if QUARANTINE.exists():
        for raw in QUARANTINE.read_text(errors="replace").splitlines():
            try:
                sha = json.loads(raw).get("sha")
            except ValueError:
                continue
            # A quarantined sha may already be unreachable; cat-file is the
            # cheap way to find out before asking for a patch.
            if sha and git(repo, "cat-file", "-t", sha) == "commit":
                found.append((sha, "quarantine"))

    return found


def main():
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--repo", default=str(DEFAULT_REPO))
    ap.add_argument("--archive", default=str(DEFAULT_ARCHIVE))
    ap.add_argument("--base-ref", default="origin/main")
    ap.add_argument("--since", default="2026-07-20",
                    help="how far back to scan base-ref for already-landed patch-ids")
    ap.add_argument("--run-name", default=None)
    ap.add_argument("--all-unlanded", action="store_true",
                    help="archive EVERY commit in the scanned ranges, ignoring the\nsubject-prefix heuristic. Use when correctness matters more than tidiness -- a\nmissed commit that was dangling is unrecoverable.")
    args = ap.parse_args()

    repo = Path(args.repo)
    stamp = args.run_name or datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    out = Path(args.archive) / stamp
    patches = out / "patches"
    patches.mkdir(parents=True, exist_ok=True)

    git(repo, "fetch", "origin", "main")
    base_sha = git(repo, "rev-parse", args.base_ref)
    print(f"base {args.base_ref} = {base_sha[:12]}")

    print("indexing patch-ids already on main...")
    landed = landed_patch_ids(repo, args.base_ref, args.since)
    print(f"  {len(landed)} landed patch-ids indexed")

    print("collecting candidates from worktrees, salvage branches, quarantine...")
    candidates = collect_sources(repo, args.base_ref)
    print(f"  {len(candidates)} raw candidate commits")

    entries = {}
    for sha, source in candidates:
        meta = commit_meta(repo, sha)
        if not args.all_unlanded and not any(
            meta["subject"].startswith(h) or h in meta["subject"]
            for h in FIX_SUBJECT_HINTS
        ):
            continue
        pid = patch_id(repo, sha)
        if not pid:
            continue
        if pid in entries:
            entries[pid]["sources"].append({"sha": sha, "where": source})
            continue
        entries[pid] = {
            "patch_id": pid,
            "sources": [{"sha": sha, "where": source}],
            "already_on_main": pid in landed,
            "landed_as": landed.get(pid),
            **meta,
        }

    written, outstanding, tags_out = 0, 0, 0
    for pid, e in sorted(entries.items(), key=lambda kv: kv[1]["date"]):
        sha = e["sources"][0]["sha"]
        fmt = (e["format"] or "unknown").lower().replace("/", "_")
        name = f"{fmt}-{pid[:12]}.patch"
        text = subprocess.run(  # nosec B603
            ["git", "format-patch", "-1", "--stdout", sha],
            cwd=str(repo), capture_output=True, text=True,
        ).stdout
        if not text:
            continue
        (patches / name).write_text(text)
        e["patch_file"] = name
        # Re-appliability against CURRENT main is recorded, not required: a
        # patch that conflicts today is exactly the kind we most need backed
        # up, and its conflict is a restore-time problem, not a backup-time
        # one.
        chk = subprocess.run(  # nosec B603
            ["git", "apply", "--check", "-"], cwd=str(repo),
            input=text, capture_output=True, text=True,
        )
        e["applies_cleanly_to_main"] = chk.returncode == 0
        written += 1
        if not e["already_on_main"]:
            outstanding += 1
            tags_out += e["tag_count"]

    # A git BUNDLE alongside the patch files. Patches preserve content and
    # message but throw away parentage, and parentage is exactly what makes a
    # conflicted restore tractable: with the original commit and its real base
    # you can rebase or merge with full history, instead of hand-resolving a
    # context-free hunk. Measured on the first run: 40 of 67 patches conflict
    # against current main (files like src/parsers/raw/metadata.rs moved under
    # them), so the bundle is the difference between "we kept the diffs" and
    # "we kept the commits".
    bundle = out / "commits.bundle"
    bundle_refs = []
    for pid, e in entries.items():
        sha = e["sources"][0]["sha"]
        ref = f"refs/archive/{pid[:12]}"
        # Pin every archived commit under a ref so it is reachable, and so gc
        # can never collect it out from under the bundle.
        git(repo, "update-ref", ref, sha)
        bundle_refs.append(ref)
    if bundle_refs:
        proc = subprocess.run(  # nosec B603
            ["git", "bundle", "create", str(bundle), *bundle_refs],
            cwd=str(repo), capture_output=True, text=True,
        )
        bundle_ok = proc.returncode == 0
        if bundle_ok:
            verify = subprocess.run(  # nosec B603
                ["git", "bundle", "verify", str(bundle)],
                cwd=str(repo), capture_output=True, text=True,
            )
            bundle_ok = verify.returncode == 0
    else:
        bundle_ok = False

    manifest = {
        "created": datetime.now(timezone.utc).isoformat(),
        "bundle": "commits.bundle" if bundle_ok else None,
        "bundle_verified": bundle_ok,
        "base_ref": args.base_ref,
        "base_sha": base_sha,
        "distinct_patches": written,
        "outstanding": outstanding,
        "outstanding_tags": tags_out,
        "entries": sorted(entries.values(), key=lambda e: e["date"]),
    }
    (out / "manifest.json").write_text(json.dumps(manifest, indent=2))
    (out / "RESTORE.md").write_text(f"""# Fleet patch archive {stamp}

{written} distinct patches (deduped by patch-id), {outstanding} not yet on
main, carrying {tags_out} tag trailers. Base was `{args.base_ref}` @
`{base_sha[:12]}`.

## Two copies of everything

`commits.bundle` holds the real commits with their parentage
(verified: {bundle_ok}). `patches/` holds the same work as
`git format-patch` output. Prefer the bundle when a patch conflicts -- with
the original commit and its true base you can rebase or merge with history,
instead of hand-resolving a context-free hunk.

    git fetch {bundle.name} 'refs/archive/*:refs/archive/*'
    git log --oneline refs/archive/<patch-id-prefix>
    git cherry-pick refs/archive/<patch-id-prefix>     # or rebase onto main

Every patch file is `git format-patch` output, so the full commit message and
its evidence trailers survive. Restore one with:

    git am patches/<file>.patch

If it conflicts (`applies_cleanly_to_main: false` in the manifest -- expected
for anything written against an older base):

    git am --3way patches/<file>.patch
    # or, to inspect first:
    git apply --3way --reject patches/<file>.patch

Restore every outstanding patch, oldest first:

    python3 - <<'EOF'
    import json, subprocess, pathlib
    m = json.load(open("manifest.json"))
    for e in m["entries"]:
        if e["already_on_main"]:
            continue
        p = pathlib.Path("patches") / e["patch_file"]
        print(e["format"], e["tag_count"], "tags", e["subject"][:60])
        subprocess.run(["git", "am", "--3way", str(p)])
    EOF

`manifest.json` records, per patch: every (sha, location) it was found at,
whether its patch-id is already on main, its tag trailers, and its
`Verified:` stamp.
""")

    print(f"\narchive: {out}")
    print(f"  {written} distinct patches written")
    print(f"  {outstanding} NOT yet on main, carrying {tags_out} tag trailers")
    return 0


if __name__ == "__main__":
    sys.exit(main())
