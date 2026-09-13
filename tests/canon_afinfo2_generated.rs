//! Public-reader coverage for the generated `Canon::AFInfo2` serial table.
//!
//! These TIFF carriers are assembled as real TIFF -> ExifIFD -> Canon MakerNote
//! structures so `read_metadata()` takes the normal public path.  They do not
//! import the generated table or its descriptors.  Expected output was recorded
//! from pinned ExifTool 13.59 / Perl 5.38.2 in
//! `shared-pilot/serial-afinfo-integration-20260913/parent-bridge-contract/`:
//! the `semantic-r2/dynamic-corrected-*` cases prove signed arrays, multiword
//! DecodeBits and both byte orders; the parent bridge controls prove the
//! `Validate` rejection, `AFInfo3` state and continuation to Main 0x0081.
//!
//! The minimal synthetic TIFFs used during that collection cause ExifTool's
//! known MakerNote-offset warning.  This test intentionally does not assert
//! warning text: OxiDex's public reader exposes metadata, while the contract
//! here is the generated child route and the parent entries surrounding it.

use oxidex::core::MetadataMap;
use oxidex::core::operations::read_metadata;
use std::fs;
use std::io::Write;
use std::path::Path;

#[derive(Clone, Copy, Debug)]
enum Endian {
    Ii,
    Mm,
}

impl Endian {
    fn marker(self) -> [u8; 2] {
        match self {
            Self::Ii => *b"II",
            Self::Mm => *b"MM",
        }
    }

    fn u16(self, value: u16) -> [u8; 2] {
        match self {
            Self::Ii => value.to_le_bytes(),
            Self::Mm => value.to_be_bytes(),
        }
    }

    fn u32(self, value: u32) -> [u8; 4] {
        match self {
            Self::Ii => value.to_le_bytes(),
            Self::Mm => value.to_be_bytes(),
        }
    }
}

#[derive(Clone, Copy)]
enum Parent {
    AfInfo2,
    AfInfo3,
}

impl Parent {
    fn raw_id(self) -> u16 {
        match self {
            Self::AfInfo2 => 0x0026,
            Self::AfInfo3 => 0x003c,
        }
    }
}

#[derive(Clone)]
struct Child {
    parent: Parent,
    /// The TIFF entry count that the parent `U16SizeCheck` compares against
    /// the child record's first inherited-order u16.
    declared_len: u32,
    bytes: Vec<u8>,
}

fn put(bytes: &mut [u8], at: usize, value: &[u8]) {
    bytes[at..at + value.len()].copy_from_slice(value);
}

fn put_u16(bytes: &mut [u8], at: usize, order: Endian, value: u16) {
    put(bytes, at, &order.u16(value));
}

fn put_u32(bytes: &mut [u8], at: usize, order: Endian, value: u32) {
    put(bytes, at, &order.u32(value));
}

fn align2(value: usize) -> usize {
    (value + 1) & !1
}

/// The actual `%Canon::AFInfo2` wire layout: eight fixed u16 slots, then four
/// signed `NumAFPoints` arrays, a ceil(n / 16) bitset, and the native
/// non-EOS unknown tail plus PrimaryAFPoint.  `AFInfo3` uses the same child
/// payload but its parent sets state that prevents the final primary field.
fn afinfo2_child(order: Endian, point_count: u16, primary: u16) -> Vec<u8> {
    let n = usize::from(point_count);
    let bit_words = (n + 15) / 16;
    let word_count = 8 + (4 * n) + bit_words + (bit_words + 1) + 1;
    let size = u16::try_from(word_count * 2).expect("test child stays within u16 size");
    let mut words = Vec::with_capacity(word_count);
    words.extend([size, 2, point_count, 1, 100, 80, 50, 40]);

    // A signed high-bit value verifies that child data is decoded as int16s,
    // followed by an ordinary positive value.  The other three arrays are
    // deliberately zeroed, matching the pinned native fixture shape.
    for array in 0..4 {
        for index in 0..n {
            let value = match (array, index) {
                (0, 0) => 0x8001,
                (0, 1) => 2,
                _ => 0,
            };
            words.push(value);
        }
    }
    if bit_words > 0 {
        words.push(0x8001); // point 0 and point 15
        for _ in 1..bit_words {
            words.push(1); // point 16 for the n=17 control
        }
    }
    // On non-EOS bodies key 13 is an Unknown field, but ProcessSerialData
    // still consumes its ceil(n/16)+1 words before key 14.
    for _ in 0..bit_words + 1 {
        words.push(0);
    }
    words.push(primary);

    let mut bytes = Vec::with_capacity(words.len() * 2);
    for word in words {
        bytes.extend_from_slice(&order.u16(word));
    }
    bytes
}

