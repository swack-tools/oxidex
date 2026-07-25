//! ELF note section parsing
//!
//! This module provides parsing for PT_NOTE segments and SHT_NOTE sections.
//! Notes contain auxiliary information such as:
//! - GNU build ID (unique binary identifier)
//! - GNU ABI tag (OS/kernel version info)
//! - GNU properties (security features like CET, BTI)

use crate::io::{ByteOrder, EndianReader};
use crate::parsers::elf::structures::{NoteEntry, nt_core, nt_gnu};
use nom::{
    IResult,
    bytes::complete::take,
    number::complete::{be_u32, le_u32},
};

/// Aligns a value up to a 4-byte boundary
fn align_up(value: usize, alignment: usize) -> usize {
    (value + alignment - 1) & !(alignment - 1)
}

/// Parses a single note entry in little-endian format
fn parse_note_entry_le(input: &[u8]) -> IResult<&[u8], NoteEntry> {
    let (input, namesz) = le_u32(input)?;
    let (input, descsz) = le_u32(input)?;
    let (input, note_type) = le_u32(input)?;

    // Name is padded to 4-byte boundary
    let name_aligned = align_up(namesz as usize, 4);
    let (input, name_bytes) = take(name_aligned)(input)?;

    // Extract actual name (without padding and trailing null)
    let name_end = (namesz as usize).min(name_bytes.len());
    let name_slice = &name_bytes[..name_end];
    let name = String::from_utf8_lossy(name_slice)
        .trim_end_matches('\0')
        .to_string();

    // Descriptor is also padded to 4-byte boundary
    let desc_aligned = align_up(descsz as usize, 4);
    let (input, desc_bytes) = take(desc_aligned)(input)?;

    // Extract actual descriptor (without padding)
    let desc_end = (descsz as usize).min(desc_bytes.len());
    let desc = desc_bytes[..desc_end].to_vec();

    Ok((
        input,
        NoteEntry {
            name,
            note_type,
            desc,
        },
    ))
}

/// Parses a single note entry in big-endian format
fn parse_note_entry_be(input: &[u8]) -> IResult<&[u8], NoteEntry> {
    let (input, namesz) = be_u32(input)?;
    let (input, descsz) = be_u32(input)?;
    let (input, note_type) = be_u32(input)?;

    let name_aligned = align_up(namesz as usize, 4);
    let (input, name_bytes) = take(name_aligned)(input)?;

    let name_end = (namesz as usize).min(name_bytes.len());
    let name_slice = &name_bytes[..name_end];
    let name = String::from_utf8_lossy(name_slice)
        .trim_end_matches('\0')
        .to_string();

    let desc_aligned = align_up(descsz as usize, 4);
    let (input, desc_bytes) = take(desc_aligned)(input)?;

    let desc_end = (descsz as usize).min(desc_bytes.len());
    let desc = desc_bytes[..desc_end].to_vec();

    Ok((
        input,
        NoteEntry {
            name,
            note_type,
            desc,
        },
    ))
}

/// Parses all note entries from a note segment or section
///
/// # Arguments
/// * `input` - Byte slice containing the note data
/// * `is_little_endian` - True for little-endian, false for big-endian
///
/// # Returns
/// * `Vec<NoteEntry>` - All note entries found
pub fn parse_notes(input: &[u8], is_little_endian: bool) -> Vec<NoteEntry> {
    let parser: fn(&[u8]) -> IResult<&[u8], NoteEntry> = if is_little_endian {
        parse_note_entry_le
    } else {
        parse_note_entry_be
    };

    let mut notes = Vec::new();
    let mut remaining = input;

    // Each note has at least 12 bytes (3 x u32 for namesz, descsz, type)
    while remaining.len() >= 12 {
        match parser(remaining) {
            Ok((rest, note)) => {
                notes.push(note);
                remaining = rest;
            }
            Err(_) => break,
        }
    }

    notes
}

/// Extracts the GNU build ID from notes
///
/// # Arguments
/// * `notes` - Parsed note entries
///
/// # Returns
/// * `Some(String)` - Build ID as hex string
/// * `None` - If no build ID note found
pub fn extract_build_id(notes: &[NoteEntry]) -> Option<String> {
    for note in notes {
        if note.name == "GNU" && note.note_type == nt_gnu::NT_GNU_BUILD_ID {
            return Some(
                note.desc
                    .iter()
                    .map(|b| format!("{:02x}", b))
                    .collect::<String>(),
            );
        }
    }
    None
}

