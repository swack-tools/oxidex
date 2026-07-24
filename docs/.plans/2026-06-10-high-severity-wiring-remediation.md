# High Severity Wiring Remediation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the high-severity QA findings where implemented code is not reachable through the production CLI/API/FFI paths.

**Architecture:** Use production-path tests first, then wire existing parser/writer/binding code into the real dispatch surfaces. Keep each fix narrow: route existing code before expanding parser fidelity, and update tests so direct parser tests cannot hide missing production wiring again.

**Tech Stack:** Rust 2024, Cargo workspace, lexopt CLI parsing, existing `MetadataMap`/`FileReader` abstractions, C ABI via `extern "C"`, Python ctypes bindings.

---

## Scope

This plan covers only high-severity findings from the QA swarm:

- Production parser routing is missing for implemented formats.
- iWork formats are detected but dispatched to OOXML parsers.
- High-level writes only route JPEG while PNG/PDF/TIFF writers exist.
- Python bindings do not load or bind the current C ABI.
- C FFI integration test is not part of the executable verification surface.
- Generated tag tables are present as unreachable legacy code and coverage tests use a hard-coded count.
- ExifTool-style single-dash long CLI options and batch output options are advertised but not wired.

Medium and low findings are intentionally left for a second plan.

## File Structure

- Modify `src/core/format_dispatch.rs`: import and route existing parser entrypoints.
- Modify `src/parsers/detection/signatures.rs`: add missing OLE and DER X.509 detection.
- Modify `src/parsers/detection/text.rs`: detect EML and ICS before plain text fallback.
- Modify `src/parsers/archive/ole.rs`: add a `parse_ole_metadata()` wrapper matching other dispatch APIs.
- Modify `src/core/operations.rs`: route PNG/PDF/TIFF high-level writes.
- Modify `tests/integration.rs`: include new production wiring tests.
- Create `tests/integration/production_wiring_tests.rs`: assert `read_metadata()` and `write_metadata()` hit production routing.
- Modify `bindings/python/oxidex.py`: bind `exiftool_*` symbols and correct library name.
- Create `bindings/python/test_bindings.py`: import and basic read test for the ctypes wrapper.
- Create `tests/ffi_c_integration.rs`: make the existing C integration test part of Cargo verification.
- Modify `src/tag_db/generated_tags.rs`: make coverage count reflect reachable registry state.
- Remove `src/tag_db/generated/`: delete orphan generated modules after proving no compiled code imports them.
- Modify `tests/tag_database_coverage.rs`: validate reachable descriptors and actual count.
- Modify `src/cli/args.rs`: normalize supported single-dash long options before lexopt parsing.
- Modify `src/cli/batch_processor.rs`: reuse output formatters for batch output flags.
- Modify or create `tests/integration/cli_batch_wiring_tests.rs`: verify batch flags and single-dash long options.

---

### Task 1: Add Production Parser Wiring Regression Tests

**Files:**
- Modify: `tests/integration.rs`
- Create: `tests/integration/production_wiring_tests.rs`

- [ ] **Step 1: Include the new integration test module**

Add this near the other `#[path = "integration/..."]` entries in `tests/integration.rs`:

```rust
#[path = "integration/production_wiring_tests.rs"]
mod production_wiring_tests;
```

- [ ] **Step 2: Create failing tests for production read routing**

Create `tests/integration/production_wiring_tests.rs` with production-path tests. The helpers should write bytes to a `tempfile::NamedTempFile` with a realistic extension and call `oxidex::core::operations::read_metadata()`, not parser entrypoints.

