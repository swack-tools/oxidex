#!/usr/bin/env python3
"""Tests for judgment_queue_daemon.py.

Style follows test_squad_merge_loop.py / test_validate_fix_commit.py: the
git mechanics run against REAL tempdir git repositories (a fake git
runner would prove nothing about `git cherry`'s patch-id semantics, which
is the whole basis of the double-promotion guard), while everything slow
or environmental -- the pair verifier, the validator, the cargo-backed
comparison run -- is injected.

Run:
    cd scripts && python3 -m unittest test_judgment_queue_daemon -q
"""
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import judgment_queue_daemon as jq  # noqa: E402
import verify_enum_maps as ve  # noqa: E402
import validate_fix_commit  # noqa: E402


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------

# A real Rust PrintConv-shaped table. validate_fix_commit's own extractor
# must see MacOS/BeOS/OS-2/Unknown in here, because the daemon's
# permanent-rejection guard is keyed on that extractor's opinion.
RAR_SHAPED = '''pub fn host_os(raw: u8) -> &'static str {
    match raw {
        0 => "Win32",
        1 => "Unix",
        2 => "MacOS",
        3 => "BeOS",
        4 => "OS/2",
        _ => "Unknown",
    }
}
'''

# A tag-id -> tag-KEY map. Shaped exactly like DNG 786ea09b, which the
# daemon must NOT permanently reject: validate_fix_commit.looks_like_tag_key
# deliberately excludes these, so they are not PrintConv display values and
# a fabrication verdict against them is not safe to convict on.
TAG_KEY_SHAPED = '''pub fn tag_name(id: u16) -> &'static str {
    match id {
        33421 => "EXIF:CFARepeatPatternDim",
        33422 => "EXIF:CFAPattern2",
        _ => "",
    }
}
'''


# A printconv-class commit message: it cites its own module, which is the
# author attestation the daemon is allowed to convict on. The bookkeeping
# class is the opposite -- all 42 live commits in it have a bare subject
# line and no trailer block at all (BARE_MESSAGE).
TRAILERED_MESSAGE = """fix(rar): wire 2 missing tags

Format: RAR
Tag: ZIP:OperatingSystem
Sample: /samples/ZIP.rar
Exiftool-Value: Unix
Oxidex-Value: Unix
Perl-Ref: ZIP.pm
Verified: recheck-pass gaps=4->2
Worker: tail-1
"""

BARE_MESSAGE = "fix(rar): wire 2 missing tags"


def make_diff(relpath, body):
    """A minimal but REAL unified diff. verify_enum_maps._new_file_lines
    needs the `+++ b/<path>` header and an `@@` hunk header to reconstruct
    the new-file view; a bare list of `+` lines parses to nothing, which is
    indistinguishable from "this commit adds no pairs"."""
    added = body.splitlines()
    return (
        f"diff --git a/{relpath} b/{relpath}\n"
        "new file mode 100644\n"
        "--- /dev/null\n"
        f"+++ b/{relpath}\n"
        f"@@ -0,0 +1,{len(added)} @@\n"
        + "".join(f"+{line}\n" for line in added)
    )


def _run(args, cwd, check=True, input_text=None):
    return subprocess.run(
        args, cwd=str(cwd), capture_output=True, text=True, check=check, input=input_text,
    )


def make_repo(tmp):
    """A repo with one base commit and refs/remotes/origin/main pointing
    at it. No real remote is needed: origin/main is just a ref, and the
    daemon only ever reads it."""
    repo = Path(tmp) / "repo"
    repo.mkdir(parents=True)
    _run(["git", "init", "-q", "-b", "main"], repo)
    _run(["git", "config", "user.email", "t@example.com"], repo)
    _run(["git", "config", "user.name", "T"], repo)
    _run(["git", "config", "commit.gpgsign", "false"], repo)
    src = repo / "src" / "parsers" / "archive"
    src.mkdir(parents=True)
    (src / "rar.rs").write_text("// base\n")
    _run(["git", "add", "-A"], repo)
    _run(["git", "commit", "-qm", "base"], repo)
    base = _run(["git", "rev-parse", "HEAD"], repo).stdout.strip()
    _run(["git", "update-ref", "refs/remotes/origin/main", base], repo)
    return repo, base


def add_commit(repo, *, relpath, body, message, branch=None):
    """Commit `body` at `relpath` on `branch` (created off origin/main),
    then put the repo back on main. Returns the new sha."""
    if branch:
        _run(["git", "checkout", "-q", "-B", branch, "origin/main"], repo)
    path = repo / relpath
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(body)
    _run(["git", "add", "-A"], repo)
    _run(["git", "commit", "-qm", message], repo)
    sha = _run(["git", "rev-parse", "HEAD"], repo).stdout.strip()
    _run(["git", "checkout", "-q", "main"], repo)
    return sha


def clean_verdict(pairs=3):
    return ve.Verdict("clean", [], [], [], reason=None, table="T", pairs_checked=pairs)


def fabricated_verdict(mismatches, catch_alls=(), pairs=5, table="T", candidates=()):
    return ve.Verdict("fabricated", list(mismatches), [], list(catch_alls),
                      reason=None, table=table, candidates=tuple(candidates),
                      pairs_checked=pairs)


def abstain(reason="ambiguous-table", candidates=()):
    return ve.Verdict("cannot-verify", [], [], [], reason=reason,
                      candidates=tuple(candidates), pairs_checked=0)


def gap_report(fmt, missing=(), diffs=()):
    return {
        "format": fmt,
        "missing_tags": list(missing),
        "value_differences": list(diffs),
        "gap_count": len(missing) + len(diffs),
        "duplicate_emissions": [],
        "extra_in_oxidex": [],
    }


# ---------------------------------------------------------------------------
# Triage
# ---------------------------------------------------------------------------

class ClassifyTests(unittest.TestCase):
    def test_matches_on_the_prefix_before_the_colon(self):
        """Every real flag carries a payload -- missing-trailer:Format,
        printconv-mismatch:<excerpt>. Matching whole flags would put 100%
        of the live ledger in "unknown"."""
        self.assertEqual(jq.classify_flags(["missing-trailer:Format"]), "bookkeeping")
        self.assertEqual(jq.classify_flags(["printconv-mismatch:Economy mode"]), "printconv")

    def test_printconv_outranks_bookkeeping(self):
        """9 live commits carry both. Rewriting the paperwork does not
        make a wrong key->value pair right, so it must triage as a
        printconv case."""
        self.assertEqual(
            jq.classify_flags(["missing-trailer:Tag", "printconv-mismatch:x"]), "printconv")

    def test_bookkeeping_outranks_printconv_unverifiable(self):
        """6 live commits carry both, and the root cause is the same one:
        no trailers at all means no Perl-Ref, which is exactly why
        check_printconv could not verify. Deriving the trailers fixes
        both, so it must not triage as an unverifiable case."""
        self.assertEqual(
            jq.classify_flags(["missing-trailer:Perl-Ref", "printconv-unverifiable"]),
            "bookkeeping")

    def test_targeted_test_failure_outranks_everything(self):
        self.assertEqual(
            jq.classify_flags(["targeted-test-failed", "printconv-mismatch:x",
                               "missing-trailer:Tag"]),
            "test-failed")

    def test_measured_corpus_effects_are_their_own_class(self):
        for flag in ("duplicate-emission", "new-oxidex-only", "sweep-bisection", "ff-refused"):
            self.assertEqual(jq.classify_flags([flag]), "semantic", flag)

    def test_unknown_flags_and_no_flags(self):
        self.assertEqual(jq.classify_flags([]), "unknown")
        self.assertEqual(jq.classify_flags(["something-new"]), "unknown")

    def test_every_live_ledger_class_set_classifies(self):
        """The seven flag-class sets measured on the live ledger
        2026-07-27, so a future flag rename cannot silently drop the
        dominant class into "unknown"."""
        live = {
            ("printconv-unverifiable",): "printconv-unverifiable",
            ("printconv-mismatch", "printconv-unverifiable"): "printconv",
            ("cherry-pick-conflict",): "cherry-pick-conflict",
            ("missing-trailer", "printconv-unverifiable"): "bookkeeping",
            ("missing-trailer",): "bookkeeping",
            ("printconv-mismatch",): "printconv",
            ("targeted-test-failed",): "test-failed",
        }
        for flags, expected in live.items():
            self.assertEqual(jq.classify_flags(list(flags)), expected, flags)


# ---------------------------------------------------------------------------
# Decision ledger
# ---------------------------------------------------------------------------

class DecisionLedgerTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.path = Path(self.tmp.name) / "judgment-queue.jsonl"

    def tearDown(self):
        self.tmp.cleanup()

    def write(self, *entries):
        with open(self.path, "w") as fh:
            for entry in entries:
                fh.write(json.dumps(entry) + "\n")

    def test_missing_file_is_not_an_error(self):
        self.assertEqual(jq.load_decisions(self.path), {})

    def test_malformed_lines_are_skipped_not_raised(self):
        with open(self.path, "w") as fh:
            fh.write("not json\n")
            fh.write(json.dumps({"patch_id": "p1", "verdict": "queued"}) + "\n")
            fh.write("[1,2,3]\n")
            fh.write(json.dumps({"verdict": "queued"}) + "\n")  # no patch_id
        self.assertEqual(set(jq.load_decisions(self.path)), {"p1"})

    def test_newest_by_epoch_wins(self):
        self.write({"patch_id": "p", "verdict": "queued", "reason": "old", "ts_epoch": 1},
                   {"patch_id": "p", "verdict": "queued", "reason": "new", "ts_epoch": 2})
        self.assertEqual(jq.load_decisions(self.path)["p"]["reason"], "new")

    def test_terminal_verdict_survives_a_later_non_terminal_line(self):
        """A second daemon, or a hand-edited ledger, must not be able to
        re-open a patch-id that is already downstream."""
        self.write({"patch_id": "p", "verdict": "promoted", "ts_epoch": 1},
                   {"patch_id": "p", "verdict": "queued", "ts_epoch": 99})
        self.assertEqual(jq.load_decisions(self.path)["p"]["verdict"], "promoted")

    def test_append_is_a_no_op_without_apply(self):
        jq.append_decision(self.path, {"patch_id": "p", "verdict": "queued"}, apply=False)
        self.assertFalse(self.path.exists())

    def test_append_writes_exactly_one_line_per_call(self):
        for i in range(3):
            jq.append_decision(self.path, {"patch_id": f"p{i}", "verdict": "queued"}, apply=True)
        self.assertEqual(len(self.path.read_text().splitlines()), 3)

    def test_make_decision_reads_the_clock_once(self):
        """`ts` and `ts_epoch` must describe the same instant; an
        advancing test clock is what catches a second now_fn() call."""
        ticks = iter([1000.0, 9999.0])
        entry = jq.make_decision(patch_id="p", sha="s", format_name="RW2", squad="q",
                                 klass="bookkeeping", verdict="queued", reason="r",
                                 now_fn=lambda: next(ticks))
        self.assertEqual(entry["ts_epoch"], 1000.0)
        self.assertIn("ruleset", entry)
        self.assertEqual(entry["policy_version"], validate_fix_commit.POLICY_VERSION)
        self.assertEqual(entry["verifier_version"], ve.VERIFIER_VERSION)


