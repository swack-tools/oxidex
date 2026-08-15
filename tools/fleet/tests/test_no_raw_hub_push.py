#!/usr/bin/env python3
"""Guard: no fleet tool raw-writes the hub outside `fleetlib.Hub` (ARCH-FIX R9).

The day's proof of need: `train.py`'s staging-ref cleanup used a raw
`git push origin --delete <ref>` -- no compare-and-swap, no lease, nothing
stopping two racing trains (or a train and a human) from deleting a ref out
from under each other. `fleetlib.Hub` exists precisely to make hub writes
safe (`create`/`update`/`delete` all go through a CAS `sha`-compare or a
`--force-with-lease`); a raw `git push` or `git update-ref` bypasses all of
that. This test is the mechanical fence: it greps every production fleet
tool (`tools/fleet/*.py`, i.e. direct children only -- not `hooks/`,
`rollout/`, `units/`, or `tests/`) for the two raw-hub-write shapes named in
ARCH-FIX-SPEC.md R9 ("git push invocations, update-ref against a hub path")
and fails, listing offenders, for anything not on the explicit allowlist
below.

`fleetlib.py` itself is exempt -- it *is* the sanctioned implementation of
hub writes; everyone else is supposed to call into it, not reimplement it.

Every allowlist entry below is a live claim about a specific line, not a
blanket exemption: matching is by a short, distinctive substring of the
offending line (robust to line-number drift as neighboring code changes),
and this test also asserts every entry actually matches something. An
allowlist entry that stops matching means the offending line changed shape
(most likely because it was fixed) and the entry -- and whatever excuse it
was carrying -- must be deleted, not left to rot as permission for a defect
that no longer exists.

Run with:
    python3 -m unittest discover -s tools/fleet/tests -v
"""

from __future__ import annotations

import re
import sys
import unittest
from pathlib import Path

FLEET_DIR = Path(__file__).resolve().parents[1]  # tools/fleet

# Excluded from scanning entirely: fleetlib.py IS the sanctioned hub-write
# implementation (create/update/delete, all CAS-protected); tests exercise
# fixtures, never the real hub, and grepping test bodies for these shapes
# would just flag the fixtures those tests build "raw" git repos with.
EXEMPT_FILES = frozenset({"fleetlib.py"})

# A raw `git push`/`git update-ref` invocation, as a quoted string literal
# used as a list/tuple element passed to a subprocess call -- e.g.
# `["push", ...]` or `(..., "update-ref", ...)`. Anchored on `[`, `(` or `,`
# immediately before the quote so prose that merely *mentions* "push" or
# "update-ref" in a comment or docstring (this file included) is never
# mistaken for an actual invocation.
_PATTERN = re.compile(r'[\[(,]\s*["\'](push|update-ref)["\']')

# (filename, distinctive substring of the offending line) -> justification.
#
# The T2 x T4 reconciliation (ARCH-FIX FIX-3b): this allowlist used to carry
# four train.py entries excusing raw pushes -- the two TODO(T2) staging-ref
# and temp-gate-ref `--delete`s (T2's R3(b) fix routed both through
# `fleetlib.Hub.delete(expect_sha)`), plus two "Hub has no generic
# branch-content push primitive" exemptions for the tip push and the temp
# gate ref push. That reasoning no longer holds: `fleetlib.Hub.push_ref`
# (built by T3 for exactly this) now carries push_options plumbing for a
# plain branch-content push, so train.py's tip push and temp-gate-ref push
# are routed through it too (fetching the commit into the hub's own local
# cache first -- see `train._fetch_into_hub_cache` / `Hub.push_ref`'s
# docstring) and neither raw `git push` line exists anymore. Only the
# rescued/<slug> push below is still raw and still justified: `rescued/*`
# is explicitly outside R1's tip protection, and `Hub.push_ref` would work
# for it too, but nothing here has needed CAS or push_options for it -- if
# that changes, route it through `push_ref` as well and drop this entry.
_ALLOWLIST = {
    (
        "drift.py",
        '_git_dir(git_dir, ["update-ref", ref, new_commit_sha, old_commit_sha])',
    ): (
        "bump_tip_signal's CAS loop -- ARCH-FIX R9 explicitly keeps this "
        "function; it runs locally via `git --git-dir` against the hub's "
        "OWN bare repo when installed as the post-receive hook (never a "
        "network push -- the hook already is the hub, per this module's "
        "docstring), and only ever touches refs/fleet/signals/tip, a "
        "non-gated ref. It mirrors fleetlib.Hub's own read-current/"
        "compute-next/CAS-write pattern, just without the ssh hop."
    ),
    (
        "workqueue.py",
        'self._git(["update-ref", "-d", ref])',
    ): (
        "_cleanup_cache() deletes a ref inside workqueue's own disposable "
        "local fetch-cache (self.hub.workdir, a private object-store "
        "mirror populated by _fetch_for_ancestry's own throwaway "
        "namespace under refs/fleet-queue-cache/<uuid>/...). The ref was "
        "never pushed anywhere and is never visible on the actual hub; "
        "this is local scratch-state cleanup, not a hub write."
    ),
    (
        "train.py",
        '_git(["push", "origin", f"{c.sha}:{rescued_ref}"], cwd=clone, check=False)',
    ): (
        "Pushes a rescue copy to refs/heads/rescued/<slug> -- a namespace "
        "R1 explicitly exempts from tip protection ('must NOT affect "
        "staging/*, rescued/*, wip/*, refs/fleet/*'). Unlike the tip push "
        "and the temp gate-ref push (both now routed through "
        "fleetlib.Hub.push_ref), this one has no CAS or push_options need, "
        "so it has not been moved off a plain `git push`; if that changes, "
        "route it through `push_ref` too and drop this entry."
    ),
}