```rust
use oxidex::core::{MetadataMap, TagValue};
use oxidex::core::operations::{read_metadata, write_metadata};
use std::fs;
use std::io::Write;
use tempfile::NamedTempFile;

fn temp_with_suffix(suffix: &str) -> NamedTempFile {
    tempfile::Builder::new()
        .suffix(suffix)
        .tempfile()
        .expect("create temp file")
}

fn read_temp_file(bytes: &[u8], suffix: &str) -> MetadataMap {
    let mut file = temp_with_suffix(suffix);
    file.write_all(bytes).expect("write fixture bytes");
    read_metadata(file.path()).expect("read through production metadata path")
}

fn evtx_fixture() -> Vec<u8> {
    let mut data = vec![0u8; 4096];
    data[0..8].copy_from_slice(b"ElfFile\0");
    data[16..24].copy_from_slice(&4u64.to_le_bytes());
    data[24..32].copy_from_slice(&501u64.to_le_bytes());
    data[32..36].copy_from_slice(&128u32.to_le_bytes());
    data[36..38].copy_from_slice(&1u16.to_le_bytes());
    data[38..40].copy_from_slice(&3u16.to_le_bytes());
    data[40..42].copy_from_slice(&4096u16.to_le_bytes());
    data[42..44].copy_from_slice(&5u16.to_le_bytes());
    data[120..124].copy_from_slice(&0xDEADBEEFu32.to_le_bytes());
    data
}

fn prefetch_fixture() -> Vec<u8> {
    let mut data = vec![0u8; 256];
    data[0..4].copy_from_slice(&30u32.to_le_bytes());
    data[4..8].copy_from_slice(b"SCCA");
    data[12..16].copy_from_slice(&45_000u32.to_le_bytes());
    for (i, ch) in "NOTEPAD.EXE".encode_utf16().take(30).enumerate() {
        data[16 + i * 2..18 + i * 2].copy_from_slice(&ch.to_le_bytes());
    }
    data[76..80].copy_from_slice(&0xDEADBEEFu32.to_le_bytes());
    data[128..136].copy_from_slice(&133500420450000000u64.to_le_bytes());
    data[144..148].copy_from_slice(&7u32.to_le_bytes());
    data
}

fn registry_fixture() -> Vec<u8> {
    let mut data = vec![0u8; 4096];
    data[0..4].copy_from_slice(b"regf");
    data[4..8].copy_from_slice(&100u32.to_le_bytes());
    data[8..12].copy_from_slice(&100u32.to_le_bytes());
    data[12..20].copy_from_slice(&133000000000000000u64.to_le_bytes());
    data[20..24].copy_from_slice(&1u32.to_le_bytes());
    data[24..28].copy_from_slice(&5u32.to_le_bytes());
    data[36..40].copy_from_slice(&0x1000u32.to_le_bytes());
    data[40..44].copy_from_slice(&1_048_576u32.to_le_bytes());
    for (i, ch) in "SYSTEM".encode_utf16().enumerate() {
        data[48 + i * 2..50 + i * 2].copy_from_slice(&ch.to_le_bytes());
    }
    data
}

fn pcap_fixture() -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(&[0xd4, 0xc3, 0xb2, 0xa1]);
    data.extend_from_slice(&2u16.to_le_bytes());
    data.extend_from_slice(&4u16.to_le_bytes());
    data.extend_from_slice(&0i32.to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&65535u32.to_le_bytes());
    data.extend_from_slice(&1u32.to_le_bytes());
    data
}

fn binary_plist_fixture() -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(b"bplist00");
    data.extend(vec![0u8; 100]);
    let mut trailer = vec![0u8; 32];
    trailer[6] = 2;
    trailer[7] = 1;
    trailer[8..16].copy_from_slice(&5u64.to_be_bytes());
    trailer[24..32].copy_from_slice(&108u64.to_be_bytes());
    data.extend(trailer);
    data
}

fn der_x509_fixture() -> Vec<u8> {
    let mut der = vec![0x30, 0x10, 0x30, 0x0E];
    der.extend_from_slice(&[0x02, 0x01, 0x01]);
    der.extend_from_slice(&[0x02, 0x04, 0x12, 0x34, 0x56, 0x78]);
    der.extend_from_slice(&[0x30, 0x03, 0x06, 0x01, 0x00]);
    der
}

#[test]
fn read_metadata_routes_signature_detected_forensic_formats() {
    assert_eq!(
        read_temp_file(&evtx_fixture(), ".evtx").get("FileType"),
        Some(&TagValue::String("Windows Event Log".to_string()))
    );
    assert_eq!(
        read_temp_file(&prefetch_fixture(), ".pf").get("Prefetch:FileType"),
        Some(&TagValue::String("Windows Prefetch".to_string()))
    );
    assert_eq!(
        read_temp_file(&registry_fixture(), ".dat").get("Registry:SequenceValid"),
        Some(&TagValue::String("Yes".to_string()))
    );
    assert!(read_temp_file(&pcap_fixture(), ".pcap").contains_key("PCAP:Version"));
    assert_eq!(
        read_temp_file(&binary_plist_fixture(), ".plist").get("Plist:Format"),
        Some(&TagValue::String("Binary".to_string()))
    );
}

#[test]
fn read_metadata_routes_text_document_formats_before_txt() {
    let ics = b"BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nSUMMARY:Planning\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let eml = b"From: a@example.com\r\nTo: b@example.com\r\nSubject: Wiring\r\nDate: Wed, 10 Jun 2026 12:00:00 +0000\r\n\r\nBody";

    assert!(read_temp_file(ics, ".ics").contains_key("ICS:EventCount"));
    assert!(read_temp_file(eml, ".eml").contains_key("EML:Subject"));
}

#[test]
fn read_metadata_routes_der_x509_detection() {
    let metadata = read_temp_file(&der_x509_fixture(), ".der");
    assert!(metadata.contains_key("X509:SHA256Fingerprint"));
}
```

