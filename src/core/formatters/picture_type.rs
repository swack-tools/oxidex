//! Attached-picture type -- the single implementation of ExifTool's shared
//! picture-type PrintConv.
//!
//! ID3 (`APIC` / `PIC`), FLAC (`METADATA_BLOCK_PICTURE`) and ASF all carry the
//! same one-byte picture-type code, and ExifTool gives all three the same
//! 21-entry table: `Image::ExifTool::ID3::v2_2{'PIC-2'}`, `::ID3::v2_3{'APIC-2'}`,
//! `::ID3::v2_4{'APIC-2'}`, `::FLAC::Picture{0}` and `::ASF::Picture{0}` were
//! each dumped from the installed ExifTool 13.55 and are byte-for-byte
//! identical.
//!
//! None of the five declares an `OTHER` handler, so a code outside 0-20 falls
//! through to ExifTool's default for a missing PrintConv key and prints as
//! `Unknown (N)`.

/// ExifTool's picture-type table, in code order.
const PICTURE_TYPES: [&str; 21] = [
    "Other",
    "32x32 PNG Icon",
    "Other Icon",
    "Front Cover",
    "Back Cover",
    "Leaflet",
    "Media",
    "Lead Artist",
    "Artist",
    "Conductor",
    "Band",
    "Composer",
    "Lyricist",
    "Recording Studio or Location",
    "Recording Session",
    "Performance",
    "Capture from Movie or Video",
    "Bright(ly) Colored Fish",
    "Illustration",
    "Band Logo",
    "Publisher Logo",
];

/// Names an attached-picture type code the way ExifTool does.
///
/// A code outside 0-20 prints as `Unknown (N)`. It must not borrow a real
/// label: an ID3 frame carrying code 21 is not a picture of something "Other",
/// it is a code this version of the spec does not define, and reporting it as
/// `Other` makes an unrecognised value indistinguishable from a recognised one.
///
/// # Examples
///
/// ```
/// use oxidex::core::formatters::picture_type::picture_type_name;
///
/// assert_eq!(picture_type_name(3), "Front Cover");
/// assert_eq!(picture_type_name(13), "Recording Studio or Location");
/// assert_eq!(picture_type_name(21), "Unknown (21)");
/// ```
pub fn picture_type_name(code: u32) -> String {
    match PICTURE_TYPES.get(code as usize) {
        Some(name) => (*name).to_string(),
        None => format!("Unknown ({})", code),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The full table, as dumped from ExifTool 13.55. All five of its
    /// picture-type PrintConvs (ID3 v2.2/v2.3/v2.4, FLAC, ASF) are identical
    /// to this.
    #[test]
    fn test_matches_exiftool_table() {
        let expected = [
            (0u32, "Other"),
            (1, "32x32 PNG Icon"),
            (2, "Other Icon"),
            (3, "Front Cover"),
            (4, "Back Cover"),
            (5, "Leaflet"),
            (6, "Media"),
            (7, "Lead Artist"),
            (8, "Artist"),
            (9, "Conductor"),
            (10, "Band"),
            (11, "Composer"),
            (12, "Lyricist"),
            (13, "Recording Studio or Location"),
            (14, "Recording Session"),
            (15, "Performance"),
            (16, "Capture from Movie or Video"),
            (17, "Bright(ly) Colored Fish"),
            (18, "Illustration"),
            (19, "Band Logo"),
            (20, "Publisher Logo"),
        ];
        for (code, name) in expected {
            assert_eq!(picture_type_name(code), name, "code {}", code);
        }
    }

    /// The ten labels the ASF copy disagreed with, and the seven the mp3 copy
    /// disagreed with, are all in this set.
    #[test]
    fn test_labels_that_the_duplicates_got_wrong() {
        // ASF said "32x32 File Icon" / "Other File Icon" / "Leaflet Page".
        assert_eq!(picture_type_name(1), "32x32 PNG Icon");
        assert_eq!(picture_type_name(2), "Other Icon");
        assert_eq!(picture_type_name(5), "Leaflet");
        // Both ASF and mp3 said "Recording Location" / "During Recording" /
        // "During Performance" / "Video Capture" / "A Bright Coloured Fish" /
        // "Band Logotype" / "Publisher Logotype".
        assert_eq!(picture_type_name(13), "Recording Studio or Location");
        assert_eq!(picture_type_name(14), "Recording Session");
        assert_eq!(picture_type_name(15), "Performance");
        assert_eq!(picture_type_name(16), "Capture from Movie or Video");
        assert_eq!(picture_type_name(17), "Bright(ly) Colored Fish");
        assert_eq!(picture_type_name(19), "Band Logo");
        assert_eq!(picture_type_name(20), "Publisher Logo");
    }

    /// An unrecognised code reports itself. The mp3 copy returned `"Other"`,
    /// which is a real label -- so `PictureType = Other` could mean either
    /// "code 0" or "any code above 20", with nothing to tell them apart. The
    /// ASF copy returned a bare `"Unknown"`, losing the code.
    #[test]
    fn test_unknown_codes_report_themselves() {
        assert_eq!(picture_type_name(21), "Unknown (21)");
        assert_eq!(picture_type_name(99), "Unknown (99)");
        assert_eq!(picture_type_name(255), "Unknown (255)");
        // ...and are not confusable with code 0.
        assert_ne!(picture_type_name(21), picture_type_name(0));
    }
}
