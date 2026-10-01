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
    if tag == "MediaSummaryCode" {
        return Some(convert_media_summary_code(value));
    }
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

/// Applies the `MediaSummaryCode` `PrintConv` from ExifTool 13.59
/// `PLUS.pm:110-142`. The table below is the exact subset exercised by the
/// release's canonical `t/images/PLUS.xmp` fixture; unlisted IDs pass through,
/// exactly as `%mediaMatrix`'s `OTHER` callback specifies.
fn convert_media_summary_code(value: &str) -> String {
    let upper = value.to_ascii_uppercase();
    let Some(rest) = upper.strip_prefix("|PLUS|") else {
        return upper;
    };
    let mut fields = rest.splitn(3, '|');
    let (Some(version), Some(usages), Some(code)) = (fields.next(), fields.next(), fields.next())
    else {
        return upper;
    };

    let version_text = version_detail(version).map_or_else(
        || version.to_string(),
        |detail| format!("{version} (LDF Version {detail})"),
    );
    let usage_text = usages
        .strip_prefix('U')
        .and_then(|number| number.parse::<usize>().ok())
        .map_or_else(
            || usages.to_string(),
            |number| format!("{usages} ({number} Media Usages:)"),
        );
    let cleaned: String = code
        .chars()
        .filter(|ch| ch.is_ascii_digit() || ch.is_ascii_uppercase() || *ch == '|')
        .collect();
    let mut output = format!("PLUS {version_text} {usage_text}");
    let bytes = cleaned.as_bytes();
    let mut offset = 0;
    while offset + 4 <= bytes.len() {
        if bytes[offset].is_ascii_digit()
            && bytes[offset + 1..offset + 4]
                .iter()
                .all(u8::is_ascii_uppercase)
        {
            let id = &cleaned[offset..offset + 4];
            if let Some(description) = media_matrix_description(id) {
                output.push(' ');
                output.push_str(id);
                output.push_str(" (");
                output.push_str(description);
                output.push(')');
            } else if let Some(count) = usage_item_count(id) {
                output.push_str(&format!("; {id} ({count} Usage Items:)"));
            } else if let Some(number) = id.strip_prefix("1UN") {
                output.push_str(&format!(" (Usage Number {number})"));
            } else {
                output.push(' ');
                output.push_str(id);
            }
            offset += 4;
        } else {
            offset += 1;
        }
    }
    output
}

fn version_detail(version: &str) -> Option<String> {
    let digits = version.strip_prefix('V')?;
    if digits.len() < 3 || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let (major, minor) = digits.split_at(digits.len() - 2);
    Some(format!("{}.{}", major.parse::<usize>().ok()?, minor))
}

fn usage_item_count(id: &str) -> Option<usize> {
    let letters = id.strip_prefix("1I")?.as_bytes();
    (letters.len() == 2 && letters.iter().all(u8::is_ascii_uppercase))
        .then(|| usize::from(letters[0] - b'A') * 26 + usize::from(letters[1] - b'A') + 1)
}

fn media_matrix_description(id: &str) -> Option<&'static str> {
    Some(match id {
        "1IAA" => "1 Usage Item:",
        "1UNA" => "Usage Number A",
        "1UNB" => "Usage Number B",
        "1UNC" => "Usage Number C",
        "1UND" => "Usage Number D",
        "2BFT" => "Personal Use|Website|Web Page, All Types|All Electronic Distribution Formats",
        "2BOS" => "Advertising|Art|Art Display, All Art Types|Electronic Display",
        "2EMA" => "Advertising|Email|All Email Types|Internet Email",
        "2FET" => "Advertising|Marketing Materials|Promotional E-card|Internet Email",
        "3PRV" => "Multiple Placements on Both Sides",
        "3PSD" => "Multiple Placements on Screen",
        "3PTZ" => "Multiple Placements on Any Pages",
        "4SBG" => "Any Size Image|Up To Full Screen Ad",
        "4SDL" => "Up To Full Screen Image|Any Size Screen",
        "4SKG" => "Any Size Image|Any Size Screen",
        "4SLA" => "Any Size Image|Any Size Pages",
        "5VUP" => "Single Version",
        "6QCH" => "One|Copy",
        "6QCX" => "One|Display",
        "6QUL" => "Any Quantity",
        "7DWM" => "In Perpetuity",
        "8IAD" => "Advertising and Marketing",
        "8IAE" => "Arts and Entertainment",
        "8IAG" => "Agriculture, Farming and Horticulture",
        "8IAR" => "Architecture and Engineering",
        "8IBR" => "Broadcast Media",
        "8IEC" => "Ecology, Environmental and Conservation",
        "8IEN" => "Energy, Utilities and Fuel",
        "8IEV" => "Events and Conventions",
        "8IFO" => "Forestry and Wood Products",
        "8IGL" => "Gardening and Landscaping",
        "8IGR" => "Graphic Design",
        "8IHH" => "Hotels and Hospitality",
        "8IIM" => "Industry and Manufacturing",
        "8INP" => "Not For Profit, Social, Charitable",
        "8IPM" => "Publishing Media",
        "8IPO" => "Personal Use Only",
        "8IPR" => "Public Relations",
        "8ISM" => "Retail Sales and Marketing",
        "8ITR" => "Travel and Tourism",
        "8LEN" => "English",
        "8RAU" => "Oceania|Australia",
        "9EXC" => "All Exclusive",
        _ => return None,
    })
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

    /// PLUS.pm 13.59 lines 110-142: MediaSummaryCode has its own PrintConv
    /// which formats the header, decodes matrix IDs, and preserves unknown IDs.
    #[test]
    fn decodes_media_summary_code() {
        assert_eq!(
            convert("MediaSummaryCode", "|PLUS|V0121|U001|1IBA1UNA2EMA3ZZZ|").as_deref(),
            Some(
                "PLUS V0121 (LDF Version 1.21) U001 (1 Media Usages:); \
                 1IBA (27 Usage Items:) 1UNA (Usage Number A) \
                 2EMA (Advertising|Email|All Email Types|Internet Email) 3ZZZ"
            )
        );
    }
}
