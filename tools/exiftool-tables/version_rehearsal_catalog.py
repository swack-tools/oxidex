#!/usr/bin/env python3
"""Capture and resolve immutable source identities for version rehearsals.

The capture stage reads only GitHub's official ExifTool tag APIs. It preserves
exact page bodies and digest records, then resolves each numeric tag through
its ref (and annotated-tag chain when needed) to an immutable commit. Pair
selection uses that complete tag/commit population. ``resolve-selected``
fetches and hashes only plan-selected archives, preserving their exact bytes in
a content-addressed cache. ``materialize-selected`` re-verifies those bytes and
safely extracts each archive into a new commit-named source directory.

Neither command changes OxiDex, its ExifTool pin, generated files, builds,
oracles, or promotion state.  A resolved source identity is still not native
read/write conformance evidence.
"""
from __future__ import annotations

import argparse
import gzip
import hashlib
import io
import json
import os
import re
import shutil
import sys
import tarfile
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable

import version_rehearsal as rehearsal

CAPTURE_SCHEMA = 1
RESOLUTION_SCHEMA = 2
MATERIALIZATION_SCHEMA = 1
REPOSITORY = "exiftool/exiftool"
REPOSITORY_ID = "132751855"
API_ROOT = "https://api.github.com"
TAG_PAGE_URL = f"{API_ROOT}/repos/{REPOSITORY}/tags?per_page=100&page=1"
MAX_TAG_DEPTH = 8
MAX_ARCHIVE_BYTES = 128 * 1024 * 1024
ARCHIVE_CACHE_DIRECTORY = "archives"
LINK_NEXT_RE = re.compile(r'<([^>]+)>;\s*rel="?next"?')


@dataclass(frozen=True)
class Response:
    status: int
    headers: dict[str, str]
    body: bytes


class Refused(ValueError):
    """The captured source cannot support an attributable rehearsal."""


def canonical_json(value: Any) -> bytes:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True).encode("utf-8")


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def sha256_json(value: Any) -> str:
    return sha256_bytes(canonical_json(value))


def atomic_json(path: Path, value: dict[str, Any]) -> None:
    rehearsal.atomic_json(path, value)


