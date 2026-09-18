#!/usr/bin/env python3
"""Refuse a change that loses a corpus read the published snapshot proved.

The parity ratchet (`parity_ratchet.py`) guards the committed measurements
under `docs/public/measurements/`, but those only move when someone
regenerates them. A pull request that breaks a reader leaves the snapshot
untouched, so the ratchet sees nothing and the loss surfaces at the next
manual refresh. This closes that gap by measuring the head itself.

`catalog-corpus-observed-<pin>.json` marks every catalog entry whose exact
source row OxiDex was proven to read as `observed_matched_read`. A single
authenticated corpus read receipt (`corpus_read_receipt.py`) taken at the head
names every `(table, tag ID, variant)` coordinate the head is credited with.
Credit only ever grows after the snapshot unless something regressed: the
ExifTool oracle and corpus are pinned, and a coordinate loses credit only when
OxiDex stops matching ExifTool in a file that exercises it. So no base
receipt is needed -- every published entry must still be credited.

The receipt's coordinates are mapped onto catalog identities by
`catalog_corpus_reads.catalog_credit`, the same mapping that produced the
snapshot's `observed_matched_read` set in the first place.

Verdicts, with distinct exit codes so a slow runner can never look like a
code regression and a code regression can never look like a slow runner:

  0  PASS        every published read is still credited. Newly credited
                 entries are reported as information only (the snapshot is
                 stale and worth refreshing), never as a failure.
  1  REGRESSION  a published entry ExifTool still reads in this corpus is no
                 longer credited, or OxiDex exited non-zero / printed
                 unparseable JSON on a corpus file. Each is named.
  2  REFUSED     the measurement itself cannot be trusted: a public read timed
                 out (runner load), the receipt fails its integrity replay,
                 the ExifTool sources or corpus differ from the snapshot's, a
                 published row is no longer read by ExifTool at all, or the
                 snapshot is empty or malformed. Never a pass.
"""
from __future__ import annotations

import argparse
import json
import sys
from collections import Counter
from dataclasses import dataclass, field
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SNAPSHOT_SCHEMA = "oxidex_catalog_observed_snapshot_v2"
OBSERVED_STATE = "observed_matched_read"
TIMEOUT_RETURNCODE = -1  # corpus_read_receipt.transcript records a timeout as -1
EXIT_PASS, EXIT_REGRESSION, EXIT_REFUSED = 0, 1, 2


class Refused(Exception):
    """The measurement is degraded or does not describe what it claims to."""


@dataclass
class Published:
    reads: set            # identities proven read by the snapshot
    catalog_ids: set      # every catalog identity the snapshot joins
    names: dict           # identity -> catalog tag name, for messages
    corpus_files: int     # corpus size the snapshot's receipt observed
    source_commit: str


@dataclass
class Verdict:
    status: str
    reasons: list = field(default_factory=list)
    lost: list = field(default_factory=list)
    newly_credited: list = field(default_factory=list)
    public_failures: list = field(default_factory=list)

    @property
    def exit_code(self) -> int:
        return {"PASS": EXIT_PASS, "REGRESSION": EXIT_REGRESSION}.get(self.status, EXIT_REFUSED)


def _identity(entry) -> tuple:
    identity = entry.get("identity") if isinstance(entry, dict) else None
    if not isinstance(identity, dict):
        raise Refused("snapshot entry has no identity")
    table, raw_key, variant = identity.get("table"), identity.get("raw_key"), identity.get("variant_index")
    if (not isinstance(table, str) or not table or not isinstance(raw_key, str)
            or type(variant) is not int or variant < 0):
        raise Refused(f"snapshot identity is malformed: {identity!r}")
    return table, raw_key, variant


