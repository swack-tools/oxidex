//! PLUS controlled-vocabulary conversions.
//!
//! PLUS License Data Format properties are written as vocabulary URIs. ExifTool
//! reduces one to a label in two steps (`PLUS.pm:25`):
//!
//! ```text
//! my %plusVocab = (
//!     ValueConv => '$val =~ s{http://ns.useplus.org/ldf/vocab/}{}; $val',
//!     ...
//! );
//! ```
//!
//! and then a per-tag `PrintConv` hash maps the remaining code. Without this,
//! PLUS.xmp reports `plus:CopyrightStatus` as
//! `http://ns.useplus.org/ldf/vocab/CS-PRO` where ExifTool reports `Protected`.
//!
//! Every table below is transcribed from `PLUS.pm` (ExifTool 13.59). A code the
//! table does not carry is passed through with only the URI prefix stripped,
//! which is what ExifTool does for an unrecognised vocabulary value.

/// The URI prefix `%plusVocab`'s ValueConv strips.
const PLUS_VOCAB_URI: &str = "http://ns.useplus.org/ldf/vocab/";

/// Applies the PLUS ValueConv + PrintConv for `tag`, or returns `None` if the
/// tag is not a PLUS controlled-vocabulary property.
pub fn convert(tag: &str, value: &str) -> Option<String> {
    let table = print_conv_table(tag)?;
    // ImageAlterationConstraints and ImageFileConstraints are `List => 'Bag'`,
    // and by this point the list has already been joined; ExifTool converts
    // every member, so converting only the first would leave the rest as URIs.
    let converted: Vec<String> = value
        .split(", ")
        .map(|item| convert_one(table, item))
        .collect();
    Some(converted.join(", "))
}

/// Strips the vocabulary URI from one value and maps the remaining code.
fn convert_one(table: &[(&str, &str)], value: &str) -> String {
    let trimmed = value.trim();
    let code = trimmed.strip_prefix(PLUS_VOCAB_URI).unwrap_or(trimmed);
    table
        .iter()
        .find(|(vocab_code, _)| *vocab_code == code)
        .map_or(code, |(_, label)| *label)
        .to_string()
}