Use `zip::ZipWriter` in the same file for iWork regression tests:

```rust
fn write_zip_fixture(entries: &[(&str, &[u8])], suffix: &str) -> tempfile::NamedTempFile {
    let file = temp_with_suffix(suffix);
    {
        let output = fs::File::create(file.path()).expect("open temp zip");
        let mut zip = zip::ZipWriter::new(output);
        let options = zip::write::FileOptions::default();
        for (name, data) in entries {
            zip.start_file(*name, options).expect("start zip entry");
            zip.write_all(data).expect("write zip entry");
        }
        zip.finish().expect("finish zip");
    }
    file
}

#[test]
fn read_metadata_routes_iwork_to_iwork_parsers() {
    let metadata_plist = br#"<plist><dict><key>Author</key><string>Ada</string></dict></plist>"#;

    let pages = write_zip_fixture(
        &[("Index/Document.iwa", b""), ("Index/Metadata.plist", metadata_plist)],
        ".pages",
    );
    let numbers = write_zip_fixture(
        &[("Index/Document.iwa", b""), ("Index/Tables/table.iwa", b"")],
        ".numbers",
    );
    let keynote = write_zip_fixture(
        &[("Index/Presentation.iwa", b"")],
        ".key",
    );

    assert_eq!(
        read_metadata(pages.path()).unwrap().get("iWork:Application"),
        Some(&TagValue::String("Pages".to_string()))
    );
    assert_eq!(
        read_metadata(numbers.path()).unwrap().get("iWork:Application"),
        Some(&TagValue::String("Numbers".to_string()))
    );
    assert_eq!(
        read_metadata(keynote.path()).unwrap().get("iWork:Application"),
        Some(&TagValue::String("Keynote".to_string()))
    );
}
```

- [ ] **Step 3: Run the tests and verify they fail for missing routes**

Run:

```bash
cargo test --test integration production_wiring_tests -- --nocapture
```

Expected before implementation: failures mentioning unsupported formats or wrong `iWork:Application` values.

- [ ] **Step 4: Commit tests**

```bash
git add tests/integration.rs tests/integration/production_wiring_tests.rs
git commit -m "test: cover production parser wiring"
```

---

### Task 2: Wire Existing Parsers Into Detection And Dispatch

**Files:**
- Modify: `src/core/format_dispatch.rs`
- Modify: `src/parsers/archive/ole.rs`
- Modify: `src/parsers/detection/signatures.rs`
- Modify: `src/parsers/detection/text.rs`

- [ ] **Step 1: Add an OLE parser wrapper**

Append this near the `OLEParser` implementation in `src/parsers/archive/ole.rs`:

```rust
/// Parses metadata from OLE Compound File Binary Format files.
pub fn parse_ole_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = OLEParser;
    parser.parse(reader).map_err(|e| e.to_string())
}
```

- [ ] **Step 2: Add missing dispatch imports and arms**

In `src/core/format_dispatch.rs`, add imports:

```rust
use crate::parsers::archive::ole::parse_ole_metadata;
use crate::parsers::document::eml::parse_eml_metadata;
use crate::parsers::document::ics::parse_ics_metadata;
use crate::parsers::document::iwork::{parse_keynote_metadata, parse_numbers_metadata, parse_pages_metadata};
use crate::parsers::specialized::evtx::parse_evtx_metadata;
use crate::parsers::specialized::pcap::parse_pcap_metadata;
use crate::parsers::specialized::plist::parse_plist_metadata;
use crate::parsers::specialized::prefetch::parse_prefetch_metadata;
use crate::parsers::specialized::registry::parse_registry_metadata;
```

If `src/parsers/document/iwork.rs` does not yet expose functional wrappers, add them:

```rust
pub fn parse_pages_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = PagesParser;
    parser.parse(reader).map_err(|e| e.to_string())
}

pub fn parse_numbers_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = NumbersParser;
    parser.parse(reader).map_err(|e| e.to_string())
}

pub fn parse_keynote_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = KeynoteParser;
    parser.parse(reader).map_err(|e| e.to_string())
}
```

Replace the current iWork arms:

```rust
FileFormat::Pages => convert_string_error(parse_pages_metadata(reader), "Pages"),
FileFormat::Numbers => convert_string_error(parse_numbers_metadata(reader), "Numbers"),
FileFormat::Keynote => convert_string_error(parse_keynote_metadata(reader), "Keynote"),
```

