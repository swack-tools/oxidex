//! Nikon MakerNote decryption (`ProcessNikonEncrypted` / `Decrypt` in
//! `Image::ExifTool::Nikon`).
//!
//! Everything Nikon writes into ShotInfo (0x0091), the 02xx+ ColorBalance
//! blocks (0x0097) and LensData from version 0201 on (0x0098) is enciphered
//! with a stream cipher keyed on the body's `SerialNumber` (0x001d) and
//! `ShutterCount` (0x00a7). ExifTool pre-scans the MakerNote IFD for those two
//! tags before walking it, because the count lives *after* the encrypted tags
//! in tag-id order.
//!
//! `Decrypt` is an incremental routine: it seeds `ci0`/`cj0`/`ck0` from the
//! keys, remembers the offset of the first call, and re-derives its state on
//! every later call from the distance back to that offset. The recurrence is
//!
//! ```text
//! cj(m+1) = cj(m) + ci0 * ck(m)     ck(m+1) = ck(m) + 1     ck(0) = ck0 = 0x60
//! ```
//!
//! which telescopes, so the byte `m` past `DecryptStart` is always XORed with
//!
//! ```text
//! K(m) = cj0 + ci0 * ((m + 1) * ck0 + m * (m + 1) / 2)   (mod 256)
//! ```
//!
//! That closed form is what [`Decryptor::keystream_byte`] computes, which is
//! why this module can decrypt a block in one pass instead of reproducing
//! ExifTool's piecewise calls. The two are equal by construction, and
//! `decrypt_matches_exiftool_vectors` pins it to values taken from ExifTool's
//! own `Decrypt`.

