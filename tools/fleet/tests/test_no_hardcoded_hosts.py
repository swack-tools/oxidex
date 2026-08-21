#!/usr/bin/env python3
"""Guard: no host-specific URL/path survives in the runtime-config surface
(PLAN Stage 1 task 4, "de-hardcode URLs and paths").

`gate.sh` and `fleetd.py` used to hardcode the old single-repo topology --
the production hub over ssh (`work2.oxidex.net`, `/home/allen/git/
oxidex.git`) and the pinned-oracle cache directory (`/tmp/oxidex-exiftool-
cache`) -- in several places, and the `units/*` templates baked the same
ssh URL into their `Environment`/`EnvironmentVariables`/crontab lines.
Stage 1 splits the hub into two required remotes (`FLEET_HUB_URL` for the
verdict/state repo, `FLEET_CODE_URL` for the repo holding staging
branches) and makes the oracle cache directory `EXIFTOOL_CACHE_DIR`-
overridable; this test is the mechanical fence that keeps either literal
from creeping back into the files this task fixed.

SCOPE, DELIBERATELY NARROW. This scans exactly `gate.sh`, `fleetd.py`, and
every file under `units/` -- the runtime-config surface this task
actually touched -- not the whole `tools/fleet` tree. Two classes of
pre-existing, *intentional* literal reference live elsewhere and must not
be flagged:
  1. `ledger.py`/`doctor.py` already read `EXIFTOOL_CACHE_DIR` with the
     same literal fallback (kept as-is per the reuse map; not part of
     this task).
  2. Half the test suite (`test_fleetlib.py`, `test_claim.py`,
     `test_queue.py`, ...) asserts `assertNotIn("work2.oxidex.net",
     resolved)` as a fixture-isolation guard -- the string appears there
     as a *negative* to check against, never as a value anything
     connects to. Scanning `tests/` would flag the safety net, not a
     regression.
A file-scope fence beats a tree-wide grep here: it catches the real
regression (one of these three literals reappearing in the config surface
that ships to a host) without producing noise on code that has a
legitimate, different reason to hold the same substring.

"Outside comment lines": a comment line (`#`-prefixed for the shell/
Python/ini/crontab files here, `<!-- ... -->` for the one XML file) may
still document the old topology as history -- gate.sh's own header keeps
several such lines, e.g. the PLAN Stage 1 task 4 addendum naming
`work2.oxidex.net` and `/tmp/oxidex-exiftool-cache` as what USED to be
hardcoded. Only a non-comment occurrence is a hardcoded value actually in
effect.

Run with:
    python3 -m unittest discover -s tools/fleet/tests -v
"""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

FLEET_DIR = Path(__file__).resolve().parents[1]  # tools/fleet
REPO_ROOT = FLEET_DIR.parents[1]

# The exact three strings named in PLAN Stage 1 task 4. Plain substrings,
# not a regex -- these are literal hostnames/paths, and treating them as
# regex would risk "." matching more than intended.
_FORBIDDEN = (
    "work2.oxidex.net",
    "/home/allen/git/oxidex.git",
    "/tmp/oxidex-exiftool-cache",
)


def _candidate_files():
    """gate.sh, fleetd.py, and every file directly under units/ -- the
    runtime-config surface PLAN Stage 1 task 4 de-hardcoded. Sorted for a
    deterministic scan order and failure message.
    """
    files = [FLEET_DIR / "gate.sh", FLEET_DIR / "fleetd.py"]
    units_dir = FLEET_DIR / "units"
    files.extend(sorted(p for p in units_dir.iterdir() if p.is_file()))
    return files


def _is_xml(path: Path) -> bool:
    return path.suffix == ".plist"