Add missing arms before the wildcard:

```rust
FileFormat::ICS => convert_string_error(parse_ics_metadata(reader), "ICS"),
FileFormat::EML => convert_string_error(parse_eml_metadata(reader), "EML"),
FileFormat::OLE => convert_string_error(parse_ole_metadata(reader), "OLE"),
FileFormat::Prefetch => convert_string_error(parse_prefetch_metadata(reader), "Prefetch"),
FileFormat::Registry => convert_string_error(parse_registry_metadata(reader), "Registry"),
FileFormat::EVTX => convert_string_error(parse_evtx_metadata(reader), "EVTX"),
FileFormat::Plist => convert_string_error(parse_plist_metadata(reader), "Plist"),
FileFormat::PCAP | FileFormat::PCAPNG => convert_string_error(parse_pcap_metadata(reader), "PCAP"),
```

- [ ] **Step 3: Add OLE and DER X.509 signatures**

In `src/parsers/detection/signatures.rs`, add these signatures in the binary/forensic region:

```rust
signature!(b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1", 0, FileFormat::OLE),
signature!(b"\x30\x82", 0, FileFormat::X509),
signature!(b"\x30\x81", 0, FileFormat::X509),
```

- [ ] **Step 4: Detect ICS and EML before generic TXT**

In `src/parsers/detection/text.rs`, add helpers and checks before DXF/OBJ/GLTF/STL:

```rust
fn looks_like_ics(text: &str) -> bool {
    text.starts_with("BEGIN:VCALENDAR") || text.contains("\nBEGIN:VCALENDAR")
}

fn looks_like_eml(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("\nsubject:") || lower.starts_with("subject:") || lower.contains("\nfrom:")
}
```

Then in `detect_text_formats()`:

```rust
if looks_like_ics(text) {
    return Some(FileFormat::ICS);
}

if looks_like_eml(text) {
    return Some(FileFormat::EML);
}
```

- [ ] **Step 5: Run targeted production wiring tests**

Run:

```bash
cargo test --test integration production_wiring_tests -- --nocapture
```

Expected: all `production_wiring_tests` pass.

- [ ] **Step 6: Run existing parser tests to catch regressions**

Run:

```bash
cargo test --test integration forensic::evtx_tests forensic::prefetch_tests forensic::registry_tests forensic::pcap_tests forensic::plist_tests -- --nocapture
cargo test -p oxidex --test unit_tests iwork -- --nocapture
```

Expected: all selected tests pass.

- [ ] **Step 7: Commit parser wiring**

```bash
git add src/core/format_dispatch.rs src/parsers/archive/ole.rs src/parsers/document/iwork.rs src/parsers/detection/signatures.rs src/parsers/detection/text.rs
git commit -m "fix: route implemented parsers through production dispatch"
```

---

### Task 3: Wire PNG/PDF/TIFF Writers Through `write_metadata()`

**Files:**
- Modify: `src/core/operations.rs`
- Modify: `tests/integration/production_wiring_tests.rs`

- [ ] **Step 1: Add failing high-level writer tests**

Append these tests to `tests/integration/production_wiring_tests.rs`:

```rust
fn copy_fixture_to_temp(path: &str, suffix: &str) -> tempfile::NamedTempFile {
    let temp = tempfile::Builder::new()
        .suffix(suffix)
        .tempfile()
        .expect("create temp fixture copy");
    fs::copy(path, temp.path()).expect("copy fixture");
    temp
}

#[test]
fn write_metadata_routes_png_pdf_and_tiff_writers() {
    let png = copy_fixture_to_temp("tests/fixtures/png/sample.png", ".png");
    let pdf = copy_fixture_to_temp("tests/fixtures/pdf/sample.pdf", ".pdf");
    let tiff = copy_fixture_to_temp("tests/fixtures/tiff/sample.tif", ".tif");

    let mut png_metadata = read_metadata(png.path()).expect("read png");
    png_metadata.insert("PNG:tEXt:Author", TagValue::new_string("OxiDex QA"));
    write_metadata(png.path(), &png_metadata).expect("write png through high-level API");

    let mut pdf_metadata = read_metadata(pdf.path()).expect("read pdf");
    pdf_metadata.insert("PDF:Title", TagValue::new_string("OxiDex QA"));
    write_metadata(pdf.path(), &pdf_metadata).expect("write pdf through high-level API");

    let mut tiff_metadata = read_metadata(tiff.path()).expect("read tiff");
    tiff_metadata.insert("EXIF:Make", TagValue::new_string("OxiDex QA"));
    write_metadata(tiff.path(), &tiff_metadata).expect("write tiff through high-level API");
}
```