/// Represents GNU ABI tag information
#[derive(Debug, Clone)]
pub struct GnuAbiTag {
    /// Operating system (0 = Linux, 1 = GNU Hurd, 2 = Solaris)
    pub os: u32,
    /// Major version
    pub major: u32,
    /// Minor version
    pub minor: u32,
    /// Subminor version
    pub subminor: u32,
}

impl GnuAbiTag {
    /// Returns the OS name
    pub fn os_name(&self) -> &'static str {
        match self.os {
            0 => "Linux",
            1 => "GNU Hurd",
            2 => "Solaris",
            3 => "FreeBSD",
            4 => "NetBSD",
            5 => "Syllable",
            6 => "NaCl",
            _ => "Unknown",
        }
    }

    /// Returns the ABI version as a string
    pub fn version_string(&self) -> String {
        format!("{}.{}.{}", self.major, self.minor, self.subminor)
    }
}

/// Extracts GNU ABI tag from notes
///
/// # Arguments
/// * `notes` - Parsed note entries
/// * `is_little_endian` - Byte order for parsing descriptor
///
/// # Returns
/// * `Some(GnuAbiTag)` - Parsed ABI tag
/// * `None` - If no ABI tag note found
pub fn extract_gnu_abi_tag(notes: &[NoteEntry], is_little_endian: bool) -> Option<GnuAbiTag> {
    for note in notes {
        // ABI tag can be under "GNU" or empty name with type 1
        if (note.name == "GNU" || note.name.is_empty())
            && note.note_type == nt_core::NT_GNU_ABI_TAG
            && note.desc.len() >= 16
        {
            let byte_order = if is_little_endian {
                ByteOrder::Little
            } else {
                ByteOrder::Big
            };
            let reader = EndianReader::new(&note.desc, byte_order);

            return Some(GnuAbiTag {
                os: reader.u32_at(0)?,
                major: reader.u32_at(4)?,
                minor: reader.u32_at(8)?,
                subminor: reader.u32_at(12)?,
            });
        }
    }
    None
}