fn invalid_size_child(order: Endian) -> Child {
    let mut bytes = afinfo2_child(order, 0, 99);
    // `Validate($dirData, $subdirStart, $size)` must see the declared 20-byte
    // child length.  This 19 makes the validation false before table lookup.
    put_u16(&mut bytes, 0, order, 19);
    Child {
        parent: Parent::AfInfo2,
        declared_len: u32::try_from(bytes.len()).expect("fixture length"),
        bytes,
    }
}

fn truncated_child(order: Endian) -> Child {
    let mut bytes = afinfo2_child(order, 17, 99);
    // The parent advertises the complete 164-byte record, but the carrier
    // ends after twenty words.  Pinned ExifTool warns while continuing Main.
    bytes.truncate(40);
    Child {
        parent: Parent::AfInfo2,
        declared_len: 164,
        bytes,
    }
}

fn child(parent: Parent, order: Endian, point_count: u16) -> Child {
    let bytes = afinfo2_child(order, point_count, 99);
    Child {
        parent,
        declared_len: u32::try_from(bytes.len()).expect("fixture length"),
        bytes,
    }
}

/// Build offsets after the IFDs and their entry arrays have been laid out.
/// Every pointer is absolute from the TIFF start and every child begins after
/// the MakerNote directory, so fixture data never overlaps an IFD entry.
fn canon_tiff(order: Endian, model: &str, children: &[Child], later_sibling: bool) -> Vec<u8> {
    const IFD0: usize = 8;
    const MAKE_AT: usize = 64;
    const MODEL_AT: usize = 80;
    const EXIF_IFD: usize = 112;
    const MAKERNOTE: usize = 160;
    const TIFF_TYPE_ASCII: u16 = 2;
    const TIFF_TYPE_LONG: u16 = 4;
    const TIFF_TYPE_UNDEFINED: u16 = 7;

    let maker_entries = children.len() + usize::from(later_sibling);
    let maker_dir_len = 2 + (maker_entries * 12) + 4;
    let mut child_at = align2(MAKERNOTE + maker_dir_len);
    let mut bytes = vec![0_u8; child_at];

    put(&mut bytes, 0, &order.marker());
    put_u16(&mut bytes, 2, order, 42);
    put_u32(&mut bytes, 4, order, IFD0 as u32);

    put_u16(&mut bytes, IFD0, order, 3);
    let mut entry = IFD0 + 2;
    for (tag, format, count, value) in [
        (0x010f, TIFF_TYPE_ASCII, 6_u32, MAKE_AT as u32),
        (
            0x0110,
            TIFF_TYPE_ASCII,
            u32::try_from(model.len() + 1).expect("model length"),
            MODEL_AT as u32,
        ),
        (0x8769, TIFF_TYPE_LONG, 1_u32, EXIF_IFD as u32),
    ] {
        put_u16(&mut bytes, entry, order, tag);
        put_u16(&mut bytes, entry + 2, order, format);
        put_u32(&mut bytes, entry + 4, order, count);
        put_u32(&mut bytes, entry + 8, order, value);
        entry += 12;
    }
    put_u32(&mut bytes, entry, order, 0);
    put(&mut bytes, MAKE_AT, b"Canon\0");
    put(&mut bytes, MODEL_AT, model.as_bytes());
    bytes[MODEL_AT + model.len()] = 0;

    put_u16(&mut bytes, EXIF_IFD, order, 1);
    put_u16(&mut bytes, EXIF_IFD + 2, order, 0x927c);
    put_u16(&mut bytes, EXIF_IFD + 4, order, TIFF_TYPE_UNDEFINED);
    put_u32(
        &mut bytes,
        EXIF_IFD + 6,
        order,
        u32::try_from(maker_dir_len).expect("maker directory length")
            + children
                .iter()
                .map(|child| u32::try_from(child.bytes.len()).expect("child length"))
                .sum::<u32>(),
    );
    put_u32(&mut bytes, EXIF_IFD + 10, order, MAKERNOTE as u32);
    put_u32(&mut bytes, EXIF_IFD + 14, order, 0);

    put_u16(
        &mut bytes,
        MAKERNOTE,
        order,
        u16::try_from(maker_entries).expect("maker entry count"),
    );
    entry = MAKERNOTE + 2;
    for child in children {
        put_u16(&mut bytes, entry, order, child.parent.raw_id());
        put_u16(&mut bytes, entry + 2, order, TIFF_TYPE_UNDEFINED);
        put_u32(&mut bytes, entry + 4, order, child.declared_len);
        put_u32(&mut bytes, entry + 8, order, child_at as u32);
        let end = child_at + child.bytes.len();
        if bytes.len() < end {
            bytes.resize(end, 0);
        }
        put(&mut bytes, child_at, &child.bytes);
        child_at = align2(end);
        entry += 12;
    }
    if later_sibling {
        put_u16(&mut bytes, entry, order, 0x0081);
        put_u16(&mut bytes, entry + 2, order, TIFF_TYPE_LONG);
        put_u32(&mut bytes, entry + 4, order, 1);
        put_u32(&mut bytes, entry + 8, order, 0x1234_5678);
        entry += 12;
    }
    put_u32(&mut bytes, entry, order, 0);
    bytes
}