- [ ] **Step 2: Run the test and verify it fails on non-JPEG routes**

Run:

```bash
cargo test --test integration production_wiring_tests::write_metadata_routes_png_pdf_and_tiff_writers -- --nocapture
```

Expected before implementation: failure with unsupported write operations for PNG/PDF/TIFF.

- [ ] **Step 3: Refactor `write_metadata()` routing**

In `src/core/operations.rs`, add imports:

```rust
use crate::writers::pdf_writer::write_pdf_file;
use crate::writers::png_writer::write_png_metadata;
use crate::writers::tiff_writer::write_tiff_file;
```

Replace the serialization block in `write_metadata()` with direct writer routing:

```rust
match format {
    FileFormat::JPEG => {
        let serialized_bytes = write_exif_to_jpeg(&reader, metadata)?;
        write_atomic(path, &serialized_bytes)?;
    }
    FileFormat::PNG => {
        write_png_metadata(path, &reader, metadata)?;
    }
    FileFormat::PDF => {
        write_pdf_file(path, &reader, metadata)?;
    }
    FileFormat::TIFF => {
        write_tiff_file(path, &reader, metadata)?;
    }
    _ => {
        return Err(ExifToolError::unsupported_format(format!(
            "Write operations for format {:?} are not supported",
            format
        )));
    }
}

Ok(())
```

- [ ] **Step 4: Run writer tests**

Run:

```bash
cargo test --test integration production_wiring_tests::write_metadata_routes_png_pdf_and_tiff_writers -- --nocapture
cargo test --test integration png_write_tests pdf_write_tests tiff_write_tests write_operations_tests -- --nocapture
```

Expected: all selected writer tests pass.

- [ ] **Step 5: Commit high-level writer routing**

```bash
git add src/core/operations.rs tests/integration/production_wiring_tests.rs
git commit -m "fix: route high-level writes to existing format writers"
```

---

### Task 4: Repair Python ctypes Binding Against Current ABI

**Files:**
- Modify: `bindings/python/oxidex.py`
- Create: `bindings/python/test_bindings.py`

- [ ] **Step 1: Add a failing Python binding smoke test**

Create `bindings/python/test_bindings.py`:

```python
import os
import unittest

from oxidex import Oxidex


FIXTURE = os.path.abspath(
    os.path.join(os.path.dirname(__file__), "..", "..", "tests", "fixtures", "jpeg", "sample_with_exif.jpg")
)


class OxidexBindingTests(unittest.TestCase):
    def test_import_read_and_count_tags(self):
        with Oxidex() as ox:
            ox.read_file(FIXTURE)
            self.assertGreater(ox.get_tag_count(), 0)


if __name__ == "__main__":
    unittest.main()
```

- [ ] **Step 2: Run the test and verify it fails before the binding fix**

Run:

```bash
cargo build --lib
PYTHONPATH=bindings/python python3 -m unittest bindings/python/test_bindings.py
```

Expected before implementation: import/load failure or missing `oxidex_create` symbol.

- [ ] **Step 3: Correct the library name and symbol names**

In `bindings/python/oxidex.py`, change macOS library name:

```python
if sys.platform == "darwin":
    lib_name = "liboxidex.dylib"
```

Change function declarations from `oxidex_*` to `exiftool_*`:

```python
_lib.exiftool_create.restype = ctypes.c_void_p
_lib.exiftool_create.argtypes = []

_lib.exiftool_destroy.restype = None
_lib.exiftool_destroy.argtypes = [ctypes.c_void_p]

_lib.exiftool_read_file.restype = ctypes.c_int
_lib.exiftool_read_file.argtypes = [ctypes.c_void_p, ctypes.c_char_p]

_lib.exiftool_get_tag_count.restype = ctypes.c_size_t
_lib.exiftool_get_tag_count.argtypes = [ctypes.c_void_p]

_lib.exiftool_get_tag_name_at.restype = ctypes.c_char_p
_lib.exiftool_get_tag_name_at.argtypes = [ctypes.c_void_p, ctypes.c_size_t]

_lib.exiftool_has_tag.restype = ctypes.c_int
_lib.exiftool_has_tag.argtypes = [ctypes.c_void_p, ctypes.c_char_p]

_lib.exiftool_get_tag_string.restype = ctypes.c_char_p
_lib.exiftool_get_tag_string.argtypes = [ctypes.c_void_p, ctypes.c_char_p]

_lib.exiftool_get_tag_integer.restype = ctypes.c_int
_lib.exiftool_get_tag_integer.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.POINTER(ctypes.c_int64)]

_lib.exiftool_get_tag_float.restype = ctypes.c_int
_lib.exiftool_get_tag_float.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.POINTER(ctypes.c_double)]

_lib.exiftool_get_last_error.restype = ctypes.c_char_p
_lib.exiftool_get_last_error.argtypes = []
```