def published_reads(snapshot, expected_version: str) -> Published:
    """The snapshot's proven reads. An empty or inconsistent snapshot refuses; it never passes."""
    if not isinstance(snapshot, dict) or snapshot.get("schema") != SNAPSHOT_SCHEMA:
        raise Refused(f"snapshot schema is not {SNAPSHOT_SCHEMA}")
    join = snapshot.get("observed_join")
    if not isinstance(join, dict) or not isinstance(join.get("entries"), list) or not join["entries"]:
        raise Refused("snapshot carries no catalog entries")
    if join.get("inputs", {}).get("exiftool_version") != expected_version:
        raise Refused(f"snapshot was not measured against the pinned ExifTool {expected_version}")
    reads, catalog_ids, names, states = set(), set(), {}, Counter()
    for entry in join["entries"]:
        identity = _identity(entry)
        if identity in catalog_ids:
            raise Refused(f"snapshot repeats identity {identity}")
        catalog_ids.add(identity)
        state = entry.get("observed_read")
        if not isinstance(state, str):
            raise Refused(f"snapshot entry {identity} has no observed_read state")
        states[state] += 1
        names[identity] = (entry.get("catalog") or {}).get("name", "?")
        if state == OBSERVED_STATE:
            reads.add(identity)
    counts = join.get("counts") or {}
    if counts.get("observed_read") != dict(states):
        raise Refused("snapshot observed_read counts disagree with its own entries")
    if not reads:
        raise Refused("snapshot proves no reads; there is nothing to protect and nothing to pass on")
    attribution = counts.get("corpus_read_attribution") or {}
    if attribution.get("credited_catalog_entries") != len(reads):
        raise Refused("snapshot credited_catalog_entries disagrees with its observed_matched_read entries")
    corpus_files = (attribution.get("metric_c") or {}).get("corpus_files")
    if type(corpus_files) is not int or corpus_files <= 0:
        raise Refused("snapshot does not record the corpus size it observed")
    commit = (snapshot.get("native_evidence") or {}).get("source_commit", "?")
    return Published(reads, catalog_ids, names, corpus_files, commit)


def evaluate(published: Published, credited: set, native: set, public_failures: list,
             corpus_files: int) -> Verdict:
    """Pure verdict over catalog-mapped head credit.

    `credited` and `native` are catalog identities credited to / read natively
    by the head's receipt; `public_failures` is `[(file, mode, returncode)]`
    for every public read that failed (returncode 0 = unparseable output).
    """
    if corpus_files != published.corpus_files:
        return Verdict("REFUSED", [f"corpus differs from the snapshot's: {corpus_files} files observed, "
                                   f"snapshot observed {published.corpus_files}"])
    timeouts = [failure for failure in public_failures if failure[2] == TIMEOUT_RETURNCODE]
    if timeouts:
        return Verdict("REFUSED", [f"measurement degraded: {len(timeouts)} public read(s) timed out; this is "
                                   "runner load, not a verdict on the code -- re-run the job"],
                       public_failures=sorted(public_failures))
    if not credited <= published.catalog_ids or not native <= published.catalog_ids:
        return Verdict("REFUSED", ["head credit names identities outside the snapshot's catalog"])
    lost = published.reads - credited
    unread = sorted(lost - native)
    if unread:
        return Verdict("REFUSED", [f"measurement differs from the snapshot's: pinned ExifTool no longer reads "
                                   f"{len(unread)} published row(s) in this corpus, so their credit cannot be "
                                   "judged (oracle or corpus is not the snapshot's)"], lost=unread)
    newly = sorted(credited - published.reads)
    reasons = []
    if public_failures:
        reasons.append(f"{len(public_failures)} public read(s) failed (non-zero exit or unparseable JSON)")
    if lost:
        reasons.append(f"{len(lost)} published read(s) are no longer credited")
    if reasons:
        return Verdict("REGRESSION", reasons, lost=sorted(lost), newly_credited=newly,
                       public_failures=sorted(public_failures))
    return Verdict("PASS", newly_credited=newly)


def _public_failures(receipt) -> list:
    """Every failed public read in a validated receipt, reclassified from its own transcripts."""
    import corpus_read_receipt
    failures = []
    for row in receipt["observations"]:
        fact = row["oxidex"]
        if fact["returncode"] != 0:
            failures.append((row["file"], row["mode"], fact["returncode"]))
            continue
        try:
            corpus_read_receipt.identities(bytes.fromhex(fact["stdout_hex"]), "public")
        except ValueError:
            failures.append((row["file"], row["mode"], 0))
    return failures