/// Extracts Gold linker version from notes
///
/// # Arguments
/// * `notes` - Parsed note entries
///
/// # Returns
/// * `Some(String)` - Gold version string
/// * `None` - If no Gold version note found
pub fn extract_gold_version(notes: &[NoteEntry]) -> Option<String> {
    for note in notes {
        if note.name == "GNU" && note.note_type == nt_gnu::NT_GNU_GOLD_VERSION {
            return Some(
                String::from_utf8_lossy(&note.desc)
                    .trim_end_matches('\0')
                    .to_string(),
            );
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Creates a test note entry (little-endian)
    fn create_note_le(name: &str, note_type: u32, desc: &[u8]) -> Vec<u8> {
        let mut data = Vec::new();

        // Include null terminator in name
        let name_with_null = format!("{}\0", name);
        let namesz = name_with_null.len() as u32;
        let descsz = desc.len() as u32;

        data.extend_from_slice(&namesz.to_le_bytes());
        data.extend_from_slice(&descsz.to_le_bytes());
        data.extend_from_slice(&note_type.to_le_bytes());

        // Name padded to 4-byte boundary
        data.extend_from_slice(name_with_null.as_bytes());
        let name_padding = align_up(namesz as usize, 4) - namesz as usize;
        data.extend(vec![0u8; name_padding]);

        // Descriptor padded to 4-byte boundary
        data.extend_from_slice(desc);
        let desc_padding = align_up(descsz as usize, 4) - descsz as usize;
        data.extend(vec![0u8; desc_padding]);

        data
    }

    #[test]
    fn test_parse_single_note() {
        let data = create_note_le("GNU", nt_gnu::NT_GNU_BUILD_ID, &[0xDE, 0xAD, 0xBE, 0xEF]);

        let notes = parse_notes(&data, true);
        assert_eq!(notes.len(), 1);

        let note = &notes[0];
        assert_eq!(note.name, "GNU");
        assert_eq!(note.note_type, nt_gnu::NT_GNU_BUILD_ID);
        assert_eq!(note.desc, vec![0xDE, 0xAD, 0xBE, 0xEF]);
        assert_eq!(note.gnu_type_str(), "Build ID");
    }

    #[test]
    fn test_parse_multiple_notes() {
        let mut data = Vec::new();

        // Build ID note
        data.extend(create_note_le(
            "GNU",
            nt_gnu::NT_GNU_BUILD_ID,
            &[0x01, 0x02, 0x03, 0x04],
        ));

        // ABI tag note
        let abi_desc = [
            0x00, 0x00, 0x00, 0x00, // OS = Linux
            0x05, 0x00, 0x00, 0x00, // Major = 5
            0x04, 0x00, 0x00, 0x00, // Minor = 4
            0x00, 0x00, 0x00, 0x00, // Subminor = 0
        ];
        data.extend(create_note_le("GNU", nt_core::NT_GNU_ABI_TAG, &abi_desc));

        let notes = parse_notes(&data, true);
        assert_eq!(notes.len(), 2);

        assert_eq!(notes[0].note_type, nt_gnu::NT_GNU_BUILD_ID);
        assert_eq!(notes[1].note_type, nt_core::NT_GNU_ABI_TAG);
    }

    #[test]
    fn test_extract_build_id() {
        let data = create_note_le(
            "GNU",
            nt_gnu::NT_GNU_BUILD_ID,
            &[
                0xDE, 0xAD, 0xBE, 0xEF, 0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF, 0xFE, 0xDC,
                0xBA, 0x98, 0x76, 0x54, 0x32, 0x10,
            ],
        );

        let notes = parse_notes(&data, true);
        let build_id = extract_build_id(&notes);

        assert!(build_id.is_some());
        assert_eq!(
            build_id.unwrap(),
            "deadbeef0123456789abcdeffedcba9876543210"
        );
    }

    #[test]
    fn test_extract_build_id_none() {
        let data = create_note_le("TEST", 999, &[0x00]);
        let notes = parse_notes(&data, true);
        let build_id = extract_build_id(&notes);
        assert!(build_id.is_none());
    }

    #[test]
    fn test_extract_gnu_abi_tag() {
        let abi_desc = [
            0x00, 0x00, 0x00, 0x00, // OS = Linux (0)
            0x05, 0x00, 0x00, 0x00, // Major = 5
            0x04, 0x00, 0x00, 0x00, // Minor = 4
            0x00, 0x00, 0x00, 0x00, // Subminor = 0
        ];
        let data = create_note_le("GNU", nt_core::NT_GNU_ABI_TAG, &abi_desc);

        let notes = parse_notes(&data, true);
        let abi = extract_gnu_abi_tag(&notes, true);

        assert!(abi.is_some());
        let abi = abi.unwrap();
        assert_eq!(abi.os, 0);
        assert_eq!(abi.os_name(), "Linux");
        assert_eq!(abi.major, 5);
        assert_eq!(abi.minor, 4);
        assert_eq!(abi.subminor, 0);
        assert_eq!(abi.version_string(), "5.4.0");
    }

    #[test]
    fn test_extract_gold_version() {
        let data = create_note_le("GNU", nt_gnu::NT_GNU_GOLD_VERSION, b"gold 1.16\0");

        let notes = parse_notes(&data, true);
        let version = extract_gold_version(&notes);

        assert!(version.is_some());
        assert_eq!(version.unwrap(), "gold 1.16");
    }

    #[test]
    fn test_note_entry_build_id_hex() {
        let note = NoteEntry {
            name: "GNU".to_string(),
            note_type: nt_gnu::NT_GNU_BUILD_ID,
            desc: vec![0xDE, 0xAD, 0xBE, 0xEF],
        };

        assert_eq!(note.build_id_hex(), Some("deadbeef".to_string()));
    }

    #[test]
    fn test_note_entry_build_id_hex_non_build_id() {
        let note = NoteEntry {
            name: "GNU".to_string(),
            note_type: nt_core::NT_GNU_ABI_TAG,
            desc: vec![0x00, 0x00, 0x00, 0x00],
        };

        assert!(note.build_id_hex().is_none());
    }

    #[test]
    fn test_note_alignment() {
        // Test that notes with various name/desc sizes are parsed correctly
        let data = create_note_le("A", nt_gnu::NT_GNU_BUILD_ID, &[0x01]); // Short name and desc

        let notes = parse_notes(&data, true);
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].name, "A");
        assert_eq!(notes[0].desc, vec![0x01]);
    }

    #[test]
    fn test_parse_truncated_note() {
        // Only header, no name or desc
        let data = vec![0x04, 0x00, 0x00, 0x00]; // namesz = 4, but no more data
        let notes = parse_notes(&data, true);
        assert!(notes.is_empty());
    }

    #[test]
    fn test_abi_tag_os_names() {
        let test_cases = vec![
            (0, "Linux"),
            (1, "GNU Hurd"),
            (2, "Solaris"),
            (3, "FreeBSD"),
            (4, "NetBSD"),
            (5, "Syllable"),
            (6, "NaCl"),
            (99, "Unknown"),
        ];

        for (os, expected) in test_cases {
            let abi = GnuAbiTag {
                os,
                major: 0,
                minor: 0,
                subminor: 0,
            };
            assert_eq!(abi.os_name(), expected);
        }
    }
}
