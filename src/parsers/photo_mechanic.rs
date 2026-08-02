//! Photo Mechanic trailer parser
//! (`Image::ExifTool::PhotoMechanic::ProcessPhotoMechanic`, PhotoMechanic.pm:150-192)
//!
//! Photo Mechanic appends a trailer after the end of the image -- ExifTool
//! runs `ProcessPhotoMechanic` from the format-agnostic `ProcessTrailers`
//! (ExifTool.pm:7019), so the same block shape shows up after a PSD's image
//! data or after a JPEG's EOI. That makes this module deliberately
//! format-agnostic too: it takes the whole file as bytes and finds the
//! trailer itself.
//!
//! The trailer is `[IPTC records]["size" as 4-byte big-endian int32u]["cbipcbbl"]`.
//! oxidex has no trailer chain to walk -- see [`crate::parsers::trailer`] -- so
//! this scans backwards for the `cbipcbbl` marker with [`trailer::find_last`]
//! and validates the second point PhotoMechanic.pm's own check relies on: the
//! declared size must land a run of IPTC records that fits inside the file.
//! In `combined-samples/ExifTool.jpg` the PhotoMechanic trailer is not last --
//! MIE, Samsung and Vivo trailers all follow it -- so a plain "read the last
//! twelve bytes" check (correct for every PSD in the sample corpus, where the
//! trailer is always the file's own end) is not enough here.
//!
//! Record 2 is the SoftEdit table (PhotoMechanic.pm:61-97); only the datasets
//! whose conversion is unambiguous from the bytes are reported.

use crate::core::{MetadataMap, TagValue};
use crate::parsers::jpeg::iptc_parser::parse_all_iptc_records;
use crate::parsers::trailer;

/// Trailing signature of a Photo Mechanic trailer.
const SIGNATURE: &[u8] = b"cbipcbbl";

/// Bytes of footer following the IPTC records: a 4-byte size plus the
/// 8-byte signature.
const FOOTER_LEN: usize = 12;

/// Extracts the tags a file's Photo Mechanic trailer carries, if one is
/// present.
///
/// # Arguments
///
/// * `file` - The complete file contents
///
/// # Returns
///
/// A metadata map keyed `PhotoMechanic:<Name>`; empty when the file carries
/// no Photo Mechanic trailer.
pub fn parse_photo_mechanic_trailer(file: &[u8]) -> MetadataMap {
    let mut metadata = MetadataMap::new();
    let Some(data) = find_trailer(file) else {
        return metadata;
    };
    let Ok(records) = parse_all_iptc_records(data) else {
        return metadata;
    };

    for record in records {
        // Only record 2 carries soft-edit information.
        if record.record_number != 2 {
            continue;
        }
        let Some((name, value)) = soft_edit_tag(record.dataset_number, &record.data) else {
            continue;
        };
        metadata.insert(format!("PhotoMechanic:{name}"), value);
    }
    metadata
}

/// Finds the IPTC record block of the outermost valid Photo Mechanic
/// trailer.
///
/// Mirrors PhotoMechanic.pm:167-172: read the size before the signature,
/// reject one with the high bit set, and require that many bytes to fit
/// before it.
fn find_trailer(file: &[u8]) -> Option<&[u8]> {
    trailer::find_last(file, FOOTER_LEN, SIGNATURE, SIGNATURE.len(), |file, end| {
        let signature_start = end.checked_sub(SIGNATURE.len())?;
        let size_start = signature_start.checked_sub(4)?;
        let size_bytes = file.get(size_start..signature_start)?;
        let size = u32::from_be_bytes(size_bytes.try_into().ok()?);
        // PhotoMechanic.pm:167 rejects a size with the high bit set.
        if size & 0x8000_0000 != 0 || size == 0 {
            return None;
        }
        let size = size as usize;
        let data_start = size_start.checked_sub(size)?;
        file.get(data_start..size_start)
    })
}

