//! Canon CIFF (`HEAPJPGM`), embedded verbatim inside a JPEG APP0 segment.
//!
//! This is the same container a standalone `.CRW` file is, carried as the
//! payload of one APP0 marker in an otherwise-ordinary JPEG. ExifTool reaches
//! both through the very same `ProcessCRW` (ExifTool.pm:7730-7739 for the APP0
//! case, `JPEG.pm:34-35` for its table entry), so this module does not decode
//! CIFF itself: it hands the payload to [`decode_ciff_container`], the body of
//! the standalone-CRW path (`src/parsers/raw/metadata.rs`), and only re-homes
//! the result under the group ExifTool gives it.
//!
//! # Why every key becomes `CIFF:`
//!
//! Before calling `ProcessCRW`, the APP0 branch sets `$$self{SET_GROUP1} =
//! 'CIFF'` (ExifTool.pm:7734) and clears it afterwards. That overrides family
//! 1 for *every* tag the walk produces -- the `%CanonRaw::*` tables' own
//! `CanonRaw` and the shared `%Canon::*` records' `Canon` alike -- which is why
//! the pinned 13.59 oracle prints `[MakerNotes:CIFF] FocalLength`,
//! `[MakerNotes:CIFF] ImageWidth` and `[MakerNotes:CIFF] Make` for
//! `t/images/ExifTool.jpg` and the early PowerShot JPEGs, where the same
//! records in a `.CRW` print as `CanonRaw:` and `Canon:`.
//! `cli::tag_resolution::resolve_family0` maps this `"CIFF"` group back to
//! family-0 `MakerNotes` on request.
//!
//! # Record order
//!
//! `t/images/ExifTool.jpg` also carries an `IFD0` `Make`/`Model`
//! (`FUJIFILM`), and `-Make` resolves to the CIFF `Canon` because both are
//! ordinary occurrences and the CIFF one is recorded later in file order
//! (Step 18/19's newest-wins tie rule, `TagSink::record`). So this must run
//! after `process_exif_segments`, and the re-homed occurrences are appended to
//! the file's map in the decoder's own order.
//!
//! Ported from `origin/main` 9b215f03 (#696), which read CIFF-in-JPEG with a
//! separate hand-written walker (`jpeg/app_segments/ciff.rs`). The tip already
//! had a fuller CIFF decoder on its CRW path, so the fix is re-expressed as
//! reuse of that decoder rather than a second copy of `%CanonRaw::Main`.

use crate::core::MetadataMap;
use crate::parsers::jpeg::segment_parser::Segment;
use crate::parsers::raw::metadata::decode_ciff_container;

/// JPEG APP0 marker (0xFFE0). CIFF containers embedded this way arrive as
/// the payload of an APP0 segment, same as JFIF/JFXX/OCAD.
const APP0_MARKER: u16 = 0xFFE0;

/// ExifTool's `SET_GROUP1` for an APP0 CIFF record (ExifTool.pm:7734).
const CIFF_GROUP: &str = "CIFF";