Update wrapper method calls from `_lib.oxidex_*` to `_lib.exiftool_*`.

- [ ] **Step 4: Preserve Python-facing class name**

Keep the public class name `Oxidex`. Do not rename user-facing Python APIs to `ExifTool`; only the C symbol names change.

- [ ] **Step 5: Run Python binding tests**

Run:

```bash
cargo build --lib
PYTHONPATH=bindings/python python3 -m unittest bindings/python/test_bindings.py
```

Expected: one passing test.

- [ ] **Step 6: Commit Python binding fix**

```bash
git add bindings/python/oxidex.py bindings/python/test_bindings.py
git commit -m "fix: bind Python wrapper to current C ABI"
```

---

### Task 5: Wire C FFI Integration Test Into Verification

**Files:**
- Create: `tests/ffi_c_integration.rs`
- Modify: `Justfile`
- Modify: `.github/workflows/ci.yml`

- [ ] **Step 1: Add a Cargo-visible test that compiles and runs the C test**

Create `tests/ffi_c_integration.rs`:

```rust
use std::env;
use std::path::PathBuf;
use std::process::Command;

#[test]
fn c_ffi_integration_test_compiles_and_runs() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let target_dir = env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| manifest_dir.join("target"));
    let profile_dir = target_dir.join("debug");

    let build_status = Command::new("cargo")
        .args(["build", "--lib"])
        .current_dir(&manifest_dir)
        .status()
        .expect("run cargo build --lib");
    assert!(build_status.success(), "cargo build --lib failed");

    let out = env::temp_dir().join("oxidex_c_integration_test");
    let compile_status = Command::new("cc")
        .arg("tests/ffi/c_integration_test.c")
        .arg("-Iinclude")
        .arg("-L")
        .arg(&profile_dir)
        .arg("-loxidex")
        .arg("-o")
        .arg(&out)
        .current_dir(&manifest_dir)
        .status()
        .expect("compile C FFI integration test");
    assert!(compile_status.success(), "C FFI integration compile failed");

    let mut run = Command::new(&out);
    run.current_dir(&manifest_dir);
    run.env("DYLD_LIBRARY_PATH", &profile_dir);
    run.env("LD_LIBRARY_PATH", &profile_dir);
    run.env("PATH", format!("{}:{}", profile_dir.display(), env::var("PATH").unwrap_or_default()));

    let run_status = run.status().expect("run C FFI integration test");
    assert!(run_status.success(), "C FFI integration test failed");
}
```

- [ ] **Step 2: Run the C FFI integration test**

Run:

```bash
cargo test --test ffi_c_integration -- --nocapture
```

Expected: test passes and compiles `tests/ffi/c_integration_test.c`.

- [ ] **Step 3: Add the test to local CI recipes**

In `Justfile`, add a recipe:

```make
test-ffi-c:
    @echo "Running C FFI integration test..."
    cargo test --test ffi_c_integration -- --nocapture
```

Add `test-ffi-c` to the `ci-standard` dependency list:

```make
ci-standard: fmt-check lint-release build-release test test-ffi-c
```

- [ ] **Step 4: Add the test to GitHub Actions**

In `.github/workflows/ci.yml`, add this after the main test step:

```yaml
      - name: C FFI integration test
        run: cargo test --test ffi_c_integration -- --nocapture
```

- [ ] **Step 5: Commit C FFI test wiring**

```bash
git add tests/ffi_c_integration.rs Justfile .github/workflows/ci.yml
git commit -m "test: wire C FFI integration into cargo checks"
```

---

### Task 6: Remove Or Fix Orphan Generated Tag Coverage

**Files:**
- Modify: `src/tag_db/generated_tags.rs`
- Modify: `tests/tag_database_coverage.rs`
- Delete: `src/tag_db/generated/`

- [ ] **Step 1: Prove generated modules are currently unreferenced**

Run:

```bash
rg -n "tag_db::generated::|generated::tags_|tags_exif::|get_tags\\(" src tests oxidex-tags*
```

Expected before deletion: no production references to `src/tag_db/generated/tags_*.rs`.

- [ ] **Step 2: Change generated count to reachable registry count**

Replace `generated_tag_count()` in `src/tag_db/generated_tags.rs`:

```rust
/// Returns the number of tags reachable through the active registry.
pub fn generated_tag_count() -> usize {
    crate::tag_db::tag_registry::tag_count()
}
```

