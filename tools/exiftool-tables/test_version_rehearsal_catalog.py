#!/usr/bin/env python3
"""Offline tests for catalog capture and selected-source resolution."""
from __future__ import annotations

import gzip
import importlib.util
import io
import json
import sys
import tarfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("version_rehearsal_catalog", HERE / "version_rehearsal_catalog.py")
assert spec and spec.loader
catalog_stage = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = catalog_stage
spec.loader.exec_module(catalog_stage)
rehearsal = catalog_stage.rehearsal

OID_A = "a" * 40
OID_B = "b" * 40
OID_C = "c" * 40
OID_D = "d" * 40


def response(value, headers=None, status=200):
    body = value if isinstance(value, bytes) else json.dumps(value, separators=(",", ":")).encode()
    return catalog_stage.Response(status=status, headers=headers or {}, body=body)


def tag(name, commit):
    return {"name": name, "commit": {"sha": commit, "url": f"https://api.github.com/repos/exiftool/exiftool/commits/{commit}"}}


def tar_gz(label):
    output = io.BytesIO()
    with gzip.GzipFile(fileobj=output, mode="wb") as zipped:
        with tarfile.open(fileobj=zipped, mode="w") as archive:
            data = label.encode()
            info = tarfile.TarInfo(f"exiftool-{label}/README")
            info.size = len(data)
            archive.addfile(info, io.BytesIO(data))
    return output.getvalue()


class FixtureGet:
    def __init__(self, responses):
        self.responses = responses
        self.calls = []

    def __call__(self, url):
        self.calls.append(url)
        value = self.responses.get(url)
        if isinstance(value, Exception):
            raise value
        if value is None:
            raise catalog_stage.Refused(f"unexpected URL {url}")
        return value


def complete_responses():
    second = "https://api.github.com/repositories/132751855/tags?per_page=100&page=2"
    return {
        catalog_stage.TAG_PAGE_URL: response(
            [tag("13.59", OID_C), tag("v13.58", OID_B)],
            {"Link": f'<{second}>; rel="next", <https://api.github.com/repositories/132751855/tags?per_page=100&page=2>; rel="last"'},
        ),
        second: response([tag("13.58", OID_B), tag("13.57", OID_A)]),
        catalog_stage._ref_url("13.59"): response({"object": {"type": "tag", "sha": OID_D}}),
        catalog_stage._tag_object_url(OID_D): response({"object": {"type": "commit", "sha": OID_C}}),
        catalog_stage._ref_url("13.58"): response({"object": {"type": "commit", "sha": OID_B}}),
        catalog_stage._ref_url("13.57"): response({"object": {"type": "commit", "sha": OID_A}}),
    }


