# Code Refinement Task

The previous code submission did not pass verification. You must fix the following issues and resubmit your work.

---

## Original Task Description

_Not provided in verification context._

---

## Issues Detected

* **Test Failure:** `exiftool_comparison_tests::test_comparison_jpeg_with_exif_xmp` fails because the new XMP tag canonicalization in `src/parsers/jpeg/xmp_parser.rs:160` renames tags to `XMP:*`, so our JSON no longer matches Perl ExifTool's namespace-qualified tag names. Restore the original tag names or otherwise emit the same keys Perl produces.
* **Test Failure:** `exiftool_comparison_tests::test_comparison_pdf` fails because `src/writers/pdf_writer.rs:140-149` filters Info dictionary keys to `CreationDate`/`ModDate`, dropping the `PDF:CreateDate` and `PDF:ModifyDate` entries Perl emits, and the keywords serialization now returns a single string instead of the expected list. Preserve the `CreateDate`/`ModifyDate` tags and make sure `PDF:Keywords` matches the Perl structure.
* **Test Failure:** `exiftool_comparison_tests::test_comparison_tiff_big_endian` and `exiftool_comparison_tests::test_comparison_tiff_multipage` now report mismatches because `src/core/operations.rs:867-874` blocks enum string conversion for Orientation, returning numeric values instead of the human-readable strings Perl emits. Allow `tiff_enum_to_string` to format Orientation again so outputs match.

---

## Best Approach to Fix

1. Update the XMP parsing logic to stop canonicalizing tag names away from the original namespace-qualified form so that comparison tests continue to see keys like `XMP-xmp:Creator`, `XMP-dc:Title`, etc.
2. Adjust the PDF writer to accept and emit both `CreateDate`/`ModifyDate` aliases (matching Perl's keys), and ensure `PDF:Keywords` is serialized in the same structure Perl ExifTool uses rather than a comma-joined string.
3. Revert the Orientation-special casing in `raw_bytes_to_tag_value` so SHORT enum values—including Orientation—always return the enum string. Re-run `cargo clippy --all-targets --all-features` and `cargo test --all-features` to confirm the comparison suite passes.
