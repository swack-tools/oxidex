#!/usr/bin/env python3
"""Exercise a bounded, native ExifTool scalar-write matrix.

This is a versioned acceptance instrument for default-option scalar writes.
It records actual native output and checks the explicit ExifTool 13.59 byte
contract, relocation and preservation. A different release requires separately
captured expectations. This is not an OxiDex writer or a parity claim.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
from instrument import git_state, print_header, refuse_if_dirty

CASES = ("insert", "update", "growth", "shrinkage", "delete", "empty", "utf8", "embedded_nul")
NAMES = ("EXIF:HostComputer", "IFD0:HostComputer")
# Classic TIFF wire widths; preserve raw values without interpreting tag rules.
# Native JPEG EXIF creation can add RATIONAL resolution entries even when the
# requested field is a string, so the carrier inspector must support them.
TIFF_TYPES = {1: 1, 2: 1, 3: 2, 4: 4, 5: 8, 6: 1, 7: 1, 8: 2,
              9: 4, 10: 8, 11: 4, 12: 8}
IFD_POINTERS = {"34665": "ExifIFD", "34853": "GPS", "40965": "InteropIFD"}
CONTRACT_EXIFTOOL_RELEASE = "13.59"

# This is deliberately a pinned-native acceptance contract, not a writer
# implementation rule.  With the explicit Perl scalars in NATIVE_WRITE,
# ExifTool 13.59 writes these HostComputer values as TIFF ASCII (type 2) and
# terminates the byte sequence once.  Any changed native behavior is a failed
# baseline acceptance test that must be reviewed, not silently accommodated.
CASE_INPUT_BYTES = {
    "insert": b"insert-value",
    "update": b"updated",
    "growth": b"this-is-a-deliberately-longer-host-computer-value",
    "shrinkage": b"x",
    "empty": b"",
    "utf8": bytes.fromhex("c3a9"),
    "embedded_nul": bytes.fromhex("610062"),
}
CASE_UTF8_STATE = {case: False for case in CASE_INPUT_BYTES} | {"utf8": True}

NATIVE_LIBRARY_GUARD = r'''
use Cwd (); use Digest::SHA ();
my $selected_library = Cwd::abs_path(shift @ARGV) or die "selected library is absent";
sub loaded_native_modules {
  my %modules;
  for my $key (sort keys %INC) {
    next unless $key eq 'Image/ExifTool.pm' || index($key, 'Image/ExifTool/') == 0;
    my $path = Cwd::abs_path($INC{$key});
    die "native module outside selected library: $key" unless
      defined($path) && index($path, $selected_library . '/') == 0;
    $modules{$key} = $path;
  }
  die "selected native main module or writer was not loaded" unless
    exists $modules{'Image/ExifTool.pm'} && exists $modules{'Image/ExifTool/Writer.pl'};
  return \%modules;
}
loaded_native_modules();
'''

# The value construction happens in Perl.  That makes the scalar state part of
# the native call: utf8 is a flagged character scalar, embedded_nul is bytes.
NATIVE_WRITE = r'''
BEGIN { no warnings 'once'; $Image::ExifTool::configFile = ''; }
use strict; use warnings; use utf8; use JSON::PP; use Encode ();
use Image::ExifTool; require 'Image/ExifTool/Writer.pl';
''' + NATIVE_LIBRARY_GUARD + r'''
my ($in, $out, $action, $tag, $batch_json) = @ARGV;
sub state { my ($v) = @_; return {defined => JSON::PP::false} unless defined $v;
  my $utf8 = utf8::is_utf8($v) ? JSON::PP::true : JSON::PP::false;
  my $bytes = $utf8 ? Encode::encode('UTF-8', $v) : $v;
  return {defined => JSON::PP::true, utf8 => $utf8, hex => unpack('H*', $bytes), char_length => length $v}; }
sub value_for { my ($name) = @_;
  return undef if $name eq 'delete';
  return '' if $name eq 'empty';
  return 'insert-value' if $name eq 'insert';
  return 'updated' if $name eq 'update';
  return 'this-is-a-deliberately-longer-host-computer-value' if $name eq 'growth';
  return 'x' if $name eq 'shrinkage';
  return Encode::decode('UTF-8', pack('H*', 'c3a9')) if $name eq 'utf8';
  return pack('H*', '610062') if $name eq 'embedded_nul';
  die "unknown action $name";
}
sub batch_value { my ($spec) = @_;
  die 'batch item must be an object' unless ref($spec) eq 'HASH';
  my $scalar = $spec->{scalar};
  die 'batch item scalar is absent' unless defined($scalar) && !ref($scalar);
  return undef if $scalar eq 'undefined';
  my $value = $spec->{value};
  die 'defined batch item value is absent' unless defined($value) && !ref($value);
  if ($scalar eq 'bytes') {
    die 'batch bytes must be even lowercase hex' unless $value =~ /\A(?:[0-9a-f]{2})*\z/;
    return pack('H*', $value);
  }
  return $value if $scalar eq 'utf8';
  die "unsupported batch scalar $scalar";
}
my $et = Image::ExifTool->new;
my @sets;
if ($action eq 'batch') {
  die 'batch JSON is absent' unless defined($batch_json);
  my $batch = JSON::PP::decode_json($batch_json);
  die 'batch must be a non-empty array' unless ref($batch) eq 'ARRAY' && @$batch;
  for my $spec (@$batch) {
    die 'batch tag is absent' unless ref($spec) eq 'HASH' && defined($spec->{tag}) && !ref($spec->{tag}) && length($spec->{tag});
    my $value = batch_value($spec);
    my $set_return = scalar $et->SetNewValue($spec->{tag}, $value);
    push @sets, {tag => $spec->{tag}, input => state($value), return => $set_return};
  }
} elsif ($action eq 'seed_artist' || $action eq 'seed_artist_target') {
  my $artist_return = scalar $et->SetNewValue('IFD0:Artist', 'seed-artist');
  push @sets, {tag => 'IFD0:Artist', input => state('seed-artist'), return => $artist_return};
  if ($action eq 'seed_artist_target') {
    die 'seed target is required' unless defined($tag) && length($tag);
    my $target_return = scalar $et->SetNewValue($tag, 'seed-target');
    push @sets, {tag => $tag, input => state('seed-target'), return => $target_return};
  }
} else {
  my $value = value_for($action);
  my $set_return = scalar $et->SetNewValue($tag, $value);
  push @sets, {tag => $tag, input => state($value), return => $set_return};
}
my $write = $et->WriteInfo($in, $out);
print JSON::PP->new->canonical->utf8->encode({
  action => $action, requested_tag => ($tag || undef), set_calls => \@sets,
  write_return => $write, error => scalar $et->GetValue('Error'),
  native => {exiftool_version => $Image::ExifTool::VERSION, perl => $^X, perl_version => "$^V",
             loaded_modules => loaded_native_modules()},
});
'''

NATIVE_IDENTITY = r'''
BEGIN { no warnings 'once'; $Image::ExifTool::configFile = ''; }
use strict; use warnings; use JSON::PP; use Image::ExifTool;
require 'Image/ExifTool/Writer.pl';
''' + NATIVE_LIBRARY_GUARD + r'''
my $modules = loaded_native_modules();
my %hashes;
for my $key ('Image/ExifTool.pm', 'Image/ExifTool/Writer.pl') {
  open my $fh, '<:raw', $modules->{$key} or die "cannot read native source: $key";
  $hashes{$key} = Digest::SHA->new(256)->addfile($fh)->hexdigest;
}
print JSON::PP->new->canonical->utf8->encode({
  exiftool_version => $Image::ExifTool::VERSION, perl => $^X, perl_version => "$^V",
  config_file => $Image::ExifTool::configFile,
  loaded_modules => $modules, source_sha256 => \%hashes,
});
'''


def clean_env() -> dict[str, str]:
    return {key: value for key, value in os.environ.items()
            if key not in {"PERL5LIB", "PERLLIB", "PERL5OPT"} and not key.startswith("PERL5")}


def resolve_perl(value: Path) -> Path:
    raw = str(value)
    if os.sep not in raw and (not os.altsep or os.altsep not in raw):
        found = shutil.which(raw, path=os.environ.get("PATH"))
        if not found:
            raise ValueError(f"selected Perl executable is not on PATH: {raw}")
        return Path(found).resolve()
    return value.expanduser().resolve()


def resolve_library(value: Path) -> Path:
    path = value.expanduser().resolve()
    return (path / "lib").resolve() if (path / "lib").is_dir() else path


def validate_library(library: Path) -> None:
    for name in ("Image/ExifTool.pm", "Image/ExifTool/Writer.pl"):
        path = library / name
        if not path.is_file() or not path.resolve().is_relative_to(library.resolve()):
            raise ValueError(f"{name} absent from or outside selected library: {library}")


def run_native(perl: Path, library: Path, source: Path, target: Path,
               action: str, tag: str | None) -> dict[str, Any]:
    validate_library(library)
    command = [str(perl), "-I" + str(library), "-e", NATIVE_WRITE,
               str(library), str(source), str(target), action, tag or ""]
    completed = subprocess.run(command, env=clean_env(), capture_output=True, timeout=30)
    stdout = completed.stdout.decode("utf-8", errors="replace")
    stderr = completed.stderr.decode("utf-8", errors="replace")
    parsed: dict[str, Any] | None = None
    if stdout:
        try:
            parsed = json.loads(stdout)
        except json.JSONDecodeError:
            pass
    return {"command": [str(perl), "-I" + str(library), "-e", "<native-write-program>",
                         str(library), str(source), str(target), action, tag or ""],
            "returncode": completed.returncode, "stdout": stdout, "stderr": stderr,
            "result": parsed}


def run_native_batch(perl: Path, library: Path, source: Path, target: Path,
                     batch: list[dict[str, str]]) -> dict[str, Any]:
    """Apply one ordered native SetNewValue batch through one WriteInfo call.

    This accepts only the scalar states exercised by the generated public
    transaction probe.  Validating here makes the recorded JSON both a native
    operand record and an unambiguous replay input; it never falls back to a
    shell or an ambient ExifTool executable.
    """
    if not batch:
        raise ValueError("native batch is empty")
    checked: list[dict[str, str]] = []
    for item in batch:
        if set(item) - {"tag", "scalar", "value"} or not isinstance(item.get("tag"), str) or not item["tag"]:
            raise ValueError("native batch item has malformed tag/schema")
        scalar, value = item.get("scalar"), item.get("value")
        if scalar == "undefined":
            if value is not None:
                raise ValueError("undefined native batch item has a value")
            checked.append({"tag": item["tag"], "scalar": scalar})
        elif scalar == "utf8" and isinstance(value, str):
            checked.append({"tag": item["tag"], "scalar": scalar, "value": value})
        elif scalar == "bytes" and isinstance(value, str) and len(value) % 2 == 0 and all(char in "0123456789abcdef" for char in value):
            checked.append({"tag": item["tag"], "scalar": scalar, "value": value})
        else:
            raise ValueError("native batch item has unsupported scalar/value")
    validate_library(library)
    encoded = json.dumps(checked, sort_keys=True, separators=(",", ":"))
    command = [str(perl), "-I" + str(library), "-e", NATIVE_WRITE,
               str(library), str(source), str(target), "batch", "", encoded]
    completed = subprocess.run(command, env=clean_env(), capture_output=True, timeout=30)
    stdout = completed.stdout.decode("utf-8", errors="replace")
    stderr = completed.stderr.decode("utf-8", errors="replace")
    parsed: dict[str, Any] | None = None
    if stdout:
        try:
            parsed = json.loads(stdout)
        except json.JSONDecodeError:
            pass
    return {"command": [str(perl), "-I" + str(library), "-e", "<native-write-program>",
                         str(library), str(source), str(target), "batch", "", checked],
            "returncode": completed.returncode, "stdout": stdout, "stderr": stderr,
            "result": parsed}


def native_identity(perl: Path, library: Path) -> dict[str, Any]:
    validate_library(library)
    completed = subprocess.run([str(perl), "-I" + str(library), "-e", NATIVE_IDENTITY, str(library)],
                               env=clean_env(), capture_output=True, timeout=30)
    stdout = completed.stdout.decode("utf-8", errors="replace")
    stderr = completed.stderr.decode("utf-8", errors="replace")
    parsed: dict[str, Any] | None = None
    if stdout:
        try:
            parsed = json.loads(stdout)
        except json.JSONDecodeError:
            pass
    if completed.returncode != 0 or parsed is None:
        raise RuntimeError(f"native identity probe failed: {stderr or stdout}")
    return {"command": [str(perl), "-I" + str(library), "-e", "<native-identity-program>", str(library)],
            "returncode": completed.returncode, "stdout": stdout, "stderr": stderr, "result": parsed}


def assert_contract_version(identity: dict[str, Any], pin_file: Path | None = None) -> None:
    pinned = (pin_file or ROOT / ".exiftool-version").read_text().strip()
    if pinned != CONTRACT_EXIFTOOL_RELEASE:
        raise RuntimeError(f"repository pin {pinned} has no reviewed native-write acceptance baseline; "
                           f"the {CONTRACT_EXIFTOOL_RELEASE} baseline is stale")
    actual = identity["result"]["exiftool_version"]
    if actual != pinned:
        raise RuntimeError(
            f"selected ExifTool {actual} does not match this {CONTRACT_EXIFTOOL_RELEASE} native-write "
            "acceptance baseline; capture a separate version-rehearsal expectation before comparing it")


def pack(number: int, width: int, order: str) -> bytes:
    return number.to_bytes(width, order)


def make_tiff(path: Path, order: str) -> None:
    """Make a small valid carrier; Artist and HostComputer are seeded natively."""
    marker = b"II" if order == "little" else b"MM"
    entries = [(256, 4, 1, 1), (257, 4, 1, 1), (273, 4, 1, 0), (279, 4, 1, 1)]
    ifd_offset = 8
    strip_offset = ifd_offset + 2 + len(entries) * 12 + 4
    entries[2] = (273, 4, 1, strip_offset)
    data = bytearray(marker + pack(42, 2, order) + pack(ifd_offset, 4, order))
    data += pack(len(entries), 2, order)
    for tag, kind, count, value in entries:
        data += pack(tag, 2, order) + pack(kind, 2, order) + pack(count, 4, order) + pack(value, 4, order)
    data += pack(0, 4, order) + b"\xff"
    path.write_bytes(data)


def parse_tiff(data: bytes, require_strip: bool = True, *, _offset=None, _visited=None) -> dict[str, Any]:
    if len(data) < 10 or data[:2] not in (b"II", b"MM"):
        raise ValueError("not a complete TIFF header")
    order = "little" if data[:2] == b"II" else "big"
    if int.from_bytes(data[2:4], order) != 42:
        raise ValueError("TIFF magic is not 42")
    offset = int.from_bytes(data[4:8], order) if _offset is None else _offset
    visited = set() if _visited is None else _visited
    if offset < 8 or offset in visited or len(visited) >= 32:
        raise ValueError("invalid, repeated or excessive TIFF directory pointer")
    visited.add(offset)
    if offset + 2 > len(data):
        raise ValueError("IFD count is out of bounds")
    count = int.from_bytes(data[offset:offset + 2], order)
    end = offset + 2 + count * 12 + 4
    if end > len(data):
        raise ValueError("IFD entries are truncated")
    tags: dict[str, Any] = {}
    for position in range(offset + 2, end - 4, 12):
        tag = int.from_bytes(data[position:position + 2], order)
        if str(tag) in tags:
            raise ValueError(f"duplicate tag {tag} in TIFF directory")
        kind = int.from_bytes(data[position + 2:position + 4], order)
        item_count = int.from_bytes(data[position + 4:position + 8], order)
        if kind not in TIFF_TYPES:
            raise ValueError(f"unsupported TIFF type {kind} for tag {tag}")
        length = item_count * TIFF_TYPES[kind]
        slot = data[position + 8:position + 12]
        inline = length <= 4
        if inline:
            value = slot[:length]
            value_offset = None
        else:
            value_offset = int.from_bytes(slot, order)
            if value_offset + length > len(data):
                raise ValueError(f"IFD value is out of bounds for tag {tag}")
            value = data[value_offset:value_offset + length]
        tags[str(tag)] = {"type": kind, "count": item_count, "value_hex": value.hex(),
                          "inline": inline, "value_offset": value_offset}
    image_payload_hex: str | None = None
    if "273" in tags and "279" in tags:
        strip_offset = int.from_bytes(bytes.fromhex(tags["273"]["value_hex"]), order)
        strip_length = int.from_bytes(bytes.fromhex(tags["279"]["value_hex"]), order)
        if strip_offset + strip_length > len(data):
            raise ValueError("strip payload is out of bounds")
        image_payload_hex = data[strip_offset:strip_offset + strip_length].hex()
    elif require_strip:
        raise ValueError("carrier lacks StripOffsets/StripByteCounts")
    children = {}
    for tag, name in IFD_POINTERS.items():
        if tag not in tags:
            continue
        pointer = tags[tag]
        if pointer["type"] != 4 or pointer["count"] != 1:
            raise ValueError(f"malformed TIFF {name} pointer")
        at = int.from_bytes(bytes.fromhex(pointer["value_hex"]), order)
        children[name] = parse_tiff(data, False, _offset=at, _visited=visited)
    next_ifd = int.from_bytes(data[end - 4:end], order)
    if next_ifd:
        children["NextIFD"] = parse_tiff(data, False, _offset=next_ifd, _visited=visited)
    return {"byte_order": order, "sha256": hashlib.sha256(data).hexdigest(), "tags": tags, "children": children,
            "image_payload_hex": image_payload_hex}


def parse_jpeg(path: Path) -> dict[str, Any]:
    data = path.read_bytes()
    if not data.startswith(b"\xff\xd8"):
        raise ValueError("not a JPEG SOI")
    position = 2
    exif: dict[str, Any] | None = None
    sos_tail: bytes | None = None
    non_exif_parts = [data[:2]]
    while position < len(data):
        if data[position] != 0xff:
            raise ValueError("JPEG marker is missing 0xff prefix")
        marker_start = position
        while position < len(data) and data[position] == 0xff:
            position += 1
        if position >= len(data):
            raise ValueError("truncated JPEG marker")
        marker = data[position]
        position += 1
        if marker == 0xda:
            sos_tail = data[marker_start:]
            non_exif_parts.append(sos_tail)
            break
        if marker == 0xd9:
            break
        if marker in {0x01, *range(0xd0, 0xd8)}:
            non_exif_parts.append(data[marker_start:position])
            continue
        if position + 2 > len(data):
            raise ValueError("truncated JPEG segment length")
        length = int.from_bytes(data[position:position + 2], "big")
        if length < 2 or position + length > len(data):
            raise ValueError("JPEG segment is out of bounds")
        payload = data[position + 2:position + length]
        if marker == 0xe1 and payload.startswith(b"Exif\x00\x00"):
            if exif is not None:
                raise ValueError("ambiguous multiple JPEG EXIF segments")
            exif = parse_tiff(payload[6:], require_strip=False)
            # Preserve marker fill bytes, which precede the final FF marker.
            non_exif_parts.append(data[marker_start:position - 2])
        else:
            non_exif_parts.append(data[marker_start:position + length])
        position += length
    if sos_tail is None:
        raise ValueError("JPEG lacks SOS marker")
    return {"sha256": hashlib.sha256(data).hexdigest(),
            "sos_to_end_sha256": hashlib.sha256(sos_tail).hexdigest(),
            "non_exif_sha256": hashlib.sha256(b"".join(non_exif_parts)).hexdigest(),
            "sos_to_end_length": len(sos_tail), "exif": exif}


def inspect(path: Path, carrier: str) -> dict[str, Any]:
    if carrier.startswith("tiff"):
        return parse_tiff(path.read_bytes())
    return parse_jpeg(path)


def tags(document: dict[str, Any], carrier: str) -> dict[str, Any]:
    if carrier.startswith("tiff"):
        return document["tags"]
    exif = document["exif"]
    if exif is None:
        raise ValueError("JPEG output has no Exif APP1")
    return exif["tags"]


def image_identity(document: dict[str, Any], carrier: str) -> str:
    return document["image_payload_hex"] if carrier.startswith("tiff") else document["sos_to_end_sha256"]


def assert_native(call: dict[str, Any], label: str) -> None:
    if call["returncode"] != 0 or call["result"] is None:
        raise AssertionError(f"{label}: native process failed: {call['stderr'] or call['stdout']}")
    result = call["result"]
    if result["write_return"] != 1 or result["error"] is not None:
        raise AssertionError(f"{label}: native write did not succeed: {result}")
    if not all(entry["return"] in (1, 2) for entry in result["set_calls"]):
        raise AssertionError(f"{label}: native SetNewValue was rejected: {result}")


def expected_host(operation: str) -> dict[str, Any] | None:
    if operation == "delete":
        return None
    raw = CASE_INPUT_BYTES[operation]
    value = raw + b"\x00"
    return {"type": 2, "count": len(value), "value_hex": value.hex()}


def entry_storage(entry: dict[str, Any] | None) -> dict[str, Any] | None:
    if entry is None:
        return None
    return {key: entry[key] for key in ("type", "count", "value_hex")}


# Compatibility alias for the original single-target native matrix.
host_storage = entry_storage


def target_entry(document: dict[str, Any], carrier: str, raw_tag_id: int) -> dict[str, Any] | None:
    return tags(document, carrier).get(str(raw_tag_id))


def assert_target_transition(seed: dict[str, Any], expected: dict[str, Any], carrier: str,
                             raw_tag_id: int, operation: str) -> None:
    before, after = target_entry(seed, carrier, raw_tag_id), target_entry(expected, carrier, raw_tag_id)
    if operation == "insert":
        if before is not None or after is None:
            raise AssertionError(f"insert did not create requested tag {raw_tag_id}")
    elif operation == "delete":
        if before is None or after is not None:
            raise AssertionError(f"delete did not remove requested tag {raw_tag_id}")
    elif entry_storage(before) == entry_storage(after):
        raise AssertionError(f"native operation was a no-op for requested tag {raw_tag_id}")


def assert_host_contract(actual: dict[str, Any] | None, expected: dict[str, Any] | None) -> None:
    """Require the exact 13.59 native type/count/value-byte acceptance contract."""
    if actual is None:
        if expected is not None:
            raise AssertionError(f"HostComputer missing; expected {expected}")
        return
    if expected is None:
        raise AssertionError(f"HostComputer present after native delete: {host_storage(actual)}")
    observed = host_storage(actual)
    for dimension in ("type", "count", "value_hex"):
        if observed[dimension] != expected[dimension]:
            raise AssertionError(
                f"HostComputer native {dimension} mismatch: expected {expected[dimension]!r}, "
                f"observed {observed[dimension]!r}")


def verify_row(row: dict[str, Any], carrier: str, operation: str) -> dict[str, Any]:
    seeded = row["seeded_inspection"]
    output = row["output_inspection"]
    seeded_tags, output_tags = tags(seeded, carrier), tags(output, carrier)
    if image_identity(row["carrier_inspection"], carrier) != image_identity(seeded, carrier):
        raise AssertionError("seeding changed carrier image bytes")
    if image_identity(seeded, carrier) != image_identity(output, carrier):
        raise AssertionError("operation changed carrier image bytes")
    # Writer layout may legitimately relocate an out-of-line value.  Metadata
    # preservation is its actual type/count/value bytes, not its old offset.
    artist_before = seeded_tags.get("315")
    artist_after = output_tags.get("315")
    if ({key: artist_before.get(key) for key in ("type", "count", "value_hex")} if artist_before else None) != \
       ({key: artist_after.get(key) for key in ("type", "count", "value_hex")} if artist_after else None):
        raise AssertionError("operation did not preserve seeded Artist")
    before = seeded_tags.get("316")
    after = output_tags.get("316")
    expected = expected_host(operation)
    if operation != "delete":
        scalar = row["operation_call"]["result"]["set_calls"][0]["input"]
        if scalar != {"defined": True, "utf8": CASE_UTF8_STATE[operation],
                      "hex": CASE_INPUT_BYTES[operation].hex(),
                      "char_length": 1 if operation == "utf8" else len(CASE_INPUT_BYTES[operation])}:
            raise AssertionError(f"operation scalar state drifted from the declared case: {scalar}")
    if operation == "insert":
        if before is not None or after is None:
            raise AssertionError("insert did not transition HostComputer from absent to present")
    elif operation == "delete":
        if before is None or after is not None:
            raise AssertionError("delete did not remove HostComputer")
    elif before == after:
        raise AssertionError("native operation was a no-op masquerading as a change")
    assert_host_contract(after, expected)
    if after is not None and after["count"] <= 4 and not after["inline"]:
        raise AssertionError("short HostComputer value was not stored inline")
    return {"carrier_image_preserved": True, "artist_preserved": True,
            "host_before": before, "host_after": after, "expected_host": expected,
            "no_op_detected": before == after}


def make_carrier(source: Path, carrier: str, jpeg_base: Path) -> None:
    if carrier == "tiff_little":
        make_tiff(source, "little")
    elif carrier == "tiff_big":
        make_tiff(source, "big")
    else:
        shutil.copyfile(jpeg_base, source)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--perl", type=Path, required=True)
    parser.add_argument("--lib", type=Path, required=True)
    parser.add_argument("--jpeg-base", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args(argv)
    state = git_state(ROOT)
    overridden = refuse_if_dirty(state, "native_write_matrix")
    perl, library = resolve_perl(args.perl), resolve_library(args.lib)
    if not perl.is_file() or not os.access(perl, os.X_OK):
        parser.error(f"selected Perl is not executable: {perl}")
    validate_library(library)
    if not args.jpeg_base.is_file():
        parser.error(f"JPEG carrier is absent: {args.jpeg_base}")
    identity = native_identity(perl, library)
    assert_contract_version(identity)
    evidence = {"source_commit": state.commit, "dirty": state.dirty,
                "dirty_files": state.dirty_files, "dirty_override": overridden,
                "instrument_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                "jpeg_base_sha256": hashlib.sha256(args.jpeg_base.read_bytes()).hexdigest(),
                "repository_exiftool_pin": (ROOT / ".exiftool-version").read_text().strip()}
    print_header(tool="native_write_matrix_v2", git=state, dirty_overridden=overridden,
                 extra=[f"native: {identity['result']}", "48 native scalar write cases; no OxiDex binary"])
    root = args.output.parent / "native-write-matrix-files"
    root.mkdir(parents=True, exist_ok=True)
    rows: list[dict[str, Any]] = []
    for carrier in ("tiff_little", "tiff_big", "jpeg"):
        suffix = ".tif" if carrier.startswith("tiff") else ".jpg"
        for name in NAMES:
            for operation in CASES:
                stem = f"{carrier}-{name.replace(':', '_')}-{operation}"
                source, seeded, output = (root / f"{stem}-{part}{suffix}" for part in ("carrier", "seeded", "output"))
                make_carrier(source, carrier, args.jpeg_base)
                seed_action = "seed_artist" if operation == "insert" else "seed_artist_target"
                seed_call = run_native(perl, library, source, seeded, seed_action, None if operation == "insert" else name)
                assert_native(seed_call, f"{stem} seed")
                operation_call = run_native(perl, library, seeded, output, operation, name)
                assert_native(operation_call, f"{stem} {operation}")
                row = {"id": stem, "carrier": carrier, "requested_name": name, "operation": operation,
                       "source": str(source), "seeded": str(seeded), "output": str(output),
                       "seed_call": seed_call, "operation_call": operation_call,
                       "carrier_inspection": inspect(source, carrier),
                       "seeded_inspection": inspect(seeded, carrier), "output_inspection": inspect(output, carrier)}
                row["verification"] = verify_row(row, carrier, operation)
                rows.append(row)
    document = {"instrument": "native_write_matrix_v2", "source": evidence,
                "scope": "default-option native ExifTool scalar writes; minimal TIFF and supplied small JPEG carriers",
                "contract_exiftool_release": CONTRACT_EXIFTOOL_RELEASE, "native_identity": identity,
                "native": {"perl": str(perl), "library": str(library), "config_file": "", "scrubbed_environment": ["PERL5LIB", "PERLLIB", "PERL5OPT"]},
                "declared_rows": len(CASES) * len(NAMES) * 3, "executed_rows": len(rows), "rows": rows,
                "limitations": ["This records native behavior only; it does not claim OxiDex write parity.", "Coverage is limited to HostComputer scalar writes, default ExifTool options, minimal TIFF carriers, and the supplied small JPEG.", "A generated Rust comparison remains unfinished."]}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(document, indent=2, sort_keys=True) + "\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