def _json_body(response: Response, context: str) -> Any:
    try:
        return json.loads(response.body.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise Refused(f"{context} did not return UTF-8 JSON") from exc


def _body_text(body: bytes) -> str:
    try:
        return body.decode("utf-8")
    except UnicodeDecodeError as exc:
        raise Refused("official API response was not UTF-8") from exc


def _official_api_url(url: str) -> bool:
    parsed = urllib.parse.urlparse(url)
    if parsed.scheme != "https" or parsed.netloc != "api.github.com":
        return False
    return parsed.path in {f"/repos/{REPOSITORY}/tags", f"/repositories/{REPOSITORY_ID}/tags"}


def _page_coordinates(url: str) -> tuple[int, int]:
    """Return the canonical official tag-page coordinates.

    GitHub may switch the repository path in a Link header, but the page
    sequence itself is part of the captured population.  Accepting arbitrary
    query strings would let a saved page 1 point directly at page 4 and still
    appear complete.  Parse query pairs rather than using a dict so duplicate
    keys cannot make the effective page ambiguous.
    """
    if not _official_api_url(url):
        raise Refused("pagination link is not the official ExifTool tag API")
    parsed = urllib.parse.urlparse(url)
    pairs = urllib.parse.parse_qsl(parsed.query, keep_blank_values=True)
    values: dict[str, str] = {}
    for key, value in pairs:
        if key in values:
            raise Refused("pagination link has a duplicate query key")
        values[key] = value
    if set(values) != {"per_page", "page"}:
        raise Refused("pagination link has unsupported or missing query keys")
    try:
        per_page = int(values["per_page"])
        page = int(values["page"])
    except ValueError as exc:
        raise Refused("pagination link has a nonnumeric page coordinate") from exc
    if not 1 <= per_page <= 100 or page < 1:
        raise Refused("pagination link has an out-of-range page coordinate")
    if values["per_page"] != str(per_page) or values["page"] != str(page):
        raise Refused("pagination link has a noncanonical page coordinate")
    return per_page, page


def _next_url(headers: dict[str, str]) -> str | None:
    link = next((value for key, value in headers.items() if key.lower() == "link"), "")
    matches = LINK_NEXT_RE.findall(link)
    if not matches:
        return None
    if len(matches) != 1:
        raise Refused("pagination link has multiple rel=next relations")
    url = matches[0]
    _page_coordinates(url)
    return url


def http_get(url: str, timeout: float, max_bytes: int | None = None) -> Response:
    """Read one official URL with a caller-bounded timeout and no retries."""
    headers = {"Accept": "application/vnd.github+json", "User-Agent": "oxidex-version-rehearsal"}
    token = os.environ.get("GITHUB_TOKEN") or os.environ.get("GH_TOKEN")
    if token:
        headers["Authorization"] = f"Bearer {token}"
    request = urllib.request.Request(url, headers=headers)
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:  # noqa: S310 -- URL is authenticated below
            if max_bytes is not None:
                content_length = response.headers.get("Content-Length")
                if content_length is not None:
                    try:
                        declared_length = int(content_length)
                    except ValueError as exc:
                        raise Refused("response has an invalid content length") from exc
                    if declared_length > max_bytes:
                        raise Refused("response exceeds configured byte limit")
                body = response.read(max_bytes + 1)
                if len(body) > max_bytes:
                    raise Refused("response exceeds configured byte limit")
            else:
                body = response.read()
            return Response(response.status, dict(response.headers.items()), body)
    except urllib.error.HTTPError as exc:
        return Response(exc.code, dict(exc.headers.items()) if exc.headers else {}, exc.read())
    except urllib.error.URLError as exc:
        raise Refused(f"request failed for {url}: {exc.reason}") from exc


def _response_record(url: str, response: Response) -> dict[str, Any]:
    link = next((value for key, value in response.headers.items() if key.lower() == "link"), "")
    return {
        "url": url,
        "status": response.status,
        "body_utf8": _body_text(response.body),
        "body_sha256": sha256_bytes(response.body),
        "link_header": link,
    }


def _ref_url(tag_name: str) -> str:
    return f"{API_ROOT}/repos/{REPOSITORY}/git/ref/tags/{urllib.parse.quote(tag_name, safe='')}"


def _tag_object_url(oid: str) -> str:
    return f"{API_ROOT}/repos/{REPOSITORY}/git/tags/{oid}"


def _list_commit(listed: dict[str, Any]) -> str | None:
    commit = listed.get("commit")
    sha = commit.get("sha") if isinstance(commit, dict) else None
    return sha if isinstance(sha, str) and rehearsal.GIT_OID_RE.fullmatch(sha) else None


def _resolve_numeric_tag(name: str, listed: dict[str, Any], get: Callable[[str], Response]) -> dict[str, Any]:
    """Resolve a listed tag to its ref object and terminal commit.

    Every response is retained.  Ref/type failures are records rather than
    discarded tags, so a partial capture cannot masquerade as a complete
    candidate population.
    """
    trace: list[dict[str, Any]] = []
    try:
        url = _ref_url(name)
        response = get(url)
        trace.append(_response_record(url, response))
        if response.status != 200:
            return {"state": "failed", "reason": "tag_ref_http_status", "trace": trace}
        ref = _json_body(response, "tag ref")
        obj = ref.get("object") if isinstance(ref, dict) else None
        kind = obj.get("type") if isinstance(obj, dict) else None
        oid = obj.get("sha") if isinstance(obj, dict) else None
        if kind not in {"commit", "tag"} or not isinstance(oid, str) or not rehearsal.GIT_OID_RE.fullmatch(oid):
            return {"state": "failed", "reason": "tag_ref_missing_object", "trace": trace}
        tag_object = oid
        depth = 0
        while kind == "tag":
            depth += 1
            if depth > MAX_TAG_DEPTH:
                return {"state": "failed", "reason": "annotated_tag_depth_exceeded", "trace": trace}
            url = _tag_object_url(oid)
            response = get(url)
            trace.append(_response_record(url, response))
            if response.status != 200:
                return {"state": "failed", "reason": "annotated_tag_http_status", "trace": trace}
            tag = _json_body(response, "annotated tag")
            obj = tag.get("object") if isinstance(tag, dict) else None
            kind = obj.get("type") if isinstance(obj, dict) else None
            oid = obj.get("sha") if isinstance(obj, dict) else None
            if kind not in {"commit", "tag"} or not isinstance(oid, str) or not rehearsal.GIT_OID_RE.fullmatch(oid):
                return {"state": "failed", "reason": "annotated_tag_missing_object", "trace": trace}
        listed_commit = _list_commit(listed)
        if listed_commit is not None and listed_commit != oid:
            return {
                "state": "failed",
                "reason": "listed_commit_differs_from_resolved_ref",
                "trace": trace,
                "tag_object": tag_object,
                "peeled_commit": oid,
                "listed_commit": listed_commit,
            }
        return {
            "state": "resolved",
            "tag_object": tag_object,
            "peeled_commit": oid,
            "listed_commit": listed_commit,
            "trace": trace,
        }
    except Refused as exc:
        return {"state": "failed", "reason": "identity_request_failed", "detail": str(exc), "trace": trace}


def capture_tag_catalog(get: Callable[[str], Response], captured_at: str | None = None) -> dict[str, Any]:
    """Capture every official tag page and resolve each numeric release tag."""
    pages: list[dict[str, Any]] = []
    entries: list[dict[str, Any]] = []
    failures: list[dict[str, Any]] = []
    current = TAG_PAGE_URL
    seen: set[str] = set()
    expected_per_page, expected_page = _page_coordinates(current)
    complete = True
    while current is not None:
        try:
            per_page, page = _page_coordinates(current)
            if per_page != expected_per_page or page != expected_page:
                raise Refused("pagination sequence is not contiguous")
        except Refused as exc:
            complete = False
            failures.append({"kind": "page_malformed", "url": current, "detail": str(exc)})
            break
        if current in seen:
            complete = False
            failures.append({"kind": "pagination_cycle", "url": current})
            break
        seen.add(current)
        try:
            response = get(current)
        except Refused as exc:
            complete = False
            failures.append({"kind": "page_request_failed", "url": current, "detail": str(exc)})
            break
        record = _response_record(current, response)
        if response.status != 200:
            complete = False
            failures.append({"kind": "page_http_status", "url": current, "status": response.status, "body_sha256": record["body_sha256"]})
            break
        try:
            listed = _json_body(response, "tag page")
            if not isinstance(listed, list):
                raise Refused("tag page was not a JSON list")
            next_url = _next_url(response.headers)
            if next_url is not None:
                next_per_page, next_page = _page_coordinates(next_url)
                if next_per_page != expected_per_page or next_page != expected_page + 1:
                    raise Refused("pagination next link does not advance exactly one page")
        except Refused as exc:
            complete = False
            failures.append({"kind": "page_malformed", "url": current, "detail": str(exc), "body_sha256": record["body_sha256"]})
            break
        page_index = len(pages)
        pages.append({**record, "next_url": next_url})
        for entry_index, listed_entry in enumerate(listed):
            name = listed_entry.get("name") if isinstance(listed_entry, dict) else None
            item: dict[str, Any] = {
                "source_page": page_index,
                "source_index": entry_index,
                "listed": listed_entry,
            }
            if not isinstance(name, str) or not rehearsal.RELEASE_RE.fullmatch(name):
                item["identity"] = {"state": "not_requested", "reason": "tag_name_not_numeric_release"}
            else:
                item["identity"] = _resolve_numeric_tag(name, listed_entry, get)
            entries.append(item)
        current = next_url
        expected_page += 1
    payload = {
        "schema": CAPTURE_SCHEMA,
        "kind": "oxidex_exiftool_official_tag_capture",
        "repository": REPOSITORY,
        "tag_page_start_url": TAG_PAGE_URL,
        "captured_at": captured_at or time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "complete": complete,
        "pages": pages,
        "entries": entries,
        "failures": failures,
        "execution": {"state": "source_identity_only", "native_read": "unrun", "native_write": "unrun"},
    }
    return {**payload, "capture_sha256": sha256_json(payload)}


def verify_capture(capture: dict[str, Any]) -> None:
    if (capture.get("schema") != CAPTURE_SCHEMA or capture.get("kind") != "oxidex_exiftool_official_tag_capture"
            or capture.get("repository") != REPOSITORY or capture.get("tag_page_start_url") != TAG_PAGE_URL
            or not isinstance(capture.get("captured_at"), str) or not isinstance(capture.get("complete"), bool)
            or not isinstance(capture.get("pages"), list) or not isinstance(capture.get("entries"), list)
            or not isinstance(capture.get("failures"), list)):
        raise Refused("unsupported capture manifest")
    payload = {key: value for key, value in capture.items() if key != "capture_sha256"}
    if capture.get("capture_sha256") != sha256_json(payload):
        raise Refused("capture manifest identity changed")
    expected_per_page, expected_page = _page_coordinates(TAG_PAGE_URL)
    if capture["complete"] and not capture["pages"]:
        raise Refused("complete capture has no tag pages")
    source_rows: list[Any] = []
    for page_index, page in enumerate(capture["pages"]):
        if not isinstance(page, dict) or not _official_api_url(page.get("url", "")) or page.get("status") != 200:
            raise Refused("capture page identity is malformed")
        per_page, page_number = _page_coordinates(page["url"])
        if per_page != expected_per_page or page_number != expected_page:
            raise Refused("capture page sequence is not contiguous")
        text = page.get("body_utf8")
        if not isinstance(text, str) or page.get("body_sha256") != sha256_bytes(text.encode("utf-8")):
            raise Refused("capture page body digest differs")
        if not isinstance(page.get("link_header"), str):
            raise Refused("capture page link header is malformed")
        if page.get("next_url") != _next_url({"Link": page["link_header"]}):
            raise Refused("capture pagination link differs from raw header")
        if page["next_url"] is not None:
            next_per_page, next_page = _page_coordinates(page["next_url"])
            if next_per_page != expected_per_page or next_page != expected_page + 1:
                raise Refused("capture pagination next link does not advance exactly one page")
        if page_index + 1 < len(capture["pages"]) and page["next_url"] != capture["pages"][page_index + 1].get("url"):
            raise Refused("capture page sequence differs from pagination links")
        if page_index + 1 == len(capture["pages"]) and capture["complete"] and page["next_url"] is not None:
            raise Refused("complete capture ended before pagination ended")
        try:
            listed = json.loads(text)
        except json.JSONDecodeError as exc:
            raise Refused("capture page body is not JSON") from exc
        if not isinstance(listed, list):
            raise Refused("capture page body is not a tag list")
        for index, row in enumerate(listed):
            source_rows.append((page_index, index, row))
        expected_page += 1
    if len(source_rows) != len(capture["entries"]):
        raise Refused("capture entries do not cover raw pages")
    for item, (page_index, index, listed) in zip(capture["entries"], source_rows, strict=True):
        if not isinstance(item, dict) or item.get("source_page") != page_index or item.get("source_index") != index or item.get("listed") != listed:
            raise Refused("capture entry differs from raw page")
        identity = item.get("identity")
        if not isinstance(identity, dict):
            raise Refused("capture entry lacks identity record")
        name = listed.get("name") if isinstance(listed, dict) else None
        if not isinstance(name, str) or not rehearsal.RELEASE_RE.fullmatch(name):
            if identity != {"state": "not_requested", "reason": "tag_name_not_numeric_release"}:
                raise Refused("nonnumeric tag identity was not preserved")
        elif identity.get("state") == "resolved":
            _verify_resolved_identity(name, listed, identity)
        elif identity.get("state") != "failed" or not isinstance(identity.get("reason"), str):
            raise Refused("numeric tag identity state is malformed")


def _trace_json(record: dict[str, Any], expected_url: str, context: str) -> Any:
    if (not isinstance(record, dict) or record.get("url") != expected_url or record.get("status") != 200
            or not isinstance(record.get("body_utf8"), str) or record.get("body_sha256") != sha256_bytes(record["body_utf8"].encode("utf-8"))):
        raise Refused(f"{context} trace differs from saved response")
    try:
        return json.loads(record["body_utf8"])
    except json.JSONDecodeError as exc:
        raise Refused(f"{context} trace is not JSON") from exc


def _verify_resolved_identity(name: str, listed: dict[str, Any], identity: dict[str, Any]) -> None:
    trace = identity.get("trace")
    if not isinstance(trace, list) or not trace:
        raise Refused("resolved numeric tag lacks immutable trace")
    ref = _trace_json(trace[0], _ref_url(name), "tag ref")
    obj = ref.get("object") if isinstance(ref, dict) else None
    kind = obj.get("type") if isinstance(obj, dict) else None
    oid = obj.get("sha") if isinstance(obj, dict) else None
    if kind not in {"commit", "tag"} or not isinstance(oid, str) or not rehearsal.GIT_OID_RE.fullmatch(oid):
        raise Refused("tag ref trace has no resolvable object")
    tag_object = oid
    trace_index = 1
    while kind == "tag":
        if trace_index >= len(trace) or trace_index > MAX_TAG_DEPTH:
            raise Refused("annotated tag trace is incomplete")
        tag = _trace_json(trace[trace_index], _tag_object_url(oid), "annotated tag")
        trace_index += 1
        obj = tag.get("object") if isinstance(tag, dict) else None
        kind = obj.get("type") if isinstance(obj, dict) else None
        oid = obj.get("sha") if isinstance(obj, dict) else None
        if kind not in {"commit", "tag"} or not isinstance(oid, str) or not rehearsal.GIT_OID_RE.fullmatch(oid):
            raise Refused("annotated tag trace has no resolvable object")
    if trace_index != len(trace):
        raise Refused("resolved tag trace has unused responses")
    listed_commit = _list_commit(listed)
    if listed_commit is not None and listed_commit != oid:
        raise Refused("resolved tag trace disagrees with listed commit")
    if identity != {
        "state": "resolved",
        "tag_object": tag_object,
        "peeled_commit": oid,
        "listed_commit": listed_commit,
        "trace": trace,
    }:
        raise Refused("resolved tag identity differs from raw trace")


def _identity_fields(identity: dict[str, Any]) -> tuple[str, str] | None:
    tag_object, commit = identity.get("tag_object"), identity.get("peeled_commit")
    if not isinstance(tag_object, str) or not rehearsal.GIT_OID_RE.fullmatch(tag_object):
        return None
    if not isinstance(commit, str) or not rehearsal.GIT_OID_RE.fullmatch(commit):
        return None
    return tag_object, commit


def raw_catalog_from_capture(capture: dict[str, Any]) -> dict[str, Any]:
    """Make the planner input only when every numeric tag identity is known."""
    verify_capture(capture)
    if not capture["complete"]:
        raise Refused("incomplete pagination cannot define a release population")
    entries: list[dict[str, Any]] = []
    for item in capture["entries"]:
        listed = item["listed"]
        name = listed.get("name") if isinstance(listed, dict) else None
        identity = item["identity"]
        if isinstance(name, str) and rehearsal.RELEASE_RE.fullmatch(name):
            fields = _identity_fields(identity)
            if identity.get("state") != "resolved" or fields is None:
                raise Refused(f"numeric release {name!r} lacks an immutable resolved identity")
            tag_object, peeled_commit = fields
            entries.append({
                "name": name,
                "tag_object": tag_object,
                "peeled_commit": peeled_commit,
                "capture_provenance": {"source_page": item["source_page"], "source_index": item["source_index"], "identity_sha256": sha256_json(identity)},
            })
        else:
            entries.append({"name": name, "capture_provenance": {"source_page": item["source_page"], "source_index": item["source_index"]}})
    return {
        "catalog_source": {
            "kind": "official_exiftool_tag_catalog",
            "repository": REPOSITORY,
            "capture_schema": CAPTURE_SCHEMA,
            "capture_sha256": capture["capture_sha256"],
            "pages": [{"url": page["url"], "sha256": page["body_sha256"]} for page in capture["pages"]],
        },
        "captured_at": capture["captured_at"],
        "entries": entries,
    }


def verify_capture_binding(capture: dict[str, Any], catalog: dict[str, Any]) -> None:
    """Require a planner catalog to be exactly derived from saved raw pages."""
    verify_capture(capture)
    expected = rehearsal.normalize_catalog(raw_catalog_from_capture(capture))
    if catalog != expected:
        raise Refused("planner catalog differs from its saved source capture")


def immutable_archive_url(release: str, peeled_commit: str) -> str:
    if not isinstance(peeled_commit, str) or not rehearsal.GIT_OID_RE.fullmatch(peeled_commit):
        raise Refused("immutable archive URL requires a commit object id")
    return rehearsal.archive_url(release, peeled_commit)


def _read_limited(response: Response, limit: int) -> bytes:
    if len(response.body) > limit:
        raise Refused("archive exceeds configured byte limit")
    return response.body


def _verify_tar_gz(body: bytes) -> None:
    try:
        with gzip.GzipFile(fileobj=io.BytesIO(body)) as stream:
            with tarfile.open(fileobj=stream, mode="r:") as archive:
                if not archive.getmembers():
                    raise Refused("archive has no members")
    except (OSError, tarfile.TarError, EOFError) as exc:
        raise Refused("archive bytes are not a readable tar.gz") from exc


def _archive_cache_key(sha256: str) -> str:
    if not isinstance(sha256, str) or rehearsal.SHA256_RE.fullmatch(sha256) is None:
        raise Refused("archive cache key requires a SHA-256 digest")
    return f"{ARCHIVE_CACHE_DIRECTORY}/{sha256}.tar.gz"


def _archive_cache_path(cache_root: Path, cache_key: str) -> Path:
    expected_prefix = f"{ARCHIVE_CACHE_DIRECTORY}/"
    if (not isinstance(cache_key, str) or not cache_key.startswith(expected_prefix)
            or cache_key != _archive_cache_key(cache_key.removeprefix(expected_prefix).removesuffix(".tar.gz"))):
        raise Refused("archive cache key is malformed")
    return cache_root / Path(*cache_key.split("/"))


def _store_archive(cache_root: Path, body: bytes, archive: dict[str, Any]) -> str:
    """Persist exact bytes under their digest without trusting a release name."""
    sha256 = archive.get("sha256")
    byte_count = archive.get("bytes")
    if sha256_bytes(body) != sha256 or len(body) != byte_count:
        raise Refused("archive bytes differ from their resolved identity")
    cache_key = _archive_cache_key(sha256)
    path = _archive_cache_path(cache_root, cache_key)
    path.parent.mkdir(parents=True, exist_ok=True)
    if path.exists() or path.is_symlink():
        if path.is_symlink() or not path.is_file():
            raise Refused("archive cache entry is not a regular file")
        cached = path.read_bytes()
        if sha256_bytes(cached) != sha256 or len(cached) != byte_count:
            raise Refused("archive cache entry differs from resolved identity")
        return cache_key
    descriptor, temporary_name = tempfile.mkstemp(prefix=f".{sha256}.", suffix=".partial", dir=path.parent)
    os.close(descriptor)
    temporary = Path(temporary_name)
    try:
        with temporary.open("wb") as stream:
            stream.write(body)
            stream.flush()
            os.fsync(stream.fileno())
        try:
            os.link(temporary, path)
        except FileExistsError:
            cached = path.read_bytes()
            if path.is_symlink() or sha256_bytes(cached) != sha256 or len(cached) != byte_count:
                raise Refused("archive cache entry differs from resolved identity")
        finally:
            temporary.unlink(missing_ok=True)
    except OSError as exc:
        temporary.unlink(missing_ok=True)
        raise Refused(f"cannot preserve resolved archive bytes: {exc}") from exc
    return cache_key


def _read_cached_archive(cache_root: Path, archive: dict[str, Any]) -> bytes:
    sha256 = archive.get("sha256")
    byte_count = archive.get("bytes")
    cache_key = archive.get("cache_key")
    if cache_key != _archive_cache_key(sha256):
        raise Refused("resolved archive cache key differs from digest")
    path = _archive_cache_path(cache_root, cache_key)
    if path.is_symlink() or not path.is_file():
        raise Refused("resolved archive cache entry is unavailable")
    body = path.read_bytes()
    if len(body) != byte_count or sha256_bytes(body) != sha256:
        raise Refused("resolved archive cache bytes differ from manifest")
    _verify_tar_gz(body)
    return body


def resolve_selected_archives(plan: dict[str, Any], catalog: dict[str, Any], capture: dict[str, Any], get: Callable[[str], Response], archive_cache: Path, max_archive_bytes: int = MAX_ARCHIVE_BYTES) -> dict[str, Any]:
    """Fetch/hash and durably retain exactly the plan-selected archives."""
    verify_capture_binding(capture, catalog)
    rehearsal.verify_plan(plan, catalog)
    if not isinstance(max_archive_bytes, int) or isinstance(max_archive_bytes, bool) or max_archive_bytes < 1:
        raise Refused("archive byte limit must be positive")
    selected = {
        side["release"]: side
        for pair in plan["pairs"]
        for side in (pair["old"], pair["new"])
    }
    required_urls: dict[str, str] = {}
    for pair in plan["pairs"]:
        requirement = pair.get("source_resolution")
        if (not isinstance(requirement, dict) or requirement.get("state") != "unresolved"
                or not isinstance(requirement.get("required_archive_urls"), dict)):
            raise Refused("selected pair lacks unresolved archive requirement")
        for label in ("old", "new"):
            identity = pair[label]
            expected_url = immutable_archive_url(identity["release"], identity["peeled_commit"])
            if requirement["required_archive_urls"].get(label) != expected_url:
                raise Refused("selected pair archive requirement is not commit-pinned")
            required_urls[identity["release"]] = expected_url
    resolved: list[dict[str, Any]] = []
    for release, identity in sorted(selected.items(), key=lambda item: rehearsal.release_key(item[0])):
        url = required_urls[release]
        try:
            response = get(url)
            if response.status != 200:
                raise Refused(f"archive HTTP status {response.status}")
            body = _read_limited(response, max_archive_bytes)
            _verify_tar_gz(body)
        except Refused as exc:
            raise Refused(f"selected archive {release} cannot be resolved: {exc}") from exc
        archive = {"url": url, "sha256": sha256_bytes(body), "bytes": len(body), "format": "tar.gz"}
        archive["cache_key"] = _store_archive(archive_cache, body, archive)
        resolved.append({**identity, "archive": archive})
    payload = {
        "schema": RESOLUTION_SCHEMA,
        "kind": "oxidex_exiftool_selected_source_resolution",
        "plan_sha256": plan["plan_sha256"],
        "catalog_sha256": catalog["catalog_sha256"],
        "capture_sha256": capture["capture_sha256"],
        "selected_releases": resolved,
        "execution": {"state": "source_identity_resolved_only", "native_read": "unrun", "native_write": "unrun"},
    }
    return {**payload, "resolution_sha256": sha256_json(payload)}


def _safe_archive_members(body: bytes) -> list[tuple[tarfile.TarInfo, tuple[str, ...]]]:
    """Validate a tarball before extracting any member into a source tree."""
    try:
        with gzip.GzipFile(fileobj=io.BytesIO(body)) as stream:
            with tarfile.open(fileobj=stream, mode="r:") as archive:
                members = archive.getmembers()
    except (OSError, tarfile.TarError, EOFError) as exc:
        raise Refused("archive bytes are not a readable tar.gz") from exc
    if not members:
        raise Refused("archive has no members")
    roots: set[str] = set()
    result: list[tuple[tarfile.TarInfo, tuple[str, ...]]] = []
    destinations: set[tuple[str, ...]] = set()
    regular_paths: set[tuple[str, ...]] = set()
    for member in members:
        name = member.name
        if not isinstance(name, str) or not name or "\\" in name:
            raise Refused("archive member has an unsafe path")
        parts = tuple(name.split("/"))
        if any(part in {"", ".", ".."} for part in parts):
            raise Refused("archive member has an unsafe path")
        if name.startswith("/") or member.issym() or member.islnk() or not (member.isfile() or member.isdir()):
            raise Refused("archive member type is unsupported")
        roots.add(parts[0])
        relative = parts[1:]
        if not relative:
            if not member.isdir():
                raise Refused("archive member is outside its top-level source directory")
            continue
        if relative in destinations:
            raise Refused("archive has duplicate extraction destinations")
        destinations.add(relative)
        for parent_index in range(1, len(relative)):
            if relative[:parent_index] in regular_paths:
                raise Refused("archive member descends through a regular file")
        if member.isfile():
            if any(destination[:len(relative)] == relative for destination in destinations if destination != relative):
                raise Refused("archive regular file conflicts with a directory")
            regular_paths.add(relative)
        result.append((member, relative))
    if len(roots) != 1:
        raise Refused("archive must have one top-level source directory")
    if not result or not any(member.isfile() for member, _ in result):
        raise Refused("archive source directory has no regular files")
    return result


def _source_directory_name(release: str, peeled_commit: str) -> str:
    if not isinstance(release, str) or rehearsal.RELEASE_RE.fullmatch(release) is None:
        raise Refused("source directory requires a numeric release")
    if not isinstance(peeled_commit, str) or rehearsal.GIT_OID_RE.fullmatch(peeled_commit) is None:
        raise Refused("source directory requires a commit object id")
    return f"exiftool-{release}-{peeled_commit}"


def _tree_identity_from_files(files: list[dict[str, Any]]) -> dict[str, Any]:
    """Return the canonical regular-file identity used for an archive or tree."""
    if not files:
        raise Refused("materialized source has no regular files")
    if any(
        not isinstance(item, dict)
        or not isinstance(item.get("path"), str)
        or not item["path"]
        or not isinstance(item.get("sha256"), str)
        or rehearsal.SHA256_RE.fullmatch(item["sha256"]) is None
        or not isinstance(item.get("bytes"), int)
        or item["bytes"] < 0
        for item in files
    ):
        raise Refused("materialized source file identity is malformed")
    files = sorted(files, key=lambda item: item["path"])
    if len({item["path"] for item in files}) != len(files):
        raise Refused("materialized source has duplicate regular files")
    payload = {"files": files}
    return {**payload, "tree_sha256": sha256_json(payload)}


def _tree_identity(root: Path) -> dict[str, Any]:
    files: list[dict[str, Any]] = []
    for path in sorted(root.rglob("*")):
        if path.is_symlink():
            raise Refused("materialized source contains a symbolic link")
        if path.is_dir():
            continue
        if not path.is_file():
            raise Refused("materialized source contains a non-regular file")
        relative = path.relative_to(root).as_posix()
        body = path.read_bytes()
        files.append({"path": relative, "sha256": sha256_bytes(body), "bytes": len(body)})
    return _tree_identity_from_files(files)


def _archive_tree_identity(body: bytes) -> dict[str, Any]:
    """Derive the expected extracted tree directly from verified archive bytes.

    A saved materialization manifest is evidence, never authority for the
    extracted source.  This reads exactly the same validated regular members
    that extraction permits, so a rehashed local tree cannot authenticate
    different source bytes against the retained archive.
    """
    validated = _safe_archive_members(body)
    files: list[dict[str, Any]] = []
    try:
        with gzip.GzipFile(fileobj=io.BytesIO(body)) as stream:
            with tarfile.open(fileobj=stream, mode="r:") as archive:
                for original, relative in validated:
                    if original.isdir():
                        continue
                    member = archive.getmember(original.name)
                    source = archive.extractfile(member)
                    if source is None:
                        raise Refused("archive regular member cannot be read")
                    with source:
                        member_bytes = source.read()
                    if len(member_bytes) != member.size:
                        raise Refused("archive regular member size differs from header")
                    files.append({
                        "path": "/".join(relative),
                        "sha256": sha256_bytes(member_bytes),
                        "bytes": len(member_bytes),
                    })
    except (OSError, tarfile.TarError, EOFError) as exc:
        raise Refused("archive bytes are not a readable tar.gz") from exc
    return _tree_identity_from_files(files)


def _extract_archive_to_new_source(body: bytes, destination: Path) -> dict[str, Any]:
    if destination.exists() or destination.is_symlink():
        raise Refused("materialized source directory already exists")
    destination.parent.mkdir(parents=True, exist_ok=True)
    temporary = Path(tempfile.mkdtemp(prefix=f".{destination.name}.", suffix=".partial", dir=destination.parent))
    try:
        validated = _safe_archive_members(body)
        expected_identity = _archive_tree_identity(body)
        with gzip.GzipFile(fileobj=io.BytesIO(body)) as stream:
            with tarfile.open(fileobj=stream, mode="r:") as archive:
                for original, relative in validated:
                    member = archive.getmember(original.name)
                    path = temporary.joinpath(*relative)
                    if member.isdir():
                        path.mkdir(parents=True, exist_ok=False)
                        os.chmod(path, member.mode & 0o777)
                        continue
                    path.parent.mkdir(parents=True, exist_ok=True)
                    source = archive.extractfile(member)
                    if source is None:
                        raise Refused("archive regular member cannot be read")
                    with source, path.open("xb") as target:
                        shutil.copyfileobj(source, target)
                    os.chmod(path, member.mode & 0o777)
        identity = _tree_identity(temporary)
        if identity != expected_identity:
            raise Refused("extracted source tree differs from verified archive")
        os.replace(temporary, destination)
        return identity
    except (OSError, tarfile.TarError, EOFError) as exc:
        raise Refused(f"cannot extract selected archive safely: {exc}") from exc
    finally:
        if temporary.exists():
            shutil.rmtree(temporary)


def materialize_selected_sources(
    plan: dict[str, Any], catalog: dict[str, Any], capture: dict[str, Any], resolution: dict[str, Any],
    archive_cache: Path, source_root: Path,
) -> dict[str, Any]:
    """Extract verified retained archives into new per-version source trees.

    This stage records failures rather than converting an incomplete source
    population into a success.  It does not build OxiDex or establish a native
    read/write oracle result.
    """
    verify_source_resolution(resolution, plan, catalog, capture)
    try:
        source_root.mkdir(parents=True, exist_ok=True)
    except OSError as exc:
        raise Refused(f"cannot prepare source materialization root: {exc}") from exc
    selected: list[dict[str, Any]] = []
    for row in resolution["selected_releases"]:
        destination_name = _source_directory_name(row["release"], row["peeled_commit"])
        result = {
            "release": row["release"], "tag_object": row["tag_object"], "peeled_commit": row["peeled_commit"],
            "archive": row["archive"], "source_directory": destination_name,
        }
        try:
            body = _read_cached_archive(archive_cache, row["archive"])
            result.update({"state": "materialized", "tree": _extract_archive_to_new_source(body, source_root / destination_name)})
        except (Refused, OSError) as exc:
            result.update({"state": "failed", "failure": str(exc)})
        selected.append(result)
    complete = all(row["state"] == "materialized" for row in selected)
    payload = {
        "schema": MATERIALIZATION_SCHEMA,
        "kind": "oxidex_exiftool_selected_source_materialization",
        "plan_sha256": plan["plan_sha256"],
        "catalog_sha256": catalog["catalog_sha256"],
        "capture_sha256": capture["capture_sha256"],
        "resolution_sha256": resolution["resolution_sha256"],
        "archive_cache_layout": f"{ARCHIVE_CACHE_DIRECTORY}/<archive-sha256>.tar.gz",
        "source_directory_naming": "exiftool-<release>-<full-peeled-commit>",
        "complete": complete,
        "selected_releases": selected,
        "execution": {
            "state": "source_materialization_only",
            "native_read": "unrun", "native_write": "unrun",
            "native_old_to_native_new_delta": "unrun",
            "limit": "materialized source is not a generated build, native oracle, or conformance result",
        },
    }
    return {**payload, "materialization_sha256": sha256_json(payload)}


def verify_source_resolution(resolution: dict[str, Any], plan: dict[str, Any], catalog: dict[str, Any], capture: dict[str, Any]) -> None:
    verify_capture_binding(capture, catalog)
    rehearsal.verify_plan(plan, catalog)
    payload = {key: value for key, value in resolution.items() if key != "resolution_sha256"}
    if (resolution.get("schema") != RESOLUTION_SCHEMA or resolution.get("kind") != "oxidex_exiftool_selected_source_resolution"
            or resolution.get("plan_sha256") != plan["plan_sha256"] or resolution.get("catalog_sha256") != catalog["catalog_sha256"]
            or resolution.get("capture_sha256") != capture["capture_sha256"]
            or resolution.get("resolution_sha256") != sha256_json(payload) or not isinstance(resolution.get("selected_releases"), list)):
        raise Refused("source resolution identity changed or is malformed")
    expected = {
        side["release"]: side
        for pair in plan["pairs"]
        for side in (pair["old"], pair["new"])
    }
    if len(resolution["selected_releases"]) != len(expected) or {row.get("release") for row in resolution["selected_releases"] if isinstance(row, dict)} != set(expected):
        raise Refused("source resolution has missing or extra selected releases")
    for row in resolution["selected_releases"]:
        if not isinstance(row, dict) or row.get("release") not in expected:
            raise Refused("source resolution has unselected release")
        identity = expected[row["release"]]
        if any(row.get(key) != identity[key] for key in ("release", "tag_object", "peeled_commit")):
            raise Refused("source resolution identity differs from selected plan")
        archive = row.get("archive")
        if (not isinstance(archive, dict) or archive.get("url") != immutable_archive_url(row["release"], identity["peeled_commit"])
                or not isinstance(archive.get("sha256"), str) or not rehearsal.SHA256_RE.fullmatch(archive["sha256"])
                or not isinstance(archive.get("bytes"), int) or archive["bytes"] < 1 or archive.get("format") != "tar.gz"):
            raise Refused("source resolution archive identity is malformed")
        if archive.get("cache_key") != _archive_cache_key(archive["sha256"]):
            raise Refused("source resolution archive cache key is malformed")


def verify_source_materialization(
    materialization: dict[str, Any], plan: dict[str, Any], catalog: dict[str, Any], capture: dict[str, Any],
    resolution: dict[str, Any], archive_cache: Path, source_root: Path, *, require_complete: bool = True,
) -> None:
    """Verify a materialization record and, for use, its still-intact trees."""
    verify_source_resolution(resolution, plan, catalog, capture)
    payload = {key: value for key, value in materialization.items() if key != "materialization_sha256"}
    if (
        materialization.get("schema") != MATERIALIZATION_SCHEMA
        or materialization.get("kind") != "oxidex_exiftool_selected_source_materialization"
        or materialization.get("plan_sha256") != plan["plan_sha256"]
        or materialization.get("catalog_sha256") != catalog["catalog_sha256"]
        or materialization.get("capture_sha256") != capture["capture_sha256"]
        or materialization.get("resolution_sha256") != resolution["resolution_sha256"]
        or materialization.get("archive_cache_layout") != f"{ARCHIVE_CACHE_DIRECTORY}/<archive-sha256>.tar.gz"
        or materialization.get("source_directory_naming") != "exiftool-<release>-<full-peeled-commit>"
        or materialization.get("materialization_sha256") != sha256_json(payload)
        or not isinstance(materialization.get("complete"), bool)
        or not isinstance(materialization.get("selected_releases"), list)
    ):
        raise Refused("source materialization identity changed or is malformed")
    expected = {row["release"]: row for row in resolution["selected_releases"]}
    rows = materialization["selected_releases"]
    if len(rows) != len(expected) or {row.get("release") for row in rows if isinstance(row, dict)} != set(expected):
        raise Refused("source materialization has missing or extra selected releases")
    all_materialized = True
    for row in rows:
        if not isinstance(row, dict) or row.get("release") not in expected:
            raise Refused("source materialization has an invalid selected release")
        resolved = expected[row["release"]]
        if any(row.get(key) != resolved[key] for key in ("release", "tag_object", "peeled_commit", "archive")):
            raise Refused("source materialization release differs from source resolution")
        destination = _source_directory_name(row["release"], row["peeled_commit"])
        if row.get("source_directory") != destination:
            raise Refused("source materialization directory name differs from release identity")
        state = row.get("state")
        if state == "materialized":
            if not isinstance(row.get("tree"), dict) or "failure" in row:
                raise Refused("materialized source record is malformed")
            body = _read_cached_archive(archive_cache, resolved["archive"])
            expected_tree = _archive_tree_identity(body)
            if row["tree"] != expected_tree:
                raise Refused("materialized source manifest differs from verified archive")
            source = source_root / destination
            if source.is_symlink() or not source.is_dir() or _tree_identity(source) != expected_tree:
                raise Refused("materialized source tree differs from verified archive")
        elif state == "failed":
            all_materialized = False
            if not isinstance(row.get("failure"), str) or not row["failure"] or "tree" in row:
                raise Refused("failed source materialization record is malformed")
        else:
            raise Refused("source materialization state is unsupported")
    if materialization["complete"] != all_materialized:
        raise Refused("source materialization completeness disagrees with release states")
    if require_complete and not all_materialized:
        raise Refused("source materialization is incomplete")


def _cmd_capture(args: argparse.Namespace) -> int:
    output = Path(args.output)
    if output.exists():
        raise Refused(f"capture output already exists: {output}")
    capture = capture_tag_catalog(lambda url: http_get(url, args.timeout))
    atomic_json(output, capture)
    try:
        raw_catalog = raw_catalog_from_capture(capture)
    except Refused as exc:
        print(json.dumps({"capture": str(output), "selection_ready": False, "reason": str(exc)}, sort_keys=True))
        return 2
    if args.catalog_output:
        catalog_output = Path(args.catalog_output)
        if catalog_output.exists():
            raise Refused(f"catalog output already exists: {catalog_output}")
        atomic_json(catalog_output, raw_catalog)
    print(json.dumps({"capture": str(output), "capture_sha256": capture["capture_sha256"], "selection_ready": True, "native_read": "unrun", "native_write": "unrun"}, sort_keys=True))
    return 0


def _cmd_resolve(args: argparse.Namespace) -> int:
    output = Path(args.output)
    if output.exists():
        raise Refused(f"resolution output already exists: {output}")
    capture = rehearsal.read_json(Path(args.capture))
    catalog = rehearsal.normalize_catalog(rehearsal.read_json(Path(args.catalog)))
    plan = rehearsal.read_json(Path(args.plan))
    resolution = resolve_selected_archives(
        plan, catalog, capture, lambda url: http_get(url, args.timeout, args.max_archive_bytes),
        Path(args.archive_cache), args.max_archive_bytes,
    )
    atomic_json(output, resolution)
    print(json.dumps({"resolution": str(output), "resolution_sha256": resolution["resolution_sha256"],
                      "archive_cache": "preserved", "native_read": "unrun", "native_write": "unrun"}, sort_keys=True))
    return 0


def _cmd_materialize(args: argparse.Namespace) -> int:
    output = Path(args.output)
    if output.exists():
        raise Refused(f"materialization output already exists: {output}")
    capture = rehearsal.read_json(Path(args.capture))
    catalog = rehearsal.normalize_catalog(rehearsal.read_json(Path(args.catalog)))
    plan = rehearsal.read_json(Path(args.plan))
    resolution = rehearsal.read_json(Path(args.resolution))
    materialization = materialize_selected_sources(
        plan, catalog, capture, resolution, Path(args.archive_cache), Path(args.source_root)
    )
    atomic_json(output, materialization)
    print(json.dumps({"materialization": str(output), "materialization_sha256": materialization["materialization_sha256"],
                      "complete": materialization["complete"], "native_read": "unrun", "native_write": "unrun"}, sort_keys=True))
    return 0 if materialization["complete"] else 2


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    capture = sub.add_parser("capture", help="capture official tag pages and tag-to-commit identities only")
    capture.add_argument("--output", required=True)
    capture.add_argument("--catalog-output", help="optional raw planner input; written only after complete identity capture")
    capture.add_argument("--timeout", type=float, default=20.0)
    capture.set_defaults(func=_cmd_capture)
    resolve = sub.add_parser("resolve-selected", help="fetch/hash immutable commit archives for already selected releases")
    resolve.add_argument("--capture", required=True, help="raw official capture bound to the planner catalog")
    resolve.add_argument("--catalog", required=True, help="raw capture-derived catalog input")
    resolve.add_argument("--plan", required=True)
    resolve.add_argument("--output", required=True)
    resolve.add_argument("--archive-cache", required=True, help="durable content-addressed archive cache root")
    resolve.add_argument("--timeout", type=float, default=30.0)
    resolve.add_argument("--max-archive-bytes", type=int, default=MAX_ARCHIVE_BYTES)
    resolve.set_defaults(func=_cmd_resolve)
    materialize = sub.add_parser("materialize-selected", help="safely extract retained selected archives into new source directories")
    materialize.add_argument("--capture", required=True)
    materialize.add_argument("--catalog", required=True)
    materialize.add_argument("--plan", required=True)
    materialize.add_argument("--resolution", required=True)
    materialize.add_argument("--archive-cache", required=True, help="archive cache created by resolve-selected")
    materialize.add_argument("--source-root", required=True, help="new per-version source directories are created here")
    materialize.add_argument("--output", required=True)
    materialize.set_defaults(func=_cmd_materialize)
    args = parser.parse_args(argv)
    try:
        if hasattr(args, "timeout") and args.timeout <= 0:
            raise Refused("timeout must be positive")
        return args.func(args)
    except Refused as exc:
        print(f"version rehearsal catalog refused: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
