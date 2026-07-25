"""Hermetic tests for scripts/distill_lessons.py (spec K1/K2/K3).

Everything runs in tempdirs with injected clocks, kill fns and replace
fns -- no network, no cargo, no real ~/.oxidex. Run from scripts/:

    uv run python -m unittest test_distill_lessons -v
"""
import json
import os
import tempfile
import unittest
from pathlib import Path

from distill_lessons import (
    PITFALLS_BULLET_CAP,
    PITFALLS_CHAR_CAP,
    append_lesson,
    distill_once,
    encode_lesson_line,
    fingerprint_generic,
    fingerprint_scoped,
    main,
    make_lesson,
    migrate_format_memory,
)

NOW = 1_784_900_000  # fixed fake clock for every test


class KillRecorder:
    """Injected kill fn: records (pid, signum) instead of signalling."""

    def __init__(self):
        self.calls = []

    def __call__(self, pid, signum):
        self.calls.append((pid, signum))


def make_event(module="Canon.pm", event="wrong_value",
               reason="PrintConv strings must match Perl byte-for-byte",
               tag_key="JPEG:MakerNotes:AELButton", ts="2026-07-24T10:00:00",
               checklist_id="", fmt="JPEG"):
    return {
        "ts": ts, "worker": "w1", "format": fmt, "module": module,
        "table": "Main", "tag_key": tag_key, "event": event,
        "reason": reason, "evidence": "", "checklist_id": checklist_id,
    }


def ledger_path(home):
    return Path(home) / "logs" / "lessons.jsonl"


def knowledge(home):
    return Path(home) / "logs" / "knowledge"


def write_ledger(home, events, partial_tail=b""):
    """Write events as complete JSONL lines plus an optional un-terminated
    tail fragment (simulates a writer caught mid-append)."""
    path = ledger_path(home)
    path.parent.mkdir(parents=True, exist_ok=True)
    data = b"".join(json.dumps(e).encode() + b"\n" for e in events) + partial_tail
    path.write_bytes(data)
    return len(data)


def run(home, **kwargs):
    kwargs.setdefault("now_fn", lambda: NOW)
    kwargs.setdefault("kill_fn", KillRecorder())
    kwargs.setdefault("script_sha", "test-sha")
    return distill_once(home, **kwargs)


def cursor_value(home):
    return int((knowledge(home) / "distiller.cursor").read_text())


class CursorTests(unittest.TestCase):
    def test_cursor_advances_only_past_complete_lines(self):
        with tempfile.TemporaryDirectory() as home:
            complete = [make_event(reason="alpha lesson")]
            partial = json.dumps(make_event(module="Nikon.pm", reason="bravo lesson")).encode()
            head, tail = partial[:-10], partial[-10:] + b"\n"
            write_ledger(home, complete, partial_tail=head)
            complete_len = len(json.dumps(complete[0]).encode()) + 1

            result = run(home)
            self.assertEqual(result["status"], "ok")
            self.assertEqual(cursor_value(home), complete_len)
            self.assertTrue((knowledge(home) / "modules" / "Canon.md").exists())
            self.assertFalse((knowledge(home) / "modules" / "Nikon.md").exists())

            # Complete the torn line by appending its remainder; a second
            # pass consumes it and the cursor reaches EOF.
            with ledger_path(home).open("ab") as f:
                f.write(tail)
            result = run(home)
            self.assertEqual(cursor_value(home), ledger_path(home).stat().st_size)
            self.assertEqual(result["events_applied"], 1)
            self.assertIn("bravo lesson",
                          (knowledge(home) / "modules" / "Nikon.md").read_text())

    def test_cursor_not_advanced_when_output_replace_fails(self):
        def failing_replace(src, dst):
            raise RuntimeError("simulated replace failure")

        with tempfile.TemporaryDirectory() as home:
            write_ledger(home, [make_event()])
            with self.assertRaises(RuntimeError):
                run(home, replace_fn=failing_replace)
            self.assertFalse((knowledge(home) / "distiller.cursor").exists())
            # Recovery: a later pass with a working replace applies the
            # replayed batch exactly once.
            result = run(home)
            self.assertEqual(result["status"], "ok")
            self.assertIn("wrong_value x1",
                          (knowledge(home) / "modules" / "Canon.md").read_text())

    def test_reprocessing_is_idempotent_by_line_hash(self):
        with tempfile.TemporaryDirectory() as home:
            write_ledger(home, [make_event(), make_event(ts="2026-07-24T11:00:00")])
            run(home)
            canon = (knowledge(home) / "modules" / "Canon.md").read_text()
            self.assertIn("wrong_value x2", canon)
            # Simulate a crash after outputs+state but before the cursor
            # write: rewind the cursor and re-run over the same bytes.
            (knowledge(home) / "distiller.cursor").write_text("0")
            result = run(home)
            self.assertEqual(result["events_applied"], 0)
            self.assertEqual((knowledge(home) / "modules" / "Canon.md").read_text(), canon)
            self.assertEqual(cursor_value(home), ledger_path(home).stat().st_size)


