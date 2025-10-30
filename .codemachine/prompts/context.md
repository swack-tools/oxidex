# Task Briefing Package

This package contains all necessary information and strategic guidance for the Coder Agent.

---

## 1. Current Task Details

This is the full specification of the task you must complete.

```json
{
  "task_id": "I4.T5",
  "iteration_id": "I4",
  "iteration_goal": "Add support for PDF and MP4/QuickTime formats, implement batch processing with recursive directory traversal and parallel execution, add metadata copying between files, and expand tag registry.",
  "description": "Expand tag registry in src/tag_db/tag_registry.rs from 100 to 500 tags. Add tags from: (1) EXIF (complete common tags, add maker-specific tags for Canon, Nikon, Sony), (2) XMP (Dublin Core, IPTC Core, Camera Raw), (3) IPTC (Application Record), (4) GPS (complete all GPS tags), (5) PDF metadata, (6) QuickTime/MP4 metadata. Reference ExifTool tag documentation for definitions. Update unit tests to verify new tags.",
  "agent_type_hint": "BackendAgent",
  "inputs": "ExifTool tag documentation (https://exiftool.org/TagNames/), I2.T2 tag registry structure",
  "target_files": ["src/tag_db/tag_registry.rs"],
  "input_files": ["src/tag_db/tag_registry.rs"],
  "deliverables": "Tag registry with 500 tags, updated unit tests",
  "acceptance_criteria": "Registry contains 500+ TagDescriptor entries, tags cover: EXIF (300+), XMP (100+), IPTC (50+), GPS (30+), PDF (10+), QuickTime (10+), all tags have valid type and format family information, unit tests verify lookup for at least 50 tags across all families, cargo test tag_registry passes",
  "dependencies": ["I2.T2"],
  "parallelizable": true,
  "done": false
}
```

---

## 2. Architectural & Planning Context

The following are the relevant sections from the architecture and plan documents, which I found by analyzing the task description.

### Context: Tag Registry Structure and Requirements

**From Task I2.T2 (Completed Dependency):**

The tag registry was initially created with 100 common tags manually covering:
- EXIF (60+ tags): Camera information, exposure settings, image properties, date/time, color/scene information
- GPS (20+ tags): Location data, altitude, timestamps, speed, direction
- XMP (20+ tags): Dublin Core and XMP Basic metadata

The registry uses a `HashMap<&'static str, TagDescriptor>` with lazy initialization via `once_cell::Lazy`. Each TagDescriptor contains:
- `tag_id`: Either numeric (EXIF/GPS) or named (XMP)
- `tag_name`: Canonical name like "EXIF:Make" or "XMP:Creator"
- `format_family`: FormatFamily enum (EXIF, XMP, IPTC, GPS, PDF, QuickTime, etc.)
- `writable`: Boolean indicating if tag can be written
- `value_type`: ValueType enum (String, Integer, Float, Rational, Binary, DateTime, Struct)
- `description`: Human-readable purpose
- `example_values`: Vec of example strings

**Key Design Patterns:**
- Tags use prefix notation: "EXIF:Make", "GPS:GPSLatitude", "XMP:Creator"
- Numeric tag IDs for EXIF/GPS (e.g., 0x010F for Make)
- String-based tag IDs for XMP (e.g., "XMP-dc:Creator")
- Lookup function: `get_tag_descriptor(name: &str) -> Option<&TagDescriptor>`
- Count function: `tag_count() -> usize`

### Context: Tag Database Schema

**From api/tag_database_schema.json:**

The JSON Schema defines validation rules for TagDescriptor objects:
- Required fields: tag_id, tag_name, format_family, writable, value_type, description, example_values
- tag_id: oneOf [integer 0-65535, string with minLength 1]
- tag_name: Must match pattern `^[A-Za-z0-9_:-]+$`
- format_family: Enum with 11 values including EXIF, XMP, IPTC, GPS, ICC_Profile, Photoshop, MakerNotes, JFIF, PNG, PDF, QuickTime
- value_type: Enum with 7 types (String, Integer, Float, Rational, Binary, DateTime, Struct)
- example_values: Array with minItems 1 and uniqueItems constraint