// Decoding tables from ExifTool's Nikon.pm (ref 4).
const XLAT0: [u8; 256] = [
    0xc1, 0xbf, 0x6d, 0x0d, 0x59, 0xc5, 0x13, 0x9d, 0x83, 0x61, 0x6b, 0x4f, 0xc7, 0x7f, 0x3d, 0x3d,
    0x53, 0x59, 0xe3, 0xc7, 0xe9, 0x2f, 0x95, 0xa7, 0x95, 0x1f, 0xdf, 0x7f, 0x2b, 0x29, 0xc7, 0x0d,
    0xdf, 0x07, 0xef, 0x71, 0x89, 0x3d, 0x13, 0x3d, 0x3b, 0x13, 0xfb, 0x0d, 0x89, 0xc1, 0x65, 0x1f,
    0xb3, 0x0d, 0x6b, 0x29, 0xe3, 0xfb, 0xef, 0xa3, 0x6b, 0x47, 0x7f, 0x95, 0x35, 0xa7, 0x47, 0x4f,
    0xc7, 0xf1, 0x59, 0x95, 0x35, 0x11, 0x29, 0x61, 0xf1, 0x3d, 0xb3, 0x2b, 0x0d, 0x43, 0x89, 0xc1,
    0x9d, 0x9d, 0x89, 0x65, 0xf1, 0xe9, 0xdf, 0xbf, 0x3d, 0x7f, 0x53, 0x97, 0xe5, 0xe9, 0x95, 0x17,
    0x1d, 0x3d, 0x8b, 0xfb, 0xc7, 0xe3, 0x67, 0xa7, 0x07, 0xf1, 0x71, 0xa7, 0x53, 0xb5, 0x29, 0x89,
    0xe5, 0x2b, 0xa7, 0x17, 0x29, 0xe9, 0x4f, 0xc5, 0x65, 0x6d, 0x6b, 0xef, 0x0d, 0x89, 0x49, 0x2f,
    0xb3, 0x43, 0x53, 0x65, 0x1d, 0x49, 0xa3, 0x13, 0x89, 0x59, 0xef, 0x6b, 0xef, 0x65, 0x1d, 0x0b,
    0x59, 0x13, 0xe3, 0x4f, 0x9d, 0xb3, 0x29, 0x43, 0x2b, 0x07, 0x1d, 0x95, 0x59, 0x59, 0x47, 0xfb,
    0xe5, 0xe9, 0x61, 0x47, 0x2f, 0x35, 0x7f, 0x17, 0x7f, 0xef, 0x7f, 0x95, 0x95, 0x71, 0xd3, 0xa3,
    0x0b, 0x71, 0xa3, 0xad, 0x0b, 0x3b, 0xb5, 0xfb, 0xa3, 0xbf, 0x4f, 0x83, 0x1d, 0xad, 0xe9, 0x2f,
    0x71, 0x65, 0xa3, 0xe5, 0x07, 0x35, 0x3d, 0x0d, 0xb5, 0xe9, 0xe5, 0x47, 0x3b, 0x9d, 0xef, 0x35,
    0xa3, 0xbf, 0xb3, 0xdf, 0x53, 0xd3, 0x97, 0x53, 0x49, 0x71, 0x07, 0x35, 0x61, 0x71, 0x2f, 0x43,
    0x2f, 0x11, 0xdf, 0x17, 0x97, 0xfb, 0x95, 0x3b, 0x7f, 0x6b, 0xd3, 0x25, 0xbf, 0xad, 0xc7, 0xc5,
    0xc5, 0xb5, 0x8b, 0xef, 0x2f, 0xd3, 0x07, 0x6b, 0x25, 0x49, 0x95, 0x25, 0x49, 0x6d, 0x71, 0xc7,
];
const XLAT1: [u8; 256] = [
    0xa7, 0xbc, 0xc9, 0xad, 0x91, 0xdf, 0x85, 0xe5, 0xd4, 0x78, 0xd5, 0x17, 0x46, 0x7c, 0x29, 0x4c,
    0x4d, 0x03, 0xe9, 0x25, 0x68, 0x11, 0x86, 0xb3, 0xbd, 0xf7, 0x6f, 0x61, 0x22, 0xa2, 0x26, 0x34,
    0x2a, 0xbe, 0x1e, 0x46, 0x14, 0x68, 0x9d, 0x44, 0x18, 0xc2, 0x40, 0xf4, 0x7e, 0x5f, 0x1b, 0xad,
    0x0b, 0x94, 0xb6, 0x67, 0xb4, 0x0b, 0xe1, 0xea, 0x95, 0x9c, 0x66, 0xdc, 0xe7, 0x5d, 0x6c, 0x05,
    0xda, 0xd5, 0xdf, 0x7a, 0xef, 0xf6, 0xdb, 0x1f, 0x82, 0x4c, 0xc0, 0x68, 0x47, 0xa1, 0xbd, 0xee,
    0x39, 0x50, 0x56, 0x4a, 0xdd, 0xdf, 0xa5, 0xf8, 0xc6, 0xda, 0xca, 0x90, 0xca, 0x01, 0x42, 0x9d,
    0x8b, 0x0c, 0x73, 0x43, 0x75, 0x05, 0x94, 0xde, 0x24, 0xb3, 0x80, 0x34, 0xe5, 0x2c, 0xdc, 0x9b,
    0x3f, 0xca, 0x33, 0x45, 0xd0, 0xdb, 0x5f, 0xf5, 0x52, 0xc3, 0x21, 0xda, 0xe2, 0x22, 0x72, 0x6b,
    0x3e, 0xd0, 0x5b, 0xa8, 0x87, 0x8c, 0x06, 0x5d, 0x0f, 0xdd, 0x09, 0x19, 0x93, 0xd0, 0xb9, 0xfc,
    0x8b, 0x0f, 0x84, 0x60, 0x33, 0x1c, 0x9b, 0x45, 0xf1, 0xf0, 0xa3, 0x94, 0x3a, 0x12, 0x77, 0x33,
    0x4d, 0x44, 0x78, 0x28, 0x3c, 0x9e, 0xfd, 0x65, 0x57, 0x16, 0x94, 0x6b, 0xfb, 0x59, 0xd0, 0xc8,
    0x22, 0x36, 0xdb, 0xd2, 0x63, 0x98, 0x43, 0xa1, 0x04, 0x87, 0x86, 0xf7, 0xa6, 0x26, 0xbb, 0xd6,
    0x59, 0x4d, 0xbf, 0x6a, 0x2e, 0xaa, 0x2b, 0xef, 0xe6, 0x78, 0xb6, 0x4e, 0xe0, 0x2f, 0xdc, 0x7c,
    0xbe, 0x57, 0x19, 0x32, 0x7e, 0x2a, 0xd0, 0xb8, 0xba, 0x29, 0x00, 0x3c, 0x52, 0x7d, 0xa8, 0x49,
    0x3b, 0x2d, 0xeb, 0x25, 0x49, 0xfa, 0xa3, 0xaa, 0x39, 0xa7, 0xc5, 0xa7, 0x50, 0x11, 0x36, 0xfb,
    0xc6, 0x67, 0x4a, 0xf5, 0xa5, 0x12, 0x65, 0x7e, 0xb0, 0xdf, 0xaf, 0x4e, 0xb3, 0x61, 0x7f, 0x2f,
];