class LedgerToleranceTests(unittest.TestCase):
    def test_malformed_lines_are_skipped_but_consumed(self):
        with tempfile.TemporaryDirectory() as home:
            path = ledger_path(home)
            path.parent.mkdir(parents=True, exist_ok=True)
            lines = [
                b"this is not json at all",
                b"[1, 2, 3]",  # json, but not a dict
                json.dumps({"reason": "no event field"}).encode(),
                json.dumps(make_event(reason="the one good line")).encode(),
                b"\xff\xfe binary garbage",
            ]
            path.write_bytes(b"\n".join(lines) + b"\n")
            result = run(home)
            self.assertEqual(result["status"], "ok")
            self.assertEqual(result["events_applied"], 1)
            self.assertEqual(cursor_value(home), path.stat().st_size)
            self.assertIn("the one good line",
                          (knowledge(home) / "modules" / "Canon.md").read_text())

    def test_infra_events_are_excluded_everywhere(self):
        with tempfile.TemporaryDirectory() as home:
            events = [
                make_event(event="infra", module="Canon.pm", reason="HTTP 429"),
                make_event(event="infra", module="Nikon.pm", reason="HTTP 429"),
                make_event(event="infra", module="Sony.pm", reason="HTTP 429"),
            ]
            write_ledger(home, events)
            result = run(home)
            self.assertEqual(result["status"], "ok")
            self.assertEqual(result["events_applied"], 0)
            modules_dir = knowledge(home) / "modules"
            self.assertTrue(not modules_dir.exists() or not list(modules_dir.iterdir()))
            self.assertFalse((knowledge(home) / "GLOBAL-PITFALLS.md").exists())
            # Cursor still passes the excluded lines.
            self.assertEqual(cursor_value(home), ledger_path(home).stat().st_size)


class PromotionTests(unittest.TestCase):
    def test_three_occurrences_across_two_modules_promotes(self):
        with tempfile.TemporaryDirectory() as home:
            reason = "PrintConv strings must match Perl byte-for-byte"
            write_ledger(home, [
                make_event(module="Canon.pm", reason=reason, ts="2026-07-22T09:00:00"),
                make_event(module="Canon.pm", reason=reason, ts="2026-07-23T09:00:00"),
                make_event(module="Minolta.pm", reason=reason, ts="2026-07-24T09:00:00"),
            ])
            run(home)
            text = (knowledge(home) / "GLOBAL-PITFALLS.md").read_text()
            self.assertIn("wrong_value x3 (Canon.pm, Minolta.pm)", text)
            self.assertIn(reason, text)
            self.assertIn("last: JPEG:MakerNotes:AELButton 2026-07-24", text)

    def test_three_occurrences_single_module_not_promoted(self):
        with tempfile.TemporaryDirectory() as home:
            write_ledger(home, [make_event(ts=f"2026-07-2{i}T09:00:00") for i in range(3)])
            run(home)
            self.assertFalse((knowledge(home) / "GLOBAL-PITFALLS.md").exists())
            # ...but the module playbook still shows the cluster.
            self.assertIn("wrong_value x3",
                          (knowledge(home) / "modules" / "Canon.md").read_text())

    def test_two_occurrences_two_modules_not_promoted(self):
        with tempfile.TemporaryDirectory() as home:
            write_ledger(home, [
                make_event(module="Canon.pm"),
                make_event(module="Nikon.pm"),
            ])
            run(home)
            self.assertFalse((knowledge(home) / "GLOBAL-PITFALLS.md").exists())

    def test_explicit_fingerprint_generic_clusters_across_reasons(self):
        with tempfile.TemporaryDirectory() as home:
            shared_fp = "f" * 40
            a = make_event(module="Canon.pm", reason="first phrasing")
            b = make_event(module="Nikon.pm", reason="second phrasing")
            a["fingerprint_generic"] = shared_fp
            b["fingerprint_generic"] = shared_fp
            write_ledger(home, [a, b])
            run(home)
            self.assertIn("wrong_value x2 (Canon.pm, Nikon.pm)",
                          (knowledge(home) / "modules" / "Canon.md").read_text())