### Context: Tag Categories to Add

**Based on Task Description, you must expand to 500 tags covering:**

1. **EXIF Tags (expand from 60 to 300+):**
   - Complete common EXIF tags from TIFF specification
   - Maker-specific tags for Canon (CanonCustom, CanonFileInfo, CanonShotInfo, CanonAFInfo)
   - Maker-specific tags for Nikon (NikonColorMode, NikonFlashInfo, NikonShootingMode, NikonVRInfo)
   - Maker-specific tags for Sony (SonyModelID, SonyCreativeStyle, SonyColorMode, SonyAutoHDR)
   - Additional exposure and scene tags
   - File structure tags (StripOffsets, StripByteCounts, TileOffsets, etc.)

2. **XMP Tags (expand from 20 to 100+):**
   - Complete Dublin Core namespace (dc:source, dc:type, dc:coverage, dc:relation)
   - IPTC Core for XMP (Iptc4xmpCore:Location, Iptc4xmpCore:CountryCode, Iptc4xmpCore:Scene)
   - Camera Raw namespace (crs:Temperature, crs:Tint, crs:Exposure2012, crs:Contrast2012, crs:Highlights2012, crs:Shadows2012)
   - Photoshop namespace (photoshop:Credit, photoshop:Source, photoshop:Headline, photoshop:City, photoshop:Country)
   - Rights management (xmpRights:UsageTerms, xmpRights:WebStatement)

3. **IPTC Tags (add 50+ new):**
   - Application Record 2 tags (Caption, Headline, Keywords, Category, SupplementalCategories)
   - By-line, By-line Title, Credit, Source
   - City, Province-State, Country, Original Transmission Reference
   - Date and time stamps
   - Priority, Urgency, Object Name

4. **GPS Tags (expand from 20 to 30+):**
   - Complete all GPS IFD tags (GPSDestBearing, GPSDestDistance, GPSDestLatitude, GPSDestLongitude)
   - GPS Processing Method, GPS Area Information
   - GPS Differential correction

5. **PDF Metadata (add 10+ tags):**
   - Info dictionary: Title, Author, Subject, Keywords, Creator, Producer, CreationDate, ModDate
   - Trapped, GTS_PDFXVersion

6. **QuickTime/MP4 Metadata (add 10+ tags):**
   - User data atoms: ©nam (title), ©ART (artist), ©alb (album), ©day (year)
   - ©cmt (comment), ©gen (genre), ©wrt (composer)
   - Location data for videos, Duration, TrackID

---

## 3. Codebase Analysis & Strategic Guidance

The following analysis is based on my direct review of the current codebase. Use these notes and tips to guide your implementation.

### Relevant Existing Code

*   **File:** `src/tag_db/tag_registry.rs`
    *   **Summary:** This is the main file you MUST modify. It currently contains exactly 100 TagDescriptor entries organized into three categories: 60 EXIF tags (subdivided into Camera Information, Exposure Settings, Image Properties, Date/Time, and Color/Scene), 20 GPS tags, and 20 XMP tags. The registry uses `once_cell::Lazy<HashMap<&'static str, TagDescriptor>>` for lazy initialization with zero-cost abstraction.
    *   **Current Structure:** The file is 1,627 lines. Lines 11-1391 define the TAG_REGISTRY static with all tag insertions. Lines 1393-1415 contain the public API functions (`get_tag_descriptor`, `tag_count`). Lines 1424-1626 contain comprehensive unit tests.
    *   **Recommendation:** You MUST expand this registry from 100 to 500+ tags. Follow the existing pattern exactly:
        - Keep the organizational comments (e.g., `// ===== EXIF TAGS (300 total) =====`)
        - Use sub-categories with descriptive comments (e.g., `// --- Camera Information (X tags) ---`)
        - Each tag insertion follows this pattern:
          ```rust
          registry.insert(
              "EXIF:TagName",
              TagDescriptor::new(
                  TagId::new_numeric(0xXXXX),  // or TagId::new_named("XMP-ns:Name")
                  "EXIF:TagName".to_string(),
                  FormatFamily::EXIF,
                  true,  // or false for writable
                  ValueType::String,  // or Integer, Rational, etc.
                  "Human-readable description".to_string(),
                  vec!["example1".to_string(), "example2".to_string()],
              ),
          );
          ```
    *   **Critical:** Update line 14 to change capacity from 100 to 512: `HashMap::with_capacity(512)`
    *   **Critical:** Update line 1430 test to expect 500+ tags: `assert_eq!(tag_count(), 500, "Registry must contain at least 500 tags");` (or use >= assertion)

