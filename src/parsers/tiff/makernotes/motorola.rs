//! Motorola MakerNote parser
//!
//! Parses Motorola smartphone camera-specific EXIF MakerNote tags.
//! Motorola phones used custom MakerNote tags before adopting Android
//! standard EXIF, and some modern Moto phones still include them.
//!
//! ## Supported Devices
//! - RAZR series phones
//! - DROID series phones
//! - Moto G/X/E series (modern smartphones)
//!
//! ## Supported Features
//! - Camera mode and scene detection
//! - HDR and night mode settings
//! - Burst shot information
//! - Computational photography features
//! - Flash and focus modes
//!
//! ## Tag Structure
//! Motorola uses a simple IFD format with phone-specific tags.

#![allow(dead_code)]

use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use once_cell::sync::Lazy;
use std::collections::HashMap;

use super::registries::motorola::motorola_registry;
use super::shared::MakerNoteParser;
use super::shared::array_extractors::extract_string;
use super::shared::ifd_parser_base::{IfdParserConfig, parse_ifd_entries};
use super::shared::tag_registry::TagRegistry;

// ============================================================================
// Decoders
// ============================================================================
//
// ExifTool's Motorola::Main (Motorola.pm:20-127) names six tags and every one
// of them is `Writable => 'string'` - there is no PrintConv anywhere in the
// table, so this parser needs no value decoders.

// ============================================================================
// Helper Functions
// ============================================================================

// Chooses the maker note's own byte order.
//
// ExifTool declares `ByteOrder => 'Unknown'` for Motorola (MakerNotes.pm:534),
// meaning the order is detected rather than inherited. The IFD entry count at
// offset 8 is the discriminator: a real count is small, and the byte-swapped
// reading of a small count is enormous.
fn detect_byte_order(data: &[u8], fallback: ByteOrder) -> ByteOrder {
    let Some(bytes) = data.get(8..10) else {
        return fallback;
    };
    let be = u16::from_be_bytes([bytes[0], bytes[1]]);
    let le = u16::from_le_bytes([bytes[0], bytes[1]]);
    match (be <= 500, le <= 500) {
        (true, false) => ByteOrder::BigEndian,
        (false, true) => ByteOrder::LittleEndian,
        _ => fallback,
    }
}

// ============================================================================
// Tag Registry
// ============================================================================

// Lazy-initialized tag registry using centralized registry function
static TAG_REGISTRY: Lazy<TagRegistry> = Lazy::new(motorola_registry);

// ============================================================================
// Parser Implementation
// ============================================================================

/// Parser for Motorola MakerNotes
pub struct MotorolaParser;

impl Default for MotorolaParser {
    fn default() -> Self {
        Self::new()
    }
}

impl MotorolaParser {
    /// Creates a new Motorola parser instance
    pub fn new() -> Self {
        MotorolaParser
    }

    /// Parses a single IFD entry and extracts the tag value
    /// Delegates to registry for tag decoding when available
    fn parse_entry(
        &self,
        entry: &IfdEntry,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) {
        let tag_name = match TAG_REGISTRY.get_tag_name(entry.tag_id) {
            Some(name) => name,
            None => return,
        };

        // All six tags ExifTool names in Motorola::Main are strings.
        if let Some(s) = extract_string(entry, data, byte_order) {
            tags.insert(format!("Motorola:{}", tag_name), s);
        }
    }
}