class ModulePlaybookTests(unittest.TestCase):
    def test_bullets_render_newest_first(self):
        with tempfile.TemporaryDirectory() as home:
            write_ledger(home, [
                make_event(reason="older lesson text", ts="2026-07-20T09:00:00"),
                make_event(event="gap_not_closed", reason="newer lesson text",
                           ts="2026-07-24T09:00:00"),
            ])
            run(home)
            text = (knowledge(home) / "modules" / "Canon.md").read_text()
            self.assertLess(text.index("newer lesson text"), text.index("older lesson text"))

    def test_module_file_capped_at_4000_chars(self):
        with tempfile.TemporaryDirectory() as home:
            events = [make_event(reason=f"{chr(97 + i)} lesson " + "y" * 200)
                      for i in range(26)]
            write_ledger(home, events)
            run(home)
            text = (knowledge(home) / "modules" / "Canon.md").read_text()
            self.assertLessEqual(len(text), 4000)

    def test_format_used_as_fallback_module_key(self):
        with tempfile.TemporaryDirectory() as home:
            ev = make_event(reason="no module attribution", fmt="CR2")
            ev["module"] = ""
            write_ledger(home, [ev])
            run(home)
            self.assertIn("no module attribution",
                          (knowledge(home) / "modules" / "CR2.md").read_text())