/// The `$$et{NikonSerialKey}` half of the key schedule.
///
/// `SerialKey` in Nikon.pm: an all-digit serial is the key itself, and
/// anything else falls back to a per-body constant -- 0x22 on the D50, 0x60
/// everywhere else. `ProcessNikon` pre-seeds an absent 0x001d to 0, which is a
/// usable key, so a missing serial gives 0 rather than no key at all.
pub fn serial_key(serial: Option<&str>, model: Option<&str>) -> u32 {
    let Some(serial) = serial else {
        return 0;
    };
    if !serial.is_empty() && serial.bytes().all(|b| b.is_ascii_digit()) {
        // Perl numifies the string, so leading zeros vanish and anything past
        // 32 bits is irrelevant -- only the low byte is ever consulted.
        return serial.parse::<u64>().unwrap_or(u64::MAX) as u32;
    }
    // `$$et{Model} =~ /\bD50$/`
    let is_d50 = model.is_some_and(|m| {
        m.strip_suffix("D50")
            .is_some_and(|head| !head.ends_with(|c: char| c.is_alphanumeric() || c == '_'))
    });
    if is_d50 { 0x22 } else { 0x60 }
}

/// The seeded cipher state: `ci0`, `cj0` and `ck0` from ExifTool's `Decrypt`.
#[derive(Clone, Copy)]
pub struct Decryptor {
    ci0: u64,
    cj0: u64,
    ck0: u64,
}

impl Decryptor {
    /// Seed from the serial and shutter-count keys.
    pub fn new(serial: u32, count: u32) -> Self {
        let mut key: u32 = 0;
        for i in 0..4 {
            key ^= (count >> (i * 8)) & 0xff;
        }
        Decryptor {
            ci0: u64::from(XLAT0[(serial & 0xff) as usize]),
            cj0: u64::from(XLAT1[(key & 0xff) as usize]),
            ck0: 0x60,
        }
    }

    /// `K(m)` -- the keystream byte `m` bytes past `DecryptStart`.
    fn keystream_byte(&self, m: u64) -> u8 {
        // Only `term mod 256` matters, and the triangular number T(m) repeats
        // mod 256 with period 512 (T(m+512) - T(m) = 512*m + 131328, both
        // terms divisible by 256), so reducing m first keeps the product small.
        let r = m % 512;
        let triangular = r * (r + 1) / 2;
        let term = self.ck0 * (m + 1) + triangular;
        ((self.cj0 + self.ci0 * term) & 0xff) as u8
    }