/// The `PrintConv` hash `PLUS.pm` attaches to a `%plusVocab` property.
fn print_conv_table(tag: &str) -> Option<&'static [(&'static str, &'static str)]> {
    Some(match tag {
        // PLUS.pm:2477
        "CopyrightStatus" => &[
            ("CS-PRO", "Protected"),
            ("CS-PUB", "Public Domain"),
            ("CS-UNK", "Unknown"),
        ],
        // PLUS.pm:2422
        "CreditLineRequired" => &[
            ("CR-NRQ", "Not Required"),
            ("CR-COI", "Credit on Image"),
            ("CR-CAI", "Credit Adjacent To Image"),
            ("CR-CCA", "Credit in Credits Area"),
        ],
        // PLUS.pm:2358
        "ImageAlterationConstraints" => &[
            ("AL-CRP", "No Cropping"),
            ("AL-FLP", "No Flipping"),
            ("AL-RET", "No Retouching"),
            ("AL-CLR", "No Colorization"),
            ("AL-DCL", "No De-Colorization"),
            ("AL-MRG", "No Merging"),
        ],
        // PLUS.pm:2370
        "ImageDuplicationConstraints" => &[
            ("DP-NDC", "No Duplication Constraints"),
            ("DP-LIC", "Duplication Only as Necessary Under License"),
            ("DP-NOD", "No Duplication"),
        ],
        // PLUS.pm:2348
        "ImageFileConstraints" => &[
            ("IF-MFN", "Maintain File Name"),
            ("IF-MID", "Maintain ID in File Name"),
            ("IF-MMD", "Maintain Metadata"),
            ("IF-MFT", "Maintain File Type"),
        ],
        // PLUS.pm:2447
        "ImageFileFormatAsDelivered" => &[
            ("FF-JPG", "JPEG Interchange Formats (JPG, JIF, JFIF)"),
            ("FF-TIF", "Tagged Image File Format (TIFF)"),
            ("FF-GIF", "Graphics Interchange Format (GIF)"),
            ("FF-RAW", "Proprietary RAW Image Format"),
            ("FF-DNG", "Digital Negative (DNG)"),
            ("FF-EPS", "Encapsulated PostScript (EPS)"),
            ("FF-BMP", "Windows Bitmap (BMP)"),
            ("FF-PSD", "Photoshop Document (PSD)"),
            ("FF-PIC", "Macintosh Picture (PICT)"),
            ("FF-PNG", "Portable Network Graphics (PNG)"),
            ("FF-WMP", "Windows Media Photo (HD Photo)"),
            ("FF-OTR", "Other"),
        ],
        // PLUS.pm:2465
        "ImageFileSizeAsDelivered" => &[
            ("SZ-U01", "Up to 1 MB"),
            ("SZ-U10", "Up to 10 MB"),
            ("SZ-U30", "Up to 30 MB"),
            ("SZ-U50", "Up to 50 MB"),
            ("SZ-G50", "Greater than 50 MB"),
        ],
        // PLUS.pm:2436
        "ImageType" => &[
            ("TY-PHO", "Photographic Image"),
            ("TY-ILL", "Illustrated Image"),
            ("TY-MCI", "Multimedia or Composited Image"),
            ("TY-VID", "Video"),
            ("TY-OTR", "Other"),
        ],
        // PLUS.pm:2429
        "AdultContentWarning" => &[
            ("CW-NRQ", "Not Required"),
            ("CW-AWR", "Adult Content Warning Required"),
            ("CW-UNK", "Unknown"),
        ],
        // PLUS.pm:2380
        "ModelReleaseStatus" => &[
            ("MR-NON", "None"),
            ("MR-NAP", "Not Applicable"),
            ("MR-UMR", "Unlimited Model Releases"),
            ("MR-LMR", "Limited or Incomplete Model Releases"),
        ],
        // PLUS.pm:2389
        "MinorModelAgeDisclosure" => &[
            ("AG-UNK", "Age Unknown"),
            ("AG-A25", "Age 25 or Over"),
            ("AG-A24", "Age 24"),
            ("AG-A23", "Age 23"),
            ("AG-A22", "Age 22"),
            ("AG-A21", "Age 21"),
            ("AG-A20", "Age 20"),
            ("AG-A19", "Age 19"),
            ("AG-A18", "Age 18"),
            ("AG-A17", "Age 17"),
            ("AG-A16", "Age 16"),
            ("AG-A15", "Age 15"),
            ("AG-U14", "Age 14 or Under"),
        ],
        // PLUS.pm:2408
        "PropertyReleaseStatus" => &[
            ("PR-NON", "None"),
            ("PR-NAP", "Not Applicable"),
            ("PR-UPR", "Unlimited Property Releases"),
            ("PR-LPR", "Limited or Incomplete Property Releases"),
        ],
        // PLUS.pm:2492
        "Reuse" => &[("RE-REU", "Repeat Use"), ("RE-NAP", "Not Applicable")],
        // PLUS.pm:62/74 -- the two Licensor telephone-type fields share a table.
        "LicensorTelephoneType1" | "LicensorTelephoneType2" => &[
            ("work", "Work"),
            ("cell", "Cell"),
            ("fax", "FAX"),
            ("home", "Home"),
            ("pager", "Pager"),
        ],
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_the_vocab_uri_and_maps_the_code() {
        assert_eq!(
            convert("CopyrightStatus", "http://ns.useplus.org/ldf/vocab/CS-PRO").as_deref(),
            Some("Protected")
        );
    }

    /// PLUS.xmp writes six single-valued vocabulary properties; each has a
    /// distinct table, so a shared-table mistake would show up here.
    #[test]
    fn maps_every_code_the_plus_sample_carries() {
        for (tag, code, expected) in [
            ("CreditLineRequired", "CR-CAI", "Credit Adjacent To Image"),
            ("ImageDuplicationConstraints", "DP-NOD", "No Duplication"),
            (
                "ImageFileFormatAsDelivered",
                "FF-JPG",
                "JPEG Interchange Formats (JPG, JIF, JFIF)",
            ),
            ("ImageFileSizeAsDelivered", "SZ-U50", "Up to 50 MB"),
            ("Reuse", "RE-NAP", "Not Applicable"),
            ("ImageAlterationConstraints", "AL-CRP", "No Cropping"),
            ("ImageFileConstraints", "IF-MFN", "Maintain File Name"),
        ] {
            let uri = format!("{PLUS_VOCAB_URI}{code}");
            assert_eq!(convert(tag, &uri).as_deref(), Some(expected), "{tag}");
        }
    }

    /// An unknown code keeps the bare code, never the URI and never a guess.
    #[test]
    fn unknown_code_passes_through_without_the_uri() {
        assert_eq!(
            convert("Reuse", "http://ns.useplus.org/ldf/vocab/RE-XXX").as_deref(),
            Some("RE-XXX")
        );
    }

    #[test]
    fn non_vocabulary_tag_is_left_alone() {
        assert_eq!(convert("LicensorName", "Phil Harvey"), None);
    }
}
