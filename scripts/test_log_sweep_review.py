import json
import re
import tempfile
import unittest
from pathlib import Path

from log_sweep_review import append_sweep_review, main


class AppendSweepReviewTests(unittest.TestCase):
    def test_writes_one_json_line_with_expected_fields(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            log_path = Path(tmpdir) / "log.jsonl"
            entry = append_sweep_review(
                log_path, "RW2", "IFD0:BlackLevelBlue", "accepted",
                "Matches this codebase's IFD0: convention", commit="7d7896a",
                now_fn=lambda: 1_700_000_000,
            )
            lines = log_path.read_text().splitlines()
        self.assertEqual(len(lines), 1)
        parsed = json.loads(lines[0])
        self.assertEqual(parsed["format"], "RW2")
        self.assertEqual(parsed["tag"], "IFD0:BlackLevelBlue")
        self.assertEqual(parsed["verdict"], "accepted")
        self.assertEqual(parsed["commit"], "7d7896a")
        self.assertEqual(parsed, entry)

    def test_appends_without_clobbering_existing_entries(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            log_path = Path(tmpdir) / "log.jsonl"
            append_sweep_review(log_path, "NEF", "A", "accepted", "r1")
            append_sweep_review(log_path, "NEF", "B", "rejected", "r2")
            lines = log_path.read_text().splitlines()
        self.assertEqual(len(lines), 2)

    def test_rejects_invalid_verdict(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            log_path = Path(tmpdir) / "log.jsonl"
            with self.assertRaises(ValueError):
                append_sweep_review(log_path, "NEF", "A", "maybe", "r")

    def test_creates_parent_directories(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            log_path = Path(tmpdir) / "nested" / "dir" / "log.jsonl"
            append_sweep_review(log_path, "NEF", "A", "accepted", "r")
            self.assertTrue(log_path.exists())

    def test_accepted_verdict_appends_to_landed_log(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            log = Path(tmpdir) / "reviews.jsonl"
            landed = Path(tmpdir) / "landed.log"
            append_sweep_review(log, "JPEG", "APP12:MODE3", "accepted", "verified",
                                landed_log_path=landed, now_fn=lambda: 1_784_800_000)
            self.assertIn("JPEG:APP12:MODE3", landed.read_text())

    def test_rejected_verdict_does_not_touch_landed_log(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            log = Path(tmpdir) / "reviews.jsonl"
            landed = Path(tmpdir) / "landed.log"
            append_sweep_review(log, "XMP", "XMP:ArtworkTitle", "rejected", "wrong value",
                                landed_log_path=landed, now_fn=lambda: 1_784_800_000)
            self.assertFalse(landed.exists())


class MainTests(unittest.TestCase):
    def test_cli_writes_expected_entry(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            log_path = Path(tmpdir) / "log.jsonl"
            rc = main([
                "--format", "XMP", "--tag", "XMP:AboutCvTermCvId",
                "--verdict", "rejected",
                "--reason", "guessed JSON bracket syntax instead of comma-space text",
                "--commit", "e77bffe",
                "--log-path", str(log_path),
                # --home keeps the unconditional human-verdict lesson
                # mirror inside the tempdir (hermetic: no real ~/.oxidex).
                "--home", tmpdir,
            ])
            parsed = json.loads(log_path.read_text().splitlines()[0])
        self.assertEqual(rc, 0)
        self.assertEqual(parsed["format"], "XMP")
        self.assertEqual(parsed["verdict"], "rejected")


# --- New coverage below: K4 verdict classes, K1 lesson events, M5 tombstones,
# --- M6 trailer-derived entries. Everything above is the pre-existing suite,
# --- untouched, proving the legacy CLI/API contract still holds.


class VerdictClassTests(unittest.TestCase):
    """Spec K4: legacy spellings alias to human_*; new classes are stored
    verbatim in verdict_class while the legacy verdict field degrades to
    the accepted/rejected binary for old readers."""

    def _entry(self, verdict):
        with tempfile.TemporaryDirectory() as tmpdir:
            log = Path(tmpdir) / "log.jsonl"
            entry = append_sweep_review(log, "NEF", "A", verdict, "r")
            parsed = json.loads(log.read_text().splitlines()[0])
        self.assertEqual(parsed, entry)
        return entry

    def test_legacy_accepted_aliases_to_human_accepted(self):
        entry = self._entry("accepted")
        self.assertEqual(entry["verdict_class"], "human_accepted")
        self.assertEqual(entry["verdict"], "accepted")

    def test_legacy_rejected_aliases_to_human_rejected(self):
        entry = self._entry("rejected")
        self.assertEqual(entry["verdict_class"], "human_rejected")
        self.assertEqual(entry["verdict"], "rejected")

    def test_machine_accepted_keeps_class_and_degrades_verdict(self):
        entry = self._entry("machine_accepted")
        self.assertEqual(entry["verdict_class"], "machine_accepted")
        self.assertEqual(entry["verdict"], "accepted")

    def test_machine_rejected_degrades_to_rejected(self):
        entry = self._entry("machine_rejected")
        self.assertEqual(entry["verdict_class"], "machine_rejected")
        self.assertEqual(entry["verdict"], "rejected")

    def test_reverted_degrades_to_rejected(self):
        entry = self._entry("reverted")
        self.assertEqual(entry["verdict_class"], "reverted")
        self.assertEqual(entry["verdict"], "rejected")

    def test_machine_accepted_never_writes_landed_log(self):
        # landed-tags is the human-verified skip set; machine acceptance
        # must not populate it even when a landed log path is supplied.
        with tempfile.TemporaryDirectory() as tmpdir:
            log = Path(tmpdir) / "log.jsonl"
            landed = Path(tmpdir) / "landed.log"
            append_sweep_review(log, "JPEG", "APP1:X", "machine_accepted", "r",
                                landed_log_path=landed)
            self.assertFalse(landed.exists())

    def test_human_accepted_spelling_writes_landed_log(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            log = Path(tmpdir) / "log.jsonl"
            landed = Path(tmpdir) / "landed.log"
            append_sweep_review(log, "JPEG", "APP1:X", "human_accepted", "r",
                                landed_log_path=landed)
            self.assertIn("JPEG:APP1:X", landed.read_text())


class LessonLineTests(unittest.TestCase):
    """Spec K1: append_lesson_line's schema, fingerprints, and the
    single-write 2000-byte clamp contract."""

    def _event(self, **overrides):
        from log_sweep_review import build_lesson_event
        kwargs = dict(
            event="human_rejected",
            reason="PrintConv strings must match Perl byte-for-byte",
            format_name="JPEG", tag_key="MakerNotes:AELButton",
            module="Canon.pm", now_fn=lambda: 1_784_800_000,
        )
        kwargs.update(overrides)
        return build_lesson_event(**kwargs)

    def test_event_has_full_k1_schema(self):
        event = self._event()
        self.assertEqual(
            list(event),
            ["ts", "worker", "format", "module", "table", "tag_key", "event",
             "reason", "evidence", "checklist_id", "fingerprint_scoped",
             "fingerprint_generic"],
        )
        self.assertEqual(event["event"], "human_rejected")
        self.assertEqual(event["tag_key"], "MakerNotes:AELButton")

    def test_rejects_event_outside_enum(self):
        from log_sweep_review import build_lesson_event
        with self.assertRaises(ValueError):
            build_lesson_event("machine_rejected", "r")  # not a K1 event

    def test_fingerprints_scoped_by_module_generic_not(self):
        canon = self._event(module="Canon.pm")
        nikon = self._event(module="Nikon.pm")
        # Same mistake in two modules: generic clusters, scoped separates.
        self.assertEqual(canon["fingerprint_generic"], nikon["fingerprint_generic"])
        self.assertNotEqual(canon["fingerprint_scoped"], nikon["fingerprint_scoped"])

    def test_fingerprints_prefer_checklist_id_over_reason(self):
        a = self._event(checklist_id="C2", reason="one wording")
        b = self._event(checklist_id="C2", reason="totally different wording")
        self.assertEqual(a["fingerprint_scoped"], b["fingerprint_scoped"])
        self.assertEqual(a["fingerprint_generic"], b["fingerprint_generic"])

    def test_fingerprints_normalize_reason_whitespace_and_case(self):
        a = self._event(reason="Match  Perl BYTE-for-byte")
        b = self._event(reason="match perl byte-for-byte")
        self.assertEqual(a["fingerprint_generic"], b["fingerprint_generic"])

    def test_append_writes_one_parseable_line(self):
        from log_sweep_review import append_lesson_line
        with tempfile.TemporaryDirectory() as tmpdir:
            append_lesson_line(tmpdir, self._event())
            path = Path(tmpdir) / "logs" / "lessons.jsonl"
            lines = path.read_bytes().splitlines(keepends=True)
        self.assertEqual(len(lines), 1)
        self.assertTrue(lines[0].endswith(b"\n"))
        parsed = json.loads(lines[0])
        self.assertEqual(parsed["event"], "human_rejected")

    def test_appends_do_not_clobber(self):
        from log_sweep_review import append_lesson_line
        with tempfile.TemporaryDirectory() as tmpdir:
            append_lesson_line(tmpdir, self._event())
            append_lesson_line(tmpdir, self._event(reason="second"))
            path = Path(tmpdir) / "logs" / "lessons.jsonl"
            self.assertEqual(len(path.read_text().splitlines()), 2)

    def test_oversized_reason_clamped_to_2000_bytes_still_valid_json(self):
        from log_sweep_review import LESSON_LINE_MAX_BYTES, append_lesson_line
        with tempfile.TemporaryDirectory() as tmpdir:
            raw = append_lesson_line(tmpdir, self._event(reason="x" * 5000))
            on_disk = (Path(tmpdir) / "logs" / "lessons.jsonl").read_bytes()
        self.assertEqual(raw, on_disk)
        self.assertLessEqual(len(raw), LESSON_LINE_MAX_BYTES)
        self.assertTrue(raw.endswith(b"\n"))
        # Overflow was in the free-text reason, so the clamp preserved JSON.
        parsed = json.loads(raw)
        self.assertTrue(parsed["reason"].startswith("xxx"))

    def test_best_effort_shrink_also_covers_an_oversized_evidence_blob(self):
        # append_lesson_line now delegates to distill_lessons'
        # encode_lesson_line (the canonical K1 owner), whose best-effort
        # shrink covers BOTH evidence and reason -- strictly more capable
        # than this script's old reason-only shrink, so an oversized
        # evidence blob no longer needs the hard clamp to fit.
        from log_sweep_review import LESSON_LINE_MAX_BYTES, append_lesson_line
        with tempfile.TemporaryDirectory() as tmpdir:
            raw = append_lesson_line(
                tmpdir, self._event(reason="short", evidence={"blob": "y" * 5000}))
        self.assertLessEqual(len(raw), LESSON_LINE_MAX_BYTES)
        self.assertTrue(raw.endswith(b"\n"))
        parsed = json.loads(raw)
        self.assertEqual(parsed["reason"], "short")

    def test_hard_clamp_when_no_shrinkable_field_can_absorb_overflow(self):
        # Overflow lives in tag_key, a field the shrink loop does not
        # touch (only evidence/reason): neither can help, so the hard
        # byte clamp still applies. Readers skip the malformed line --
        # the contract is bounded size, not validity.
        from log_sweep_review import LESSON_LINE_MAX_BYTES, append_lesson_line
        with tempfile.TemporaryDirectory() as tmpdir:
            raw = append_lesson_line(
                tmpdir, self._event(reason="short", tag_key="T" * 5000))
        self.assertEqual(len(raw), LESSON_LINE_MAX_BYTES)
        self.assertTrue(raw.endswith(b"\n"))

    def test_cli_lesson_flag_mirrors_verdict_into_lessons_jsonl(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            rc = main([
                "--format", "XMP", "--tag", "XMP:AboutCvTermCvId",
                "--verdict", "rejected", "--reason", "wrong list separator",
                "--commit", "e77bffe",
                "--lesson", "verify against exiftool text output, not -j",
                "--home", tmpdir,
            ], now_fn=lambda: 1_784_800_000)
            lesson_lines = (Path(tmpdir) / "logs" / "lessons.jsonl").read_text().splitlines()
            review_lines = (Path(tmpdir) / "logs" / "sweep-review-history.jsonl").read_text().splitlines()
        self.assertEqual(rc, 0)
        self.assertEqual(len(lesson_lines), 1)
        self.assertEqual(len(review_lines), 1)
        event = json.loads(lesson_lines[0])
        self.assertEqual(event["event"], "human_rejected")  # mirrors the verdict
        self.assertEqual(event["reason"], "verify against exiftool text output, not -j")
        self.assertEqual(event["format"], "XMP")
        self.assertEqual(event["tag_key"], "XMP:AboutCvTermCvId")
        self.assertEqual(event["evidence"], {"commit": "e77bffe"})
        self.assertEqual(len(event["fingerprint_scoped"]), 40)
        self.assertEqual(len(event["fingerprint_generic"]), 40)

    def test_cli_lesson_on_accepted_verdict_maps_to_human_accepted(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            main([
                "--format", "RW2", "--tag", "IFD0:X", "--verdict", "accepted",
                "--reason", "r", "--lesson", "IFD0 prefix is the convention",
                "--home", tmpdir,
            ])
            event = json.loads((Path(tmpdir) / "logs" / "lessons.jsonl").read_text())
        self.assertEqual(event["event"], "human_accepted")

    def test_human_verdict_without_lesson_still_mirrors_reason(self):
        # Spec K1: mirroring of HUMAN verdicts is unconditional; --lesson
        # only swaps in nicer free text. Without it, --reason is the lesson.
        with tempfile.TemporaryDirectory() as tmpdir:
            main(["--format", "RW2", "--tag", "IFD0:X", "--verdict", "accepted",
                  "--reason", "matches the IFD0 convention", "--home", tmpdir])
            event = json.loads((Path(tmpdir) / "logs" / "lessons.jsonl").read_text())
        self.assertEqual(event["event"], "human_accepted")
        self.assertEqual(event["reason"], "matches the IFD0 convention")
        self.assertEqual(event["tag_key"], "IFD0:X")

    def test_human_rejected_without_lesson_mirrors_too(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            main(["--format", "XMP", "--tag", "XMP:A", "--verdict", "rejected",
                  "--reason", "wrong list separator", "--home", tmpdir])
            event = json.loads((Path(tmpdir) / "logs" / "lessons.jsonl").read_text())
        self.assertEqual(event["event"], "human_rejected")
        self.assertEqual(event["reason"], "wrong list separator")

    def test_machine_verdict_without_lesson_does_not_mirror(self):
        # Machine verdicts entered via the CLI mirror only on an explicit
        # --lesson: the merger/sweep write their own machine ledger events,
        # so unconditional mirroring here would double-count them.
        with tempfile.TemporaryDirectory() as tmpdir:
            main(["--format", "RW2", "--tag", "A", "--verdict", "machine_accepted",
                  "--reason", "r", "--home", tmpdir])
            self.assertFalse((Path(tmpdir) / "logs" / "lessons.jsonl").exists())

    def test_revert_mirrors_human_rejected_lesson_unconditionally(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            main(["--revert", "2433c79", "--format", "JPEG",
                  "--tag", "MakerNotes:AELButton",
                  "--reason", "wrong on Canon bodies", "--home", tmpdir])
            event = json.loads((Path(tmpdir) / "logs" / "lessons.jsonl").read_text())
        self.assertEqual(event["event"], "human_rejected")
        self.assertEqual(event["reason"], "wrong on Canon bodies")
        self.assertEqual(event["evidence"], {"commit": "2433c79"})


class FingerprintParityTests(unittest.TestCase):
    """Spec K1: every ledger writer must produce byte-identical
    fingerprints, or events written by log_sweep_review and events written
    by distill_lessons (migration, future model_fix_loop writers) fork
    into dialects that never share a cluster -- silently defeating the
    K2/K3 promotion rule (>=3 occurrences across >=2 modules)."""

    def _event(self, reason, module="Canon.pm", checklist_id=None):
        from log_sweep_review import build_lesson_event
        return build_lesson_event("wrong_value", reason, module=module,
                                  checklist_id=checklist_id,
                                  now_fn=lambda: 1_784_800_000)

    def test_generic_matches_distiller_for_digit_hex_and_date_reasons(self):
        import distill_lessons
        for reason in (
            "[2026-07-24] PrintConv for tag 0x4001 index 42 must match Perl",
            "index 42 out of range for table 0x1f",
            "plain digit-free reason",
        ):
            event = self._event(reason)
            self.assertEqual(
                event["fingerprint_generic"],
                distill_lessons.fingerprint_generic("wrong_value", "", reason),
                f"generic fingerprint dialect fork for reason {reason!r}")
            self.assertEqual(
                event["fingerprint_scoped"],
                distill_lessons.fingerprint_scoped(
                    "wrong_value", "Canon.pm", "", reason))

    def test_scoped_matches_distiller_when_module_is_none(self):
        import distill_lessons
        event = self._event("some reason", module=None)
        self.assertEqual(
            event["fingerprint_scoped"],
            distill_lessons.fingerprint_scoped("wrong_value", None, "", "some reason"))

    def test_digit_variants_cluster_like_the_distiller(self):
        # distill's norm_reason folds digit runs to '#': "index 42" and
        # "index 43" are the same mistake; this writer must agree.
        a = self._event("index 42 mismatch")
        b = self._event("index 43 mismatch")
        self.assertEqual(a["fingerprint_generic"], b["fingerprint_generic"])

    def test_checklist_id_parity_with_distiller(self):
        import distill_lessons
        event = self._event("any wording", checklist_id="C2")
        self.assertEqual(
            event["fingerprint_generic"],
            distill_lessons.fingerprint_generic("wrong_value", "C2", "ignored"))


class RevertTests(unittest.TestCase):
    """Spec M5: --revert writes the tombstone line load_landed_tags honors
    plus a verdict_class=reverted review entry."""

    def test_revert_appends_tombstone_and_review_entry(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            rc = main([
                "--revert", "2433c79", "--format", "JPEG",
                "--tag", "MakerNotes:AELButton",
                "--reason", "wrong on Canon bodies",
                "--home", tmpdir,
            ], now_fn=lambda: 1_784_800_000)
            landed = (Path(tmpdir) / "logs" / "landed-tags.log").read_text()
            entry = json.loads(
                (Path(tmpdir) / "logs" / "sweep-review-history.jsonl").read_text())
        self.assertEqual(rc, 0)
        lines = landed.splitlines()
        self.assertEqual(len(lines), 1)
        self.assertRegex(
            lines[0],
            r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2} REVERTED JPEG:MakerNotes:AELButton$",
        )
        self.assertEqual(entry["verdict_class"], "reverted")
        self.assertEqual(entry["commit"], "2433c79")
        self.assertEqual(entry["reason"], "wrong on Canon bodies")

    def test_revert_requires_format_and_tag(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            with self.assertRaises(SystemExit):
                main(["--revert", "abc123", "--home", tmpdir])

    def test_revert_reason_defaults_to_sha_reference(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            main(["--revert", "abc123", "--format", "NEF", "--tag", "A",
                  "--home", tmpdir])
            entry = json.loads(
                (Path(tmpdir) / "logs" / "sweep-review-history.jsonl").read_text())
        self.assertEqual(entry["reason"], "reverted abc123")


def make_fake_git(commits, range_order=None):
    """Fake run_git(args, input_text=None) covering exactly the calls
    append_from_commits/resolve_range_shas make. commits maps sha ->
    {"body": full message, "subject": str, "patch_id": str}. The
    interpret-trailers fake extracts trailing Key: Value lines from the
    piped-in body, mimicking `git interpret-trailers --parse` closely
    enough for the parser under test."""
    def run_git(args, input_text=None):
        if args[:3] == ["log", "-1", "--format=%B"]:
            return commits[args[3]]["body"]
        if args[:3] == ["log", "-1", "--format=%s"]:
            return commits[args[3]]["subject"] + "\n"
        if args[:2] == ["interpret-trailers", "--parse"]:
            out = []
            for line in (input_text or "").splitlines():
                if re.match(r"^[A-Za-z][A-Za-z-]*: \S", line):
                    out.append(line)
            return "\n".join(out) + "\n"
        if args[0] == "show":
            if commits[args[1]]["patch_id"] is None:
                return ""  # empty-diff commit: `git show` emits no patch
            return f"diff-of-{args[1]}\n"
        if args[:2] == ["patch-id", "--stable"]:
            if not (input_text or "").strip():
                return ""  # git patch-id emits nothing for an empty diff
            sha = (input_text or "").removeprefix("diff-of-").strip()
            return f"{commits[sha]['patch_id']} {sha}\n"
        if args[:2] == ["rev-list", "--reverse"]:
            return "\n".join(range_order or []) + "\n"
        raise AssertionError(f"unexpected git call: {args}")
    return run_git


COMMIT_BODY = """\
fix(jpeg): wire AELButton and AFPointSelected

Format: JPEG
Tag: MakerNotes:AELButton
Tag: MakerNotes:AFPointSelected
Sample: canon-eos.jpg
Perl-Ref: Canon.pm:1234
Verified: recheck-pass gaps=12->10
Worker: w03
Table: Canon::CameraSettings
"""


class FromCommitTests(unittest.TestCase):
    """Spec M6: trailer-derived entries with a fake git runner, one entry
    per Tag: trailer, patch-id recorded, (patch_id, reason) dedup."""

    def _commits(self):
        return {
            "aaa111": {"body": COMMIT_BODY, "subject":
                       "fix(jpeg): wire AELButton and AFPointSelected",
                       "patch_id": "p-1"},
        }

    def test_from_commit_writes_one_entry_per_tag_trailer(self):
        from log_sweep_review import append_from_commits
        with tempfile.TemporaryDirectory() as tmpdir:
            log = Path(tmpdir) / "log.jsonl"
            written, skipped = append_from_commits(
                log, ["aaa111"], make_fake_git(self._commits()),
                now_fn=lambda: 1_784_800_000)
            lines = [json.loads(l) for l in log.read_text().splitlines()]
        self.assertEqual(skipped, [])
        self.assertEqual(len(written), 2)
        self.assertEqual(len(lines), 2)
        self.assertEqual([e["tag"] for e in lines],
                         ["MakerNotes:AELButton", "MakerNotes:AFPointSelected"])
        for e in lines:
            self.assertEqual(e["format"], "JPEG")
            self.assertEqual(e["verdict_class"], "machine_accepted")  # default
            self.assertEqual(e["verdict"], "accepted")  # legacy degradation
            self.assertEqual(e["patch_id"], "p-1")
            self.assertEqual(e["commit"], "aaa111")
            self.assertEqual(e["worker"], "w03")
            self.assertEqual(e["table"], "Canon::CameraSettings")
            self.assertEqual(
                e["reason"], "auto: fix(jpeg): wire AELButton and AFPointSelected")

    def test_rerun_dedups_by_patch_id_and_reason(self):
        from log_sweep_review import append_from_commits
        with tempfile.TemporaryDirectory() as tmpdir:
            log = Path(tmpdir) / "log.jsonl"
            git = make_fake_git(self._commits())
            append_from_commits(log, ["aaa111"], git)
            written, skipped = append_from_commits(log, ["aaa111"], git)
            lines = log.read_text().splitlines()
        self.assertEqual(written, [])  # duplicate writes nothing
        self.assertEqual(skipped, ["aaa111"])
        self.assertEqual(len(lines), 2)  # only the first run's two tag entries

    def test_same_patch_id_twice_in_one_range_writes_once(self):
        # A cherry-picked duplicate (new sha, same patch, same subject)
        # inside one batch dedups against the batch, not just the log.
        from log_sweep_review import append_from_commits
        commits = self._commits()
        commits["bbb222"] = dict(commits["aaa111"])
        with tempfile.TemporaryDirectory() as tmpdir:
            log = Path(tmpdir) / "log.jsonl"
            written, skipped = append_from_commits(
                log, ["aaa111", "bbb222"], make_fake_git(commits))
        self.assertEqual(len(written), 2)  # the two tags of the first sha only
        self.assertEqual(skipped, ["bbb222"])

    def test_commit_without_tag_trailers_writes_nothing(self):
        from log_sweep_review import append_from_commits
        commits = {"ccc333": {"body": "docs: readme\n", "subject": "docs: readme",
                              "patch_id": "p-9"}}
        with tempfile.TemporaryDirectory() as tmpdir:
            log = Path(tmpdir) / "log.jsonl"
            written, skipped = append_from_commits(
                log, ["ccc333"], make_fake_git(commits))
            self.assertFalse(log.exists())
        self.assertEqual(written, [])
        self.assertEqual(skipped, [])

    def test_cli_from_commit_with_injected_runner(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            rc = main(
                ["--from-commit", "aaa111", "--home", tmpdir],
                run_git=make_fake_git(self._commits()),
            )
            lines = (Path(tmpdir) / "logs" / "sweep-review-history.jsonl").read_text().splitlines()
        self.assertEqual(rc, 0)
        self.assertEqual(len(lines), 2)

    def test_cli_from_commit_honors_explicit_verdict(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            main(
                ["--from-commit", "aaa111", "--verdict", "machine_rejected",
                 "--reason", "targeted test failed", "--home", tmpdir],
                run_git=make_fake_git(self._commits()),
            )
            lines = [json.loads(l) for l in
                     (Path(tmpdir) / "logs" / "sweep-review-history.jsonl").read_text().splitlines()]
        self.assertTrue(all(e["verdict_class"] == "machine_rejected" for e in lines))

    def test_cli_from_range_expands_rev_list(self):
        commits = self._commits()
        commits["ddd444"] = {
            "body": "fix(nef): wire X\n\nFormat: NEF\nTag: IFD0:X\n",
            "subject": "fix(nef): wire X", "patch_id": "p-2",
        }
        with tempfile.TemporaryDirectory() as tmpdir:
            rc = main(
                ["--from-range", "origin/main..squad/tiff", "--home", tmpdir],
                run_git=make_fake_git(commits, range_order=["aaa111", "ddd444"]),
            )
            lines = [json.loads(l) for l in
                     (Path(tmpdir) / "logs" / "sweep-review-history.jsonl").read_text().splitlines()]
        self.assertEqual(rc, 0)
        self.assertEqual(len(lines), 3)  # 2 tags from aaa111 + 1 from ddd444
        self.assertEqual(lines[-1]["format"], "NEF")
        self.assertEqual(lines[-1]["patch_id"], "p-2")

    def test_two_distinct_empty_commits_sharing_a_reason_both_write(self):
        # commit_patch_id is None for empty diffs; the dedup key must fall
        # back to the sha, or the second empty commit in a batch collides
        # on (None, reason) and is silently dropped as a false duplicate.
        commits = {
            "eee111": {"body": "cherry: empty A\n\nFormat: JPEG\nTag: EXIF:A\n",
                       "subject": "cherry: empty A", "patch_id": None},
            "fff222": {"body": "cherry: empty B\n\nFormat: JPEG\nTag: EXIF:B\n",
                       "subject": "cherry: empty B", "patch_id": None},
        }
        from log_sweep_review import append_from_commits
        with tempfile.TemporaryDirectory() as tmpdir:
            log = Path(tmpdir) / "log.jsonl"
            written, skipped = append_from_commits(
                log, ["eee111", "fff222"], make_fake_git(commits),
                reason="range import")
            entries = [json.loads(l) for l in log.read_text().splitlines()]
        self.assertEqual(skipped, [])
        self.assertEqual([e["tag"] for e in entries], ["EXIF:A", "EXIF:B"])
        self.assertTrue(all(e["patch_id"] is None for e in entries))

    def test_repoll_of_empty_commit_writes_nothing(self):
        # Stored patch_id=None entries must still dedup on re-poll (via
        # the recorded commit sha), or every --from-range poll re-floods
        # the review log with the same empty commit's entries.
        commits = {
            "eee111": {"body": "cherry: empty A\n\nFormat: JPEG\nTag: EXIF:A\n",
                       "subject": "cherry: empty A", "patch_id": None},
        }
        from log_sweep_review import append_from_commits
        with tempfile.TemporaryDirectory() as tmpdir:
            log = Path(tmpdir) / "log.jsonl"
            git = make_fake_git(commits)
            append_from_commits(log, ["eee111"], git)
            written, skipped = append_from_commits(log, ["eee111"], git)
            lines = log.read_text().splitlines()
        self.assertEqual(written, [])
        self.assertEqual(skipped, ["eee111"])
        self.assertEqual(len(lines), 1)

    def test_from_commit_never_touches_landed_log(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            main(
                ["--from-commit", "aaa111", "--verdict", "human_accepted",
                 "--reason", "spot-checked", "--home", tmpdir],
                run_git=make_fake_git(self._commits()),
            )
            self.assertFalse((Path(tmpdir) / "logs" / "landed-tags.log").exists())

    def test_from_commit_and_revert_are_mutually_exclusive(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            with self.assertRaises(SystemExit):
                main(["--from-commit", "aaa111", "--revert", "bbb222",
                      "--home", tmpdir], run_git=make_fake_git(self._commits()))

    def test_legacy_mode_still_requires_verdict(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            with self.assertRaises(SystemExit):
                main(["--format", "NEF", "--tag", "A", "--reason", "r",
                      "--home", tmpdir])


if __name__ == "__main__":
    unittest.main()