class GlobalPitfallsCurationTests(unittest.TestCase):
    def seeded_file(self, home, candidates):
        pitfalls = knowledge(home) / "GLOBAL-PITFALLS.md"
        pitfalls.parent.mkdir(parents=True, exist_ok=True)
        lines = ["# Global pitfalls", "",
                 "- [seed] Seed lesson one, human curated.",
                 "- [seed] Seed lesson two, human curated."]
        lines += [f"- old candidate {i:02d}" for i in range(1, candidates + 1)]
        pitfalls.write_text("\n".join(lines) + "\n")
        return pitfalls

    def promote(self, home, reasons, module_pair=("A.pm", "B.pm")):
        # Distinct ts per event: byte-identical lines would (correctly) be
        # collapsed by the line-hash dedupe.
        events = []
        for reason in reasons:
            for i in range(3):
                events.append(make_event(
                    module=module_pair[i % 2], event="structural", reason=reason,
                    tag_key="", ts=f"2026-07-2{i + 1}T09:00:00"))
        write_ledger(home, events)

    def test_bullet_cap_drops_oldest_candidates_never_seeds(self):
        with tempfile.TemporaryDirectory() as home:
            pitfalls = self.seeded_file(home, candidates=11)  # 2 seeds + 11 = 13
            self.promote(home, ["alpha alpha lesson", "bravo bravo lesson",
                                "charlie charlie lesson"])
            run(home)
            text = pitfalls.read_text()
            bullets = [l for l in text.splitlines() if l.startswith("- ")]
            self.assertLessEqual(len(bullets), PITFALLS_BULLET_CAP)
            self.assertIn("[seed] Seed lesson one", text)
            self.assertIn("[seed] Seed lesson two", text)
            for gone in ("old candidate 01", "old candidate 02",
                         "old candidate 03", "old candidate 04"):
                self.assertNotIn(gone, text)
            self.assertIn("old candidate 05", text)
            for kept in ("alpha alpha lesson", "bravo bravo lesson",
                         "charlie charlie lesson"):
                self.assertIn(kept, text)

    def test_char_cap_drops_oldest_candidates_first(self):
        words = ["alpha", "bravo", "charlie", "delta", "echo",
                 "foxtrot", "golf", "hotel", "india", "juliet"]
        with tempfile.TemporaryDirectory() as home:
            pitfalls = knowledge(home) / "GLOBAL-PITFALLS.md"
            pitfalls.parent.mkdir(parents=True, exist_ok=True)
            pitfalls.write_text(
                "# Global pitfalls\n\n"
                "- [seed] " + "Seed one. " * 20 + "\n"
                "- [seed] " + "Seed two. " * 20 + "\n")
            self.promote(home, [f"{w} " + "z" * 230 for w in words])
            run(home)
            text = pitfalls.read_text()
            self.assertLessEqual(len(text), PITFALLS_CHAR_CAP)
            self.assertEqual(text.count("[seed]"), 2)
            self.assertNotIn("alpha zzz", text)      # oldest candidate evicted
            self.assertIn("juliet zzz", text)        # newest survives

    def test_unchanged_content_is_not_rewritten_and_history_kept_on_change(self):
        with tempfile.TemporaryDirectory() as home:
            reason = "same mistake in canon and nikon"
            write_ledger(home, [
                make_event(module="Canon.pm", reason=reason, ts="2026-07-21T09:00:00"),
                make_event(module="Canon.pm", reason=reason, ts="2026-07-22T09:00:00"),
                make_event(module="Nikon.pm", reason=reason, ts="2026-07-23T09:00:00"),
            ])
            run(home)
            pitfalls = knowledge(home) / "GLOBAL-PITFALLS.md"
            first = pitfalls.read_text()
            self.assertIn("x3", first)
            history = knowledge(home) / "history"
            self.assertTrue(not history.exists() or not list(history.iterdir()))

            # No new events -> content hash unchanged -> no write, no history.
            result = run(home)
            self.assertNotIn(str(pitfalls), result["files_written"])
            self.assertEqual(pitfalls.read_text(), first)
            self.assertTrue(not history.exists() or not list(history.iterdir()))

            # A fourth occurrence changes the count: previous version is
            # copied to history/ and the bullet is updated IN PLACE.
            with ledger_path(home).open("ab") as f:
                f.write(json.dumps(make_event(module="Nikon.pm", reason=reason,
                                              ts="2026-07-24T12:00:00")).encode() + b"\n")
            run(home)
            text = pitfalls.read_text()
            self.assertIn("x4", text)
            self.assertNotIn("x3", text)  # updated, not duplicated
            snapshots = list(history.iterdir())
            self.assertEqual(len(snapshots), 1)
            self.assertIn("x3", snapshots[0].read_text())
            self.assertTrue(snapshots[0].name.startswith("GLOBAL-PITFALLS-"))


class MultilineReasonTests(unittest.TestCase):
    def test_newline_reason_renders_one_bullet_and_stays_stable(self):
        # A reason containing "\n- ..." (a multi-line --lesson argument
        # flowing through lessons.jsonl) must be flattened at render time:
        # otherwise split_bullets re-parses the bullet as two blocks next
        # pass, the in-place identity match misses, and the cluster is
        # APPENDED again on every pass -- plus a history snapshot each
        # time -- forever.
        reason = "first line of lesson\n- looks like another bullet"
        with tempfile.TemporaryDirectory() as home:
            write_ledger(home, [
                make_event(module="Canon.pm", reason=reason, ts="2026-07-21T09:00:00"),
                make_event(module="Canon.pm", reason=reason, ts="2026-07-22T09:00:00"),
                make_event(module="Nikon.pm", reason=reason, ts="2026-07-23T09:00:00"),
            ])
            for i in range(4):
                run(home, now_fn=lambda i=i: NOW + i)
            text = (knowledge(home) / "GLOBAL-PITFALLS.md").read_text()
            bullets = [l for l in text.splitlines() if l.startswith("- ")]
            history = knowledge(home) / "history"
            snapshots = list(history.iterdir()) if history.exists() else []
            playbook = (knowledge(home) / "modules" / "Canon.md").read_text()
        # Exactly ONE single-line bullet, flattened, no duplicate appends.
        self.assertEqual(len(bullets), 1)
        self.assertIn("first line of lesson - looks like another bullet", bullets[0])
        # Passes 2-4 saw no new events: content unchanged, no snapshots.
        self.assertEqual(snapshots, [])
        # Module playbooks flatten the same way (one "- " line per cluster).
        playbook_bullets = [l for l in playbook.splitlines() if l.startswith("- ")]
        self.assertEqual(len(playbook_bullets), 1)