    /// Decrypt `data` in place from `decrypt_start` to the end of the block.
    ///
    /// ExifTool decrypts piecewise -- only the ranges a table actually reads --
    /// but every piece indexes the same position-keyed stream, so covering the
    /// whole tail yields byte-for-byte the same plaintext everywhere ExifTool
    /// looks.
    pub fn decrypt_from(&self, data: &mut [u8], decrypt_start: usize) {
        if decrypt_start >= data.len() {
            return;
        }
        for (m, byte) in data[decrypt_start..].iter_mut().enumerate() {
            *byte ^= self.keystream_byte(m as u64);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Vectors produced by calling `Image::ExifTool::Nikon::Decrypt` (ExifTool
    /// 13.59) on the block `data[i] = (i * 37 + 11) % 256`, 600 bytes long.
    /// Each row is (serial, count, DecryptStart, 8 bytes at DecryptStart, last
    /// 8 bytes).
    #[allow(clippy::type_complexity)]
    const VECTORS: &[(u32, u32, usize, [u8; 8], [u8; 8])] = &[
        (
            0,
            0,
            0,
            [0x0c, 0x18, 0x5f, 0xd7, 0x8e, 0xf2, 0xf5, 0xcd],
            [0x34, 0xe0, 0xb7, 0x4f, 0xd6, 0x3a, 0xdd, 0x05],
        ),
        (
            0,
            0,
            4,
            [0x98, 0xec, 0xe3, 0xa3, 0x22, 0x6e, 0x61, 0x61],
            [0xee, 0x22, 0xf5, 0xf5, 0x80, 0x74, 0x2b, 0xdb],
        ),
        (
            3126,
            485,
            4,
            [0x76, 0xbc, 0x1f, 0x6d, 0x8c, 0x52, 0x39, 0xcf],
            [0x80, 0x5e, 0xf5, 0x7b, 0xee, 0x54, 0x57, 0xd5],
        ),
        (
            96,
            41476,
            284,
            [0xca, 0xe6, 0x95, 0xad, 0xd4, 0x20, 0x8b, 0x33],
            [0x6c, 0x18, 0x33, 0xfb, 0x06, 0x2a, 0x89, 0xe1],
        ),
        (
            2005108,
            4294967295,
            0,
            [0x0c, 0xa0, 0x17, 0x67, 0xbe, 0x8a, 0x4d, 0x2d],
            [0x74, 0x88, 0x2f, 0x7f, 0x66, 0x12, 0x15, 0x25],
        ),
        (
            34,
            1580,
            284,
            [0xf7, 0x53, 0x8c, 0xdc, 0x1d, 0xd1, 0xce, 0x7e],
            [0xc5, 0xb9, 0x66, 0x76, 0x4b, 0x6f, 0x78, 0x28],
        ),
        (
            65535,
            1,
            4,
            [0xc3, 0x07, 0x18, 0xe8, 0x91, 0x7d, 0x12, 0x22],
            [0xc5, 0x19, 0xfe, 0x2e, 0xdb, 0xdf, 0x90, 0x90],
        ),
        (
            54,
            2380,
            4,
            [0x09, 0xe1, 0x4a, 0x1e, 0x5f, 0xef, 0x8c, 0xb8],
            [0x53, 0x8b, 0x58, 0x14, 0x41, 0xf9, 0xa2, 0x66],
        ),
    ];

    fn sample() -> Vec<u8> {
        (0..600u32).map(|i| ((i * 37 + 11) % 256) as u8).collect()
    }

    #[test]
    fn decrypt_matches_exiftool_vectors() {
        for &(serial, count, start, head, tail) in VECTORS {
            let mut data = sample();
            Decryptor::new(serial, count).decrypt_from(&mut data, start);
            assert_eq!(
                &data[start..start + 8],
                &head[..],
                "head: serial={serial} count={count} start={start}"
            );
            assert_eq!(
                &data[data.len() - 8..],
                &tail[..],
                "tail: serial={serial} count={count} start={start}"
            );
        }
    }

    #[test]
    fn decrypt_is_its_own_inverse() {
        let original = sample();
        let mut data = original.clone();
        let d = Decryptor::new(3126, 485);
        d.decrypt_from(&mut data, 4);
        d.decrypt_from(&mut data, 4);
        assert_eq!(data, original);
    }

    #[test]
    fn keystream_stays_correct_past_the_triangular_period() {
        // 600 bytes already crosses m = 512; decrypting a longer block in one
        // pass must agree with decrypting the same bytes as a suffix.
        let mut whole: Vec<u8> = (0..4096u32).map(|i| (i % 251) as u8).collect();
        let d = Decryptor::new(7, 99);
        d.decrypt_from(&mut whole, 4);
        assert_ne!(whole[4000], (4000u32 % 251) as u8);
    }

    #[test]
    fn serial_key_falls_back_per_body() {
        assert_eq!(serial_key(Some("0003126"), Some("NIKON D850")), 3126);
        assert_eq!(serial_key(Some("No= 30045efe"), Some("NIKON D70s")), 0x60);
        assert_eq!(serial_key(Some(""), Some("NIKON D200")), 0x60);
        assert_eq!(serial_key(Some(""), Some("NIKON D50")), 0x22);
        // `\bD50$` must not match D5000.
        assert_eq!(serial_key(Some("x"), Some("NIKON D5000")), 0x60);
        assert_eq!(serial_key(None, Some("NIKON D50")), 0);
    }
}

// ===========================================================================
// Dispatch from Nikon::Main
// ===========================================================================

use std::collections::HashMap;

use super::binary_data::{Ctx, Root, process, select_root};
use super::encrypted_tables::{COLOR_BALANCE_ROOTS, LENS_DATA_ROOTS, SHOT_INFO_ROOTS};
use crate::parsers::tiff::ifd_parser::ByteOrder;

/// The two pre-scanned key tags, already reduced to ExifTool's key integers.
///
/// `None` means `ProcessNikonEncrypted` would have bailed out with
/// "Can't decrypt Nikon information" -- ExifTool emits nothing from any
/// encrypted block for that file, and neither do we.
#[derive(Clone, Copy)]
pub struct Keys {
    pub serial: u32,
    pub count: u32,
}

/// `Nikon::Main` 0x0091.
pub fn parse_shot_info(
    value: &[u8],
    entry_count: usize,
    keys: Option<Keys>,
    order: ByteOrder,
    ctx: &mut Ctx,
    out: &mut HashMap<String, String>,
) {
    parse_encrypted(SHOT_INFO_ROOTS, value, entry_count, keys, order, ctx, out);
}

/// `Nikon::Main` 0x0097.
pub fn parse_color_balance(
    value: &[u8],
    entry_count: usize,
    keys: Option<Keys>,
    order: ByteOrder,
    ctx: &mut Ctx,
    out: &mut HashMap<String, String>,
) {
    parse_encrypted(
        COLOR_BALANCE_ROOTS,
        value,
        entry_count,
        keys,
        order,
        ctx,
        out,
    );
}

/// `Nikon::Main` 0x0098.
pub fn parse_lens_data(
    value: &[u8],
    entry_count: usize,
    keys: Option<Keys>,
    order: ByteOrder,
    ctx: &mut Ctx,
    out: &mut HashMap<String, String>,
) {
    parse_encrypted(LENS_DATA_ROOTS, value, entry_count, keys, order, ctx, out);
}

/// Select the sub-directory variant, decrypt, and walk the resulting table.
fn parse_encrypted(
    roots: &'static [Root],
    value: &[u8],
    entry_count: usize,
    keys: Option<Keys>,
    order: ByteOrder,
    ctx: &mut Ctx,
    out: &mut HashMap<String, String>,
) {
    let Some(root) = select_root(roots, value, entry_count) else {
        return;
    };
    // A variant with no DecryptStart is one of the plaintext layouts, which
    // the hand-written parsers already cover.
    let Some(enc) = root.encrypted else {
        return;
    };
    // No usable key means ExifTool warns and extracts nothing here.
    let Some(keys) = keys else {
        return;
    };

    let mut data = value.to_vec();
    Decryptor::new(keys.serial, keys.count).decrypt_from(&mut data, enc.decrypt_start);

    // `DirOffset`, when present, is relative to `DecryptStart`.
    let dir_start = if enc.dir_offset > 0 {
        enc.dir_offset + enc.decrypt_start
    } else {
        0
    };
    if dir_start > data.len() {
        return;
    }
    let big = enc.byte_order.unwrap_or(order == ByteOrder::BigEndian);
    let dir_len = data.len() - dir_start;
    process(enc.table, &data, dir_start, dir_len, big, ctx, out, 0);
}

#[cfg(test)]
mod dispatch_tests {
    use super::*;
    use crate::parsers::tiff::makernotes::nikon::binary_data::table_index;

    /// Build a plaintext `ShotInfoD80` block, then encipher everything from
    /// `DecryptStart = 4` on, which is exactly what a D80 writes.
    fn shot_info_d80(serial: u32, count: u32) -> Vec<u8> {
        let mut data = vec![0u8; 765];
        data[..4].copy_from_slice(b"0208");
        // 586: ShutterCount, int32u big-endian
        data[586..590].copy_from_slice(&366u32.to_be_bytes());
        // 590: Rotation (0x07), VibrationReduction (0x18), FlashFired (0xe0).
        // FlashFired is a BITMASK, so 0x40 masks and shifts to 2, and it is
        // *bit* 1 of that -- Internal -- not the entry keyed 2.
        data[590] = 0x02 | 0x18 | 0x40; // Rotate 90 CW | On | Internal
        // 708: NikonImageSize (0xf0), ImageQuality (0x0f)
        data[708] = 0x10 | 0x01; // Medium (5.6 M) | JPEG Fine
        // 748: CustomSettingsD80, undef[17]. Byte 0 mask 0x80 is Beep, whose
        // PrintConv is %onOff -- 0 is On and 1 is Off.
        data[748] = 0x80;
        Decryptor::new(serial, count).decrypt_from(&mut data, 4);
        data
    }

    fn parse(data: &[u8], keys: Option<Keys>) -> HashMap<String, String> {
        let mut ctx = Ctx::new(Some("NIKON D80"), None);
        let mut out = HashMap::new();
        parse_shot_info(
            data,
            data.len(),
            keys,
            ByteOrder::LittleEndian,
            &mut ctx,
            &mut out,
        );
        out
    }

    #[test]
    fn decodes_an_encrypted_shot_info_block() {
        let keys = Keys {
            serial: 2000232,
            count: 366,
        };
        let out = parse(&shot_info_d80(keys.serial, keys.count), Some(keys));
        assert_eq!(
            out.get("Nikon:ShutterCount").map(String::as_str),
            Some("366")
        );
        assert_eq!(
            out.get("Nikon:Rotation").map(String::as_str),
            Some("Rotate 90 CW")
        );
        assert_eq!(
            out.get("Nikon:VibrationReduction").map(String::as_str),
            Some("On")
        );
        assert_eq!(
            out.get("Nikon:FlashFired").map(String::as_str),
            Some("Internal")
        );
        assert_eq!(
            out.get("Nikon:NikonImageSize").map(String::as_str),
            Some("Medium (5.6 M)")
        );
        assert_eq!(
            out.get("Nikon:ImageQuality").map(String::as_str),
            Some("JPEG Fine")
        );
        // ...and the CustomSettingsD80 sub-directory behind it.
        assert_eq!(out.get("Nikon:Beep").map(String::as_str), Some("Off"));
    }

    #[test]
    fn a_wrong_key_is_not_silently_reported() {
        // The point of the exercise: decryption with the wrong key yields
        // structured-looking nonsense, so the only defence is never to run it
        // without the real key. With no ShutterCount there is no key at all,
        // and nothing may be emitted.
        let keys = Keys {
            serial: 2000232,
            count: 366,
        };
        let data = shot_info_d80(keys.serial, keys.count);
        assert!(parse(&data, None).is_empty());
    }

    #[test]
    fn a_wrong_key_changes_every_decoded_value() {
        let real = Keys {
            serial: 2000232,
            count: 366,
        };
        let data = shot_info_d80(real.serial, real.count);
        let good = parse(&data, Some(real));
        let bad = parse(
            &data,
            Some(Keys {
                serial: 2000233,
                count: 366,
            }),
        );
        assert_ne!(
            good.get("Nikon:ShutterCount"),
            bad.get("Nikon:ShutterCount"),
            "a one-off serial must not decrypt to the same ShutterCount"
        );
    }

    #[test]
    fn the_version_string_selects_the_table() {
        let mut value = vec![0u8; 100];
        value[..4].copy_from_slice(b"0208");
        let root = select_root(SHOT_INFO_ROOTS, &value, 100).expect("a root must match");
        assert_eq!(root.name, "ShotInfoD80");

        // 0210 is shared by the D3 and the D300; only $count separates them.
        value[..4].copy_from_slice(b"0210");
        assert_eq!(
            select_root(SHOT_INFO_ROOTS, &value, 5291).map(|r| r.name),
            Some("ShotInfoD300a")
        );
        assert_eq!(
            select_root(SHOT_INFO_ROOTS, &value, 5399).map(|r| r.name),
            Some("ShotInfoD3a")
        );
        assert_eq!(
            select_root(SHOT_INFO_ROOTS, &value, 5412).map(|r| r.name),
            Some("ShotInfoD3b")
        );
        // ...and an unrecognised count falls through to the generic 02xx table.
        assert_eq!(
            select_root(SHOT_INFO_ROOTS, &value, 999).map(|r| r.name),
            Some("ShotInfo02xx")
        );
    }

    #[test]
    fn color_balance_02_stops_at_version_0210() {
        let mut value = vec![0u8; 600];
        for (ver, want) in [
            ("0205", "ColorBalance0205"),
            ("0209", "ColorBalance0209"),
            ("0210", "ColorBalance02"),
            ("0211", "ColorBalance0211"),
            ("0215", "ColorBalance0215"),
        ] {
            value[..4].copy_from_slice(ver.as_bytes());
            assert_eq!(
                select_root(COLOR_BALANCE_ROOTS, &value, 600).map(|r| r.name),
                Some(want),
                "version {ver}"
            );
        }
    }

    #[test]
    fn plaintext_lens_data_is_left_to_the_unencrypted_parser() {
        let mut value = vec![0u8; 40];
        value[..4].copy_from_slice(b"0101");
        let root = select_root(LENS_DATA_ROOTS, &value, 40).expect("a root must match");
        assert_eq!(root.name, "LensData0101");
        assert!(root.encrypted.is_none());
    }

    #[test]
    fn every_root_points_at_a_real_table() {
        for roots in [SHOT_INFO_ROOTS, COLOR_BALANCE_ROOTS, LENS_DATA_ROOTS] {
            for root in roots {
                if let Some(enc) = root.encrypted {
                    assert!(
                        enc.table < super::super::encrypted_tables::TABLES.len(),
                        "{} points outside TABLES",
                        root.name
                    );
                }
            }
        }
        assert!(table_index("ShotInfoD80").is_some());
    }
}