*   **File:** `src/core/tag_descriptor.rs`
    *   **Summary:** Defines the `TagDescriptor` struct and related enums (`TagId`, `FormatFamily`, `ValueType`). This file provides the data structures you'll use but should NOT be modified for this task.
    *   **Key Types:**
        - `TagId`: enum with `Numeric(u16)` and `Named(String)` variants
        - `FormatFamily`: enum with 11 variants (EXIF, XMP, IPTC, GPS, ICCProfile, Photoshop, MakerNotes, JFIF, PNG, PDF, QuickTime)
        - `ValueType`: enum with 7 variants (String, Integer, Float, Rational, Binary, DateTime, Struct)
    *   **Recommendation:** Import these types as shown in tag_registry.rs line 7: `use crate::core::tag_descriptor::{FormatFamily, TagDescriptor, TagId, ValueType};`

*   **File:** `api/tag_database_schema.json`
    *   **Summary:** JSON Schema defining the structure and validation rules for TagDescriptor objects. Used for documentation and potential future code generation.
    *   **Recommendation:** Use this as a reference for valid tag structures, but you don't need to modify it for this task.

*   **File:** `src/tag_db/mod.rs`
    *   **Summary:** Module file that exports the tag registry functions. Re-exports `get_tag_descriptor` and `tag_count` for easier access.
    *   **Recommendation:** No changes needed to this file for this task.

### Implementation Tips & Notes

*   **Tip:** The task requires referencing ExifTool tag documentation at https://exiftool.org/TagNames/. You should reference these specific pages:
    - https://exiftool.org/TagNames/EXIF.html - Complete EXIF tag list
    - https://exiftool.org/TagNames/Canon.html - Canon maker notes
    - https://exiftool.org/TagNames/Nikon.html - Nikon maker notes
    - https://exiftool.org/TagNames/Sony.html - Sony maker notes
    - https://exiftool.org/TagNames/XMP.html - XMP namespaces
    - https://exiftool.org/TagNames/IPTC.html - IPTC Application Record
    - https://exiftool.org/TagNames/GPS.html - Complete GPS tags
    - https://exiftool.org/TagNames/PDF.html - PDF metadata
    - https://exiftool.org/TagNames/QuickTime.html - QuickTime/MP4 tags

*   **Note:** The existing code has comprehensive test coverage. You MUST add or update tests to verify the new tags:
    - Update `test_registry_count()` to expect 500+ tags
    - Update `test_tag_distribution()` to verify new counts:
        - EXIF: 300+ tags (including MakerNotes)
        - XMP: 100+ tags
        - IPTC: 50+ tags
        - GPS: 30+ tags
        - PDF: 10+ tags
        - QuickTime: 10+ tags
    - Add new test cases to verify lookup for at least 50 tags across all families (you can add tests like `test_iptc_headline_lookup()`, `test_pdf_title_lookup()`, `test_canon_model_id_lookup()`, etc.)

*   **Tip:** For maker-specific tags, use the FormatFamily::MakerNotes enum value. Follow the naming convention from ExifTool:
    - Canon tags: "Canon:ModelID", "Canon:CanonFirmwareVersion", etc. with TagId::new_numeric() for numeric IDs
    - Nikon tags: "Nikon:ShutterCount", "Nikon:SerialNumber", etc.
    - Sony tags: "Sony:SonyModelID", "Sony:CreativeStyle", etc.

*   **Tip:** For IPTC tags, use FormatFamily::IPTC and follow the naming convention "IPTC:Caption-Abstract", "IPTC:Headline", etc. IPTC tags typically use numeric IDs (record number + dataset number).