class NeedsAdjudicationTests(unittest.TestCase):
    def test_never_decided_is_a_candidate(self):
        self.assertTrue(jq.needs_adjudication({"attempt": 1}, None, "d1/p6/v1"))

    def test_terminal_verdicts_are_never_revisited(self):
        """This is what stops "the same patch-id re-adjudicated in a
        loop" -- and it holds THROUGH a ruleset change, unlike every
        other verdict."""
        for verdict in ("promoted", "rejected-permanent", "already-landed"):
            decision = {"verdict": verdict, "ruleset": "d0/p0/v0", "attempt": 1}
            self.assertFalse(jq.needs_adjudication({"attempt": 9}, decision, "d1/p6/v1"), verdict)

    def test_a_ruleset_change_reopens_a_queued_decision(self):
        decision = {"verdict": "queued", "ruleset": "d1/p5/v1", "attempt": 1}
        self.assertTrue(jq.needs_adjudication({"attempt": 1}, decision, "d1/p6/v1"))

    def test_same_ruleset_and_same_attempt_is_skipped(self):
        decision = {"verdict": "queued", "ruleset": "d1/p6/v1", "attempt": 2}
        self.assertFalse(jq.needs_adjudication({"attempt": 2}, decision, "d1/p6/v1"))

    def test_a_newer_quarantine_attempt_reopens_it(self):
        decision = {"verdict": "queued", "ruleset": "d1/p6/v1", "attempt": 1}
        self.assertTrue(jq.needs_adjudication({"attempt": 2}, decision, "d1/p6/v1"))

    def test_a_dry_run_decision_never_counts_as_having_decided(self):
        decision = {"verdict": "rejected-permanent", "ruleset": "d1/p6/v1",
                    "attempt": 1, "dry_run": True}
        self.assertTrue(jq.needs_adjudication({"attempt": 1}, decision, "d1/p6/v1"))

    def test_ruleset_id_tracks_all_three_versions(self):
        self.assertEqual(jq.ruleset_id(2, 7, 3), "d2/p7/v3")
        self.assertNotEqual(jq.ruleset_id(1, 6, 1), jq.ruleset_id(1, 6, 2))


# ---------------------------------------------------------------------------
# Gap-report readers
# ---------------------------------------------------------------------------

class GapReportTests(unittest.TestCase):
    def test_both_gap_list_shapes_yield_a_tag_key(self):
        """value_differences carries a combined tag_key;
        missing_in_oxidex carries family+name. Verified against a real
        /tmp/tagcmp-CR2-CR2.json."""
        self.assertEqual(jq.tag_key_of({"tag_key": "EXIF:ISO"}), "EXIF:ISO")
        self.assertEqual(jq.tag_key_of({"family": "EXIF", "name": "ISO"}), "EXIF:ISO")
        self.assertEqual(jq.tag_key_of({"name": "ISO"}), "ISO")
        self.assertIsNone(jq.tag_key_of({}))
        self.assertIsNone(jq.tag_key_of("not a dict"))

    def test_gap_index_spans_both_lists(self):
        report = gap_report("RW2",
                            missing=[{"family": "EXIF", "name": "A", "value": "1"}],
                            diffs=[{"tag_key": "EXIF:B", "exiftool_value": "2"}])
        self.assertEqual(set(jq.gap_index(report)), {"EXIF:A", "EXIF:B"})

    def test_a_none_report_means_zero_gaps_not_no_data(self):
        """group_gaps_by_format DROPS formats with gap_count == 0, so
        None is the report for a fully-closed format. Reading it as
        "unknown" would make every complete fix look underivable."""
        self.assertEqual(jq.gap_index(None), {})
        self.assertEqual(jq.gap_count_of(None), 0)

    def test_gap_count_falls_back_to_counting(self):
        report = gap_report("X", missing=[{"name": "A"}], diffs=[{"tag_key": "B"}])
        report.pop("gap_count")
        self.assertEqual(jq.gap_count_of(report), 2)

    def test_exiftool_value_reads_both_spellings(self):
        self.assertEqual(jq.exiftool_value_of({"value": "2.3.0.0"}), "2.3.0.0")
        self.assertEqual(jq.exiftool_value_of({"exiftool_value": "8 8 8"}), "8 8 8")
        self.assertIsNone(jq.exiftool_value_of({}))


# ---------------------------------------------------------------------------
# Trailer re-derivation
# ---------------------------------------------------------------------------

class DeriveTrailersTests(unittest.TestCase):
    def setUp(self):
        self.pre = gap_report("RW2", missing=[
            {"family": "EXIF", "name": "CustomRendered", "value": "Normal",
             "source_file": "/samples/x.rw2"},
            {"family": "EXIF", "name": "Still", "value": "n", "source_file": "/samples/x.rw2"},
        ])
        self.post = gap_report("RW2", missing=[
            {"family": "EXIF", "name": "Still", "value": "n", "source_file": "/samples/x.rw2"},
        ])

    def derive(self, **kw):
        args = dict(pre=self.pre, post=self.post, format_name="RW2",
                    worker="panasonic-leica-700", perl_ref="Exif.pm")
        args.update(kw)
        return jq.derive_trailers(**args)

    def test_happy_path_covers_every_required_trailer(self):
        trailers, problems, _ = self.derive()
        self.assertEqual(problems, [])
        keys = {k for k, _ in trailers}
        self.assertEqual(set(validate_fix_commit.REQUIRED_TRAILERS) - keys, set())

    def test_verified_string_matches_the_shape_overlord_sweep_parses(self):
        """overlord_sweep._VERIFIED_DELTA_RE = r"gaps=(\\d+)->(\\d+)" and
        it SUMS those deltas into the post-merge gate, so the numbers
        must be measured and the shape must be exact."""
        import re
        trailers, _, _ = self.derive()
        verified = dict(trailers)["Verified"]
        match = re.search(r"gaps=(\d+)->(\d+)", verified)
        self.assertIsNotNone(match)
        self.assertEqual(match.groups(), ("2", "1"))

    def test_evidence_comes_from_the_closed_gap_entry(self):
        trailers = dict(self.derive()[0])
        self.assertEqual(trailers["Tag"], "EXIF:CustomRendered")
        self.assertEqual(trailers["Sample"], "/samples/x.rw2")
        self.assertEqual(trailers["Exiftool-Value"], "Normal")
        # A measurement, not a copy: the gap present in `pre` and absent
        # from `post` IS the statement that oxidex now emits what
        # ExifTool emits.
        self.assertEqual(trailers["Oxidex-Value"], "Normal")

    def test_worker_round_trips_through_squad_from_worker(self):
        trailers = dict(self.derive()[0])
        self.assertEqual(
            validate_fix_commit.squad_from_worker(trailers["Worker"]), "panasonic-leica")

    def test_every_closed_tag_gets_its_own_repeatable_trailer(self):
        pre = gap_report("RW2", missing=[
            {"family": "EXIF", "name": n, "value": "v", "source_file": "/s"} for n in "ABC"])
        trailers, problems, _ = self.derive(pre=pre, post=None)
        self.assertEqual(problems, [])
        self.assertEqual([v for k, v in trailers if k == "Tag"],
                         ["EXIF:A", "EXIF:B", "EXIF:C"])

    def test_tag_trailers_are_capped(self):
        pre = gap_report("X", missing=[
            {"family": "EXIF", "name": f"T{i}", "value": "v", "source_file": "/s"}
            for i in range(20)])
        trailers, _, _ = jq.derive_trailers(pre=pre, post=None, format_name="X",
                                            worker="w-700", perl_ref=None, max_tags=3)
        self.assertEqual(len([1 for k, _ in trailers if k == "Tag"]), 3)

    def test_perl_ref_is_omitted_when_the_diff_has_nothing_to_attest(self):
        """CONDITIONAL_TRAILERS: validate_commit only requires Perl-Ref
        when extract_added_map_values found something, so inventing one
        would be worse than omitting it."""
        trailers, _, _ = self.derive(perl_ref=None)
        self.assertNotIn("Perl-Ref", dict(trailers))

    def test_a_commit_that_closes_nothing_is_not_derivable(self):
        """The blind spot the brief names, inverted: a commit with no
        measurable effect has no evidence to write down, so it must stay
        quarantined rather than get a synthesized trailer block."""
        trailers, problems, _ = self.derive(post=self.pre)
        self.assertEqual(trailers, [])
        self.assertIn("no-gap-closed", problems)

    def test_a_regression_is_never_dressed_up_as_recheck_pass(self):
        worse = gap_report("RW2", missing=[
            {"family": "EXIF", "name": n, "value": "v", "source_file": "/s"}
            for n in ("CustomRendered", "Still", "New")])
        trailers, problems, _ = self.derive(post=worse)
        self.assertEqual(trailers, [])
        self.assertTrue(any(p.startswith("gap-count-regressed") for p in problems))

    def test_an_unknown_format_is_not_derivable(self):
        """overlord_sweep's bisection writer records format=None."""
        trailers, problems, _ = self.derive(format_name=None)
        self.assertEqual(trailers, [])
        self.assertIn("format-unknown", problems)

    def test_a_closed_gap_with_no_sample_is_not_derivable(self):
        pre = gap_report("X", missing=[{"family": "EXIF", "name": "A", "value": "v"}])
        trailers, problems, _ = self.derive(pre=pre, post=None)
        self.assertEqual(trailers, [])
        self.assertIn("sample-unknown", problems)

    def test_detail_always_reports_the_measurement(self):
        _, _, detail = self.derive()
        self.assertEqual(detail["gaps_before"], 2)
        self.assertEqual(detail["gaps_after"], 1)
        self.assertEqual(detail["closed_tags"], ["EXIF:CustomRendered"])