def measure(receipt_path: Path, snapshot_path: Path, catalog_path: Path, root: Path = ROOT):
    """Load, authenticate and map a head receipt. -> (Published, Verdict, receipt)."""
    sys.path.insert(0, str(root / "tools/exiftool-tables"))
    import catalog_corpus_reads
    import corpus_read_receipt
    import runtime_evidence_inputs
    pin = corpus_read_receipt.pin(root)
    try:
        snapshot = json.loads(snapshot_path.read_bytes())
        catalog = json.loads(catalog_path.read_bytes())
        receipt = json.loads(receipt_path.read_bytes())
    except (OSError, ValueError) as error:
        raise Refused(f"cannot read gate inputs: {error}") from error
    published = published_reads(snapshot, pin)
    try:
        if catalog.get("exiftool_version") != pin:
            raise ValueError(f"catalog was not built from the pinned ExifTool {pin}")
        # Integrity replay: transcript hashes, command shapes, build proof,
        # ExifTool version, DOCX capability, canonical Perl, and every count.
        # A native ExifTool failure or timeout also refuses here.
        derived = corpus_read_receipt.validate(receipt, pin)
        if receipt["producer"]["runtime_input_manifest_sha256"] != \
                runtime_evidence_inputs.runtime_input_manifest(root):
            raise ValueError("receipt does not measure this checkout's runtime")
        catalog_corpus_reads.check_exiftool_sources(receipt, catalog)
        credited, native, _ = catalog_corpus_reads.catalog_credit(receipt, derived, published.catalog_ids)
        failures = _public_failures(receipt)
        if len(failures) != derived["metric_c"]["public_failed_file_modes"]:
            raise ValueError("public failure reclassification disagrees with the receipt's count")
    except (KeyError, TypeError, ValueError) as error:
        raise Refused(f"receipt refused: {error}") from error
    verdict = evaluate(published, credited, native, failures, derived["metric_c"]["corpus_files"])
    return published, verdict, receipt


def render(published: Published, verdict: Verdict, receipt=None) -> str:
    lines = ["=== instrument: read_regression_gate.py (corpus_read_receipt.py receipt vs "
             "published catalog-corpus-observed snapshot) ==="]
    if receipt is not None:
        native = receipt["native"]
        lines += [f"head:     {receipt['producer']['source_commit']} (runtime "
                  f"{receipt['producer']['runtime_input_manifest_sha256'][:12]})",
                  f"oxidex:   {receipt['build_proof']['binary']['path']} "
                  f"({receipt['build_proof']['binary']['sha256'][:12]})",
                  f"native:   ExifTool {native['exiftool_version']} via {native['perl']['path']} (DOCX probe ok)",
                  f"corpus:   {receipt['corpus']['root']} ({len(receipt['corpus']['files'])} files)"]
    lines.append(f"snapshot: {published.source_commit} ({len(published.reads)} observed_matched_read entries, "
                 f"{published.corpus_files} corpus files)")
    for file, mode, code in verdict.public_failures:
        kind = "timeout" if code == TIMEOUT_RETURNCODE else ("unparseable JSON" if code == 0 else f"exit {code}")
        lines.append(f"PUBLIC-FAILED {file} [{mode}] {kind}")
    for identity in verdict.lost:
        lines.append(f"LOST {identity[0]} {identity[1]} v{identity[2]} ({published.names.get(identity, '?')})")
    for identity in verdict.newly_credited:
        lines.append(f"info: NEWLY-CREDITED {identity[0]} {identity[1]} v{identity[2]} "
                     f"({published.names.get(identity, '?')})")
    lines.append(f"summary: published {len(published.reads)}, lost {len(verdict.lost)}, "
                 f"newly credited {len(verdict.newly_credited)}, public failures {len(verdict.public_failures)}")
    if verdict.newly_credited and verdict.status == "PASS":
        lines.append("info: the head is credited with entries the snapshot does not record; "
                     "the published snapshot is stale and worth refreshing")
    lines += [f"reason: {reason}" for reason in verdict.reasons]
    lines.append(f"verdict: {verdict.status}")
    return "\n".join(lines)


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--receipt", type=Path, required=True, help="corpus_read_receipt.py receipt.json at the head")
    parser.add_argument("--snapshot", type=Path, help="default: docs/public/measurements/catalog-corpus-observed-<pin>.json")
    parser.add_argument("--catalog", type=Path, help="default: docs/public/measurements/catalog-source-<pin>.json")
    args = parser.parse_args(argv)
    pin = (ROOT / ".exiftool-version").read_text().strip()
    measurements = ROOT / "docs/public/measurements"
    snapshot = args.snapshot or measurements / f"catalog-corpus-observed-{pin}.json"
    catalog = args.catalog or measurements / f"catalog-source-{pin}.json"
    try:
        published, verdict, receipt = measure(args.receipt, snapshot, catalog)
    except Refused as error:
        print(f"read regression gate REFUSED (measurement, not a code verdict): {error}")
        return EXIT_REFUSED
    print(render(published, verdict, receipt))
    return verdict.exit_code


if __name__ == "__main__":
    raise SystemExit(main())