impl MakerNoteParser for MotorolaParser {
    fn manufacturer_name(&self) -> &'static str {
        "Motorola"
    }

    fn tag_prefix(&self) -> &'static str {
        "Motorola:"
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        // ExifTool MakerNotes.pm:528-536:
        //     Name      => 'MakerNoteMotorola',
        //     Condition => '$$valPt=~/^MOT\0/',
        //     Start     => '$valuePtr + 8',
        //     Base      => '$start - 8',
        //     ByteOrder => 'Unknown',
        // The IFD begins 8 bytes past the "MOT\0" signature, but `Base` puts
        // value offsets back at the start of the maker note - so the callback
        // below resolves them against `data`, not against the post-signature
        // slice. Reading the entry count from offset 0 instead sees "MO"
        // = 19791 entries, which is why this parser produced nothing at all on
        // a real Moto file and its fabricated 0x0001-0x0008 table was never
        // contradicted.
        let config = IfdParserConfig {
            signature: Some(b"MOT\0"),
            signature_offset: 8,
            max_entries: 500,
        };

        // ByteOrder => 'Unknown': Motorola.jpg is big-endian throughout, but
        // the maker note does not have to match its container, so pick the
        // order that yields a plausible entry count.
        let byte_order = detect_byte_order(data, byte_order);

        parse_ifd_entries(data, byte_order, &config, |entry, _parse_data| {
            self.parse_entry(entry, data, byte_order, tags);
        })?;
        Ok(())
    }
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a maker-note IFD in the requested byte order. Motorola's real
    /// tags are all strings whose bytes are order-independent, but the *entry
    /// headers* are not - a little-endian-only fixture cannot catch a swapped
    /// tag-id read, which is exactly how a table can look correct and match
    /// nothing.
    /// Builds a Motorola maker note: "MOT\0" plus four filler bytes, then the
    /// IFD. Value offsets are relative to byte 0 of the whole buffer.
    /// Both byte orders are built because ExifTool declares
    /// `ByteOrder => 'Unknown'` (MakerNotes.pm:534).
    fn build_moto_makernote(
        entries: &[(u16, u16, u32, u32)],
        tail: &[u8],
        order: ByteOrder,
    ) -> Vec<u8> {
        let (w16, w32): (fn(u16) -> [u8; 2], fn(u32) -> [u8; 4]) = match order {
            ByteOrder::LittleEndian => (u16::to_le_bytes, u32::to_le_bytes),
            ByteOrder::BigEndian => (u16::to_be_bytes, u32::to_be_bytes),
        };
        let mut data = Vec::new();
        data.extend_from_slice(b"MOT\0\x01\x01\x01\x01");
        data.extend_from_slice(&w16(entries.len() as u16));
        for &(tag, typ, count, val) in entries {
            data.extend_from_slice(&w16(tag));
            data.extend_from_slice(&w16(typ));
            data.extend_from_slice(&w32(count));
            data.extend_from_slice(&w32(val));
        }
        data.extend_from_slice(&w32(0)); // next-IFD pointer
        data.extend_from_slice(tail);
        data
    }

    #[test]
    fn test_motorola_parser_trait() {
        let parser = MotorolaParser::new();
        assert_eq!(parser.manufacturer_name(), "Motorola");
        assert_eq!(parser.tag_prefix(), "Motorola:");
    }

    /// Reproduces the maker note of the corpus file `Motorola.jpg` (XT1575):
    /// the "MOT\0" signature, four filler bytes, then the IFD at offset 8 with
    /// value offsets measured from the start of the maker note
    /// (ExifTool MakerNotes.pm:532-533, `Start => '$valuePtr + 8'`,
    /// `Base => '$start - 8'`).
    ///
    /// Ground truth, `exiftool -a -G1 -s Motorola.jpg`:
    /// ```text
    /// [Motorola]      BuildNumber                     : LPH23.116-18
    /// [Motorola]      SerialNumber                    : NX0A3S0075
    /// [Motorola]      Sensor                          : BACK,IMX230
    /// [Motorola]      ManufactureDate                 : 03Jun2015
    /// ```
    #[test]
    fn test_parse_matches_exiftool_on_moto_xt1575() {
        for order in [ByteOrder::LittleEndian, ByteOrder::BigEndian] {
            let strings: [&[u8]; 4] = [
                b"LPH23.116-18\0",
                b"NX0A3S0075\0",
                b"BACK,IMX230\0",
                b"03Jun2015\0",
            ];
            // 8 (header) + 2 (count) + 5*12 (entries) + 4 (next-IFD) = 74,
            // measured from the start of the maker note because Base is
            // $start - 8.
            let mut off = 74u32;
            let mut tail = Vec::new();
            let mut offsets = Vec::new();
            for s in strings {
                offsets.push((off, s.len() as u32));
                tail.extend_from_slice(s);
                off += s.len() as u32;
            }

            let data = build_moto_makernote(
                &[
                    (0x5500, 2, offsets[0].1, offsets[0].0), // BuildNumber
                    (0x5501, 2, offsets[1].1, offsets[1].0), // SerialNumber
                    // Motorola.pm lists 0x5502 as a comment only - must be dropped.
                    (0x5502, 1, 1, 96),
                    (0x665e, 2, offsets[2].1, offsets[2].0), // Sensor
                    (0x6705, 2, offsets[3].1, offsets[3].0), // ManufactureDate
                ],
                &tail,
                order,
            );

            let mut tags = HashMap::new();
            MotorolaParser::new()
                .parse(&data, order, &mut tags)
                .expect("Motorola maker note should parse");

            assert_eq!(
                tags.get("Motorola:BuildNumber"),
                Some(&"LPH23.116-18".to_string()),
                "{order:?}"
            );
            assert_eq!(
                tags.get("Motorola:SerialNumber"),
                Some(&"NX0A3S0075".to_string()),
                "{order:?}"
            );
            assert_eq!(
                tags.get("Motorola:Sensor"),
                Some(&"BACK,IMX230".to_string()),
                "{order:?}"
            );
            assert_eq!(
                tags.get("Motorola:ManufactureDate"),
                Some(&"03Jun2015".to_string()),
                "{order:?}"
            );
            // 0x5502 is unnamed in ExifTool and must not be emitted.
            assert_eq!(tags.len(), 4, "{order:?}: {tags:?}");
        }
    }

    /// The IFD begins 8 bytes in. Reading the count from offset 0 instead sees
    /// "MO" = 19791, which is what this parser used to do - it then failed the
    /// entry-count check and emitted nothing, so its fabricated table was never
    /// contradicted by a real Moto file.
    #[test]
    fn test_ifd_starts_after_the_mot_signature() {
        // 8 (header) + 2 (count) + 1*12 (entry) + 4 (next-IFD) = 26
        let data = build_moto_makernote(&[(0x5500, 2, 5, 26)], b"ABCD\0", ByteOrder::BigEndian);
        assert_eq!(&data[0..4], b"MOT\0");
        let mut tags = HashMap::new();
        MotorolaParser::new()
            .parse(&data, ByteOrder::BigEndian, &mut tags)
            .expect("Motorola maker note should parse");
        assert_eq!(tags.get("Motorola:BuildNumber"), Some(&"ABCD".to_string()));
    }

    #[test]
    fn test_tag_registry() {
        // ExifTool Motorola.pm:27-28
        assert_eq!(TAG_REGISTRY.get_tag_name(0x5500), Some("BuildNumber"));
        assert_eq!(TAG_REGISTRY.get_tag_name(0x5501), Some("SerialNumber"));
        // Fabricated IDs from the previous table.
        assert!(!TAG_REGISTRY.has_tag(0x0001));
        assert!(!TAG_REGISTRY.has_tag(0x0002));
    }
}
