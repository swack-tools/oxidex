#!/usr/bin/env python3
"""Capture and resolve immutable source identities for version rehearsals.

The capture stage reads only GitHub's official ExifTool tag APIs.  It preserves
exact page bodies and digest records, then resolves each numeric tag through
its ref (and annotated-tag chain when needed) to an immutable commit.  Pair
selection uses that complete tag/commit population.  Archive bytes are fetched
and hashed only after a plan selects releases, so pre-existing archive cache
contents cannot bias selection.

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
import sys
import tarfile
import time
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable

import version_rehearsal as rehearsal

CAPTURE_SCHEMA = 1
RESOLUTION_SCHEMA = 1
REPOSITORY = "exiftool/exiftool"
REPOSITORY_ID = "132751855"
API_ROOT = "https://api.github.com"
TAG_PAGE_URL = f"{API_ROOT}/repos/{REPOSITORY}/tags?per_page=100&page=1"
MAX_TAG_DEPTH = 8
MAX_ARCHIVE_BYTES = 128 * 1024 * 1024
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
    return (parsed.path.startswith(f"/repos/{REPOSITORY}/")
            or parsed.path.startswith(f"/repositories/{REPOSITORY_ID}/tags"))


def _next_url(headers: dict[str, str]) -> str | None:
    link = next((value for key, value in headers.items() if key.lower() == "link"), "")
    match = LINK_NEXT_RE.search(link)
    if match is None:
        return None
    url = match.group(1)
    if not _official_api_url(url):
        raise Refused("pagination next link is not the official ExifTool API")
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
    complete = True
    while current is not None:
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
    source_rows: list[Any] = []
    for page_index, page in enumerate(capture["pages"]):
        if not isinstance(page, dict) or not _official_api_url(page.get("url", "")) or page.get("status") != 200:
            raise Refused("capture page identity is malformed")
        text = page.get("body_utf8")
        if not isinstance(text, str) or page.get("body_sha256") != sha256_bytes(text.encode("utf-8")):
            raise Refused("capture page body digest differs")
        if not isinstance(page.get("link_header"), str):
            raise Refused("capture page link header is malformed")
        if page.get("next_url") != _next_url({"Link": page["link_header"]}):
            raise Refused("capture pagination link differs from raw header")
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


def resolve_selected_archives(plan: dict[str, Any], catalog: dict[str, Any], capture: dict[str, Any], get: Callable[[str], Response], max_archive_bytes: int = MAX_ARCHIVE_BYTES) -> dict[str, Any]:
    """Fetch/hash exactly the unique releases selected by an immutable plan."""
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
        resolved.append({
            **identity,
            "archive": {"url": url, "sha256": sha256_bytes(body), "bytes": len(body), "format": "tar.gz"},
        })
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
    resolution = resolve_selected_archives(plan, catalog, capture, lambda url: http_get(url, args.timeout, args.max_archive_bytes), args.max_archive_bytes)
    atomic_json(output, resolution)
    print(json.dumps({"resolution": str(output), "resolution_sha256": resolution["resolution_sha256"], "native_read": "unrun", "native_write": "unrun"}, sort_keys=True))
    return 0


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
    resolve.add_argument("--timeout", type=float, default=30.0)
    resolve.add_argument("--max-archive-bytes", type=int, default=MAX_ARCHIVE_BYTES)
    resolve.set_defaults(func=_cmd_resolve)
    args = parser.parse_args(argv)
    try:
        if args.timeout <= 0:
            raise Refused("timeout must be positive")
        return args.func(args)
    except Refused as exc:
        print(f"version rehearsal catalog refused: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