/// Scans every APP0 segment for an embedded CIFF (`II`+`HEAPJPGM`) container
/// and records everything `ProcessCRW` would report for it, under `CIFF:`.
///
/// Must run after `process_exif_segments` (see the module note on record
/// order).
pub(crate) fn process_ciff_app0_segments(segments: &[Segment<'_>], metadata: &mut MetadataMap) {
    for segment in segments {
        if segment.marker != APP0_MARKER {
            continue;
        }
        let data = segment.data;
        // `ExifTool.pm:7730`: `$$segDataPt =~ /^(II|MM).{4}HEAPJPGM/s` --
        // only the `JPGM` form; a `HEAPCCDR` payload in APP0 is not a CIFF
        // record to ExifTool. Both byte orders occur: `ExifTool.jpg` and the
        // A5/Pro70 samples are `II`, `CanonPowerShot600.jpg` is `MM`.
        if !matches!(data.get(..2), Some(b"II") | Some(b"MM"))
            || data.get(6..14) != Some(b"HEAPJPGM")
        {
            continue;
        }
        let mut decoded = MetadataMap::new();
        decode_ciff_container(data, &mut decoded);
        for (key, occurrence) in decoded.all_occurrences() {
            let name = key.split_once(':').map_or(key.as_str(), |(_, name)| name);
            metadata.insert_renamed_occurrence(format!("{CIFF_GROUP}:{name}"), occurrence);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `%CanonRaw::Main` 0x080a `CanonRawMakeModel` (CanonRaw.pm:74-78).
    const CANON_RAW_MAKE_MODEL: u16 = 0x080A;
    /// `%CanonRaw::Main` 0x1029 `CanonFocalLength` (CanonRaw.pm:118-122), a
    /// `SubDirectory` onto `%Canon::FocalLength`.
    const CANON_RAW_FOCAL_LENGTH: u16 = 0x1029;
    /// `%CanonRaw::Main` 0x1817 `FileNumber` (CanonRaw.pm:303-309), `int32u`
    /// by its `0x18` type bits.
    const CANON_RAW_FILE_NUMBER: u16 = 0x1817;

    /// Verbatim payload shape (offsets are relative within the CIFF
    /// container, not this test's byte array): a root directory with one
    /// entry pointing at the `CanonRawMakeModel` record. This is a
    /// minimized version of what `t/images/ExifTool.jpg`'s third APP0
    /// segment actually carries, not a synthetic invention -- the record
    /// bytes are the real ones from that file's `-v3` dump (`Canon\0Canon
    /// PowerShot A5\0`).
    fn build_minimal_ciff_app0() -> Vec<u8> {
        let mut data = Vec::new();
        // Header: "II" + heap_start (u32 LE) + "HEAPJPGM".
        data.extend_from_slice(b"II");
        let heap_start_pos = data.len();
        data.extend_from_slice(&0u32.to_le_bytes()); // patched below
        data.extend_from_slice(b"HEAPJPGM");
        let heap_start = data.len() as u32;
        data[heap_start_pos..heap_start_pos + 4].copy_from_slice(&heap_start.to_le_bytes());

        // Heap: one record (the MakeModel binary blob), relative offset 0.
        let record: [u8; 25] = *b"Canon\0Canon PowerShot A5\0";
        let record_relative = 0u32;
        data.extend_from_slice(&record);

        // Root directory: entry_count=1, one 10-byte entry, trailing 4-byte
        // directory-offset word (unused by this walker, but part of the
        // real CIFF shape so bounds math matches production).
        let directory_offset_in_heap = data.len() as u32 - heap_start;
        data.extend_from_slice(&1u16.to_le_bytes()); // entry_count
        data.extend_from_slice(&CANON_RAW_MAKE_MODEL.to_le_bytes()); // tag
        data.extend_from_slice(&(record.len() as u32).to_le_bytes()); // size
        data.extend_from_slice(&record_relative.to_le_bytes()); // relative offset

        // Trailing 4-byte word: root directory's own offset, relative to
        // heap_start, from the end of the container (CIFF's own footer
        // convention).
        data.extend_from_slice(&directory_offset_in_heap.to_le_bytes());
        data
    }

    #[test]
    fn decodes_make_and_model_from_a_minimal_embedded_ciff_directory() {
        let payload = build_minimal_ciff_app0();
        let segments = vec![Segment::new(APP0_MARKER, 0, &payload)];
        let mut metadata = MetadataMap::new();

        process_ciff_app0_segments(&segments, &mut metadata);

        assert_eq!(metadata.get_string("CIFF:Make"), Some("Canon"));
        assert_eq!(
            metadata.get_string("CIFF:Model"),
            Some("Canon PowerShot A5")
        );
    }

    /// The same container plus a second, `valueInDir` entry carrying
    /// `CanonFocalLength` (`0x1029 | 0x4000` == `0x5029`).
    ///
    /// The eight record bytes are `t/images/ExifTool.jpg`'s own, read straight
    /// out of the pinned 13.59 oracle's `-v3` dump of that file's third APP0
    /// segment (`0252: 01 00 05 00 c7 00 92 00`, decoded there as
    /// `FocalType = 1`, `FocalLength = 5`, `FocalPlaneXSize = 199`,
    /// `FocalPlaneYSize = 146`), not invented for this test.
    fn build_ciff_app0_with_focal_length() -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(b"II");
        let heap_start_pos = data.len();
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(b"HEAPJPGM");
        let heap_start = data.len() as u32;
        data[heap_start_pos..heap_start_pos + 4].copy_from_slice(&heap_start.to_le_bytes());

        let record: [u8; 25] = *b"Canon\0Canon PowerShot A5\0";
        data.extend_from_slice(&record);

        let directory_offset_in_heap = data.len() as u32 - heap_start;
        data.extend_from_slice(&2u16.to_le_bytes()); // entry_count

        // Entry 0: MakeModel, value out in the heap at relative offset 0.
        data.extend_from_slice(&CANON_RAW_MAKE_MODEL.to_le_bytes());
        data.extend_from_slice(&(record.len() as u32).to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());

        // Entry 1: CanonFocalLength with CanonRaw.pm's `valueInDir` bit set --
        // the eight bytes that would be `size` and `ptr` are the record.
        data.extend_from_slice(&(CANON_RAW_FOCAL_LENGTH | 0x4000).to_le_bytes());
        data.extend_from_slice(&[0x01, 0x00, 0x05, 0x00, 0xc7, 0x00, 0x92, 0x00]);

        data.extend_from_slice(&directory_offset_in_heap.to_le_bytes());
        data
    }

    /// A `valueInDir` entry is read from the directory, and
    /// `%Canon::FocalLength` keys 2/3 come out with ExifTool's own
    /// `sprintf("%.2f mm", $val * 25.4 / 1000)` print forms.
    ///
    /// Before the `valueInDir` branch existed this walker read the entry's
    /// inline value bytes as a size and a pointer, landed out of bounds and
    /// dropped the record silently -- along with every other inline-value
    /// entry in the container.
    #[test]
    fn decodes_focal_plane_sizes_from_a_value_in_dir_entry() {
        let payload = build_ciff_app0_with_focal_length();
        let segments = vec![Segment::new(APP0_MARKER, 0, &payload)];
        let mut metadata = MetadataMap::new();

        process_ciff_app0_segments(&segments, &mut metadata);

        assert_eq!(metadata.get_string("CIFF:Make"), Some("Canon"));
        assert_eq!(metadata.get_string("CIFF:FocalPlaneXSize"), Some("5.05 mm"));
        assert_eq!(metadata.get_string("CIFF:FocalPlaneYSize"), Some("3.71 mm"));
    }

    /// The unrounded `ValueConv` form rides along, because
    /// `CalcScaleFactor35efl` squares these numbers and 5.05 is a different
    /// number than 5.0546.
    #[test]
    fn focal_plane_sizes_carry_the_unrounded_value_form() {
        let payload = build_ciff_app0_with_focal_length();
        let segments = vec![Segment::new(APP0_MARKER, 0, &payload)];
        let mut metadata = MetadataMap::new();

        process_ciff_app0_segments(&segments, &mut metadata);

        let x: f64 = metadata
            .value_form("CIFF:FocalPlaneXSize")
            .expect("FocalPlaneXSize value form")
            .parse()
            .expect("numeric value form");
        assert!((x - 199.0 * 25.4 / 1000.0).abs() < 1e-12);
    }

    /// `FocalLength` (key 1) is `$val / ($$self{FocalUnits} || 1)`
    /// (Canon.pm:2735-2741). `FocalUnits` comes from `%Canon::CameraSettings`,
    /// which this container does not carry -- exactly the early-PowerShot
    /// shape -- so ExifTool divides by 1: the pinned 13.59 oracle prints
    /// `[CIFF] FocalLength : 5 mm` for `t/images/ExifTool.jpg`, whose record
    /// these bytes are. (The previous APP0-only walker omitted it, believing
    /// the divisor unknowable; the shared CRW decoder applies the same `|| 1`.)
    #[test]
    fn focal_length_divides_by_one_without_camera_settings() {
        let payload = build_ciff_app0_with_focal_length();
        let segments = vec![Segment::new(APP0_MARKER, 0, &payload)];
        let mut metadata = MetadataMap::new();

        process_ciff_app0_segments(&segments, &mut metadata);

        assert_eq!(metadata.get_string("CIFF:FocalLength"), Some("5 mm"));
    }

    /// Canon::FocalLength is declared `Priority => 0` in ExifTool (Canon.pm),
    /// so the CIFF occurrence carries priority 0 and never displaces an earlier
    /// standard ExifIFD:FocalLength (priority 1) during tag resolution.
    #[test]
    fn focal_length_has_priority_zero_so_exif_wins_arbitration() {
        let payload = build_ciff_app0_with_focal_length();
        let segments = vec![Segment::new(APP0_MARKER, 0, &payload)];
        let mut metadata = MetadataMap::new();

        // Simulate earlier EXIF IFD occurrence (priority 1)
        metadata.insert_occurrence(
            "ExifIFD:FocalLength",
            crate::core::TagValue::new_string("6.0 mm"),
            crate::core::SHIM_DEFAULT_PRIORITY,
            "ExifIFD",
            crate::core::Instance::default(),
        );

        process_ciff_app0_segments(&segments, &mut metadata);

        // Under -a, both exist
        assert_eq!(metadata.get_string("CIFF:FocalLength"), Some("5 mm"));
        assert_eq!(metadata.get_string("ExifIFD:FocalLength"), Some("6.0 mm"));

        // In arbitration (default -s / composite), ExifIFD wins
        let resolved = crate::cli::tag_resolution::resolve_requested_tag(&metadata, "FocalLength")
            .expect("resolved FocalLength");
        assert_eq!(resolved.lookup_key(), "ExifIFD:FocalLength");
        assert_eq!(resolved.raw, crate::core::TagValue::new_string("6.0 mm"));
    }

    /// A plain `%CanonRaw::Main` scalar, and the group every key lands in.
    ///
    /// `ExifTool.jpg`'s `FileNumber` record is `int32u` 45 (`-v3`: `FileNumber
    /// = 45`); with fewer than five digits `s/(\d+)(\d{4})/$1-$2/` does not
    /// fire. Nothing may come out under the standalone-CRW groups: ExifTool's
    /// `SET_GROUP1 = 'CIFF'` re-homes `CanonRaw` and `Canon` alike.
    #[test]
    fn canonraw_main_scalars_are_rehomed_under_ciff() {
        let mut payload = build_ciff_app0_with_focal_length();
        // Splice a third, `valueInDir` entry in front of the footer word:
        // bump the count, then insert ten bytes before the last four.
        let footer = payload.split_off(payload.len() - 4);
        let directory = u32::from_le_bytes(footer[..4].try_into().unwrap()) as usize + 14;
        payload[directory..directory + 2].copy_from_slice(&3u16.to_le_bytes());
        payload.extend_from_slice(&(CANON_RAW_FILE_NUMBER | 0x4000).to_le_bytes());
        payload.extend_from_slice(&[45, 0, 0, 0, 0, 0, 0, 0]);
        payload.extend_from_slice(&footer);
        let segments = vec![Segment::new(APP0_MARKER, 0, &payload)];
        let mut metadata = MetadataMap::new();

        process_ciff_app0_segments(&segments, &mut metadata);

        assert_eq!(metadata.get_string("CIFF:FileNumber"), Some("45"));
        assert_eq!(
            metadata.get_string("CIFF:Model"),
            Some("Canon PowerShot A5")
        );
        let stray: Vec<_> = metadata
            .keys()
            .filter(|key| !key.starts_with("CIFF:"))
            .collect();
        assert!(stray.is_empty(), "keys outside CIFF: {stray:?}");
    }

    #[test]
    fn ignores_app0_segments_without_the_ciff_signature() {
        let mut metadata = MetadataMap::new();
        let jfif = b"JFIF\0\x01\x02\x00\x00\x01\x00\x01\x00\x00";
        let segments = vec![Segment::new(APP0_MARKER, 0, jfif)];

        process_ciff_app0_segments(&segments, &mut metadata);

        assert!(metadata.get_string("CIFF:Make").is_none());
    }

    /// ExifTool.pm:7730 tests for `HEAPJPGM` only; the standalone-file
    /// `HEAPCCDR` signature in an APP0 payload is not a CIFF record there.
    #[test]
    fn ignores_the_standalone_crw_signature_in_app0() {
        let mut payload = build_minimal_ciff_app0();
        payload[6..14].copy_from_slice(b"HEAPCCDR");
        let segments = vec![Segment::new(APP0_MARKER, 0, &payload)];
        let mut metadata = MetadataMap::new();

        process_ciff_app0_segments(&segments, &mut metadata);

        assert!(metadata.get_string("CIFF:Make").is_none());
    }

    #[test]
    fn ignores_non_app0_segments() {
        let payload = build_minimal_ciff_app0();
        // Same bytes, wrong marker: must not be scanned.
        let segments = vec![Segment::new(0xFFE1, 0, &payload)];
        let mut metadata = MetadataMap::new();

        process_ciff_app0_segments(&segments, &mut metadata);

        assert!(metadata.get_string("CIFF:Make").is_none());
    }
}