def _strip_comments(path: Path, text: str):
    """Yield (lineno, line) for every line of `text` that is NOT a full
    comment line, given `path`'s comment syntax.

    XML (`.plist`): a line is dropped only while inside a `<!-- ... -->`
    span that covers the WHOLE line (a `<!--`/`-->` pair on its own lines,
    or a single-line comment) -- this is a line-level filter, adequate for
    the small hand-written unit templates here, not a general XML parser.

    Everything else (`.sh`, `.py`, `.service`, `.txt`): a line is dropped
    when its stripped content starts with `#` (also `;`, the other
    systemd-ini comment marker, for `.service` files).
    """
    if _is_xml(path):
        in_comment = False
        for lineno, line in enumerate(text.splitlines(), start=1):
            stripped = line.strip()
            if in_comment:
                if "-->" in line:
                    in_comment = False
                continue
            if stripped.startswith("<!--"):
                if "-->" not in line:
                    in_comment = True
                continue
            yield lineno, line
        return

    comment_prefixes = ("#",) if path.suffix != ".service" else ("#", ";")
    for lineno, line in enumerate(text.splitlines(), start=1):
        if line.strip().startswith(comment_prefixes):
            continue
        yield lineno, line


def _scan(path: Path):
    """(lineno, line_text, matched_string) for every forbidden literal
    found on a non-comment line of `path`.
    """
    hits = []
    text = path.read_text(encoding="utf-8")
    for lineno, line in _strip_comments(path, text):
        for needle in _FORBIDDEN:
            if needle in line:
                hits.append((lineno, line.strip(), needle))
    return hits


class TestNoHardcodedHosts(unittest.TestCase):
    def test_no_hardcoded_hosts_outside_comments(self):
        offenders = []
        for path in _candidate_files():
            for lineno, line, needle in _scan(path):
                rel = path.relative_to(REPO_ROOT)
                offenders.append(f"{rel}:{lineno}: [{needle}] {line}")

        self.assertEqual(
            offenders,
            [],
            "hardcoded host/path literal(s) found outside comment lines -- read "
            "FLEET_HUB_URL / FLEET_CODE_URL / EXIFTOOL_CACHE_DIR from the "
            "environment instead (see this test's module docstring):\n  "
            + "\n  ".join(offenders),
        )

    def test_candidate_files_exist(self):
        """A typo in `_candidate_files()` (or a renamed/removed unit file)
        must fail loudly, not silently scan zero files and report a vacuous
        pass -- the same class of trap AGENTS.md's "implicit binary
        resolution" note warns about.
        """
        files = _candidate_files()
        self.assertGreaterEqual(
            len(files), 2 + 3,
            f"expected at least gate.sh + fleetd.py + the 3 known units/ files, "
            f"got {[str(p) for p in files]}",
        )
        for path in files:
            self.assertTrue(path.is_file(), f"candidate file missing: {path}")

    def test_detector_catches_a_known_shape_and_ignores_comments(self):
        """Sanity check on the detector itself, independent of the current
        tree's contents, mirroring test_no_raw_hub_push.py's own pattern
        check: a regression in `_scan`/`_strip_comments` must not silently
        turn this whole test into a no-op.
        """
        sample_bad = 'HUB_URL="${FLEET_HUB_URL:-ssh://allen@work2.oxidex.net:2244/home/allen/git/oxidex.git}"\n'
        hits = list(_strip_comments(FLEET_DIR / "gate.sh", sample_bad))
        self.assertEqual(len(hits), 1)
        self.assertIn("work2.oxidex.net", hits[0][1])

        sample_comment = "# the old hub used to be work2.oxidex.net over ssh\n"
        hits = list(_strip_comments(FLEET_DIR / "gate.sh", sample_comment))
        self.assertEqual(hits, [], "a '#'-prefixed comment line must be excluded")

        sample_indented_comment = "    # /tmp/oxidex-exiftool-cache lived here\n"
        hits = list(_strip_comments(FLEET_DIR / "fleetd.py", sample_indented_comment))
        self.assertEqual(hits, [], "leading whitespace before '#' must still count as a comment")

        sample_xml_comment = "<!-- ssh://allen@work2.oxidex.net:2244/home/allen/git/oxidex.git -->\n"
        hits = list(_strip_comments(FLEET_DIR / "units" / "com.oxidex.fleetd.plist", sample_xml_comment))
        self.assertEqual(hits, [], "a single-line XML comment must be excluded")

        sample_xml_live = "<string>ssh://allen@work2.oxidex.net:2244/home/allen/git/oxidex.git</string>\n"
        hits = list(_strip_comments(FLEET_DIR / "units" / "com.oxidex.fleetd.plist", sample_xml_live))
        self.assertEqual(len(hits), 1, "a live (non-comment) XML line must NOT be excluded")


if __name__ == "__main__":
    unittest.main()