def _candidate_files():
    """Direct `.py` children of tools/fleet/ only -- not hooks/, rollout/,
    units/, or tests/ (a plain `*.py` glob is already non-recursive, so
    this is just documenting that, not extra filtering).
    """
    for path in sorted(FLEET_DIR.glob("*.py")):
        if path.name in EXEMPT_FILES:
            continue
        yield path


def _scan(path: Path):
    """(lineno, line_text, matched_keyword) for every raw push/update-ref
    invocation found in `path`.
    """
    hits = []
    for lineno, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
        match = _PATTERN.search(line)
        if match:
            hits.append((lineno, line.strip(), match.group(1)))
    return hits


class TestNoRawHubPush(unittest.TestCase):
    def test_no_unallowlisted_raw_hub_writes(self):
        offenders = []
        matched_allowlist_keys = set()

        for path in _candidate_files():
            for lineno, line, keyword in _scan(path):
                allow_key = None
                for (fname, substring), _reason in _ALLOWLIST.items():
                    if fname == path.name and substring in line:
                        allow_key = (fname, substring)
                        break
                if allow_key is not None:
                    matched_allowlist_keys.add(allow_key)
                    continue
                offenders.append(f"{path.relative_to(FLEET_DIR.parent.parent)}:{lineno}: [{keyword}] {line}")

        self.assertEqual(
            offenders,
            [],
            "raw hub-write pattern(s) found outside fleetlib.py and the explicit "
            "allowlist -- route hub writes through fleetlib.Hub instead (see this "
            "test's module docstring), or add a justified allowlist entry:\n  "
            + "\n  ".join(offenders),
        )

    def test_allowlist_has_no_stale_entries(self):
        """Every allowlist entry must match a real line in the real file it
        names. An entry that matches nothing means the line it excused has
        changed shape (most likely fixed) -- the excuse must be deleted
        along with it, not left to quietly cover for whatever replaced it.
        """
        matched = set()
        for path in _candidate_files():
            lines = {line.strip() for _lineno, line, _kw in _scan(path)}
            for (fname, substring), _reason in _ALLOWLIST.items():
                if fname != path.name:
                    continue
                if any(substring in line for line in lines):
                    matched.add((fname, substring))

        stale = sorted(set(_ALLOWLIST) - matched)
        self.assertEqual(
            stale,
            [],
            f"stale allowlist entries (no longer match any raw push/update-ref "
            f"line -- delete them): {stale}",
        )

    def test_pattern_actually_detects_a_known_raw_delete_shape(self):
        """Sanity check on the detector itself, independent of the current
        tree's contents: a synthetic line shaped like the train's original
        `git push origin --delete <ref>` defect must be caught by
        `_PATTERN`, so a regression in the regex can't silently turn this
        whole test into a no-op.
        """
        sample = '_git(["push", "origin", "--delete", f"refs/heads/staging/{c.slug}"], cwd=clone)'
        self.assertTrue(_PATTERN.search(sample), "detector failed to match a known raw --delete push shape")

        sample_update_ref = 'subprocess.run(["git", "--git-dir", hub_path, "update-ref", ref, new_sha])'
        self.assertTrue(_PATTERN.search(sample_update_ref), "detector failed to match a raw update-ref shape")

        sample_safe = "# never use a raw git push --delete against the hub; use fleetlib.Hub.delete()"
        self.assertIsNone(_PATTERN.search(sample_safe), "detector must not match prose mentioning push/update-ref")


if __name__ == "__main__":
    unittest.main()