class LockTests(unittest.TestCase):
    def write_lock(self, home, pid=4242, sha="test-sha", heartbeat_ts=NOW - 30):
        lock = knowledge(home) / "distiller.lock"
        lock.parent.mkdir(parents=True, exist_ok=True)
        lock.write_text(json.dumps(
            {"pid": pid, "script_git_sha": sha, "heartbeat_ts": heartbeat_ts}))
        return lock

    def test_fresh_heartbeat_matching_sha_exits_quietly(self):
        with tempfile.TemporaryDirectory() as home:
            write_ledger(home, [make_event()])
            lock = self.write_lock(home)
            kill = KillRecorder()
            result = run(home, kill_fn=kill, pid=9999)
            self.assertEqual(result["status"], "already_running")
            self.assertEqual(kill.calls, [])
            self.assertFalse((knowledge(home) / "distiller.cursor").exists())
            self.assertTrue(lock.exists())  # holder's lock left untouched

    def test_stale_heartbeat_sigterms_holder_and_takes_over(self):
        import signal as signal_mod
        with tempfile.TemporaryDirectory() as home:
            write_ledger(home, [make_event()])
            self.write_lock(home, heartbeat_ts=NOW - 3600)
            kill = KillRecorder()
            result = run(home, kill_fn=kill, pid=9999)
            self.assertEqual(result["status"], "ok")
            self.assertEqual(kill.calls, [(4242, signal_mod.SIGTERM)])
            self.assertTrue((knowledge(home) / "modules" / "Canon.md").exists())
            # Our own lock is released on clean exit.
            self.assertFalse((knowledge(home) / "distiller.lock").exists())

    def test_sha_mismatch_sigterms_holder_even_when_fresh(self):
        import signal as signal_mod
        with tempfile.TemporaryDirectory() as home:
            write_ledger(home, [make_event()])
            self.write_lock(home, sha="outdated-sha", heartbeat_ts=NOW - 30)
            kill = KillRecorder()
            result = run(home, kill_fn=kill, pid=9999)
            self.assertEqual(result["status"], "ok")
            self.assertEqual(kill.calls, [(4242, signal_mod.SIGTERM)])

    def test_dead_holder_pid_is_tolerated(self):
        with tempfile.TemporaryDirectory() as home:
            write_ledger(home, [make_event()])
            self.write_lock(home, heartbeat_ts=NOW - 3600)

            def kill_dead(pid, signum):
                raise ProcessLookupError(pid)

            result = run(home, kill_fn=kill_dead, pid=9999)
            self.assertEqual(result["status"], "ok")


class MigrationTests(unittest.TestCase):
    def test_migration_drops_noise_converts_bullets_and_archives(self):
        with tempfile.TemporaryDirectory() as home:
            memory_dir = Path(home) / "logs" / "format-memory"
            memory_dir.mkdir(parents=True)
            (memory_dir / "CR2.md").write_text(
                "- Repeated model/API rate limiting (HTTP 429) blocked work on CR2 tags.\n"
                "- [2026-07-23] FAILED CR2:EXIF:Copyright: model call failed: "
                "HTTP Error 429: Too Many Requests\n"
                "- Requests kept timing out; use backoff.\n"
                "- Verify a proposed implementation actually reduces the gap "
                "count before further retries.\n"
                "- [2026-07-24] GPSVersionID needs the byte-array PrintConv from "
                "the GPS table, not a string join.\n")
            result = migrate_format_memory(
                Path(home), now_fn=lambda: NOW, kill_fn=KillRecorder(),
                script_sha="test-sha")

            self.assertEqual(result["status"], "ok")
            self.assertEqual(result["migrated_events"], 2)
            self.assertEqual(result["archived"], ["CR2.md"])

            lines = ledger_path(home).read_text().splitlines()
            self.assertEqual(len(lines), 2)
            for line in lines:
                ev = json.loads(line)
                self.assertEqual(ev["event"], "structural")
                self.assertEqual(ev["module"], "CR2")
                self.assertEqual(ev["format"], "CR2")
                self.assertNotIn("429", ev["reason"])
                self.assertNotIn("timing out", ev["reason"])
            # Leading "[date]" stamps are stripped from reasons.
            self.assertTrue(any(ev.startswith('{"ts"') for ev in lines))
            reasons = [json.loads(l)["reason"] for l in lines]
            self.assertIn("GPSVersionID needs the byte-array PrintConv from "
                          "the GPS table, not a string join.", reasons)

            # Originals archived; playbook rendered under the format-name key.
            self.assertFalse((memory_dir / "CR2.md").exists())
            self.assertTrue((memory_dir / "archived" / "CR2.md").exists())
            playbook = (knowledge(home) / "modules" / "CR2.md").read_text()
            self.assertIn("structural x1", playbook)
            self.assertIn("gap count", playbook)

    def test_migration_with_no_memory_dir_still_distills(self):
        with tempfile.TemporaryDirectory() as home:
            write_ledger(home, [make_event()])
            result = migrate_format_memory(
                Path(home), now_fn=lambda: NOW, kill_fn=KillRecorder(),
                script_sha="test-sha")
            self.assertEqual(result["status"], "ok")
            self.assertEqual(result["migrated_events"], 0)
            self.assertEqual(result["archived"], [])