- [ ] **Step 3: Make coverage tests validate reachable descriptors**

Replace `tests/tag_database_coverage.rs` with:

```rust
//! Integration tests for active tag database coverage

use oxidex::tag_db::{generated_tags::generated_tag_count, get_tag_descriptor, tag_count};

#[test]
fn test_tag_database_count_comes_from_active_registry() {
    assert_eq!(
        generated_tag_count(),
        tag_count(),
        "legacy generated count must reflect active registry count"
    );
    assert!(
        tag_count() >= 2886,
        "expected active registry to expose at least 10% ExifTool tag coverage"
    );
}

#[test]
fn test_core_tag_descriptors_are_reachable() {
    for tag in ["EXIF:Make", "EXIF:Model", "GPS:GPSLatitude", "XMP:Creator", "IPTC:ObjectName"] {
        assert!(
            get_tag_descriptor(tag).is_some(),
            "expected active registry descriptor for {tag}"
        );
    }
}
```

- [ ] **Step 4: Delete orphan generated modules**

Run:

```bash
git rm -r src/tag_db/generated
```

- [ ] **Step 5: Run registry and workspace checks**

Run:

```bash
cargo test --test tag_database_coverage -- --nocapture
cargo check --workspace --all-targets --all-features
```

Expected: coverage tests pass using active registry count; workspace check passes without `src/tag_db/generated/`.

- [ ] **Step 6: Commit tag coverage cleanup**

```bash
git add src/tag_db/generated_tags.rs tests/tag_database_coverage.rs
git commit -m "fix: make tag coverage reflect active registry"
```

---

### Task 7: Wire ExifTool-Style CLI Options And Batch Output Flags

**Files:**
- Modify: `src/cli/args.rs`
- Modify: `src/cli/batch_processor.rs`
- Modify: `tests/integration.rs`
- Create: `tests/integration/cli_batch_wiring_tests.rs`

- [ ] **Step 1: Include CLI batch wiring tests**

Add this to `tests/integration.rs`:

```rust
#[path = "integration/cli_batch_wiring_tests.rs"]
mod cli_batch_wiring_tests;
```

- [ ] **Step 2: Add failing CLI tests**

Create `tests/integration/cli_batch_wiring_tests.rs`:

```rust
use std::process::Command;

fn oxidex(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .args(args)
        .output()
        .expect("run oxidex binary")
}

#[test]
fn single_dash_json_is_accepted() {
    let output = oxidex(&["-json", "tests/fixtures/jpeg/sample_with_exif.jpg"]);
    assert!(
        output.status.success(),
        "expected -json to succeed: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).trim_start().starts_with('['));
}

#[test]
fn batch_directory_honors_short_format() {
    let output = oxidex(&["-s", "tests/fixtures/jpeg/simple"]);
    assert!(
        output.status.success(),
        "expected batch -s to succeed: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("IFD0:") || stdout.contains("EXIF:"));
    assert!(!stdout.contains("Found "));
}
```

- [ ] **Step 3: Normalize single-dash long options before lexopt**

In `src/cli/args.rs`, add a normalizer before `Parser::from_args()`:

```rust
fn normalize_exiftool_option(arg: String) -> String {
    match arg.as_str() {
        "-json" => "--json".to_string(),
        "-csv" => "--csv".to_string(),
        "-preserve-file-times" => "--preserve-file-times".to_string(),
        "-backup" => "--backup".to_string(),
        "-readonly" => "--readonly".to_string(),
        "-exiftool-compat" => "--exiftool-compat".to_string(),
        "-TagsFromFile" => "--TagsFromFile".to_string(),
        _ => arg,
    }
}
```

Use it in the raw argument loop:

```rust
let raw_args: Vec<String> = std::env::args()
    .skip(1)
    .map(normalize_exiftool_option)
    .collect();
```

- [ ] **Step 4: Reuse output formatters in batch read output**

In `src/cli/batch_processor.rs`, import formatters:

```rust
use crate::cli::output_formatter::{
    CsvFormatter, HumanReadableFormatter, JsonFormatter, OutputFormatter, ShortFormatter,
};
use crate::core::exiftool_compat::format_for_exiftool;
```

Replace the `if args.json { ... } else { ... }` output branch in `batch_read()` with:

```rust
let tag_filter = args.specific_tags();
let filter_slice = tag_filter.as_deref();

if args.csv {
    let formatter = CsvFormatter;
    for (_, result) in &results {
        if let Ok(metadata) = result {
            let metadata = if args.exiftool_compat() {
                format_for_exiftool(metadata)
            } else {
                metadata.clone()
            };
            print!("{}", formatter.format(&metadata, filter_slice));
        }
    }
} else if args.json {
    output_json_results(&results, args, filter_slice)?;
} else if args.short_format {
    let formatter = ShortFormatter;
    for (_, result) in &results {
        if let Ok(metadata) = result {
            let metadata = if args.exiftool_compat() {
                format_for_exiftool(metadata)
            } else {
                metadata.clone()
            };
            print!("{}", formatter.format(&metadata, filter_slice));
        }
    }
} else {
    let formatter = HumanReadableFormatter;
    for (path, result) in &results {
        if let Ok(metadata) = result {
            println!("File: {}", path.display());
            let metadata = if args.exiftool_compat() {
                format_for_exiftool(metadata)
            } else {
                metadata.clone()
            };
            print!("{}", formatter.format(&metadata, filter_slice));
        }
    }
}
```

Update `output_json_results()` to accept `args` and `filter_slice`, then internally use `JsonFormatter`:

```rust
fn output_json_results(
    results: &[(PathBuf, Result<crate::core::MetadataMap>)],
    args: &CliArgs,
    filter_slice: Option<&[String]>,
) -> Result<()> {
    let formatter = JsonFormatter;
    let mut rendered = Vec::new();

    for (path, result) in results {
        match result {
            Ok(metadata) => {
                let metadata = if args.exiftool_compat() {
                    format_for_exiftool(metadata)
                } else {
                    metadata.clone()
                };
                let mut value: serde_json::Value = serde_json::from_str(&formatter.format(&metadata, filter_slice))
                    .map_err(|e| ExifToolError::parse_error(format!("JSON formatting failed: {e}")))?;
                if let Some(obj) = value.as_array_mut().and_then(|items| items.first_mut()).and_then(|item| item.as_object_mut()) {
                    obj.insert("SourceFile".to_string(), serde_json::Value::String(path.display().to_string()));
                }
                if let Some(items) = value.as_array() {
                    rendered.extend(items.iter().cloned());
                }
            }
            Err(e) => {
                rendered.push(serde_json::json!({
                    "SourceFile": path.display().to_string(),
                    "Error": e.to_string()
                }));
            }
        }
    }

    println!("{}", serde_json::to_string_pretty(&rendered)
        .map_err(|e| ExifToolError::parse_error(format!("JSON serialization failed: {e}")))?);
    Ok(())
}
```

- [ ] **Step 5: Run CLI wiring tests**

Run:

```bash
cargo test --test integration cli_batch_wiring_tests -- --nocapture
```

Expected: all CLI batch wiring tests pass.

- [ ] **Step 6: Commit CLI wiring**

```bash
git add src/cli/args.rs src/cli/batch_processor.rs tests/integration.rs tests/integration/cli_batch_wiring_tests.rs
git commit -m "fix: wire advertised CLI output options"
```

---

### Task 8: Final High-Severity Verification

**Files:**
- No source edits unless verification exposes a regression.

- [ ] **Step 1: Run focused high-severity tests**

Run:

```bash
cargo test --test integration production_wiring_tests cli_batch_wiring_tests -- --nocapture
cargo test --test tag_database_coverage -- --nocapture
cargo test --test ffi_c_integration -- --nocapture
PYTHONPATH=bindings/python python3 -m unittest bindings/python/test_bindings.py
```

Expected: all commands exit 0.

- [ ] **Step 2: Run workspace compile checks**

Run:

```bash
cargo check --workspace --all-targets --all-features
cargo test --workspace --all-features --no-run
```

Expected: both commands exit 0. Warnings can remain if they are pre-existing and unrelated, but new warnings in touched files should be fixed.

- [ ] **Step 3: Inspect git diff**

Run:

```bash
git diff --stat
git diff --check
```

Expected: no whitespace errors; diff contains only files from this plan.

- [ ] **Step 4: Commit final verification-only adjustments if needed**

If Step 1 or Step 2 required small fixes, commit them:

```bash
git add <changed-files>
git commit -m "fix: complete high severity wiring remediation"
```

If no fixes were needed, do not create an empty commit.

---

## Acceptance Criteria

- `read_metadata()` reaches all high-severity implemented parser families covered by the new production tests.
- iWork files route to iWork parsers, not OOXML parsers.
- `write_metadata()` routes JPEG, PNG, PDF, and TIFF through existing writers.
- Python binding imports and reads a fixture through the current `exiftool_*` ABI.
- The C FFI integration test runs through a Cargo-visible test.
- Tag coverage tests use active registry state, not a hard-coded generated count.
- Advertised `-json` and batch output flags have production tests.
- Final verification commands in Task 8 exit 0.