class CatalogCaptureTests(unittest.TestCase):
    def capture(self):
        return catalog_stage.capture_tag_catalog(FixtureGet(complete_responses()), "2026-09-13T12:00:00Z")

    def normalized(self):
        return rehearsal.normalize_catalog(catalog_stage.raw_catalog_from_capture(self.capture()))

    def test_pagination_raw_pages_and_annotated_tag_identity_are_preserved(self):
        capture = self.capture()
        catalog_stage.verify_capture(capture)
        self.assertTrue(capture["complete"])
        self.assertEqual(len(capture["pages"]), 2)
        self.assertEqual([item["listed"]["name"] for item in capture["entries"]], ["13.59", "v13.58", "13.58", "13.57"])
        annotated = capture["entries"][0]["identity"]
        self.assertEqual((annotated["tag_object"], annotated["peeled_commit"]), (OID_D, OID_C))
        self.assertEqual(capture["entries"][1]["identity"], {"state": "not_requested", "reason": "tag_name_not_numeric_release"})
        catalog = self.normalized()
        self.assertEqual([row["name"] for row in rehearsal.eligible_releases(catalog)], ["13.57", "13.58", "13.59"])

    def test_incomplete_pagination_is_preserved_but_cannot_define_selection_population(self):
        responses = complete_responses()
        second = "https://api.github.com/repositories/132751855/tags?per_page=100&page=2"
        responses[second] = catalog_stage.Refused("bounded timeout")
        capture = catalog_stage.capture_tag_catalog(FixtureGet(responses), "2026-09-13T12:00:00Z")
        catalog_stage.verify_capture(capture)
        self.assertFalse(capture["complete"])
        self.assertEqual(capture["failures"][0]["kind"], "page_request_failed")
        with self.assertRaisesRegex(catalog_stage.Refused, "incomplete pagination"):
            catalog_stage.raw_catalog_from_capture(capture)

    def test_non_tag_pagination_target_is_refused_before_any_population_is_accepted(self):
        responses = complete_responses()
        responses[catalog_stage.TAG_PAGE_URL] = response(
            [tag("13.59", OID_C)],
            {"Link": '<https://api.github.com/repositories/132751855/tags-shadow?page=2>; rel="next"'},
        )
        capture = catalog_stage.capture_tag_catalog(FixtureGet(responses), "2026-09-13T12:00:00Z")
        self.assertFalse(capture["complete"])
        self.assertEqual(capture["failures"][0]["kind"], "page_malformed")
        with self.assertRaisesRegex(catalog_stage.Refused, "incomplete pagination"):
            catalog_stage.raw_catalog_from_capture(capture)

    def test_capture_refuses_skipped_or_ambiguous_pagination_before_fetching_it(self):
        for next_url in (
            "https://api.github.com/repositories/132751855/tags?per_page=100&page=4",
            "https://api.github.com/repositories/132751855/tags?per_page=50&page=2",
            "https://api.github.com/repositories/132751855/tags?per_page=100&page=2&page=3",
            "https://api.github.com/repositories/132751855/tags?per_page=100&page=02",
        ):
            with self.subTest(next_url=next_url):
                responses = complete_responses()
                responses[catalog_stage.TAG_PAGE_URL] = response(
                    [tag("13.59", OID_C)], {"Link": f"<{next_url}>; rel=\"next\""}
                )
                get = FixtureGet(responses)
                capture = catalog_stage.capture_tag_catalog(get, "2026-09-13T12:00:00Z")
                self.assertFalse(capture["complete"])
                self.assertEqual(capture["failures"][0]["kind"], "page_malformed")
                self.assertEqual(get.calls, [catalog_stage.TAG_PAGE_URL])
                with self.assertRaisesRegex(catalog_stage.Refused, "incomplete pagination"):
                    catalog_stage.raw_catalog_from_capture(capture)

    def test_capture_refuses_multiple_next_relations_before_fetching_any_target(self):
        page_two = "https://api.github.com/repositories/132751855/tags?per_page=100&page=2"
        page_four = "https://api.github.com/repositories/132751855/tags?per_page=100&page=4"
        responses = complete_responses()
        responses[catalog_stage.TAG_PAGE_URL] = response(
            [tag("13.59", OID_C)],
            {"Link": f'<{page_two}>; rel="next", <{page_four}>; rel="next"'},
        )
        get = FixtureGet(responses)
        capture = catalog_stage.capture_tag_catalog(get, "2026-09-13T12:00:00Z")
        self.assertFalse(capture["complete"])
        self.assertEqual(capture["failures"][0]["kind"], "page_malformed")
        self.assertEqual(get.calls, [catalog_stage.TAG_PAGE_URL])
        with self.assertRaisesRegex(catalog_stage.Refused, "incomplete pagination"):
            catalog_stage.raw_catalog_from_capture(capture)

    def test_rehashed_capture_cannot_skip_a_page_in_replay_validation(self):
        capture = self.capture()
        skipped = "https://api.github.com/repositories/132751855/tags?per_page=100&page=4"
        capture["pages"][0]["link_header"] = f"<{skipped}>; rel=\"next\""
        capture["pages"][0]["next_url"] = skipped
        capture["pages"][1]["url"] = skipped
        capture["capture_sha256"] = catalog_stage.sha256_json(
            {key: value for key, value in capture.items() if key != "capture_sha256"}
        )
        with self.assertRaisesRegex(catalog_stage.Refused, "does not advance exactly one page"):
            catalog_stage.verify_capture(capture)

    def test_rehashed_capture_cannot_hide_a_second_next_relation(self):
        capture = self.capture()
        page_two = "https://api.github.com/repositories/132751855/tags?per_page=100&page=2"
        page_four = "https://api.github.com/repositories/132751855/tags?per_page=100&page=4"
        capture["pages"][0]["link_header"] = f'<{page_two}>; rel="next", <{page_four}>; rel="next"'
        capture["pages"][0]["next_url"] = page_two
        capture["capture_sha256"] = catalog_stage.sha256_json(
            {key: value for key, value in capture.items() if key != "capture_sha256"}
        )
        with self.assertRaisesRegex(catalog_stage.Refused, "multiple rel=next"):
            catalog_stage.verify_capture(capture)

    def test_moved_ref_is_preserved_and_blocks_population(self):
        responses = complete_responses()
        responses[catalog_stage._ref_url("13.58")] = response({"object": {"type": "commit", "sha": OID_A}})
        capture = catalog_stage.capture_tag_catalog(FixtureGet(responses), "2026-09-13T12:00:00Z")
        identity = next(item["identity"] for item in capture["entries"] if item["listed"].get("name") == "13.58")
        self.assertEqual(identity["reason"], "listed_commit_differs_from_resolved_ref")
        with self.assertRaisesRegex(catalog_stage.Refused, "lacks an immutable resolved identity"):
            catalog_stage.raw_catalog_from_capture(capture)

    def test_duplicate_numeric_tags_survive_capture_and_are_not_silently_selected(self):
        responses = complete_responses()
        second = "https://api.github.com/repositories/132751855/tags?per_page=100&page=2"
        responses[second] = response([tag("13.58", OID_B), tag("13.58", OID_B), tag("13.57", OID_A)])
        capture = catalog_stage.capture_tag_catalog(FixtureGet(responses), "2026-09-13T12:00:00Z")
        catalog = rehearsal.normalize_catalog(catalog_stage.raw_catalog_from_capture(capture))
        duplicate_states = [entry["classification"] for entry in catalog["entries"] if entry["raw"].get("name") == "13.58"]
        self.assertEqual(duplicate_states, [{"state": "unclassified", "reason": "ambiguous_duplicate_release"}] * 2)
        self.assertEqual([row["name"] for row in rehearsal.eligible_releases(catalog)], ["13.57", "13.59"])

    def test_selected_archives_are_resolved_by_immutable_commit_only_after_selection(self):
        catalog = self.normalized()
        plan = rehearsal.make_plan(catalog, 3, 0, 1, "e" * 40)
        selected = {side["release"]: side for pair in plan["pairs"] for side in (pair["old"], pair["new"])}
        responses = {catalog_stage.immutable_archive_url(release, side["peeled_commit"]): response(tar_gz(release)) for release, side in selected.items()}
        get = FixtureGet(responses)
        capture = self.capture()
        resolved = catalog_stage.resolve_selected_archives(plan, catalog, capture, get)
        catalog_stage.verify_source_resolution(resolved, plan, catalog, capture)
        self.assertEqual({row["release"] for row in resolved["selected_releases"]}, set(selected))
        self.assertEqual(set(get.calls), set(responses))
        self.assertEqual(resolved["execution"]["native_read"], "unrun")
        self.assertEqual(resolved["execution"]["native_write"], "unrun")

    def test_corrupt_selected_archive_is_refused_without_resolving_unselected_releases(self):
        catalog = self.normalized()
        plan = rehearsal.make_plan(catalog, 3, 0, 1, "e" * 40)
        selected = next(side for pair in plan["pairs"] for side in (pair["old"], pair["new"]))
        get = FixtureGet({catalog_stage.immutable_archive_url(selected["release"], selected["peeled_commit"]): response(b"not-a-tar")})
        with self.assertRaisesRegex(catalog_stage.Refused, "readable tar.gz"):
            catalog_stage.resolve_selected_archives(plan, catalog, self.capture(), get)
        self.assertEqual(len(get.calls), 1)

    def test_archive_resolution_refuses_catalog_not_derived_from_its_capture(self):
        catalog = self.normalized()
        catalog["entries"][0]["raw"]["peeled_commit"] = OID_D
        catalog["catalog_sha256"] = rehearsal.sha256_json({key: value for key, value in catalog.items() if key != "catalog_sha256"})
        plan = rehearsal.make_plan(self.normalized(), 3, 0, 1, "e" * 40)
        with self.assertRaisesRegex(catalog_stage.Refused, "differs from its saved source capture"):
            catalog_stage.resolve_selected_archives(plan, catalog, self.capture(), FixtureGet({}))


if __name__ == "__main__":
    unittest.main()