class LedgerContractTests(unittest.TestCase):
    def test_encoded_line_clamped_to_2000_bytes_stays_valid_json(self):
        event = make_lesson(ts=NOW, worker="w1", format_name="JPEG",
                            module="Canon.pm", event="wrong_value",
                            reason="r" * 5000, evidence="e" * 5000)
        line = encode_lesson_line(event)
        self.assertLessEqual(len(line), 2000)
        self.assertTrue(line.endswith(b"\n"))
        parsed = json.loads(line.decode())
        self.assertEqual(parsed["event"], "wrong_value")
        self.assertEqual(parsed["module"], "Canon.pm")

    def test_append_lesson_appends_one_line(self):
        with tempfile.TemporaryDirectory() as home:
            path = ledger_path(home)
            for i in range(3):
                append_lesson(path, make_lesson(
                    ts=NOW + i, worker="w1", format_name="JPEG", module="Canon.pm",
                    event="fixed", reason=f"lesson {i}"))
            lines = path.read_text().splitlines()
            self.assertEqual(len(lines), 3)
            self.assertEqual(json.loads(lines[2])["reason"], "lesson 2")

    def test_fingerprints_scoped_vs_generic(self):
        args = dict(checklist_id="", reason="Same normalized reason 42")
        self.assertEqual(fingerprint_generic("wrong_value", **args),
                         fingerprint_generic("wrong_value", **args))
        self.assertEqual(
            fingerprint_generic("wrong_value", "", "index 42 mismatch"),
            fingerprint_generic("wrong_value", "", "index 43 mismatch"),
        )  # digit runs normalize away -> clusterable across modules
        self.assertNotEqual(
            fingerprint_scoped("wrong_value", "Canon.pm", **args),
            fingerprint_scoped("wrong_value", "Nikon.pm", **args),
        )
        self.assertNotEqual(fingerprint_generic("wrong_value", **args),
                            fingerprint_generic("duplicate", **args))
        # checklist_id wins over the reason when present.
        self.assertEqual(fingerprint_generic("review_rejected", "C2", "anything"),
                         fingerprint_generic("review_rejected", "C2", "else"))


class CliTests(unittest.TestCase):
    def test_main_single_pass_returns_zero(self):
        import contextlib
        import io
        with tempfile.TemporaryDirectory() as home:
            write_ledger(Path(home), [make_event()])
            out = io.StringIO()
            with contextlib.redirect_stdout(out):
                rc = main(["--home", home, "--once", "--script-sha", "s1"])
            self.assertEqual(rc, 0)
            self.assertEqual(json.loads(out.getvalue())["status"], "ok")
            self.assertTrue((knowledge(home) / "modules" / "Canon.md").exists())

    def test_main_exits_zero_quietly_when_fresh_holder_exists(self):
        import time as time_mod
        with tempfile.TemporaryDirectory() as home:
            lock = knowledge(home) / "distiller.lock"
            lock.parent.mkdir(parents=True, exist_ok=True)
            lock.write_text(json.dumps({"pid": 424242, "script_git_sha": "s1",
                                        "heartbeat_ts": time_mod.time()}))
            self.assertEqual(main(["--home", home, "--script-sha", "s1"]), 0)
            self.assertFalse((knowledge(home) / "distiller.cursor").exists())


if __name__ == "__main__":
    unittest.main()