/// Maps a Photo Mechanic SoftEdit dataset number to its ExifTool tag name and
/// converted value.
///
/// Table: `Image::ExifTool::PhotoMechanic::SoftEdit` (PhotoMechanic.pm:61-97),
/// whose `FORMAT => 'int32s'` makes every value here a 4-byte signed integer.
///
/// The raw and preview crop coordinates share `%rawCropConv`
/// (PhotoMechanic.pm:52-58), which is `ValueConv => '$val / 655.36'` printed by
/// `PrintConv => 'sprintf("%.3f%%",$val)'`. The remaining datasets are named
/// without a conversion in the table and print as plain integers.
fn soft_edit_tag(dataset: u8, data: &[u8]) -> Option<(&'static str, TagValue)> {
    if data.len() < 4 {
        return None;
    }
    let value = i32::from_be_bytes([data[0], data[1], data[2], data[3]]);

    // `%rawCropConv`, PhotoMechanic.pm:52-58.
    let crop_percent = |raw: i32| TagValue::String(format!("{:.3}%", f64::from(raw) / 655.36));

    let tag = match dataset {
        209 => ("RawCropLeft", crop_percent(value)),
        210 => ("RawCropTop", crop_percent(value)),
        211 => ("RawCropRight", crop_percent(value)),
        212 => ("RawCropBottom", crop_percent(value)),
        213 => ("ConstrainedCropWidth", TagValue::Integer(value as i64)),
        214 => ("ConstrainedCropHeight", TagValue::Integer(value as i64)),
        215 => ("FrameNum", TagValue::Integer(value as i64)),
        216 => {
            // PrintConv { 0 => '0', 1 => '90', 2 => '180', 3 => '270' }
            let label = match value {
                0 => "0".to_string(),
                1 => "90".to_string(),
                2 => "180".to_string(),
                3 => "270".to_string(),
                other => format!("Unknown ({other})"),
            };
            ("Rotation", TagValue::String(label))
        }
        217 => ("CropLeft", TagValue::Integer(value as i64)),
        218 => ("CropTop", TagValue::Integer(value as i64)),
        219 => ("CropRight", TagValue::Integer(value as i64)),
        220 => ("CropBottom", TagValue::Integer(value as i64)),
        221 => {
            // PrintConv { 0 => 'No', 1 => 'Yes' }
            let label = match value {
                0 => "No".to_string(),
                1 => "Yes".to_string(),
                other => format!("Unknown ({other})"),
            };
            ("Tagged", TagValue::String(label))
        }
        222 => {
            // %colorClasses, PhotoMechanic.pm:23-33 -- the printed value
            // already carries the numeric prefix.
            let label = match value {
                0 => "0 (None)".to_string(),
                1 => "1 (Winner)".to_string(),
                2 => "2 (Winner alt)".to_string(),
                3 => "3 (Superior)".to_string(),
                4 => "4 (Superior alt)".to_string(),
                5 => "5 (Typical)".to_string(),
                6 => "6 (Typical alt)".to_string(),
                7 => "7 (Extras)".to_string(),
                8 => "8 (Trash)".to_string(),
                other => format!("Unknown ({other})"),
            };
            ("ColorClass", TagValue::String(label))
        }
        223 => ("Rating", TagValue::Integer(value as i64)),
        236 => ("PreviewCropLeft", crop_percent(value)),
        237 => ("PreviewCropTop", crop_percent(value)),
        238 => ("PreviewCropRight", crop_percent(value)),
        239 => ("PreviewCropBottom", crop_percent(value)),
        _ => return None,
    };

    Some(tag)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soft_edit_datasets_apply_their_print_conversions() {
        // PhotoMechanic.pm:61-97 plus %colorClasses (lines 23-33). Rotation and
        // ColorClass are enum-mapped; the crop coordinates are plain integers.
        assert_eq!(
            soft_edit_tag(216, &2i32.to_be_bytes()),
            Some(("Rotation", TagValue::String("180".to_string())))
        );
        assert_eq!(
            soft_edit_tag(221, &1i32.to_be_bytes()),
            Some(("Tagged", TagValue::String("Yes".to_string())))
        );
        assert_eq!(
            soft_edit_tag(222, &6i32.to_be_bytes()),
            Some((
                "ColorClass",
                TagValue::String("6 (Typical alt)".to_string())
            ))
        );
        assert_eq!(
            soft_edit_tag(217, &438i32.to_be_bytes()),
            Some(("CropLeft", TagValue::Integer(438)))
        );
        assert_eq!(
            soft_edit_tag(220, &1072i32.to_be_bytes()),
            Some(("CropBottom", TagValue::Integer(1072)))
        );
        // A dataset the table does not name stays unreported rather than
        // being guessed at.
        assert_eq!(soft_edit_tag(224, &1000i32.to_be_bytes()), None);
    }

    #[test]
    fn raw_and_preview_crops_print_as_percentages() {
        // `%rawCropConv` (PhotoMechanic.pm:52-58) divides by 655.36 and prints
        // with `sprintf("%.3f%%")`, so 65536 is exactly 100.000%. Verified
        // against `exiftool -a -G1 -s` 13.55 on a PSD carrying these datasets.
        assert_eq!(
            soft_edit_tag(209, &65536i32.to_be_bytes()),
            Some(("RawCropLeft", TagValue::String("100.000%".to_string())))
        );
        assert_eq!(
            soft_edit_tag(210, &32768i32.to_be_bytes()),
            Some(("RawCropTop", TagValue::String("50.000%".to_string())))
        );
        assert_eq!(
            soft_edit_tag(239, &65536i32.to_be_bytes()),
            Some((
                "PreviewCropBottom",
                TagValue::String("100.000%".to_string())
            ))
        );
        assert_eq!(
            soft_edit_tag(237, &0i32.to_be_bytes()),
            Some(("PreviewCropTop", TagValue::String("0.000%".to_string())))
        );
    }

    #[test]
    fn constrained_crop_and_frame_number_print_as_plain_integers() {
        // PhotoMechanic.pm:72-74 names 213/214/215 with no conversion.
        assert_eq!(
            soft_edit_tag(213, &1600i32.to_be_bytes()),
            Some(("ConstrainedCropWidth", TagValue::Integer(1600)))
        );
        assert_eq!(
            soft_edit_tag(214, &1200i32.to_be_bytes()),
            Some(("ConstrainedCropHeight", TagValue::Integer(1200)))
        );
        assert_eq!(
            soft_edit_tag(215, &7i32.to_be_bytes()),
            Some(("FrameNum", TagValue::Integer(7)))
        );
    }

    fn iptc_trailer_bytes(entries: &[(u8, i32)]) -> Vec<u8> {
        let mut iptc = Vec::new();
        for (dataset, value) in entries {
            iptc.extend_from_slice(&[0x1c, 0x02, *dataset]);
            iptc.extend_from_slice(&4u16.to_be_bytes());
            iptc.extend_from_slice(&value.to_be_bytes());
        }
        let mut trailer = iptc.clone();
        trailer.extend_from_slice(&(iptc.len() as u32).to_be_bytes());
        trailer.extend_from_slice(SIGNATURE);
        trailer
    }

    #[test]
    fn trailer_is_read_from_the_end_of_the_file() {
        // PhotoMechanic.pm:150-192: [IPTC records][4-byte size]["cbipcbbl"].
        let mut file = b"padding that is not a trailer".to_vec();
        file.extend_from_slice(&iptc_trailer_bytes(&[(221, 1), (222, 6), (216, 2)]));

        let metadata = parse_photo_mechanic_trailer(&file);

        assert_eq!(
            metadata.get("PhotoMechanic:Tagged"),
            Some(&TagValue::String("Yes".to_string()))
        );
        assert_eq!(
            metadata.get("PhotoMechanic:ColorClass"),
            Some(&TagValue::String("6 (Typical alt)".to_string()))
        );
        assert_eq!(
            metadata.get("PhotoMechanic:Rotation"),
            Some(&TagValue::String("180".to_string()))
        );
    }

    #[test]
    fn a_file_without_the_signature_gets_no_photo_mechanic_tags() {
        // The footer check is the only thing standing between arbitrary file
        // tails and a fabricated PhotoMechanic group.
        let file = vec![0u8; 64];

        let metadata = parse_photo_mechanic_trailer(&file);

        assert!(
            metadata.is_empty(),
            "no trailer signature means no trailer tags"
        );
    }

    #[test]
    fn trailer_is_found_when_other_trailers_follow_it() {
        // combined-samples/ExifTool.jpg: MIE, Samsung and Vivo trailers all
        // follow the PhotoMechanic trailer, so this must not just read the
        // last twelve bytes of the file.
        let mut file = b"not a trailer".to_vec();
        file.extend_from_slice(&iptc_trailer_bytes(&[(216, 1)]));
        file.extend_from_slice(b"a later trailer chained on after it");

        let metadata = parse_photo_mechanic_trailer(&file);

        assert_eq!(
            metadata.get("PhotoMechanic:Rotation"),
            Some(&TagValue::String("90".to_string()))
        );
    }
}