*   **Tip:** For XMP tags, use TagId::new_named() with the full namespace prefix:
    - Dublin Core: "XMP-dc:source", "XMP-dc:type", etc.
    - IPTC Core: "XMP-iptcCore:Location", "XMP-iptcCore:CountryCode", etc.
    - Camera Raw: "XMP-crs:Temperature", "XMP-crs:Exposure2012", etc.
    - Photoshop: "XMP-photoshop:Credit", "XMP-photoshop:City", etc.

*   **Warning:** Be careful with tag ID conflicts. EXIF numeric IDs must be unique within the EXIF namespace. Check ExifTool documentation for correct tag IDs to avoid collisions.

*   **Note:** The acceptance criteria requires "cargo test tag_registry passes". After adding all tags, run the following to verify:
    ```bash
    cargo test --lib tag_registry
    ```
    All existing tests must pass, plus any new tests you add.

*   **Performance Note:** The HashMap uses capacity 100 currently. With 500+ tags, you should increase the initial capacity to avoid reallocations. Use `HashMap::with_capacity(512)` on line 14 of tag_registry.rs.

*   **Code Quality:** Follow the existing code style exactly:
    - Use 4-space indentation
    - Add organizational comments for major sections and subsections
    - Keep tag insertions in logical groups (don't intermix EXIF, XMP, IPTC randomly)
    - Use descriptive, accurate descriptions from ExifTool documentation
    - Provide at least 1-2 meaningful example values for each tag

*   **Documentation:** Each tag should have:
    - Accurate tag ID from ExifTool specifications
    - Correct canonical name with proper prefix (EXIF:, GPS:, XMP:, IPTC:, PDF:, QuickTime:, Canon:, Nikon:, Sony:)
    - Correct FormatFamily enum value
    - Correct writability flag (most tags are writable, but some like version numbers are read-only)
    - Correct ValueType based on data format
    - Clear, concise description
    - Realistic example values

*   **Verification Strategy:** After implementation:
    1. Run `cargo build` to ensure code compiles
    2. Run `cargo test --lib tag_registry` to verify all tests pass
    3. Run `cargo clippy` to check for any warnings
    4. Verify the count: the registry should have exactly 500+ tags (be generous, 520-550 is fine to exceed requirements)
    5. Verify distribution: Use the `test_tag_distribution()` test to ensure you meet the minimums for each family

---

## Task Execution Checklist

1. ✅ Read and understand the existing tag_registry.rs structure
2. ⬜ Research ExifTool documentation for the 400 additional tags needed
3. ⬜ Plan tag organization (decide how to group the 300+ EXIF tags, 100+ XMP tags, etc.)
4. ⬜ Update HashMap capacity to 512 on line 14
5. ⬜ Add EXIF tags (expand to 300+):
   - ⬜ Common EXIF tags from TIFF spec
   - ⬜ Canon maker notes (ModelID, FirmwareVersion, etc.)
   - ⬜ Nikon maker notes (ShutterCount, SerialNumber, etc.)
   - ⬜ Sony maker notes (SonyModelID, CreativeStyle, etc.)
6. ⬜ Add XMP tags (expand to 100+):
   - ⬜ Complete Dublin Core namespace
   - ⬜ IPTC Core for XMP
   - ⬜ Camera Raw namespace
   - ⬜ Photoshop namespace
7. ⬜ Add IPTC tags (50+): Application Record 2 tags
8. ⬜ Add GPS tags (expand to 30+): Complete GPS IFD
9. ⬜ Add PDF metadata tags (10+): Info dictionary
10. ⬜ Add QuickTime/MP4 tags (10+): User data atoms
11. ⬜ Update test_registry_count() to expect 500+
12. ⬜ Update test_tag_distribution() with new minimums
13. ⬜ Add new unit tests for tag lookup verification (50+ tags across families)
14. ⬜ Run `cargo build` and fix any compilation errors
15. ⬜ Run `cargo test --lib tag_registry` and ensure all tests pass
16. ⬜ Run `cargo clippy` and address any warnings
17. ⬜ Verify final tag count is 500+ via tag_count() function
