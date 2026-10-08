#!/usr/bin/env python3
"""Small refusal controls for the read-only Task19 receipt adapter.

These do not claim that synthetic results pass the production replay verifier.
"""
from __future__ import annotations

from contextlib import ExitStack, redirect_stderr, redirect_stdout
import io
from pathlib import Path
import os
import subprocess
import shutil
import sys
from unittest.mock import patch
from tempfile import TemporaryDirectory
import unittest

HERE = Path(__file__).resolve().parent
if str(HERE) not in sys.path:
    sys.path.insert(0, str(HERE))

import verify_task19_results as adapter


class Task19AdapterControls(unittest.TestCase):
    def test_all_tool_hashes_must_be_explicit_and_unique(self) -> None:
        items = [f"{name}={'a' * 64}" for name in adapter.TOOL_FILES]
        self.assertEqual(len(adapter.tool_expectations(items)), len(adapter.TOOL_FILES))
        for invalid in (items[:-1], items + items[:1], ["unknown.py=" + "a" * 64] + items):
            with self.subTest(invalid=invalid), self.assertRaises(adapter.qualification.Refused):
                adapter.tool_expectations(invalid)

    def test_six_side_binary_identities_must_be_explicit(self) -> None:
        items = [f"{row}:{side}:/target/{row}/{side}/oxidex:{'b' * 64}"
                 for row in adapter.ROWS for side in adapter.qualification.SIDES]
        self.assertEqual(len(adapter.binary_expectations(items)), 6)
        for invalid in (items[:-1], items + items[:1], [items[0].replace('/target/', 'relative/')]+items[1:]):
            with self.subTest(invalid=invalid), self.assertRaises(adapter.qualification.Refused):
                adapter.binary_expectations(invalid)

    def test_next_pin_markdown_selection_and_ambiguity(self) -> None:
        labels = ("Next pin", "Intended next pin", "Intended ExifTool pin",
                  "ExifTool pin after current", "Next ExifTool release",
                  "Intended ExifTool release")
        forms = ("{label}: 13.60", "- [ ] {label}: 13.60",
                 "- **{label}:** 13.60", "- [x] **{label}:** 13.60")
        for label in labels:
            for form in forms:
                text = form.format(label=label)
                with self.subTest(text=text):
                    self.assertEqual(adapter.next_pin_selection(text), "13.60")
        for text in ("- [ ] Run one current-pin -> next-pin rehearsal",
                     "**Next pin:** not selected", "- [ ] Intended next pin: not selected",
                     "For ExifTool release 13.59, confirm archive fixtures",
                     "For current pin 13.59, confirm archive fixtures",
                     "The current pin is 13.59",
                     "- [ ] Verify ExifTool version 13.59 capability",
                     "Next release: v2.0.0-beta.1",
                     "Current ExifTool release: 13.59\nNext pin: not selected",
                     "- [ ] **Current ExifTool version:** 13.59\nNext pin: not selected",
                     "Current pin: 13.59"):
            with self.subTest(text=text):
                self.assertEqual(adapter.next_pin_selection(text), "not selected")
        for text in ("- [ ] Next pin maybe 13.60", "Next pin: 13.60\nNext pin: 13.61",
                     "- [ ] Upcoming pin: 13.60", "The intended next pin is 13.60",
                     "Potential pin: 13.60", "Upcoming ExifTool release: 13.60",
                     "The intended ExifTool release is 13.60", "Next version: 13.60", "ExifTool version: 13.60",
                     "Set pin to 13.60", "Set current pin to 13.60",
                     "Current ExifTool release: 13.60",
                     "Current ExifTool version: 13.60"):
            with self.subTest(text=text), self.assertRaises(adapter.qualification.Refused):
                adapter.next_pin_selection(text)

    def test_final_marker_identity_binds_loaded_bytes_and_rejects_swap(self) -> None:
        import json
        with TemporaryDirectory() as directory:
            marker = Path(directory) / "qualification-result.json"
            original = {"schema": 1, "run_id": "original"}
            marker.write_text(json.dumps(original))
            with adapter.pin_marker(marker) as pinned:
                binding, original_bytes = pinned.binding, pinned.data
                self.assertEqual(json.loads(original_bytes), original)
                adapter.require_marker_unchanged(marker, binding, pinned)
                replacement = Path(directory) / "replacement.json"
                replacement.write_text(json.dumps({"schema": 1, "run_id": "replacement"}))
                replacement.replace(marker)
                with self.assertRaisesRegex(adapter.qualification.Refused, "changed"):
                    adapter.require_marker_unchanged(marker, binding, pinned)
                # Keep the original descriptor open across *both* replacements.
                # The filesystem cannot recycle its inode for identical bytes.
                same = Path(directory) / "same.json"
                same.write_bytes(original_bytes)
                same.replace(marker)
                with self.assertRaisesRegex(adapter.qualification.Refused, "changed"):
                    adapter.require_marker_unchanged(marker, binding, pinned)
                descriptor = pinned.descriptor
            with self.assertRaises(OSError):
                os.fstat(descriptor)

    def test_marker_pin_read_failure_closes_owned_descriptor(self) -> None:
        with TemporaryDirectory() as directory:
            marker = Path(directory) / "qualification-result.json"
            marker.write_bytes(b'{}')
            opened = []
            original_open = adapter.os.open
            def recording_open(*args, **kwargs):
                descriptor = original_open(*args, **kwargs)
                opened.append(descriptor)
                return descriptor
            with patch.object(adapter.os, "open", side_effect=recording_open), \
                 patch.object(adapter.os, "pread", side_effect=OSError("injected read failure")):
                with self.assertRaisesRegex(adapter.qualification.Refused, "read failed"):
                    with adapter.pin_marker(marker):
                        pass
            self.assertEqual(len(opened), 1)
            with self.assertRaises(OSError):
                os.fstat(opened[0])

    def test_fifo_final_marker_refuses_without_waiting_for_writer(self) -> None:
        with TemporaryDirectory() as directory:
            fifo = Path(directory) / "qualification-result.json"
            os.mkfifo(fifo)
            with self.assertRaisesRegex(adapter.qualification.Refused, "regular file"):
                adapter.marker_snapshot(fifo)

    def test_final_marker_swap_during_loader_cannot_bind_replacement(self) -> None:
        import json
        with TemporaryDirectory() as directory:
            paths = []
            for name in ("same", "forward", "reverse"):
                parent = Path(directory) / name
                parent.mkdir()
                marker = parent / "qualification-result.json"
                marker.write_text(json.dumps({"schema": 1}))
                paths.append(marker)
            expected_binaries = {
                (row, side): {"path": f"/target/{row}/{side}", "sha256": "b" * 64}
                for row in adapter.ROWS for side in adapter.qualification.SIDES}
            expected_tools = {name: adapter.sha(adapter.ROOT / name) for name in adapter.TOOL_FILES}
            def swap(_path):
                replacement = paths[0].with_name("replacement.json")
                replacement.write_text(json.dumps({"schema": 1, "replacement": True}))
                replacement.replace(paths[0])
                return {"schema": 1}
            with patch.object(adapter.qualification, "_evidence_location", side_effect=lambda path, _label: path), \
                 patch.object(adapter.qualification, "load_committed_result", side_effect=swap):
                with self.assertRaisesRegex(adapter.qualification.Refused, "changed"):
                    adapter.verify_results(
                        paths=tuple(paths), expected_head="a" * 40, expected_tree="c" * 40,
                        expected_matrix_sha256=adapter.sha(adapter.qualification.CANONICAL_MATRIX),
                        expected_policy_sha256=adapter.POLICY_SHA256,
                        expected_tools=expected_tools, expected_binaries=expected_binaries)

    def test_output_cannot_physically_overlap_owned_source(self) -> None:
        with TemporaryDirectory() as directory:
            source = Path(directory) / "source"
            source.mkdir()
            alias = Path(directory) / "alias"
            alias.symlink_to(source, target_is_directory=True)
            with patch.object(adapter, "ROOT", source):
                for output in (source / "receipt.json", alias / "receipt.json"):
                    with self.subTest(output=output), self.assertRaisesRegex(
                            adapter.qualification.Refused, "overlaps"):
                        adapter.refuse_source_output_overlap(output)
                adapter.refuse_source_output_overlap(Path(directory) / "evidence" / "receipt.json")
                # The real case alias exists only on case-insensitive volumes.
                case_alias = source.with_name(source.name.upper())
                if case_alias.exists() and os.path.samefile(case_alias, source):
                    with self.assertRaisesRegex(adapter.qualification.Refused, "overlaps"):
                        adapter.refuse_source_output_overlap(case_alias / "receipt.json")
                # A synthetic inode alias exercises the identity check on Linux.
                bind_alias = Path(directory) / "bind-alias"
                bind_alias.mkdir()
                original_samefile = os.path.samefile
                with patch.object(adapter.os.path, "samefile", side_effect=lambda left, right:
                                  left == bind_alias or original_samefile(left, right)):
                    with self.assertRaisesRegex(adapter.qualification.Refused, "overlaps"):
                        adapter.refuse_source_output_overlap(bind_alias / "receipt.json")

    def test_cli_rejects_source_output_before_replay_or_write(self) -> None:
        with TemporaryDirectory() as directory:
            repo = Path(directory)
            subprocess.run(["git", "init", "-q", str(repo)], check=True)
            subprocess.run(["git", "-C", str(repo), "config", "user.name", "test"], check=True)
            subprocess.run(["git", "-C", str(repo), "config", "user.email", "test@example.invalid"], check=True)
            (repo / ".exiftool-version").write_text("13.59\n")
            (repo / "TODO_RELEASE_BETA.md").write_text("Next pin: not selected\n")
            subprocess.run(["git", "-C", str(repo), "add", ".exiftool-version", "TODO_RELEASE_BETA.md"], check=True)
            subprocess.run(["git", "-C", str(repo), "commit", "-qm", "fixture"], check=True)
            head = subprocess.check_output(["git", "-C", str(repo), "rev-parse", "HEAD"], text=True).strip()
            tree = subprocess.check_output(["git", "-C", str(repo), "rev-parse", "HEAD^{tree}"], text=True).strip()
            output = repo / "receipt.json"
            argv = ["--same-pin-result", "/missing/same/qualification-result.json",
                    "--forward-result", "/missing/forward/qualification-result.json",
                    "--reverse-result", "/missing/reverse/qualification-result.json",
                    "--expected-head", head, "--expected-tree", tree,
                    "--expected-matrix-sha256", "a" * 64,
                    "--expected-read-policy-sha256", adapter.POLICY_SHA256,
                    "--next-pin", "not-selected", "--output", str(output)]
            for name in adapter.TOOL_FILES:
                argv.extend(["--expected-tool", f"{name}={'b' * 64}"])
            for row in adapter.ROWS:
                for side in adapter.qualification.SIDES:
                    argv.extend(["--expected-binary", f"{row}:{side}:/target/oxidex:{'c' * 64}"])
            with patch.object(adapter, "ROOT", repo), \
                 patch.object(adapter, "require_imported_source_paths"), \
                 patch.object(adapter.qualification, "_evidence_location", side_effect=lambda path, _label: path), \
                 patch.object(adapter, "verify_results") as replay, redirect_stderr(io.StringIO()) as error:
                self.assertEqual(adapter.main(argv), 2)
                self.assertIn("overlaps", error.getvalue())
                replay.assert_not_called()
            self.assertFalse(output.exists())

    def test_durable_result_path_refuses_temporary_evidence(self) -> None:
        with TemporaryDirectory() as directory:
            marker = Path(directory) / "qualification-result.json"
            marker.write_text("{}")
            with self.assertRaisesRegex(adapter.qualification.Refused, "durable"):
                adapter.qualification._evidence_location(marker, "Task19 committed result")

    def test_subordinate_inventory_detects_earlier_run_mutation(self) -> None:
        with TemporaryDirectory() as directory:
            roots = []
            for label in ("same", "forward", "reverse"):
                run = Path(directory) / label
                run.mkdir()
                marker = run / "qualification-result.json"
                marker.write_text('{}')
                roots.append(marker)
            earlier = roots[0].parent / "execution-status.json"
            earlier.write_text('{"state":"complete"}')
            policy = Path(directory) / "policy.json"
            policy.write_text('{}')
            value = {"rows": {}, "read_policy_input": {"path": str(policy)}}
            with patch.object(adapter.qualification, "_evidence_location", side_effect=lambda path, _label: path):
                before = adapter.subordinate_snapshot(tuple(roots), value)
                self.assertEqual(adapter.subordinate_snapshot(tuple(roots), value), before)
                earlier.write_text('{"state":"tampered"}')
                self.assertNotEqual(adapter.subordinate_snapshot(tuple(roots), value), before)

    def test_real_selected_extra_source_tree_closes_publication_inventory(self) -> None:
        import json
        import test_version_rehearsal_catalog as catalog_fixture
        import test_version_rehearsal_native_oracle as native_fixture

        shared = adapter.qualification
        ops_root = shared.ops_paths.ops_root()
        ops_root.mkdir(parents=True, exist_ok=True)
        with TemporaryDirectory(dir=ops_root, prefix="task19-tree-control-") as directory:
            root = Path(directory)
            capture = shared.catalog_stage.capture_tag_catalog(
                catalog_fixture.FixtureGet(catalog_fixture.complete_responses()),
                "2026-09-13T00:00:00Z")
            catalog = shared.rehearsal.normalize_catalog(
                shared.catalog_stage.raw_catalog_from_capture(capture))
            plan = shared.rehearsal.make_plan(catalog, 3, 0, 1, "e" * 40)
            selected = {item["release"]: item for pair in plan["pairs"]
                        for item in (pair["old"], pair["new"])}
            self.assertEqual(set(selected), {"13.57", "13.59"})
            responses = {
                shared.catalog_stage.immutable_archive_url(release, item["peeled_commit"]):
                catalog_fixture.response(native_fixture.archive_bytes(release))
                for release, item in selected.items()}
            cache, sources = root / "cache", root / "sources"
            resolution = shared.catalog_stage.resolve_selected_archives(
                plan, catalog, capture, catalog_fixture.FixtureGet(responses), cache)
            materialization = shared.catalog_stage.materialize_selected_sources(
                plan, catalog, capture, resolution, cache, sources)
            bundle = root / "provisioned" / "13.59"
            bundle.mkdir(parents=True)
            for name, document in zip(shared.INPUT_NAMES,
                                       (capture, catalog, plan, resolution, materialization), strict=True):
                (bundle / f"{name}.json").write_text(json.dumps(document))
            (bundle / "locations.json").write_text(json.dumps({
                "schema": 1, "kind": "oxidex_version_transition_input_locations",
                "archive_cache": str(cache), "source_root": str(sources)}))
            identity = shared.resolve_source_identity({"expected_release": "13.59"}, bundle)
            self.assertEqual(len(identity["materialized_trees"]), 2)
            extra = next(item for item in materialization["selected_releases"]
                         if item["release"] == "13.57")
            extra_dir = sources / extra["source_directory"]
            extra_file = extra_dir / "exiftool"
            original = extra_file.read_bytes()
            paths = []
            for number in range(3):
                run = root / f"run{number}"
                run.mkdir()
                marker = run / "qualification-result.json"
                marker.write_text("{}\n")
                paths.append(marker)
            policy, perl, binary = (root / name for name in ("policy.json", "perl", "oxidex"))
            for path in (policy, perl, binary):
                path.write_text("{}\n")
            native = sources / identity["source_directory"]
            source_fields = ("release", "tag_object", "peeled_commit", "source_directory",
                             "source_tree_sha256", "materialization_sha256")
            proof = {
                "source_identity": {key: identity[key] for key in source_fields},
                "source_root": identity["source_root"],
                "source_dependencies": identity["dependencies"],
                "materialized_trees": identity["materialized_trees"],
                "fixture_dependencies": [shared._source_dependency(policy, "fixture", 1024)],
                "native_identity": {"source": {"path": str(native)},
                                    "lib": {"path": str(native / "lib")},
                                    "perl": {"path": str(perl)}},
                "committed_write": {"writer_binary": {"path": str(binary)}}}
            value = {"status": "verified_read_only", "read_policy_input": {"path": str(policy)},
                     "rows": {"same-pin-13.59": {"sides": {side: proof for side in shared.SIDES},
                                               "binaries": {side: {"path": str(binary)}
                                                            for side in shared.SIDES}}}}
            for window in ("payload-write", "directory-fsync"):
                for change in ("mutate", "add", "remove", "empty-directory", "mode", "symlink"):
                    with self.subTest(window=window, change=change):
                        baseline = adapter.subordinate_snapshot(tuple(paths), value)
                        self.assertIn(str(extra_file), baseline)
                        output = root / f"{window}-{change}" / "receipt.json"
                        extra_added = extra_dir / "added.txt"
                        extra_empty = extra_dir / "added-empty"
                        extra_link = extra_dir / "added-link"
                        original_mode = extra_file.stat().st_mode & 0o777

                        def alter() -> None:
                            if change == "mutate":
                                extra_file.write_bytes(original + b"changed")
                            elif change == "add":
                                extra_added.write_bytes(b"new")
                            elif change == "remove":
                                extra_file.unlink()
                            elif change == "empty-directory":
                                extra_empty.mkdir()
                            elif change == "mode":
                                extra_file.chmod(original_mode ^ 0o100)
                            else:
                                extra_link.symlink_to("exiftool")

                        calls = 0
                        altered = False
                        def validate() -> None:
                            nonlocal calls
                            calls += 1
                            if adapter.subordinate_snapshot(tuple(paths), value) != baseline:
                                raise shared.Refused("selected source tree changed")

                        original_write = adapter._write_owned_state
                        original_sync = shared._fsync_directory
                        def write_then_alter(descriptor: int, payload: bytes) -> None:
                            original_write(descriptor, payload)
                            if window == "payload-write" and b'verified_read_only' in payload:
                                alter()

                        def sync_then_alter(path: Path) -> None:
                            nonlocal altered
                            original_sync(path)
                            if window == "directory-fsync" and not altered:
                                altered = True
                                alter()

                        try:
                            with patch.object(adapter, "_write_owned_state", side_effect=write_then_alter), \
                                 patch.object(shared, "_fsync_directory", side_effect=sync_then_alter):
                                with self.assertRaisesRegex(shared.Refused, "source tree changed"):
                                    adapter.publish_receipt_no_replace(
                                        output, value, validate_inputs=validate)
                            self.assertEqual(calls, 2)
                            self.assertEqual(json.loads(output.read_text())["status"],
                                             "publication_failed")
                        finally:
                            if change == "add":
                                extra_added.unlink(missing_ok=True)
                            elif change == "empty-directory":
                                extra_empty.rmdir()
                            elif change == "mode":
                                extra_file.chmod(original_mode)
                            elif change == "symlink":
                                extra_link.unlink(missing_ok=True)
                            else:
                                extra_file.write_bytes(original)
                        self.assertEqual(shared.resolve_source_identity(
                            {"expected_release": "13.59"}, bundle)["materialized_trees"],
                                         identity["materialized_trees"])

    def test_no_replace_publication_and_failed_durability_retract(self) -> None:
        with TemporaryDirectory() as directory:
            output = Path(directory) / "receipt.json"
            output.write_text('{"owner":"foreign"}\n')
            with self.assertRaisesRegex(adapter.qualification.Refused, "already exists"):
                adapter.publish_receipt_no_replace(output, {"status": "verified_read_only"})
            self.assertEqual(output.read_text(), '{"owner":"foreign"}\n')
            output.unlink()
            def publish_foreign(_directory):
                foreign = output.with_name("foreign.json")
                foreign.write_text('{"owner":"racer"}\n')
                os.replace(foreign, output)
                raise OSError("simulated post-publication fsync failure")
            with patch.object(adapter.qualification, "_fsync_directory", side_effect=publish_foreign):
                with self.assertRaises(OSError):
                    adapter.publish_receipt_no_replace(output, {"status": "verified_read_only"})
            self.assertEqual(output.read_text(), '{"owner":"racer"}\n')
            output.unlink()
            with patch.object(adapter.qualification, "_fsync_directory", side_effect=OSError("disk")):
                with self.assertRaises(OSError):
                    adapter.publish_receipt_no_replace(output, {"status": "verified_read_only"})
            self.assertIn('publication_failed', output.read_text())
            output.unlink()
            adapter.publish_receipt_no_replace(output, {"status": "verified_read_only"})
            self.assertIn('verified_read_only', output.read_text())

    def test_publication_marker_recheck_invalidates_owned_success(self) -> None:
        with TemporaryDirectory() as directory:
            output = Path(directory) / "receipt.json"
            calls = 0
            def changed_marker():
                nonlocal calls
                calls += 1
                if calls == 2:
                    raise adapter.qualification.Refused("marker changed before publication")
            with self.assertRaisesRegex(adapter.qualification.Refused, "marker changed"):
                adapter.publish_receipt_no_replace(
                    output, {"status": "verified_read_only"}, validate_inputs=changed_marker)
            self.assertEqual(calls, 2)
            self.assertEqual(adapter.json.loads(output.read_text())["status"], "publication_failed")

    def test_final_output_mode_and_link_custody_refuse(self) -> None:
        with TemporaryDirectory() as directory:
            for fault in ("mode", "link"):
                output = Path(directory) / f"{fault}.json"
                retained = Path(directory) / f"{fault}-retained.json"
                calls = 0
                def alter_on_final_validation():
                    nonlocal calls
                    calls += 1
                    if calls == 2:
                        if fault == "mode":
                            output.chmod(0o644)
                        else:
                            os.link(output, retained)
                with self.assertRaisesRegex(adapter.qualification.Refused, "custody"):
                    adapter.publish_receipt_no_replace(
                        output, {"status": "verified_read_only"},
                        validate_inputs=alter_on_final_validation)
                self.assertEqual(calls, 2)
                self.assertEqual(adapter.json.loads(output.read_text())["status"], "publication_failed")
                if fault == "link":
                    self.assertEqual(adapter.json.loads(retained.read_text())["status"], "publication_failed")

    def test_final_output_bytes_refuse_same_inode_changes(self) -> None:
        with TemporaryDirectory() as directory:
            for window in ("final-validation", "input-close", "primary-close"):
                for change in ("overwrite", "truncate", "extend"):
                    with self.subTest(window=window, change=change):
                        output = Path(directory) / f"{window}-{change}.json"
                        original_close = os.close
                        validation_calls = 0
                        changed = False

                        def mutate() -> None:
                            nonlocal changed
                            before = output.stat()
                            with output.open("r+b", buffering=0) as stream:
                                if change == "overwrite":
                                    stream.seek(0)
                                    stream.write(b"X")
                                elif change == "truncate":
                                    stream.truncate(1)
                                else:
                                    stream.seek(0, os.SEEK_END)
                                    stream.write(b"X")
                            changed = (before.st_dev, before.st_ino) == (
                                output.stat().st_dev, output.stat().st_ino)

                        def validate() -> None:
                            nonlocal validation_calls
                            validation_calls += 1
                            if validation_calls == 2 and window == "final-validation":
                                mutate()

                        def close_inputs() -> None:
                            if window == "input-close":
                                mutate()

                        def close_then_mutate(descriptor: int) -> None:
                            is_output = os.fstat(descriptor).st_ino == output.stat().st_ino
                            original_close(descriptor)
                            if window == "primary-close" and is_output and not changed:
                                mutate()

                        with patch.object(adapter.os, "close", side_effect=close_then_mutate):
                            with self.assertRaisesRegex(adapter.qualification.Refused,
                                                        "output bytes changed"):
                                adapter.publish_receipt_no_replace(
                                    output, {"status": "verified_read_only", "owner": "expected"},
                                    validate_inputs=validate, close_inputs=close_inputs)
                        self.assertTrue(changed)
                        self.assertEqual(validation_calls, 2)
                        self.assertEqual(adapter.json.loads(output.read_text())["status"],
                                         "publication_failed")

    def test_shared_fixture_closure_names_external_authorities(self) -> None:
        import json
        shared = adapter.qualification
        with TemporaryDirectory() as directory:
            root = Path(directory)
            read_file = root / "read.bin"
            read_file.write_bytes(b"read")
            write_file = root / "write.jpg"
            write_file.write_bytes(b"\xff\xd8write")
            native_file = root / "native.bin"
            native_file.write_bytes(b"native")
            def manifest(name, kind, fixture):
                path = root / name
                path.write_text(json.dumps({"schema": 1, "kind": kind, "fixtures": [{
                    "path": str(fixture), "sha256": adapter.sha(fixture),
                    "bytes": fixture.stat().st_size}]}))
                return path
            read_manifest = manifest("read.json", "oxidex_version_rehearsal_fixture_manifest", read_file)
            write_manifest = manifest("write.json", "oxidex_version_rehearsal_write_fixture_manifest", write_file)
            read_binding = shared.executor._fixture_binding(str(read_manifest),
                kind="oxidex_version_rehearsal_fixture_manifest", jpeg_only=False)
            write_binding = shared.executor._write_fixture_binding(str(write_manifest))
            run_dir = root / "run"
            (run_dir / "inputs").mkdir(parents=True)
            (run_dir / "inputs" / "config.json").write_text(json.dumps({
                "read_fixture_manifests": {"13.59": str(read_manifest)},
                "read_fixture_bindings": {"13.59": read_binding},
                "write_fixture_manifests": {"13.59": str(write_manifest)},
                "write_fixture_bindings": {"13.59": write_binding},
                "native_cases": {"13.59": [{"name": "fixture"}]}}))
            corpus_manifest = root / "combined-samples.manifest"
            corpus_manifest.write_text("fixture\n")
            storage = root / "evidence/20260919-beta1-functional/durable-controller-oracle-bootstrap/storage-manifest.json"
            storage.parent.mkdir(parents=True)
            storage.write_text("{}\n")
            authority = {"ops_root": str(root), "bootstrap_pin": "13.59",
                         "corpus": str(root / "combined-samples"),
                         "corpus_tree_sha256": "a" * 64,
                         "manifest": {"path": str(corpus_manifest),
                                      "sha256": adapter.sha(corpus_manifest), "file_count": 1}}
            row = {"read_union": {"original_manifests": {
                "before": read_binding, "after": read_binding}}}
            with ExitStack() as patches:
                patches.enter_context(patch.object(shared, "_evidence_location",
                    side_effect=lambda path, _label: Path(path).resolve()))
                patches.enter_context(patch.object(shared, "_fixture_corpus_authority",
                    return_value=authority))
                patches.enter_context(patch.object(shared, "_native_fixture_bindings",
                    return_value=[{"path": str(native_file)}]))
                patches.enter_context(patch.object(shared.ops_paths, "ops_root", return_value=root))
                dependencies = shared.replay_fixture_dependencies(
                    row, "before", run_dir, "13.59", {"fixture_corpus": authority})
            self.assertEqual({item["path"] for item in dependencies}, {
                *(str(path.resolve()) for path in (read_manifest, read_file, write_manifest,
                    write_file, native_file, corpus_manifest, storage))})
            self.assertTrue(all(item["sha256"] == adapter.sha(Path(item["path"]))
                                for item in dependencies))

    def test_publication_invalidation_failure_preserves_outcome_unknown(self) -> None:
        with TemporaryDirectory() as directory:
            output = Path(directory) / "receipt.json"
            original_truncate = os.ftruncate
            calls = 0
            def fail_invalidation(descriptor, length):
                nonlocal calls
                calls += 1
                if calls == 3:
                    raise OSError("cannot invalidate")
                return original_truncate(descriptor, length)
            with patch.object(adapter.qualification, "_fsync_directory", side_effect=OSError("directory")), \
                 patch.object(adapter.os, "ftruncate", side_effect=fail_invalidation):
                with self.assertRaisesRegex(adapter.qualification.OutcomeUnknown, "invalidation is uncertain"):
                    adapter.publish_receipt_no_replace(output, {"status": "verified_read_only"})
            # A storage failure can leave old bytes. It must never become a
            # routine refusal or a claimed successful publication.
            self.assertIn("verified_read_only", output.read_text())

    def test_close_only_failure_invalidates_owned_inode(self) -> None:
        with TemporaryDirectory() as directory:
            output = Path(directory) / "receipt.json"
            original_close = os.close
            calls = 0
            def fail_first_close(descriptor):
                nonlocal calls
                calls += 1
                if calls == 1:
                    raise OSError("primary close failed")
                return original_close(descriptor)
            with patch.object(adapter.qualification, "_fsync_directory"), \
                 patch.object(adapter.os, "close", side_effect=fail_first_close):
                with self.assertRaisesRegex(adapter.qualification.OutcomeUnknown, "close outcome is uncertain"):
                    adapter.publish_receipt_no_replace(output, {"status": "verified_read_only"})
            self.assertIn("publication_failed", output.read_text())

    def test_actual_source_refuses_dirty_pin_and_todo(self) -> None:
        with TemporaryDirectory() as directory:
            repo = Path(directory) / "repo"
            repo.mkdir()
            subprocess.run(["git", "init", "-q", str(repo)], check=True)
            subprocess.run(["git", "-C", str(repo), "config", "user.name", "test"], check=True)
            subprocess.run(["git", "-C", str(repo), "config", "user.email", "test@example.invalid"], check=True)
            pin, todo = repo / ".exiftool-version", repo / "TODO_RELEASE_BETA.md"
            pin.write_text("13.59\n")
            todo.write_text("Next pin: not selected\n")
            subprocess.run(["git", "-C", str(repo), "add", ".exiftool-version", "TODO_RELEASE_BETA.md"], check=True)
            subprocess.run(["git", "-C", str(repo), "commit", "-qm", "fixture"], check=True)
            head = subprocess.check_output(["git", "-C", str(repo), "rev-parse", "HEAD"], text=True).strip()
            tree = subprocess.check_output(["git", "-C", str(repo), "rev-parse", "HEAD^{tree}"], text=True).strip()
            with patch.object(adapter, "ROOT", repo), \
                 patch.object(adapter, "require_imported_source_paths"):
                bound = adapter.bind_git()
                clean = adapter.source_snapshot(head, tree, bound)
                self.assertEqual(clean["head"], head)
                pin.write_text("13.60\n")
                with self.assertRaisesRegex(adapter.qualification.Refused, "changes"):
                    adapter.source_snapshot(head, tree, bound)
                pin.write_text("13.59\n")
                todo.write_text("Next pin: 13.60\n")
                with self.assertRaisesRegex(adapter.qualification.Refused, "changes"):
                    adapter.source_snapshot(head, tree, bound)
                todo.write_text("Next pin: not selected\n")
                self.assertEqual(adapter.source_snapshot(head, tree, bound), clean)
                wrapper_dir = Path(directory) / "hostile-path"
                wrapper_dir.mkdir()
                wrapper = wrapper_dir / "git"
                invoked = wrapper_dir / "invoked"
                wrapper.write_text(f"#!/bin/sh\nprintf used > {invoked}\nprintf forged\\n\n")
                wrapper.chmod(0o755)
                with patch.dict(os.environ, {"PATH": str(wrapper_dir) + os.pathsep + os.environ.get("PATH", "")}):
                    self.assertEqual(adapter.source_snapshot(head, tree, bound), clean)
                self.assertFalse(invoked.exists())
                from dataclasses import replace
                with self.assertRaisesRegex(adapter.qualification.Refused, "custody"):
                    replace(bound, path=wrapper).revalidate()
                with self.assertRaisesRegex(adapter.qualification.Refused, "custody"):
                    replace(bound, sha256="0" * 64).revalidate()
                subprocess.run(["git", "-C", str(repo), "update-index", "--skip-worktree", ".exiftool-version"], check=True)
                with self.assertRaisesRegex(adapter.qualification.Refused, "hidden index flags"):
                    adapter.source_snapshot(head, tree, bound)
                subprocess.run(["git", "-C", str(repo), "update-index", "--no-skip-worktree", ".exiftool-version"], check=True)

    def test_committed_write_matrix_replay_from_synthetic_stage_fixture(self) -> None:
        # The stage fixture runs a real source-derived 1,530-row matrix parser.
        # Only the costly executor orchestration/stage-result seam is mocked.
        import test_version_rehearsal_stage_adapter as fixture_module
        fixture = fixture_module.AdapterTests(methodName="runTest")
        fixture.setUp()
        try:
            run_dir = fixture.root / "run" / "same-pin-13.59" / "before"
            checkout = run_dir / "checkouts" / "release-11-78"
            checkout.parent.mkdir(parents=True)
            shutil.move(str(fixture.checkout), str(checkout))
            fixture.checkout = checkout
            fixture.reports = run_dir / "stage-results" / "release-11-78"
            fixture.reports.mkdir(parents=True)
            fixture.write_manifest = fixture.write_manifest.resolve()
            stage = fixture_module.adapter
            with patch.dict(os.environ, {"CARGO_HOME": str(fixture.root / "empty-cargo-home")}):
                stage.generate(fixture.args("generate"), run=fixture.fake_run)
                built = stage.build(fixture.args("build"), run=fixture.fake_run)
                write = stage.write(fixture.args("write"), run=fixture.fake_run)
            digester = adapter.qualification.rehearsal.sha256_json
            journal = {"releases": {"11.78": {"reports": {
                name: {"path": f"stage-results/release-11-78/{name}.json", "sha256": digester(report)}
                for name, report in (("build", built), ("write", write))}}}}
            (run_dir / "execution-status.json").write_text(__import__("json").dumps(journal))
            config_path = run_dir / "inputs" / "config.json"
            config_path.parent.mkdir()
            config_path.write_text(__import__("json").dumps({
                "target_directories": {"11.78": str(fixture.target)},
                "write_fixture_bindings": {"11.78": adapter.executor._write_fixture_binding(
                    str(fixture.write_manifest))}}))
            self.assertEqual(write["fixtures"]["manifest"], adapter.executor._write_fixture_binding(str(fixture.write_manifest))["path"])
            self.assertEqual(write["fixtures"]["manifest_sha256"], adapter.executor._write_fixture_binding(str(fixture.write_manifest))["sha256"])
            self.assertEqual(adapter.executor._require_fixture_proof(write), adapter.executor._write_fixture_binding(str(fixture.write_manifest))["fixtures"])
            self.assertEqual(write["matrix_reports"][0]["path"], str(fixture.reports / "raw" / "write-matrix" / "0000" / "report.json"))
            row = {"id": "same-pin-13.59", "before": {
                "release": "11.78", "write_report_sha256": digester(write),
                "instrument": {"native_identity": write["native_identity"],
                               "native_probe_sha256": write["native_probe_sha256"]}}}
            with patch.object(adapter.executor, "_stage_result", return_value=write):
                proof = adapter.replay_committed_write(row, "before", run_dir.parent.parent,
                                                       fixture_module.COMMIT)
                self.assertEqual(proof["matrix_reports"][0]["passed"], 1530)
                self.assertEqual(proof["writer_binary"], built["writer_binary"])
                row["before"]["write_report_sha256"] = "0" * 64
                with self.assertRaisesRegex(adapter.qualification.Refused, "write digest"):
                    adapter.replay_committed_write(row, "before", run_dir.parent.parent,
                                                   fixture_module.COMMIT)
                row["before"]["write_report_sha256"] = digester(write)
                matrix = Path(write["matrix_reports"][0]["path"])
                original_matrix = matrix.read_bytes()
                matrix.unlink()
                with self.assertRaises(adapter.qualification.Refused):
                    adapter.replay_committed_write(row, "before", run_dir.parent.parent,
                                                   fixture_module.COMMIT)
                matrix.write_bytes(original_matrix)
                altered = __import__("json").loads(original_matrix)
                altered["passed"] -= 1
                matrix.write_text(__import__("json").dumps(altered))
                with self.assertRaises(adapter.qualification.Refused):
                    adapter.replay_committed_write(row, "before", run_dir.parent.parent,
                                                   fixture_module.COMMIT)
                matrix.write_bytes(original_matrix)
            raw_path = Path(write["raw_report"]["path"])
            raw_original = raw_path.read_bytes()
            malformed = __import__("json").loads(raw_original)
            malformed["commands"] = [1]
            raw_path.write_text(__import__("json").dumps(malformed))
            import copy, hashlib
            malformed_write = copy.deepcopy(write)
            malformed_write["raw_report"]["sha256"] = hashlib.sha256(raw_path.read_bytes()).hexdigest()
            row["before"]["write_report_sha256"] = digester(malformed_write)
            original_report_for = adapter.qualification._report_for
            def report_for(path, status, selected_release, stage_name):
                return malformed_write if stage_name == "write" else original_report_for(
                    path, status, selected_release, stage_name)
            with patch.object(adapter.qualification, "_report_for", side_effect=report_for), \
                 patch.object(adapter.executor, "_stage_result", return_value=malformed_write):
                with self.assertRaisesRegex(adapter.qualification.Refused, "commands must be a list of objects"):
                    adapter.replay_committed_write(row, "before", run_dir.parent.parent,
                                                   fixture_module.COMMIT)
            row["before"]["write_report_sha256"] = digester(write)
            raw_path.write_bytes(raw_original)
            wrong_writer = dict(write, writer_binary=dict(write["writer_binary"], sha256="0" * 64))
            with patch.object(adapter.executor, "_stage_result", return_value=wrong_writer):
                with self.assertRaisesRegex(adapter.qualification.Refused, "writer"):
                    adapter.replay_committed_write(row, "before", run_dir.parent.parent,
                                                   fixture_module.COMMIT)
        finally:
            fixture.doCleanups()

    def test_complete_six_side_plumbing_with_catalog_source_name(self) -> None:
        # Shared validators and the runtime loader are heavy seams here; the
        # adapter itself still traverses all six sides, inventories and publishes.
        import json
        with TemporaryDirectory() as directory:
            root = Path(directory)
            source_root = root / "sources"
            commit = "a" * 40
            sources = {}
            natives = {}
            perl = root / "perl"
            perl.write_text("perl")
            for release in ("13.59", "11.78", "12.64"):
                source_name = adapter.qualification.catalog_stage._source_directory_name(release, commit)
                native_source = source_root / source_name
                native_lib = native_source / "lib" / "Image"
                native_lib.mkdir(parents=True)
                (native_lib / "ExifTool.pm").write_text("module")
                tree = adapter.qualification.catalog_stage._tree_identity(native_source)
                sources[release] = {"release": release, "tag_object": "d" * 40,
                                    "peeled_commit": commit, "source_directory": source_name,
                                    "source_tree_sha256": tree["tree_sha256"],
                                    "materialization_sha256": "f" * 64}
                natives[release] = {"source": {"path": str(native_source)},
                                    "lib": {"path": str(native_source / "lib")},
                                    "perl": {"path": str(perl)}}
            bundle = root / "provisioned" / "verified-inputs"
            bundle.mkdir(parents=True)
            archive_cache = root / "selected-archive-cache"
            (archive_cache / "archives").mkdir(parents=True)
            selected = []
            for release in sources:
                archive_path = archive_cache / "archives" / f"{release}.tar.gz"
                archive_path.write_bytes(f"selected archive {release}\n".encode())
                digest = adapter.sha(archive_path)
                resolved_path = archive_cache / "archives" / f"{digest}.tar.gz"
                archive_path.rename(resolved_path)
                selected.append({"release": release, "archive": {"sha256": digest,
                    "bytes": resolved_path.stat().st_size,
                    "cache_key": f"archives/{digest}.tar.gz"}})
            documents = {
                "capture": {"schema": 1, "kind": "fixture-capture"},
                "catalog": {"schema": 1, "kind": "fixture-catalog"},
                "plan": {"pairs": [{"old": {"release": release, "tag_object": "d" * 40,
                    "peeled_commit": commit}, "new": {"release": release,
                    "tag_object": "d" * 40, "peeled_commit": commit}} for release in sources]},
                "resolution": {"selected_releases": selected},
                "materialization": {"selected_releases": [
                    {"release": release, "source_directory": sources[release]["source_directory"],
                     "tree": adapter.qualification.catalog_stage._tree_identity(
                         source_root / sources[release]["source_directory"])}
                    for release in sources], "materialization_sha256": "f" * 64},
                "locations": {"schema": 1, "kind": "oxidex_version_transition_input_locations",
                    "archive_cache": str(archive_cache), "source_root": str(source_root)},
            }
            for name, document in documents.items():
                (bundle / f"{name}.json").write_text(json.dumps(document))
            policy = root / "policy.json"
            policy.write_text('{}')
            repo = root / "checkout"
            repo.mkdir()
            (repo / ".exiftool-version").write_text("13.59\n")
            (repo / "TODO_RELEASE_BETA.md").write_text("Next pin: not selected\n")
            for name in adapter.TOOL_FILES:
                tool = repo / name
                tool.parent.mkdir(parents=True, exist_ok=True)
                tool.write_text(name)
            system_git = str(adapter.bind_git().path)
            subprocess.run([system_git, "-C", str(repo), "init", "-q"], check=True)
            subprocess.run([system_git, "-C", str(repo), "add", "-A"], check=True)
            subprocess.run([system_git, "-C", str(repo), "-c", "user.name=fixture",
                            "-c", "user.email=fixture@example.invalid", "commit", "-qm", "fixture"], check=True)
            expected_head = subprocess.check_output([system_git, "-C", str(repo), "rev-parse", "HEAD"], text=True).strip()
            expected_tree = subprocess.check_output([system_git, "-C", str(repo), "rev-parse", "HEAD^{tree}"], text=True).strip()
            matrix_sha = adapter.sha(adapter.qualification.CANONICAL_MATRIX)
            tools = {name: adapter.sha(repo / name) for name in adapter.TOOL_FILES}
            binaries = {}
            markers = []
            for row_name in adapter.ROWS:
                run = root / row_name
                run.mkdir()
                sides = {}
                releases = {"same-pin-13.59": ("13.59", "13.59"),
                            "11.78-to-12.64": ("11.78", "12.64"),
                            "12.64-to-11.78": ("12.64", "11.78")}[row_name]
                for side, release in zip(adapter.qualification.SIDES, releases, strict=True):
                    binary = root / f"{row_name}-{side}-oxidex"
                    binary.write_text("binary")
                    binaries[(row_name, side)] = {"path": str(binary), "sha256": adapter.sha(binary)}
                    side_run = run / row_name / side
                    (side_run / "inputs").mkdir(parents=True)
                    (side_run / "execution-status.json").write_text('{}')
                    (side_run / "inputs" / "config.json").write_text(json.dumps({
                        "verified_input_bundle": str(bundle)}))
                    sides[side] = {"release": release, "source_identity": sources[release],
                                   "instrument": {"source_commit": expected_head,
                                                  "binary": binaries[(row_name, side)],
                                                  "native_identity": natives[release],
                                                  "read_fixture_manifest_sha256": "0" * 64},
                                   "build_environment": {"toolchain": "checked"},
                                   "release_tests": {"passed": 1},
                                   "read_report_sha256": "1" * 64,
                                   "write_report_sha256": "2" * 64,
                                   "execution_journal_sha256": "3" * 64}
                marker = run / "qualification-result.json"
                marker.write_text(json.dumps({"rows": [{"id": row_name, **sides,
                    "read_payload_floors": {}, "read_policy_pair": {}}],
                    "caller": {"head": expected_head, "index_tree": expected_tree,
                               "pin_version": "13.59", "status": "clean"},
                    "matrix": {"sha256": matrix_sha},
                    "read_policy_input": {"path": str(policy), "sha256": adapter.POLICY_SHA256},
                    "run_id": row_name}))
                markers.append(marker)
            checks = []
            def build_check(*_args):
                checks.append("build")
                return {"toolchain": "checked"}
            def test_check(*_args):
                checks.append("test")
                return {"passed": 1}
            real_source_resolver = adapter.qualification.resolve_source_identity
            def source_check(identity, selected_bundle):
                checks.append("source")
                return real_source_resolver(identity, selected_bundle)
            def report_for(_run, _journal, _release, stage):
                return {"state": "passed"} if stage == "test" else {"build_environment": {}}
            output = root / "verified.json"
            argv = ["--same-pin-result", str(markers[0]), "--forward-result", str(markers[1]),
                    "--reverse-result", str(markers[2]), "--expected-head", expected_head,
                    "--expected-tree", expected_tree, "--expected-matrix-sha256", matrix_sha,
                    "--expected-read-policy-sha256", adapter.POLICY_SHA256,
                    "--next-pin", "not-selected", "--output", str(output)]
            for name, digest in tools.items():
                argv.extend(["--expected-tool", f"{name}={digest}"])
            for (row_name, side), identity in binaries.items():
                argv.extend(["--expected-binary", f"{row_name}:{side}:{identity['path']}:{identity['sha256']}"])
            with ExitStack() as patches:
                for context in (
                    patch.object(adapter, "ROOT", repo),
                    patch.object(adapter, "require_imported_source_paths"),
                    patch.object(adapter.qualification, "_evidence_location", side_effect=lambda path, _label: Path(path).resolve()),
                    patch.object(adapter.qualification, "load_matrix", return_value={"rows": [{"id": row} for row in adapter.ROWS]}),
                    patch.object(adapter.qualification, "load_committed_result", side_effect=lambda path: json.loads(path.read_text())),
                    patch.object(adapter.qualification, "_report_for", side_effect=report_for),
                    patch.object(adapter.qualification, "_build_environment_receipt", side_effect=build_check),
                    patch.object(adapter.qualification, "_release_test_receipt", side_effect=test_check),
                    patch.object(adapter.qualification, "replay_fixture_dependencies",
                                 return_value=[{"path": str(policy.resolve()), "sha256": adapter.sha(policy),
                                                "bytes": policy.stat().st_size}]),
                    patch.object(adapter.executor, "_require_test_suite_proof"),
                    patch.object(adapter.qualification.catalog_stage, "verify_capture_binding"),
                    patch.object(adapter.qualification.rehearsal, "verify_plan"),
                    patch.object(adapter.qualification.catalog_stage, "verify_source_resolution"),
                    patch.object(adapter.qualification.catalog_stage, "verify_source_materialization"),
                    patch.object(adapter.qualification, "resolve_source_identity", side_effect=source_check),
                    patch.object(adapter, "replay_committed_write", return_value={
                        "writer_binary": {"path": str(next(iter(binaries.values()))["path"]),
                                          "sha256": "4" * 64}}),
                ):
                    patches.enter_context(context)
                error = patches.enter_context(redirect_stderr(io.StringIO()))
                patches.enter_context(redirect_stdout(io.StringIO()))
                self.assertEqual(adapter.main(argv), 0, error.getvalue())
                self.assertEqual(json.loads(output.read_text())["git"]["path"], system_git)
                self.assertEqual(adapter.source_snapshot(expected_head, expected_tree, adapter.bind_git())["head"], expected_head)
                subordinate = root / adapter.ROWS[0] / adapter.ROWS[0] / "before" / "execution-status.json"
                owned_source = repo / "TODO_RELEASE_BETA.md"
                original_write = adapter._write_owned_state
                original_sync = adapter.qualification._fsync_directory
                original_close = os.close
                marker_inodes = {p.stat().st_ino for p in markers}

                # A marker close can report failure after the kernel closed it.
                close_failed = False
                close_written = False
                def write_before_close(fd, payload):
                    nonlocal close_written
                    original_write(fd, payload)
                    if b'verified_read_only' in payload:
                        close_written = True
                def close_after_success(fd):
                    nonlocal close_failed
                    is_marker = os.fstat(fd).st_ino in marker_inodes
                    original_close(fd)
                    if close_written and is_marker and not close_failed:
                        close_failed = True
                        raise OSError("injected marker close failure")
                close_output = root / "marker-close.json"
                close_argv = list(argv)
                close_argv[close_argv.index("--output") + 1] = str(close_output)
                with patch.object(adapter, "_write_owned_state", side_effect=write_before_close), \
                     patch.object(adapter.os, "close", side_effect=close_after_success):
                    self.assertEqual(adapter.main(close_argv), 2)
                self.assertTrue(close_failed)
                self.assertEqual(json.loads(close_output.read_text())["status"], "publication_failed")

                selected_archive = archive_cache / selected[0]["archive"]["cache_key"]
                for target_name, target in (("subordinate", subordinate), ("source", owned_source),
                                            ("bundle-document", bundle / "materialization.json"),
                                            ("selected-archive", selected_archive)):
                    for window in ("write", "fsync"):
                        original = target.read_bytes()
                        changed = False
                        wrote_success = False
                        def mutate():
                            nonlocal changed
                            if not changed:
                                target.write_bytes(original + b"changed")
                                changed = True
                        def write_then_mutate(fd, payload):
                            nonlocal wrote_success
                            original_write(fd, payload)
                            if b'verified_read_only' in payload:
                                wrote_success = True
                                if window == "write":
                                    mutate()
                        def sync_then_mutate(directory):
                            original_sync(directory)
                            if wrote_success and window == "fsync":
                                mutate()
                        fault_output = root / f"{target_name}-{window}.json"
                        fault_argv = list(argv)
                        fault_argv[fault_argv.index("--output") + 1] = str(fault_output)
                        try:
                            with patch.object(adapter, "_write_owned_state", side_effect=write_then_mutate), \
                                 patch.object(adapter.qualification, "_fsync_directory", side_effect=sync_then_mutate):
                                self.assertEqual(adapter.main(fault_argv), 2)
                            self.assertTrue(changed)
                            self.assertEqual(json.loads(fault_output.read_text())["status"], "publication_failed")
                        finally:
                            target.write_bytes(original)
                for window in ("final-validation", "marker-close"):
                    for action in ("replace", "remove"):
                        fault_output = root / f"output-{window}-{action}.json"
                        retained = root / f"retained-{window}-{action}.json"
                        foreign_bytes = b'{"owner":"foreign"}\n'
                        changed = False
                        wrote_success = False
                        original_snapshot = adapter.subordinate_snapshot
                        def lose_output():
                            nonlocal changed
                            if changed:
                                return
                            os.link(fault_output, retained)
                            if action == "replace":
                                replacement = root / f"foreign-{window}-{action}.json"
                                replacement.write_bytes(foreign_bytes)
                                os.replace(replacement, fault_output)
                            else:
                                fault_output.unlink()
                            changed = True
                        def mark_success(fd, payload):
                            nonlocal wrote_success
                            original_write(fd, payload)
                            if b'verified_read_only' in payload:
                                wrote_success = True
                        def snapshot_then_lose(*args):
                            if wrote_success and window == "final-validation":
                                lose_output()
                            return original_snapshot(*args)
                        def close_then_lose(fd):
                            marker_fd = os.fstat(fd).st_ino in marker_inodes
                            original_close(fd)
                            if wrote_success and marker_fd and window == "marker-close":
                                lose_output()
                        fault_argv = list(argv)
                        fault_argv[fault_argv.index("--output") + 1] = str(fault_output)
                        with ExitStack() as controls:
                            controls.enter_context(patch.object(adapter, "_write_owned_state", side_effect=mark_success))
                            controls.enter_context(patch.object(adapter, "subordinate_snapshot", side_effect=snapshot_then_lose))
                            controls.enter_context(patch.object(adapter.os, "close", side_effect=close_then_lose))
                            self.assertEqual(adapter.main(fault_argv), 4)
                        self.assertTrue(changed)
                        self.assertEqual(json.loads(retained.read_text())["status"], "publication_failed")
                        if action == "replace":
                            self.assertEqual(fault_output.read_bytes(), foreign_bytes)
                        else:
                            self.assertFalse(fault_output.exists())
                forged = json.loads(markers[0].read_text())
                forged["rows"][0]["before"]["instrument"]["native_identity"]["source"]["path"] = (
                    natives["11.78"]["source"]["path"])
                markers[0].write_text(json.dumps(forged))
                bad_output = root / "forged.json"
                bad_argv = list(argv)
                bad_argv[bad_argv.index("--output") + 1] = str(bad_output)
                self.assertEqual(adapter.main(bad_argv), 2)
            self.assertFalse(bad_output.exists())
            self.assertIn("native source differs", error.getvalue())
            self.assertEqual(json.loads(output.read_text())["status"], "verified_read_only")
            self.assertGreaterEqual(checks.count("build"), 12)
            self.assertGreaterEqual(checks.count("test"), 12)
            self.assertGreaterEqual(checks.count("source"), 12)

    def test_stale_policy_and_matrix_refuse_before_receipt_replay(self) -> None:
        with TemporaryDirectory() as directory:
            absent = Path(directory) / 'absent' / 'qualification-result.json'
            binaries = {(row, side): {"path": f"/target/{row}/{side}", "sha256": 'b' * 64}
                        for row in adapter.ROWS for side in adapter.qualification.SIDES}
            tools = {name: adapter.sha(adapter.ROOT / name) for name in adapter.TOOL_FILES}
            common = dict(paths=(absent, absent, absent), expected_head='a' * 40,
                          expected_tree='c' * 40, expected_tools=tools,
                          expected_binaries=binaries)
            with self.assertRaisesRegex(adapter.qualification.Refused, 'accepted read-policy'):
                adapter.verify_results(expected_matrix_sha256='d' * 64,
                                       expected_policy_sha256='e' * 64, **common)
            with self.assertRaisesRegex(adapter.qualification.Refused, 'local canonical matrix'):
                adapter.verify_results(expected_matrix_sha256='d' * 64,
                                       expected_policy_sha256=adapter.POLICY_SHA256, **common)

    def test_missing_committed_final_cannot_pass(self) -> None:
        with TemporaryDirectory() as directory:
            paths = tuple(Path(directory) / name / 'qualification-result.json'
                          for name in ('same', 'forward', 'reverse'))
            binaries = {(row, side): {"path": f"/target/{row}/{side}", "sha256": 'b' * 64}
                        for row in adapter.ROWS for side in adapter.qualification.SIDES}
            tools = {name: adapter.sha(adapter.ROOT / name) for name in adapter.TOOL_FILES}
            with self.assertRaises(adapter.qualification.Refused):
                adapter.verify_results(
                    paths=paths, expected_head='a' * 40, expected_tree='c' * 40,
                    expected_matrix_sha256=adapter.sha(adapter.qualification.CANONICAL_MATRIX),
                    expected_policy_sha256=adapter.POLICY_SHA256,
                    expected_tools=tools, expected_binaries=binaries)


if __name__ == '__main__':
    unittest.main()