/// When `OXIDEX_AFINFO2_TEST_EXPORT_DIR` is set, retain the exact input that
/// the public reader receives for an independent native replay.  `create_new`
/// keeps a second test run from silently replacing a reviewed fixture.
fn export_fixture(label: &str, bytes: &[u8]) {
    let Some(dir) = std::env::var_os("OXIDEX_AFINFO2_TEST_EXPORT_DIR") else {
        return;
    };
    let dir = Path::new(&dir);
    fs::create_dir_all(dir).expect("create AFInfo2 fixture export directory");
    let path = dir.join(format!("{label}.tif"));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .unwrap_or_else(|error| {
            panic!(
                "export {label} without overwrite at {}: {error}",
                path.display()
            )
        });
    file.write_all(bytes).unwrap_or_else(|error| {
        panic!("write exported AFInfo2 fixture {}: {error}", path.display())
    });
}

fn read_carrier(label: &str, bytes: &[u8]) -> MetadataMap {
    export_fixture(label, bytes);
    let file = tempfile::Builder::new()
        .suffix(".tif")
        .tempfile()
        .expect("create TIFF carrier");
    fs::write(file.path(), bytes).expect("write TIFF carrier");
    read_metadata(file.path()).expect("public metadata reader accepts Canon TIFF carrier")
}

fn shown(metadata: &MetadataMap, key: &str) -> Option<String> {
    let value = metadata.get(key)?;
    value
        .as_string()
        .map(str::to_owned)
        .or_else(|| value.as_integer().map(|value| value.to_string()))
}

fn assert_present(metadata: &MetadataMap, key: &str, expected: &str) {
    assert_eq!(shown(metadata, key).as_deref(), Some(expected), "{key}");
}