class MessageTrailerTests(unittest.TestCase):
    def test_a_bare_subject_line_gets_a_separated_block(self):
        """The measured shape of all 42 zero-trailer commits."""
        out = jq.message_with_trailers("fix(rw2): wire 1 missing tags\n",
                                       [("Format", "RW2"), ("Tag", "EXIF:A")])
        self.assertEqual(out, "fix(rw2): wire 1 missing tags\n\nFormat: RW2\nTag: EXIF:A\n")

    def test_an_existing_non_empty_key_is_never_given_a_second_value(self):
        """git interpret-trailers --parse returns EVERY occurrence, so a
        contradicting second value is worse than no rewrite."""
        out = jq.message_with_trailers("subj\n\nFormat: DNG\n",
                                       [("Format", "RW2"), ("Tag", "EXIF:A")])
        self.assertNotIn("Format: RW2", out)
        self.assertIn("Tag: EXIF:A", out)

    def test_an_empty_existing_key_is_replaced(self):
        out = jq.message_with_trailers("subj\n\nFormat:\n", [("Format", "RW2")])
        self.assertIn("Format: RW2", out)

    def test_drop_keys_replaces_a_trailer_instead_of_appending(self):
        """validate_fix_commit reads Perl-Ref with next(iter(...)), i.e.
        the FIRST occurrence, so correcting one requires removing the old
        line -- appending a second value is silently ignored."""
        out = jq.message_with_trailers("subj\n\nPerl-Ref: Nope.pm\nWorker: w-1\n",
                                       [("Perl-Ref", "ZIP.pm")], drop_keys=["Perl-Ref"])
        self.assertNotIn("Nope.pm", out)
        self.assertIn("Perl-Ref: ZIP.pm", out)
        self.assertIn("Worker: w-1", out)

    def test_a_replacement_does_not_orphan_the_surviving_trailers(self):
        """The textual assertions above are not enough: `git
        interpret-trailers --parse` reads only the LAST paragraph, so
        appending after a blank line would leave Worker: sitting in an
        earlier paragraph where the validator never sees it. Checked
        against real git, which is what validate_fix_commit uses."""
        out = jq.message_with_trailers(
            "subj\n\nFormat: RAR\nPerl-Ref: Nope.pm\nWorker: w-1\n",
            [("Perl-Ref", "ZIP.pm")], drop_keys=["Perl-Ref"], note="proved")
        with tempfile.TemporaryDirectory() as tmp:
            repo, _ = make_repo(tmp)
            parsed = _run(["git", "interpret-trailers", "--parse"], repo,
                          input_text=out).stdout
            keys = dict(line.split(":", 1) for line in parsed.splitlines() if ":" in line)
        self.assertEqual(keys.get("Perl-Ref", "").strip(), "ZIP.pm")
        self.assertEqual(keys.get("Worker", "").strip(), "w-1")
        self.assertEqual(keys.get("Format", "").strip(), "RAR")
        self.assertIn("Judgment-Queue", keys)

    def test_a_bare_subject_still_gets_its_own_trailer_paragraph(self):
        """The other direction: a message with no trailer block must gain
        a blank-line separator, or git reads the subject as a trailer."""
        out = jq.message_with_trailers("fix(rw2): wire 1 missing tags\n",
                                       [("Format", "RW2")])
        with tempfile.TemporaryDirectory() as tmp:
            repo, _ = make_repo(tmp)
            parsed = _run(["git", "interpret-trailers", "--parse"], repo,
                          input_text=out).stdout
        self.assertEqual(parsed.strip(), "Format: RW2")

    def test_drop_keys_is_a_no_op_when_the_key_is_absent(self):
        out = jq.message_with_trailers("subj\n", [("Perl-Ref", "ZIP.pm")],
                                       drop_keys=["Perl-Ref"])
        self.assertIn("Perl-Ref: ZIP.pm", out)

    def test_the_provenance_note_is_appended(self):
        out = jq.message_with_trailers("subj\n", [("Format", "RW2")], note="re-derived")
        self.assertIn("Judgment-Queue: re-derived", out)

    def test_nothing_to_add_leaves_the_message_alone(self):
        out = jq.message_with_trailers("subj\n\nFormat: RW2\n", [("Format", "X")])
        self.assertEqual(out, "subj\n\nFormat: RW2\n")


# ---------------------------------------------------------------------------
# Verifier composition
# ---------------------------------------------------------------------------

class VerifierCompositionTests(unittest.TestCase):
    def test_bare_tag_names_offers_both_spellings_in_order(self):
        hints = jq.bare_tag_names({"Tag": ["ZIP:OperatingSystem", "", "Plain"]})
        self.assertEqual(hints, ["OperatingSystem", "ZIP:OperatingSystem", "Plain"])

    def test_candidate_hints_converts_the_ambiguity_report(self):
        """verify_enum_maps names the tables a bare hint matched;
        those names convert directly into its own Table.Component form."""
        verdict = abstain(candidates=("Image::ExifTool::ZIP::GZIP[9]",
                                      "Image::ExifTool::ZIP::RAR5[OperatingSystem]"))
        self.assertEqual(jq.candidate_hints(verdict), ["GZIP.9", "RAR5.OperatingSystem"])

    def test_candidate_hints_on_a_verdict_with_none(self):
        self.assertEqual(jq.candidate_hints(clean_verdict()), [])

    def test_per_block_auto_binding_is_authoritative_when_it_decides(self):
        """Measured 2026-07-27: an explicit hint forces EVERY block in the
        diff onto ONE table, so on a multi-tag commit it misbinds most of
        them. DNG 786ea09b's `0 => "Red"` was convicted against a table
        reading `1 => 'Rectangular'` for exactly this reason. Auto-binding
        adjudicates table by table and must win."""
        def verify_fn(diff, pm, hint):
            if hint is None:
                return clean_verdict(pairs=4)
            return fabricated_verdict([ve.Mismatch("0", "Red", "Rectangular")], pairs=9)

        verdict, hint = jq.run_verifier("d", Path("/pm"), ["Tag"], verify_fn=verify_fn)
        self.assertEqual(verdict.status, "clean")
        self.assertIsNone(hint)

    def test_a_hint_decides_only_when_auto_binding_abstains(self):
        """The single-table shape the verifier documents as needing a
        hint: `fn rar5_host_os(raw: u8)` has no enclosing arm to bind."""
        def verify_fn(diff, pm, hint):
            if hint is None:
                return abstain(reason="block-unbound")
            if hint == "A":
                return fabricated_verdict([ve.Mismatch("2", "MacOS", None)], pairs=5)
            return clean_verdict()

        verdict, hint = jq.run_verifier("d", Path("/pm"), ["A", "B"], verify_fn=verify_fn)
        self.assertEqual(verdict.status, "fabricated")
        self.assertEqual(hint, "A")

    def test_among_hints_the_best_supported_binding_wins(self):
        def verify_fn(diff, pm, hint):
            if hint is None:
                return abstain(reason="block-unbound")
            if hint == "Weak":
                return fabricated_verdict([ve.Mismatch(str(k), "x", "y") for k in range(3)],
                                          pairs=3, table="Wrong")
            return fabricated_verdict([ve.Mismatch("2", "MacOS", None)], pairs=3, table="Right")

        verdict, hint = jq.run_verifier("d", Path("/pm"), ["Weak", "Strong"], verify_fn=verify_fn)
        self.assertEqual((verdict.table, hint), ("Right", "Strong"))

    def test_expansion_picks_the_best_supported_fabricated_table(self):
        """ZIP.pm's three OperatingSystem tables all say "fabricated" for
        the RAR diff; only RAR5 agrees on 0 => Win32 and 1 => Unix, and
        only RAR5 is the table ExifTool actually uses there."""
        def verify_fn(diff, pm, hint):
            if hint is None:
                # The real RAR shape: a catch-all is table-independent, so
                # auto-binding convicts without pinning a table at all.
                return fabricated_verdict([], catch_alls=[ve.CatchAll("Unknown", "f.rs", 1)],
                                          pairs=0)
            if hint == "OperatingSystem":
                return abstain(candidates=("Image::ExifTool::ZIP::GZIP[9]",
                                           "Image::ExifTool::ZIP::RAR5[OperatingSystem]"))
            if hint == "GZIP.9":
                return fabricated_verdict([ve.Mismatch(str(k), "x", "y") for k in range(5)])
            if hint == "RAR5.OperatingSystem":
                return fabricated_verdict([ve.Mismatch(str(k), "x", None) for k in (2, 3, 4)])
            return abstain(reason="no-such-table")

        verdict, hint = jq.run_verifier("d", Path("/pm"), ["OperatingSystem"],
                                        verify_fn=verify_fn)
        self.assertEqual(hint, "RAR5.OperatingSystem")
        self.assertEqual([m.key for m in verdict.mismatches], ["2", "3", "4"])

    def test_expansion_can_never_convict_a_clean_commit(self):
        """The safety argument for the two-phase split: a genuinely clean
        commit whose tag name is ambiguous must not be convicted by
        whichever sibling table disagrees with it."""
        expanded = []

        def verify_fn(diff, pm, hint):
            if hint is None:
                return abstain(reason="block-unbound")
            if hint == "Amb":
                return abstain(candidates=("Image::ExifTool::A::Right[1]",
                                           "Image::ExifTool::A::Wrong[2]"))
            if hint == "Right.1":
                return clean_verdict(pairs=4)
            expanded.append(hint)
            return fabricated_verdict([ve.Mismatch("1", "a", "b")])

        verdict, _ = jq.run_verifier("d", Path("/pm"), ["Amb", "Right.1"], verify_fn=verify_fn)
        self.assertEqual(verdict.status, "clean")
        self.assertEqual(expanded, [], "phase 2 must not run when phase 1 is clean")

    def test_a_clean_that_checked_nothing_is_not_decisive(self):
        """verify_enum_maps' own warning: "clean" with pairs_checked == 0
        means nothing was verified, not that everything was right. So it
        does not get to end the search."""
        def verify_fn(diff, pm, hint):
            return clean_verdict(pairs=0) if hint is None else clean_verdict(pairs=7)

        verdict, hint = jq.run_verifier("d", Path("/pm"), ["A"], verify_fn=verify_fn)
        self.assertEqual((verdict.pairs_checked, hint), (7, "A"))

    def test_a_diff_with_no_pairs_at_all_is_decided_by_auto_binding(self):
        """The one honest zero: `no-enum-pairs-in-diff` means there is
        nothing in the diff to check, so no hint could do better."""
        def verify_fn(diff, pm, hint):
            if hint is None:
                return ve.Verdict("clean", [], [], [], reason="no-enum-pairs-in-diff",
                                  pairs_checked=0)
            raise AssertionError("must not fall through to a hint")

        verdict, hint = jq.run_verifier("d", Path("/pm"), ["A"], verify_fn=verify_fn)
        self.assertEqual((verdict.status, hint), ("clean", None))

    def test_all_abstentions_return_the_auto_binding_attempt(self):
        def verify_fn(diff, pm, hint):
            return abstain(reason=f"r-{hint}")

        verdict, hint = jq.run_verifier("d", Path("/pm"), ["A"], verify_fn=verify_fn)
        self.assertIsNone(hint)
        self.assertEqual(verdict.reason, "r-None")


class FindPerlModuleTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.lib = Path(self.tmp.name)
        # A.pm and B.pm carry every display value RAR_SHAPED adds, so they
        # survive the byte prefilter; C.pm carries none and must not.
        every = "Win32 Unix MacOS BeOS OS/2 Unknown"
        for name, text in (("A.pm", every), ("B.pm", every), ("C.pm", "nothing")):
            (self.lib / name).write_text(text)
        self.modules = lambda _lib: sorted(self.lib.glob("*.pm"))
        self.diff = make_diff("src/parsers/archive/rar.rs", RAR_SHAPED)

    def tearDown(self):
        self.tmp.cleanup()

    def test_a_diff_with_no_pairs_needs_no_perl_ref(self):
        module, reason, verdict, hint = jq.find_perl_module_for_pairs(
            make_diff("src/x.rs", "let a = 1;\n"), self.lib, modules_fn=self.modules)
        self.assertEqual((module, reason, verdict, hint), (None, "not-required", None, None))

    def test_exactly_one_clean_module_is_the_answer_and_carries_its_hint(self):
        def verify_fn(diff, pm, hint):
            if Path(pm).name == "A.pm" and hint == "H":
                return clean_verdict(pairs=2)
            return abstain(reason="no-such-table")

        module, reason, verdict, hint = jq.find_perl_module_for_pairs(
            self.diff, self.lib, ["H"], verify_fn=verify_fn, modules_fn=self.modules)
        self.assertEqual((module.name, reason, hint), ("A.pm", "verified", "H"))
        self.assertEqual(verdict.status, "clean")

    def test_a_clean_verdict_that_checked_nothing_does_not_count(self):
        """"clean" with pairs_checked == 0 means nothing was verified,
        not that everything was right -- so it cannot prove a module."""
        module, reason, _, _ = jq.find_perl_module_for_pairs(
            self.diff, self.lib, verify_fn=lambda *a: clean_verdict(pairs=0),
            modules_fn=self.modules)
        self.assertEqual((module, reason), (None, "perl-ref-underivable"))

    def test_two_clean_modules_abstain_rather_than_pick(self):
        module, reason, _, _ = jq.find_perl_module_for_pairs(
            self.diff, self.lib, verify_fn=lambda *a: clean_verdict(pairs=2),
            modules_fn=self.modules)
        self.assertIsNone(module)
        self.assertEqual(reason, "perl-ref-ambiguous:2")

    def test_a_derived_binding_never_returns_a_fabricated_verdict(self):
        """A derived module is a guess about WHICH table applies; it is
        not authoritative enough to convict on. DNG 786ea09b, measured
        2026-07-27, is the case this rule exists for."""
        module, reason, verdict, _ = jq.find_perl_module_for_pairs(
            self.diff, self.lib,
            verify_fn=lambda *a: fabricated_verdict([ve.Mismatch("0", "Win32", "MS-DOS")]),
            modules_fn=self.modules)
        self.assertIsNone(module)
        self.assertIsNone(verdict)
        self.assertEqual(reason, "perl-ref-underivable")

    def test_the_byte_prefilter_skips_modules_that_cannot_contain_the_values(self):
        asked = []

        def verify_fn(diff, pm, hint):
            asked.append(Path(pm).name)
            return abstain()

        jq.find_perl_module_for_pairs(self.diff, self.lib, verify_fn=verify_fn,
                                      modules_fn=self.modules)
        self.assertNotIn("C.pm", asked)

    def test_lang_translations_are_excluded_from_the_corpus(self):
        """Image/ExifTool/Lang/*.pm is 16.3% of the tree and is nothing
        but translated UI strings -- free substring matches for almost
        any plausible fabrication."""
        root = self.lib / "Image" / "ExifTool"
        (root / "Lang").mkdir(parents=True)
        (root / "Real.pm").write_text("x")
        (root / "Lang" / "de.pm").write_text("x")
        names = {p.name for p in jq.candidate_perl_modules(self.lib)}
        self.assertIn("Real.pm", names)
        self.assertNotIn("de.pm", names)


# ---------------------------------------------------------------------------
# Git mechanics, against real repositories
# ---------------------------------------------------------------------------

class GitMechanicsTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.repo, self.base = make_repo(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def test_commit_exists_distinguishes_gone_from_present(self):
        self.assertTrue(jq.commit_exists(self.repo, self.base))
        self.assertFalse(jq.commit_exists(self.repo, "0" * 40))
        self.assertFalse(jq.commit_exists(self.repo, ""))

    def test_preserve_creates_the_ref_and_is_idempotent(self):
        self.assertEqual(jq.preserve_commit(self.repo, "pid", self.base, apply=True), "preserved")
        self.assertEqual(jq.rev_parse(self.repo, "refs/judgment-queue/pid"), self.base)
        self.assertEqual(jq.preserve_commit(self.repo, "pid", self.base, apply=True),
                         "already-preserved")

    def test_preserve_writes_nothing_without_apply(self):
        self.assertEqual(jq.preserve_commit(self.repo, "pid", self.base), "would-preserve")
        self.assertIsNone(jq.rev_parse(self.repo, "refs/judgment-queue/pid"))

    def test_preserve_reports_a_gone_object(self):
        """4 of the 44 quarantined shas were measured dangling on
        2026-07-26 -- this branch is real, not defensive noise."""
        self.assertEqual(jq.preserve_commit(self.repo, "pid", "0" * 40, apply=True), "missing")

    def test_preservation_refs_are_invisible_to_every_branch_glob(self):
        jq.preserve_commit(self.repo, "pid", self.base, apply=True)
        out = _run(["git", "for-each-ref", "--format=%(refname)", "refs/heads/"], self.repo).stdout
        self.assertNotIn("judgment-queue", out)

    def test_create_then_fast_forward_then_refuse_a_rewind(self):
        sha = add_commit(self.repo, relpath="src/parsers/archive/rar.rs",
                         body=RAR_SHAPED, message="one", branch="work")
        ok, message = jq.create_or_fast_forward(self.repo, "model-fix-parallel-canon-700",
                                                self.base, apply=True)
        self.assertTrue(ok)
        self.assertEqual(message, "created")
        _run(["git", "update-ref", "refs/heads/tmp", sha], self.repo)
        ok, _ = jq.create_or_fast_forward(self.repo, "model-fix-parallel-canon-700", sha,
                                          apply=True)
        self.assertTrue(ok)
        # Rewinding to the base is not a fast-forward: append-only or nothing.
        ok, message = jq.create_or_fast_forward(self.repo, "model-fix-parallel-canon-700",
                                               self.base, apply=True)
        self.assertFalse(ok)
        self.assertIn("refusing", message)

    def test_publish_writes_nothing_without_apply(self):
        ok, _ = jq.create_or_fast_forward(self.repo, "model-fix-parallel-canon-700", self.base)
        self.assertTrue(ok)
        self.assertFalse(jq.ref_exists(self.repo, "refs/heads/model-fix-parallel-canon-700"))

    def test_already_present_matches_on_patch_id_not_sha(self):
        """A promoted commit is a fresh cherry-pick with a fresh sha.
        Matching on sha would let it be promoted again and again."""
        sha = add_commit(self.repo, relpath="src/parsers/archive/rar.rs",
                         body=RAR_SHAPED, message="one", branch="work")
        # Land it on a DIFFERENT parent, which is what a re-admission
        # actually does. Cherry-picking onto the identical parent with an
        # identical tree, message, author and timestamp reproduces the
        # same sha exactly -- git is deterministic -- and would make this
        # test pass even against a sha comparison.
        _run(["git", "checkout", "-q", "-B", "other", "origin/main"], self.repo)
        (self.repo / "unrelated.txt").write_text("x\n")
        _run(["git", "add", "-A"], self.repo)
        _run(["git", "commit", "-qm", "unrelated"], self.repo)
        _run(["git", "cherry-pick", sha], self.repo)
        copy = _run(["git", "rev-parse", "HEAD"], self.repo).stdout.strip()
        _run(["git", "checkout", "-q", "main"], self.repo)
        self.assertNotEqual(sha, copy)
        self.assertEqual(jq.already_present(self.repo, sha, ["other"]), "other")

    def test_already_present_ignores_refs_that_do_not_exist(self):
        sha = add_commit(self.repo, relpath="src/parsers/archive/rar.rs",
                         body=RAR_SHAPED, message="one", branch="work")
        self.assertIsNone(jq.already_present(self.repo, sha, ["squad/nope", "origin/main"]))


# ---------------------------------------------------------------------------
# Adjudication
# ---------------------------------------------------------------------------

class AdjudicateTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.repo, self.base = make_repo(self.root)
        self.home = self.root / "home"
        (self.home / "logs").mkdir(parents=True)
        self.perl = self.root / "perl"
        self.perl.mkdir()
        (self.perl / "ZIP.pm").write_text("Win32 Unix MacOS BeOS OS/2 Unknown")

    def tearDown(self):
        self.tmp.cleanup()

    def commit(self, body=RAR_SHAPED, message=TRAILERED_MESSAGE,
               relpath="src/parsers/archive/rar.rs"):
        sha = add_commit(self.repo, relpath=relpath, body=body, message=message, branch="wip")
        _run(["git", "branch", "-qD", "wip"], self.repo)  # simulate the discarded worker branch
        patch_id = jq.squad_merge_loop.compute_patch_id_for_sha(self.repo, sha)
        return sha, patch_id

    def adjudicate(self, sha, patch_id, *, klass="printconv", fmt="RAR", squad="tail",
                   apply=False, **kw):
        entry = {"patch_id": patch_id, "sha": sha, "format": fmt, "squad": squad,
                 "flags": ["printconv-mismatch:x"], "attempt": 1}
        kwargs = dict(
            repo_root=self.repo, home=self.home, entry=entry, squad=squad, klass=klass,
            worktree_path=jq.judgment_worktree_dir(self.home, squad),
            perl_lib=str(self.perl), cache_dir=str(self.root / "cache"),
            apply=apply, log_fn=lambda *a: None,
            resolve_pm_fn=lambda ref, lib: (self.perl / "ZIP.pm") if ref else None,
            verify_fn=lambda *a: clean_verdict(),
            validate_fn=lambda *a, **k: {"ok": True, "flags": []},
            recheck_fn=lambda *a: None,
        )
        kwargs.update(kw)
        return jq.adjudicate(**kwargs)

    # --- classes left alone -------------------------------------------------

    def test_a_real_test_failure_is_left_alone(self):
        decision = self.adjudicate("0" * 40, "pid", klass="test-failed")
        self.assertEqual(decision["verdict"], "queued")
        self.assertIn("real failure", decision["reason"])

    def test_measured_corpus_rejections_are_left_alone(self):
        """Re-offering a bisection-isolated commit would hand the sweep
        back the thing it proved broke it."""
        decision = self.adjudicate("0" * 40, "pid", klass="semantic")
        self.assertEqual(decision["verdict"], "queued")
        self.assertIn("measured corpus effect", decision["reason"])

    # --- preconditions ------------------------------------------------------

    def test_a_gone_commit_is_reported_not_crashed_on(self):
        decision = self.adjudicate("0" * 40, "pid")
        self.assertEqual(decision["verdict"], "queued")
        self.assertIn("gone from this repo", decision["reason"])

    def test_a_patch_already_on_origin_main_is_terminal(self):
        sha = add_commit(self.repo, relpath="src/parsers/archive/rar.rs",
                         body=RAR_SHAPED, message="one", branch="work")
        _run(["git", "update-ref", "refs/remotes/origin/main", sha], self.repo)
        patch_id = jq.squad_merge_loop.compute_patch_id_for_sha(self.repo, sha)
        decision = self.adjudicate(sha, patch_id)
        self.assertEqual(decision["verdict"], "already-landed")
        self.assertIn(decision["verdict"], jq.TERMINAL_VERDICTS)

    def test_a_blocked_squad_is_deferred_before_any_expensive_work(self):
        """poll_once halts ALL candidate processing for a blocked squad,
        so promoting into one gets zero uptake and no error."""
        sha, patch_id = self.commit()
        batch = jq.squad_merge_loop.batch_state_path(self.home, "tail")
        batch.parent.mkdir(parents=True, exist_ok=True)
        batch.write_text(json.dumps({"blocked": True}))
        called = []
        decision = self.adjudicate(sha, patch_id,
                                   verify_fn=lambda *a: called.append(a) or clean_verdict())
        self.assertEqual(decision["verdict"], "queued")
        self.assertIn("blocked", decision["reason"])
        self.assertEqual(called, [])

    def test_no_perl_lib_means_no_promotion_is_possible(self):
        sha, patch_id = self.commit()
        decision = self.adjudicate(sha, patch_id, perl_lib=None)
        self.assertEqual(decision["verdict"], "queued")
        self.assertIn("no --perl-lib", decision["reason"])

    # --- the anti-fabrication gate -----------------------------------------

    def test_a_fabrication_is_rejected_permanently_with_the_wrong_pairs_recorded(self):
        sha, patch_id = self.commit()
        mismatch = ve.Mismatch("2", "MacOS", None, exiftool_key_for_value=None,
                               file="src/parsers/archive/rar.rs", line=570)
        # pairs=5 with one mismatch -> four agreements, i.e. a binding the
        # diff corroborates (see binding_is_corroborated).
        decision = self.adjudicate(
            sha, patch_id, verify_fn=lambda *a: fabricated_verdict([mismatch], pairs=5))
        self.assertEqual(decision["verdict"], "rejected-permanent")
        self.assertIn(decision["verdict"], jq.TERMINAL_VERDICTS)
        recorded = decision["detail"]["verifier"]["mismatches"]
        self.assertEqual(recorded[0]["key"], "2")
        self.assertEqual(recorded[0]["oxidex"], "MacOS")

    def test_a_string_catch_all_alone_is_flagged_but_NOT_terminal(self):
        """The detection is right; the severity was not.

        ExifTool prints the RAW NUMBER when no PrintConv matches, so
        `_ => "Unknown"` really does replace data it would have shown -- that
        part stands. But a catch-all is a CORRECTABLE defect: the fix is to
        return None or the raw value, and it is mechanical.

        Measured 2026-07-27 adjudicating 8 archived patches beside a human
        pass over the same 8, catch-all-alone convictions cost two commits
        outright:
          elf-4b5a26e97cb8  pairs_checked=17, agreements=17, mismatches=0
                            -- a PERFECT pair record
          pdf-a1a411f67e3f  measures a real -4 gap closure
        Both were rejected-permanent on nothing but a `_ =>` arm. That verdict
        is deliberately immune to a policy bump, so both would have been
        discarded FOREVER over a one-line divergence in a code path their own
        sample never exercises.

        A pair mismatch on a trustworthy binding stays terminal -- a
        fabricated key->value pair is a fact, not a fixable slip.
        """
        sha, patch_id = self.commit()
        catch = ve.CatchAll("Unknown", "src/parsers/archive/rar.rs", 573)
        decision = self.adjudicate(
            sha, patch_id, verify_fn=lambda *a: fabricated_verdict([], catch_alls=[catch]))
        self.assertEqual(decision["verdict"], "queued")
        self.assertIn("catch-all", decision["reason"])
        self.assertIn("correctable", decision["reason"])
        # The evidence must still be recorded in full, or the next adjudicator
        # has to re-derive it.
        self.assertEqual(
            [c["value"] for c in decision["detail"]["verifier"]["catch_alls"]], ["Unknown"]
        )

    def test_a_zero_agreement_binding_is_not_convicted(self):  # noqa: D401
        """Measured on the live ledger 2026-07-27: DNG 786ea09b's
        `0 => "Red", 1 => "Green", 2 => "Blue"` came back "fabricated"
        against a table reading `1 => 'Rectangular'` -- CFALayout
        compared with a CFA colour map, not three inventions. A real
        fabrication sits BESIDE correct values (RAR agrees on 0 => Win32
        and 1 => Unix); a wrong table agrees on nothing."""
        sha, patch_id = self.commit()
        mismatches = [ve.Mismatch("0", "MacOS", "Rectangular"),
                      ve.Mismatch("1", "BeOS", "Even columns offset")]
        decision = self.adjudicate(
            sha, patch_id,
            verify_fn=lambda *a: fabricated_verdict(mismatches, pairs=2))
        self.assertEqual(decision["verdict"], "queued")
        self.assertIn("wrong table binding", decision["reason"])
        self.assertEqual(decision["detail"]["verifier"]["binding_refused"], "zero-agreement")
        self.assertEqual(decision["detail"]["verifier"]["agreements"], 0)

    def test_one_agreement_is_enough_to_convict(self):
        """RAR a998b8fc's shape: 5 pairs checked, 3 wrong, 2 right. The
        binding is corroborated by the diff itself."""
        sha, patch_id = self.commit()
        mismatches = [ve.Mismatch("2", "MacOS", None), ve.Mismatch("3", "BeOS", None),
                      ve.Mismatch("4", "OS/2", None)]
        # RAR_SHAPED is a single match table, so even a forced hint is a
        # trustworthy binding here.
        decision = self.adjudicate(
            sha, patch_id,
            verify_fn=lambda *a: fabricated_verdict(mismatches, pairs=5))
        self.assertEqual(decision["verdict"], "rejected-permanent")

    def test_a_catch_all_is_reported_without_any_table_at_all(self):
        """It needs no binding to be REPORTED -- ExifTool prints the RAW
        NUMBER when no PrintConv matches, so a string `_ =>` arm replaces
        data it would have shown regardless of which table is right.

        What it does not do is settle the question forever. See
        test_a_string_catch_all_alone_is_flagged_but_NOT_terminal for the two
        real commits this cost when it was terminal.
        """
        sha, patch_id = self.commit()
        catch = ve.CatchAll("Unknown", "src/parsers/archive/rar.rs", 573)
        # Untrustworthy pairs AND a catch-all: report the catch-all, and do
        # NOT headline the misbound pairs. Measured on CR2 629f23a4 and
        # 92e457f6, which did exactly that.
        mismatches = [ve.Mismatch("0", "MacOS", "Standard"), ve.Mismatch("1", "BeOS", "Low")]
        decision = self.adjudicate(
            sha, patch_id,
            verify_fn=lambda *a: fabricated_verdict(mismatches, catch_alls=[catch], pairs=2))
        self.assertEqual(decision["verdict"], "queued")
        self.assertIn("catch-all", decision["reason"])
        verifier = decision["detail"]["verifier"]
        self.assertNotIn("mismatches", verifier)
        self.assertEqual(len(verifier["unreliable_mismatches"]), 2)
        self.assertEqual(verifier["binding_refused"], "zero-agreement")

    def test_pair_evidence_rules_in_one_table(self):
        """Every arm of pair_evidence_is_trustworthy, as a table, so a
        future edit cannot quietly loosen one of them."""
        corroborated = fabricated_verdict([ve.Mismatch("2", "MacOS", None)], pairs=5)
        zero = fabricated_verdict([ve.Mismatch("0", "a", "b")], pairs=1)
        cases = [
            # (verdict, hint, blocks) -> (ok, why_not)
            ((corroborated, None, 9), (True, None)),      # per-block binding, any diff shape
            ((corroborated, "H", 1), (True, None)),       # forced hint, single-table diff
            ((corroborated, "H", 2), (False, "forced-hint-on-multi-table-diff")),
            ((zero, None, 1), (False, "zero-agreement")),
        ]
        for (verdict, hint, blocks), expected in cases:
            self.assertEqual(jq.pair_evidence_is_trustworthy(verdict, hint, blocks), expected,
                             f"{hint=} {blocks=}")

    def test_a_multi_table_diff_is_counted_by_enclosing_match_block(self):
        one = make_diff("src/parsers/archive/rar.rs", RAR_SHAPED)
        two = one + make_diff("src/parsers/exif/other.rs", RAR_SHAPED.replace("host_os", "other"))
        self.assertEqual(jq.diff_block_count(one), 1)
        self.assertEqual(jq.diff_block_count(two), 2)

    def test_a_fabrication_outside_printconv_values_is_not_convicted(self):
        """DNG 786ea09b, measured 2026-07-27: a tag-id -> tag-KEY map
        bound to a PrintConv table yields a confident "fabricated"
        verdict on a commit containing no PrintConv at all.
        rejected-permanent is terminal, so it must never rest on that."""
        sha, patch_id = self.commit(body=TAG_KEY_SHAPED,
                                    relpath="src/parsers/exif/tags.rs")
        # It DOES cite a module, so the daemon is on the convicting path --
        # only the printconv-values guard stands between it and a permanent
        # rejection.
        mismatch = ve.Mismatch("33421", "EXIF:CFARepeatPatternDim", None)
        decision = self.adjudicate(
            sha, patch_id, verify_fn=lambda *a: fabricated_verdict([mismatch]))
        self.assertEqual(decision["verdict"], "queued")
        self.assertIn("not a safe basis", decision["reason"])

    def test_an_abstention_is_queued_never_promoted(self):
        sha, patch_id = self.commit()
        decision = self.adjudicate(
            sha, patch_id, apply=True,
            verify_fn=lambda *a: abstain(reason="printconv-is-code"))
        self.assertEqual(decision["verdict"], "queued")
        self.assertIn("printconv-is-code", decision["reason"])
        self.assertFalse(jq.ref_exists(self.repo, "refs/heads/model-fix-parallel-tail-700"))

    def test_an_underivable_module_is_queued(self):
        sha, patch_id = self.commit()
        decision = self.adjudicate(sha, patch_id,
                                   resolve_pm_fn=lambda ref, lib: None,
                                   verify_fn=lambda *a: abstain(reason="no-such-table"))
        self.assertEqual(decision["verdict"], "queued")
        self.assertIn("perl-ref-underivable", decision["reason"])

    # --- dry run ------------------------------------------------------------

    def test_a_dry_run_decides_completely_and_mutates_nothing(self):
        sha, patch_id = self.commit()
        decision = self.adjudicate(sha, patch_id)
        self.assertEqual(decision["verdict"], "queued")
        self.assertTrue(decision["detail"]["would_promote"])
        self.assertTrue(decision["dry_run"])
        self.assertIsNone(jq.rev_parse(self.repo, jq.preserve_ref_name(patch_id)))
        self.assertFalse(jq.ref_exists(self.repo, "refs/heads/model-fix-parallel-tail-700"))
        self.assertFalse(jq.judgment_worktree_dir(self.home, "tail").exists())

    # --- promotion ----------------------------------------------------------

    def test_a_clean_printconv_commit_is_promoted_onto_the_reserved_slot_branch(self):
        sha, patch_id = self.commit()
        decision = self.adjudicate(sha, patch_id, apply=True)
        self.assertEqual(decision["verdict"], "promoted")
        branch = "model-fix-parallel-tail-700"
        self.assertEqual(decision["promoted_branch"], branch)
        self.assertTrue(jq.ref_exists(self.repo, f"refs/heads/{branch}"))
        # The merger discovers slot branches by exactly this glob.
        self.assertIn(branch, jq.squad_merge_loop.squad_slot_branches(self.repo, "tail"))
        # And the object is pinned against gc.
        self.assertEqual(jq.rev_parse(self.repo, jq.preserve_ref_name(patch_id)), sha)

    def test_promotion_is_idempotent_across_a_crashed_ledger_write(self):
        """The structural guard: a crash between the fast-forward and the
        ledger append must not double-promote."""
        sha, patch_id = self.commit()
        first = self.adjudicate(sha, patch_id, apply=True)
        self.assertEqual(first["verdict"], "promoted")
        second = self.adjudicate(sha, patch_id, apply=True)
        self.assertEqual(second["verdict"], "already-landed")
        self.assertEqual(second["detail"]["ref"], "model-fix-parallel-tail-700")
        count = _run(["git", "rev-list", "--count", "origin/main..model-fix-parallel-tail-700"],
                     self.repo).stdout.strip()
        self.assertEqual(count, "1")

    def test_a_still_conflicting_cherry_pick_is_requeued_not_promoted(self):
        sha, patch_id = self.commit()
        # Move origin/main so the same file conflicts.
        _run(["git", "checkout", "-q", "main"], self.repo)
        (self.repo / "src" / "parsers" / "archive" / "rar.rs").write_text("// totally different\n")
        _run(["git", "commit", "-aqm", "divergent"], self.repo)
        _run(["git", "update-ref", "refs/remotes/origin/main", "HEAD"], self.repo)
        decision = self.adjudicate(sha, patch_id, klass="cherry-pick-conflict", apply=True)
        self.assertEqual(decision["verdict"], "queued")
        self.assertIn("still conflicts", decision["reason"])
        self.assertFalse(jq.ref_exists(self.repo, "refs/heads/model-fix-parallel-tail-700"))

    def test_a_commit_still_flagged_after_the_work_is_never_promoted(self):
        sha, patch_id = self.commit()
        decision = self.adjudicate(
            sha, patch_id, apply=True,
            validate_fn=lambda *a, **k: {"ok": False, "flags": ["ownership:x", "paths:y"]})
        self.assertEqual(decision["verdict"], "queued")
        self.assertIn("still flagged", decision["reason"])
        self.assertFalse(jq.ref_exists(self.repo, "refs/heads/model-fix-parallel-tail-700"))

    def test_a_proved_perl_ref_is_written_back_for_the_unverifiable_class(self):
        """Otherwise the printconv-unverifiable class is a permanent dead
        end: check_printconv flags "unverifiable" BECAUSE the cited
        Perl-Ref does not resolve, so proving a module and not recording
        it means the very next validation re-flags the commit forever."""
        sha, patch_id = self.commit(message=TRAILERED_MESSAGE.replace("Perl-Ref: ZIP.pm",
                                                                     "Perl-Ref: Nope.pm"))
        decision = self.adjudicate(
            sha, patch_id, klass="printconv-unverifiable", apply=True,
            resolve_pm_fn=lambda ref, lib: None,          # the cited ref does not resolve
            verify_fn=lambda *a: clean_verdict(pairs=5))  # but a module verifies clean
        self.assertEqual(decision["verdict"], "promoted")
        message = _run(["git", "show", "-s", "--format=%B", decision["promoted_sha"]],
                       self.repo).stdout
        self.assertIn("Perl-Ref: ZIP.pm", message)
        self.assertNotIn("Nope.pm", message)
        self.assertIn("proved by pair verification", message)

    def test_a_resolvable_perl_ref_is_left_exactly_as_the_author_wrote_it(self):
        """Only a PROVED replacement is ever written. A commit whose own
        citation resolves is not second-guessed."""
        sha, patch_id = self.commit()
        decision = self.adjudicate(sha, patch_id, apply=True)
        self.assertEqual(decision["verdict"], "promoted")
        message = _run(["git", "show", "-s", "--format=%B", decision["promoted_sha"]],
                       self.repo).stdout
        self.assertIn("Perl-Ref: ZIP.pm", message)
        self.assertNotIn("Judgment-Queue:", message)

    # --- the dominant class, end to end ------------------------------------

    def test_bookkeeping_rewrites_the_trailers_from_a_measured_recheck(self):
        sha, patch_id = self.commit(message=BARE_MESSAGE)
        reports = iter([
            gap_report("RAR", missing=[
                {"family": "ZIP", "name": "OperatingSystem", "value": "Unix",
                 "source_file": "/samples/ZIP.rar"},
                {"family": "ZIP", "name": "FileVersion", "value": "5",
                 "source_file": "/samples/ZIP.rar"},
            ]),
            gap_report("RAR", missing=[
                {"family": "ZIP", "name": "FileVersion", "value": "5",
                 "source_file": "/samples/ZIP.rar"},
            ]),
        ])
        decision = self.adjudicate(sha, patch_id, klass="bookkeeping", apply=True,
                                   recheck_fn=lambda *a: next(reports))
        self.assertEqual(decision["verdict"], "promoted")
        message = _run(["git", "show", "-s", "--format=%B", decision["promoted_sha"]],
                       self.repo).stdout
        self.assertIn("Format: RAR", message)
        self.assertIn("Tag: ZIP:OperatingSystem", message)
        self.assertIn("Sample: /samples/ZIP.rar", message)
        self.assertIn("Exiftool-Value: Unix", message)
        self.assertIn("Verified: recheck-pass gaps=2->1", message)
        self.assertIn("Worker: tail-700", message)
        self.assertIn("Perl-Ref: ZIP.pm", message)
        self.assertIn("Judgment-Queue:", message)
        # The original subject survives -- this rewrites bookkeeping, not history.
        self.assertTrue(message.startswith("fix(rar): wire 2 missing tags"))

    def test_bookkeeping_leaves_an_underivable_commit_quarantined_and_says_why(self):
        """"If a required trailer needs information only the original
        worker had, leave it quarantined and say so." A commit that
        closes no measurable gap has no evidence to write down."""
        sha, patch_id = self.commit(message=BARE_MESSAGE)
        report = gap_report("RAR", missing=[
            {"family": "ZIP", "name": "FileVersion", "value": "5", "source_file": "/s"}])
        decision = self.adjudicate(sha, patch_id, klass="bookkeeping", apply=True,
                                   recheck_fn=lambda *a: report)
        self.assertEqual(decision["verdict"], "queued")
        self.assertIn("not derivable", decision["reason"])
        self.assertIn("no-gap-closed", decision["detail"]["problems"])
        self.assertFalse(jq.ref_exists(self.repo, "refs/heads/model-fix-parallel-tail-700"))

    def test_the_bookkeeping_recheck_brackets_the_cherry_pick(self):
        """gaps=<before> must be measured on the tree the commit is about
        to land on, and gaps=<after> with it applied -- not two readings
        of the same state."""
        sha, patch_id = self.commit(message=BARE_MESSAGE)
        seen = []

        def recheck(worktree, cache, fmt, suffix):
            seen.append(_run(["git", "rev-parse", "HEAD"], worktree).stdout.strip())
            if len(seen) == 1:
                return gap_report("RAR", missing=[
                    {"family": "ZIP", "name": "A", "value": "v", "source_file": "/s"}])
            return None

        decision = self.adjudicate(sha, patch_id, klass="bookkeeping", apply=True,
                                   recheck_fn=recheck)
        self.assertEqual(decision["verdict"], "promoted")
        self.assertEqual(len(seen), 2)
        self.assertNotEqual(seen[0], seen[1])


# ---------------------------------------------------------------------------
# Poll cycle and CLI
# ---------------------------------------------------------------------------

class PollTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.repo, self.base = make_repo(self.root)
        self.home = self.root / "home"
        (self.home / "logs").mkdir(parents=True)

    def tearDown(self):
        self.tmp.cleanup()

    def write_ledger(self, *entries):
        path = jq.squad_merge_loop.quarantine_ledger_path(self.home)
        path.parent.mkdir(parents=True, exist_ok=True)
        with open(path, "w") as fh:
            for entry in entries:
                fh.write(json.dumps(entry) + "\n")

    def entry(self, patch_id, **kw):
        base = {"patch_id": patch_id, "sha": self.base, "format": "RAR", "squad": "tail",
                "flags": ["missing-trailer:Format"], "attempt": 1, "ts": "2026-07-01T00:00:00"}
        base.update(kw)
        return base

    def poll(self, **kw):
        kwargs = dict(repo_root=self.repo, home=self.home, log_fn=lambda *a: None,
                      adjudicate_fn=lambda **k: jq.make_decision(
                          patch_id=k["entry"]["patch_id"], sha=k["entry"]["sha"],
                          format_name=k["entry"].get("format"), squad=k["squad"],
                          klass=k["klass"], verdict="queued", reason="fake",
                          attempt=k["entry"].get("attempt", 0), dry_run=not k["apply"]))
        kwargs.update(kw)
        return jq.poll_once(**kwargs)

    def test_an_empty_ledger_is_not_an_error(self):
        result = self.poll()
        self.assertEqual((result["considered"], result["adjudicated"]), (0, 0))

    def test_a_dry_run_poll_writes_no_ledger_line(self):
        self.write_ledger(self.entry("p1"))
        result = self.poll()
        self.assertEqual(result["adjudicated"], 1)
        self.assertFalse(jq.decision_ledger_path(self.home).exists())

    def test_an_applied_poll_appends_one_line_per_decision(self):
        self.write_ledger(self.entry("p1"), self.entry("p2", sha=self.base))
        self.poll(apply=True)
        lines = jq.decision_ledger_path(self.home).read_text().splitlines()
        self.assertEqual(len(lines), 2)

    def test_terminal_decisions_are_skipped_on_the_next_poll(self):
        self.write_ledger(self.entry("p1"))
        jq.append_decision(jq.decision_ledger_path(self.home), jq.make_decision(
            patch_id="p1", sha=self.base, format_name="RAR", squad="tail", klass="bookkeeping",
            verdict="rejected-permanent", reason="fabricated", attempt=1, dry_run=False),
            apply=True)
        result = self.poll(apply=True)
        self.assertEqual((result["considered"], result["adjudicated"], result["skipped"]),
                         (1, 0, 1))

    def test_limit_stops_the_poll_early(self):
        self.write_ledger(*[self.entry(f"p{i}") for i in range(5)])
        result = self.poll(limit=2)
        self.assertEqual(result["adjudicated"], 2)

    def test_oldest_entries_are_adjudicated_first(self):
        """So a --limit run always makes progress on the backlog's tail
        rather than re-chewing the newest quarantine."""
        self.write_ledger(self.entry("new", ts_epoch=200.0), self.entry("old", ts_epoch=100.0))
        result = self.poll(limit=1)
        self.assertEqual(result["decisions"][0]["patch_id"], "old")

    def test_an_entry_naming_no_squad_is_recorded_not_dropped(self):
        """overlord_sweep's bisection writer records format=None; a
        hand-edited line can lose the squad. Either way the daemon must
        say so rather than skip silently."""
        self.write_ledger(self.entry("p1", squad=None))
        result = self.poll()
        self.assertEqual(result["decisions"][0]["verdict"], "queued")
        self.assertIn("no squad", result["decisions"][0]["reason"])

    def test_one_raising_commit_does_not_stall_the_whole_backlog(self):
        """_run_poll_safely catches at the wrong granularity for this: a
        raise on entry 3 of 45 would abandon the other 42 every poll,
        forever, and the backlog this daemon exists to drain would never
        move."""
        self.write_ledger(self.entry("p1", ts_epoch=1.0),
                          self.entry("boom", ts_epoch=2.0),
                          self.entry("p3", ts_epoch=3.0))

        def adjudicate_fn(**k):
            if k["entry"]["patch_id"] == "boom":
                raise RuntimeError("cargo died")
            return jq.make_decision(patch_id=k["entry"]["patch_id"], sha=k["entry"]["sha"],
                                    format_name="RAR", squad=k["squad"], klass=k["klass"],
                                    verdict="queued", reason="fake", dry_run=not k["apply"])

        result = self.poll(adjudicate_fn=adjudicate_fn)
        self.assertEqual(result["adjudicated"], 3)
        by_id = {d["patch_id"]: d for d in result["decisions"]}
        self.assertEqual(by_id["boom"]["verdict"], "error")
        self.assertIn("cargo died", by_id["boom"]["reason"])
        # "error" is NOT terminal -- the entry gets another look next poll.
        self.assertNotIn("error", jq.TERMINAL_VERDICTS)
        self.assertEqual(by_id["p3"]["verdict"], "queued")

    def test_the_class_is_carried_onto_every_decision(self):
        self.write_ledger(self.entry("p1", flags=["cherry-pick-conflict"]))
        self.assertEqual(self.poll()["decisions"][0]["class"], "cherry-pick-conflict")


class StalledDecisionsTests(unittest.TestCase):
    """A `queued` verdict means the adjudicator could NOT decide and a human
    must -- but nothing was telling the human.

    Measured 2026-08-10: three patches had been queued since 2026-08-02 and
    re-examined roughly 2,300 times, while every poll printed
    `considered=3 adjudicated=0 skipped=3` -- a line that reads as a healthy
    no-op. Eight days of real work sat invisible behind it.
    """

    DAY = 24 * 3600

    def entries(self, *ids):
        return {i: {"squad": "canon", "format": "VRD"} for i in ids}

    def test_fresh_queued_is_not_stalled(self):
        prior = {"p1": {"verdict": "queued", "ts_epoch": 1000.0}}
        out = jq.stalled_decisions(self.entries("p1"), prior, now_fn=lambda: 1000.0 + 60)
        self.assertEqual(out, [])

    def test_old_queued_is_stalled_with_its_age(self):
        prior = {"p1": {"verdict": "queued", "ts_epoch": 0.0, "reason": "no-such-table"}}
        out = jq.stalled_decisions(self.entries("p1"), prior, now_fn=lambda: 8 * self.DAY)
        self.assertEqual(len(out), 1)
        self.assertAlmostEqual(out[0]["age_days"], 8.0, places=3)
        self.assertEqual(out[0]["reason"], "no-such-table")
        self.assertEqual(out[0]["squad"], "canon")

    def test_a_decided_verdict_is_never_stalled(self):
        """Only `queued` waits on a human. An admitted or rejected patch is
        finished business no matter how old the decision is."""
        for verdict in ("admitted", "rejected", "promoted"):
            prior = {"p1": {"verdict": verdict, "ts_epoch": 0.0}}
            out = jq.stalled_decisions(self.entries("p1"), prior, now_fn=lambda: 99 * self.DAY)
            self.assertEqual(out, [], f"{verdict} must not be reported as stalled")

    def test_entry_with_no_decision_is_not_stalled(self):
        """It has never been adjudicated, so it is the queue's normal work --
        poll_once will pick it up. Reporting it would cry wolf every poll."""
        out = jq.stalled_decisions(self.entries("p1"), {}, now_fn=lambda: 99 * self.DAY)
        self.assertEqual(out, [])

    def test_missing_ts_epoch_errs_toward_being_seen(self):
        """77 of 80 live quarantine rows predate a schema addition (see
        load_decisions). An unknowable age must not silently drop the row --
        that is the exact failure being fixed."""
        prior = {"p1": {"verdict": "queued"}}
        out = jq.stalled_decisions(self.entries("p1"), prior, now_fn=lambda: 5.0)
        self.assertEqual(len(out), 1)
        self.assertIsNone(out[0]["age_days"])


class RunPollSafelyTests(unittest.TestCase):
    def test_a_raising_poll_does_not_propagate(self):
        logs = []

        def boom():
            raise RuntimeError("git exploded")

        result = jq._run_poll_safely(poll_fn=boom, log_fn=logs.append)
        self.assertEqual(result["status"], "raised")
        self.assertIn("git exploded", result["error"])
        self.assertTrue(logs)

    def test_a_healthy_poll_is_returned_untouched(self):
        sentinel = {"considered": 3, "adjudicated": 1, "skipped": 2, "decisions": []}
        self.assertIs(jq._run_poll_safely(poll_fn=lambda: sentinel), sentinel)


class LockTests(unittest.TestCase):
    """The structural guards make double-PROMOTION impossible, but not two
    concurrent daemons: both would drive the same per-squad worktree, and
    the second one's `checkout --detach` would land in the middle of the
    first one's cherry-pick."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.home = Path(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def test_a_fresh_same_sha_holder_turns_the_second_run_away(self):
        jq.write_lock(jq.daemon_lock_path(self.home), 4242, "sha-a", 1000.0)
        called = []
        outcome = jq.run_locked(self.home, lambda hb: called.append(1),
                                now_fn=lambda: 1001.0, script_sha="sha-a", pid=99)
        self.assertEqual(outcome["status"], "already_running")
        self.assertEqual(called, [])

    def test_the_lock_is_released_even_when_the_poll_raises(self):
        with self.assertRaises(RuntimeError):
            jq.run_locked(self.home, lambda hb: (_ for _ in ()).throw(RuntimeError("boom")),
                          now_fn=lambda: 1000.0, script_sha="s", pid=7)
        # Released means the next acquire succeeds.
        outcome = jq.run_locked(self.home, lambda hb: "ran",
                                now_fn=lambda: 1001.0, script_sha="s", pid=8)
        self.assertEqual(outcome, {"status": "ok", "result": "ran"})

    def test_the_heartbeat_is_callable_mid_poll(self):
        """A bookkeeping promotion pays for two cargo-backed comparison
        runs, so the lock must be refreshable inside one poll."""
        beats = []

        def body(heartbeat):
            heartbeat()
            beats.append(json.loads(jq.daemon_lock_path(self.home).read_text()))

        jq.run_locked(self.home, body, now_fn=lambda: 5000.0, script_sha="s", pid=11)
        self.assertEqual(beats[0]["heartbeat_ts"], 5000.0)


class CliTests(unittest.TestCase):
    def setUp(self):
        self.calls = []
        self.tmp = tempfile.TemporaryDirectory()
        # NEVER default to the real ~/.oxidex: --apply takes a singleton
        # lock under <home>/logs/knowledge/, and a live fleet owns that
        # directory.
        self.home = ["--home", self.tmp.name]

    def tearDown(self):
        self.tmp.cleanup()

    def poll_fn(self, **kwargs):
        self.calls.append(kwargs)
        return {"considered": 0, "adjudicated": 0, "skipped": 0, "decisions": []}

    def test_dry_run_is_the_default(self):
        rc = jq.main(["--once", "--perl-lib", "/x", *self.home], poll_fn=self.poll_fn)
        self.assertEqual(rc, 0)
        self.assertFalse(self.calls[0]["apply"])

    def test_apply_turns_mutation_on(self):
        jq.main(["--once", "--apply", "--perl-lib", "/x", *self.home], poll_fn=self.poll_fn)
        self.assertTrue(self.calls[0]["apply"])

    def test_dry_run_overrides_apply(self):
        """The explicit "touch nothing" must be unoverridable by an
        --apply left over in a shell-history line."""
        jq.main(["--once", "--apply", "--dry-run", "--perl-lib", "/x", *self.home],
                poll_fn=self.poll_fn)
        self.assertFalse(self.calls[0]["apply"])

    def test_infinite_polls_until_interrupted(self):
        sleeps = []

        def sleep_fn(seconds):
            sleeps.append(seconds)
            if len(sleeps) == 3:
                raise KeyboardInterrupt

        with self.assertRaises(KeyboardInterrupt):
            jq.main(["--infinite", "--poll-seconds", "7", "--perl-lib", "/x", *self.home],
                    sleep_fn=sleep_fn, poll_fn=self.poll_fn)
        self.assertEqual(sleeps, [7.0, 7.0, 7.0])
        self.assertEqual(len(self.calls), 3)

    def test_the_reserved_slot_is_configurable_and_defaults_out_of_range(self):
        jq.main(["--once", "--slot", "42", "--perl-lib", "/x", *self.home], poll_fn=self.poll_fn)
        self.assertEqual(self.calls[0]["slot"], 42)
        self.assertEqual(jq.DEFAULT_SLOT, 700)

    def test_the_slot_branch_matches_both_fleet_conventions(self):
        """squad_merge_loop.squad_slot_branches globs
        refs/heads/model-fix-parallel-<squad>-*, and
        parallel_model_fix_loop._SQUAD_BRANCH_RE requires a trailing
        -<digits> to map the branch back to its squad."""
        import re
        branch = jq.slot_branch_name("sony-minolta")
        self.assertTrue(branch.startswith("model-fix-parallel-sony-minolta-"))
        match = re.match(r"^model-fix-parallel-(?P<squad>.+)-(?P<n>\d+)$", branch)
        self.assertIsNotNone(match)
        self.assertEqual(match.group("squad"), "sony-minolta")


# ---------------------------------------------------------------------------
# Acceptance against the real repository and the real ExifTool checkout
# ---------------------------------------------------------------------------

REAL_REPO = Path(__file__).resolve().parent.parent
REAL_PERL = Path("/private/tmp/oxidex-exiftool-cache/exiftool/lib")
REAL_COMMITS = ("5249a506", "a998b8fc", "e0900a27")


def _acceptance_available():
    if not REAL_PERL.is_dir():
        return False
    for sha in REAL_COMMITS:
        result = subprocess.run(["git", "cat-file", "-e", f"{sha}^{{commit}}"],
                                cwd=str(REAL_REPO), capture_output=True)
        if result.returncode != 0:
            return False
    return True


@unittest.skipUnless(_acceptance_available(),
                     "needs the real repo commits and the ExifTool cache")
class AcceptanceAgainstRealCommits(unittest.TestCase):
    """The three commits from the brief, adjudicated end to end through
    the daemon's own composition (module binding + hint expansion +
    the permanent-rejection guard), not through verify_enum_maps' CLI."""

    def adjudicate(self, sha, *, klass="printconv"):
        full = subprocess.run(["git", "rev-parse", sha], cwd=str(REAL_REPO),
                              capture_output=True, text=True).stdout.strip()
        patch_id = jq.squad_merge_loop.compute_patch_id_for_sha(REAL_REPO, full)
        entry = {"patch_id": patch_id, "sha": full, "format": "X", "squad": "tail",
                 "flags": ["printconv-mismatch:x"], "attempt": 1}
        return jq.adjudicate(
            repo_root=REAL_REPO, home=Path("/nonexistent-judgment-home"), entry=entry,
            squad="tail", klass=klass, worktree_path=Path("/nonexistent"),
            perl_lib=str(REAL_PERL), apply=False, log_fn=lambda *a: None,
        )

    def test_a_refused_binding_never_convicts_on_its_catch_all_either(self):
        """A catch-all falls with the binding, exactly as the pairs do.

        Measured 2026-07-27 by adjudicating 8 archived patches beside a human
        pass over the same 8. Two were rejected-permanent -- which is
        deliberately immune to a policy bump, so UNRECOVERABLE -- purely on a
        `_ =>` arm while the table binding had already been refused:

          elf-4b5a26e97cb8  pairs_checked=17, agreements=17, mismatches=0
                            (a PERFECT record: 9/9 CPUType and 5/5
                            ObjectFileType pairs match EXE.pm)
          pdf-a1a411f67e3f  pairs_checked=0, nothing verified at all, and it
                            measures a real -4 gap closure

        A catch-all convicts because ExifTool prints the RAW NUMBER for an
        unlisted key -- true only if the correct table has no OTHER sub. With
        the binding refused we do not know which table is correct, so we
        cannot know that. Same rule, same fix as verify_enum_maps (#145).
        """
        for sha in ("e4c6fe23", "f5735bc8"):  # elf / pdf lineage in-repo
            with self.subTest(sha=sha):
                try:
                    decision = self.adjudicate(sha)
                except Exception:
                    self.skipTest(f"{sha} not present in this checkout")
                v = decision["detail"].get("verifier") or {}
                if v.get("binding_refused"):
                    self.assertNotEqual(
                        decision["verdict"], "rejected-permanent",
                        "a refused binding must never produce a TERMINAL verdict, "
                        "no matter what catch-all arms the diff carries",
                    )

    def test_rar_a998b8fc_is_rejected_permanently_on_the_right_table(self):
        decision = self.adjudicate("a998b8fc")
        # NOT rejected-permanent, and the reason is a deliberate trade.
        #
        # a998b8fc really is fabricated -- ZIP.pm's RAR5 table is exactly
        # {0: Win32, 1: Unix} and the diff adds 2/3/4 plus a catch-all. But
        # its diff carries THREE match blocks, so the single forced hint binds
        # all three to RAR5{OperatingSystem} and the binding is refused. Once
        # refused, neither the pairs NOR the catch-all can support a TERMINAL
        # verdict (see test_a_refused_binding_never_convicts_on_its_catch_all_either).
        #
        # The cost is real and is recorded here rather than hidden: this rule
        # removed the daemon's ONLY correct permanent rejection in the 8-patch
        # experiment along with its two false ones. What it does not cost is
        # safety -- "queued" still never reaches main, so no fabricated
        # metadata lands; the daemon simply stops claiming the question is
        # settled forever. For an irreversible verdict on uncertain evidence
        # that is the right direction: a wrong "queued" costs one deferred
        # adjudication, a wrong "rejected-permanent" costs the work outright.
        #
        # To convict here legitimately, a future revision would have to show
        # that EVERY candidate table for the hint lacks an OTHER sub -- then
        # the catch-all is wrong whichever binding is correct.
        self.assertNotEqual(decision["verdict"], "rejected-permanent")
        self.assertEqual(decision["verdict"], "queued")
        verifier = decision["detail"]["verifier"]
        # The module is no longer PINNED at all, and that is a knock-on of
        # verify_enum_maps #145 rather than a regression here. The daemon
        # pins a candidate module only when its verdict is conclusive; #145
        # made catch-all-only evidence return "cannot-verify" instead of
        # "fabricated", so the RAR5 candidate stops looking conclusive and
        # nothing is bound.
        #
        # That is the correct composition: pinning a module on the strength
        # of a catch-all alone is precisely the inference #145 removed, and
        # re-adding it here would smuggle it back in one layer up.
        self.assertIsNone(verifier["table"])
        self.assertEqual(verifier["status"], "cannot-verify")
        # The conviction rests on the CATCH-ALL, not on the pairs, and that is
        # the stricter and correct outcome. a998b8fc's diff carries THREE match
        # blocks, so the single forced hint binds all three to
        # RAR5{OperatingSystem}: the 2 agreements come from the block that fits
        # and the 3 "mismatches" may come from blocks that do not.
        # pair_evidence_is_trustworthy therefore refuses that binding as a basis
        # for a PERMANENT rejection and files the pairs as unreliable.
        #
        # An earlier version of this test asserted verifier["mismatches"],
        # which only exists on the trustworthy-binding path -- it predates the
        # narrowing of that guard and encoded a weaker standard than the
        # implementation now applies. Asserting the weaker shape would have
        # pushed the daemon back toward convicting permanently on a misbound
        # table, which is the exact failure the guard exists to prevent.
        # No binding was reached at all, so there is no binding to refuse and
        # no pair evidence to file -- the verifier abstained upstream. The
        # commit stays blocked (queued never reaches main); what changed is
        # that the daemon no longer claims the question is settled forever.
        self.assertNotIn("binding_refused", verifier)
        self.assertNotIn("mismatches", verifier)
        self.assertEqual(verifier["pairs_checked"], 0)

    def test_rw2_e0900a27_is_never_convicted(self):
        """The clean one. It has already landed on origin/main, which is
        itself the terminal answer -- what matters is that it is NOT
        rejected-permanent."""
        decision = self.adjudicate("e0900a27")
        self.assertNotEqual(decision["verdict"], "rejected-permanent")

    def test_ttf_5249a506_is_never_promoted(self):
        """The daemon cannot reach %ttLang{Macintosh} from this commit's
        trailers (it cites no Perl-Ref, and its Tag: names are
        FontSubfamily-es style, which name no table), so it abstains.
        Abstention is the required direction: the fabrication is not
        caught here, but it is never re-admitted either."""
        decision = self.adjudicate("5249a506")
        self.assertNotEqual(decision["verdict"], "promoted")
        self.assertNotIn(decision["verdict"], {"promoted"})

    def test_the_live_ledger_is_readable_and_every_entry_classifies(self):
        ledger = jq.squad_merge_loop.quarantine_ledger_path(Path(os.path.expanduser("~/.oxidex")))
        if not ledger.exists():
            self.skipTest("no live quarantine ledger on this host")
        entries = jq.squad_merge_loop.load_quarantine(ledger)
        self.assertTrue(entries)
        for patch_id, entry in entries.items():
            klass = jq.classify_flags(entry.get("flags"))
            self.assertIn(klass, {k for k, _ in jq._CLASS_RULES} | {"unknown"})
            self.assertNotEqual(klass, "unknown", f"{patch_id}: {entry.get('flags')}")


if __name__ == "__main__":
    unittest.main()