#[test]
fn generated_afinfo2_route_handles_signed_multiword_records_in_both_orders() {
    for order in [Endian::Ii, Endian::Mm] {
        let label = match order {
            Endian::Ii => "valid-afinfo2-ii",
            Endian::Mm => "valid-afinfo2-mm",
        };
        let metadata = read_carrier(
            label,
            &canon_tiff(
                order,
                "Canon Test",
                &[child(Parent::AfInfo2, order, 17)],
                true,
            ),
        );

        // Pinned ExifTool's dynamic-corrected controls report these exact
        // values.  In particular, 0x8001 is signed -32767 and the two bit
        // words render points 0, 15 and 16 rather than a numeric mask.
        assert_present(&metadata, "Canon:AFAreaMode", "Single-point AF");
        assert_present(
            &metadata,
            "Canon:AFAreaWidths",
            "-32767 2 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        );
        assert_present(&metadata, "Canon:AFPointsInFocus", "0,15,16");
        assert_present(&metadata, "Canon:PrimaryAFPoint", "99");
        assert_present(&metadata, "Canon:RawDataOffset", "305419896");
    }
}

#[test]
fn afinfo3_state_and_eos_condition_suppress_primary_but_leave_child_output() {
    for (parent, model, assertion_label, export_label) in [
        (
            Parent::AfInfo3,
            "Canon Test",
            "AFInfo3 parent state",
            "afinfo3-state-ii",
        ),
        (
            Parent::AfInfo2,
            "Canon EOS Test",
            "EOS model condition",
            "eos-afinfo2-ii",
        ),
    ] {
        let metadata = read_carrier(
            export_label,
            &canon_tiff(Endian::Ii, model, &[child(parent, Endian::Ii, 17)], true),
        );
        assert_present(&metadata, "Canon:AFAreaMode", "Single-point AF");
        assert_present(&metadata, "Canon:AFPointsInFocus", "0,15,16");
        assert!(
            shown(&metadata, "Canon:PrimaryAFPoint").is_none(),
            "{assertion_label} must select no PrimaryAFPoint alternative"
        );
        assert_present(&metadata, "Canon:RawDataOffset", "305419896");
    }
}

#[test]
fn rejected_size_and_zero_count_do_not_prevent_later_main_entries() {
    let invalid = read_carrier(
        "invalid-size-ii",
        &canon_tiff(
            Endian::Ii,
            "Canon Test",
            &[invalid_size_child(Endian::Ii)],
            true,
        ),
    );
    // Native `Validate($dirData, $subdirStart, $size)` rejects this child
    // before AFInfo2 selection.  It continues the parent IFD, which is why
    // the unrelated Main scalar must still be visible.
    assert!(shown(&invalid, "Canon:AFAreaMode").is_none());
    assert_present(&invalid, "Canon:RawDataOffset", "305419896");

    let truncated = read_carrier(
        "truncated-afinfo2-ii",
        &canon_tiff(
            Endian::Ii,
            "Canon Test",
            &[truncated_child(Endian::Ii)],
            true,
        ),
    );
    // The native control warns that it cannot read Main 0x0026, but continues
    // to 0x0081.  The public reader has no warning channel, so absence plus
    // continuation is the observable contract.
    assert!(shown(&truncated, "Canon:AFAreaMode").is_none());
    assert_present(&truncated, "Canon:RawDataOffset", "305419896");

    let zero = read_carrier(
        "zero-count-afinfo2-mm",
        &canon_tiff(
            Endian::Mm,
            "Canon Test",
            &[child(Parent::AfInfo2, Endian::Mm, 0)],
            true,
        ),
    );
    // A well-formed zero-count child is 20 bytes: eight fixed fields, no
    // arrays/bitset, then the one native non-EOS unknown word and primary.
    // It has no AFAreaWidths but its later scalar and parent sibling remain.
    assert!(shown(&zero, "Canon:AFAreaWidths").is_none());
    assert_present(&zero, "Canon:PrimaryAFPoint", "99");
    assert_present(&zero, "Canon:RawDataOffset", "305419896");
}
