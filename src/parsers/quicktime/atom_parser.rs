//! QuickTime/MP4 atom (box) structure parser.
//!
//! This module provides low-level parsing for QuickTime and MP4 atom structures.
//! Atoms (also called boxes in MP4 terminology) are the fundamental building blocks
//! of QuickTime and MP4 files.
//!
//! # Atom Structure
//!
//! Each atom consists of:
//! - 4 bytes: size (big-endian u32, includes 8-byte header)
//! - 4 bytes: type (FourCC - four ASCII characters)
//! - N bytes: data (may contain nested atoms)
//!
//! Special cases:
//! - Size = 1: Extended size in next 8 bytes (64-bit)
//! - Size = 0: Atom extends to end of file

use nom::{
    IResult,
    bytes::complete::take,
    combinator::map_opt,
    multi::many0,
    number::complete::{be_u32, be_u64},
};

/// Four-character code identifying atom type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FourCC([u8; 4]);

impl FourCC {
    /// Create a FourCC from a byte slice
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() == 4 {
            Some(FourCC([bytes[0], bytes[1], bytes[2], bytes[3]]))
        } else {
            None
        }
    }

    /// Create a FourCC from a string
    pub fn from_string(s: &str) -> Option<Self> {
        if s.len() == 4 {
            let bytes = s.as_bytes();
            Some(FourCC([bytes[0], bytes[1], bytes[2], bytes[3]]))
        } else {
            None
        }
    }

    /// Get the FourCC as a string slice
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.0).unwrap_or("????")
    }

    /// Get the raw bytes
    pub fn as_bytes(&self) -> &[u8; 4] {
        &self.0
    }

    /// Check if this matches a given string
    pub fn matches(&self, s: &str) -> bool {
        self.as_str() == s
    }
}

impl std::fmt::Display for FourCC {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Represents a QuickTime/MP4 atom
#[derive(Debug, Clone)]
pub struct Atom<'a> {
    /// Atom type (FourCC)
    pub atom_type: FourCC,
    /// Atom data (excluding size and type header)
    pub data: &'a [u8],
    /// Header size (8 for normal atoms, 16 for extended size atoms)
    pub header_size: u8,
}

impl<'a> Atom<'a> {
    /// Parse nested atoms from this atom's data
    pub fn parse_children(&self) -> Result<Vec<Atom<'a>>, String> {
        match parse_atoms(self.data) {
            Ok((_, atoms)) => Ok(atoms),
            Err(e) => Err(format!("Failed to parse child atoms: {}", e)),
        }
    }

    /// Find a child atom by type
    pub fn find_child(&self, atom_type: &str) -> Option<Atom<'a>> {
        let children = self.parse_children().ok()?;
        children
            .into_iter()
            .find(move |atom| atom.atom_type.matches(atom_type))
    }

    /// Find a nested atom by path (e.g., ["udta", "meta"])
    pub fn find_by_path(&self, path: &[&str]) -> Option<Atom<'a>> {
        if path.is_empty() {
            return Some(self.clone());
        }

        let child = self.find_child(path[0])?;
        if path.len() == 1 {
            Some(child)
        } else {
            child.find_by_path(&path[1..])
        }
    }
}

/// Parse a single atom from input
fn parse_atom(input: &[u8]) -> IResult<&[u8], Atom<'_>> {
    // Parse size (4 bytes, big-endian)
    let (input, size) = be_u32(input)?;

    // Parse type (4 bytes, FourCC)
    let (input, type_bytes) = take(4usize)(input)?;
    use nom::Parser;
    let atom_type =
        map_opt(take(0usize), |_: &[u8]| FourCC::from_bytes(type_bytes)).parse(input)?;

    // Handle extended size (size == 1)
    let (input, actual_size, header_size) = if size == 1 {
        let (input, extended_size) = be_u64(input)?;
        // Extended size includes the 16-byte header (4 + 4 + 8)
        (input, extended_size.saturating_sub(16) as usize, 16u8)
    } else if size == 0 {
        // Size 0 means atom extends to end of file
        (input, input.len(), 8u8)
    } else {
        // Normal size includes the 8-byte header (4 + 4)
        (input, (size as usize).saturating_sub(8), 8u8)
    };

    // Take the atom data
    let (input, data) = take(actual_size)(input)?;

    Ok((
        input,
        Atom {
            atom_type: atom_type.1,
            data,
            header_size,
        },
    ))
}

/// Parse multiple atoms from input
pub fn parse_atoms(input: &[u8]) -> IResult<&[u8], Vec<Atom<'_>>> {
    use nom::Parser;
    many0(parse_atom).parse(input)
}

/// Find a top-level atom by type in the input data
pub fn find_atom<'a>(data: &'a [u8], atom_type: &str) -> Option<Atom<'a>> {
    match parse_atoms(data) {
        Ok((_, atoms)) => atoms
            .into_iter()
            .find(|atom| atom.atom_type.matches(atom_type)),
        Err(_) => None,
    }
}

/// Find a nested atom by path (e.g., ["moov", "udta", "meta"])
pub fn find_atom_by_path<'a>(data: &'a [u8], path: &[&str]) -> Option<Atom<'a>> {
    if path.is_empty() {
        return None;
    }

    let root = find_atom(data, path[0])?;
    if path.len() == 1 {
        Some(root)
    } else {
        root.find_by_path(&path[1..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fourcc_creation() {
        let fourcc = FourCC::from_string("moov").unwrap();
        assert_eq!(fourcc.as_str(), "moov");
        assert!(fourcc.matches("moov"));
        assert!(!fourcc.matches("mdat"));
    }

    #[test]
    fn test_fourcc_from_bytes() {
        let fourcc = FourCC::from_bytes(b"ftyp").unwrap();
        assert_eq!(fourcc.as_str(), "ftyp");
    }

    #[test]
    fn test_parse_simple_atom() {
        // Create a simple atom: size=12, type="test", data="data"
        let data = [
            0x00, 0x00, 0x00, 0x10, // size = 16 (8 header + 8 data)
            b't', b'e', b's', b't', // type = "test"
            b'd', b'a', b't', b'a', // data
            b'm', b'o', b'r', b'e', // more data
        ];

        let result = parse_atom(&data);
        assert!(result.is_ok());

        let (remaining, atom) = result.unwrap();
        assert!(remaining.is_empty());
        assert_eq!(atom.atom_type.as_str(), "test");
        assert_eq!(atom.data, b"datamore");
        assert_eq!(atom.header_size, 8);
    }

    #[test]
    fn test_parse_multiple_atoms() {
        // Create two atoms
        let data = [
            0x00, 0x00, 0x00, 0x0C, // size = 12
            b'a', b'a', b'a', b'a', // type = "aaaa"
            b'1', b'2', b'3', b'4', // data
            0x00, 0x00, 0x00, 0x0C, // size = 12
            b'b', b'b', b'b', b'b', // type = "bbbb"
            b'5', b'6', b'7', b'8', // data
        ];

        let result = parse_atoms(&data);
        assert!(result.is_ok());

        let (remaining, atoms) = result.unwrap();
        assert!(remaining.is_empty());
        assert_eq!(atoms.len(), 2);
        assert_eq!(atoms[0].atom_type.as_str(), "aaaa");
        assert_eq!(atoms[1].atom_type.as_str(), "bbbb");
    }

    #[test]
    fn test_find_atom() {
        let data = [
            0x00, 0x00, 0x00, 0x0C, // size = 12
            b'm', b'o', b'o', b'v', // type = "moov"
            b'1', b'2', b'3', b'4', // data
            0x00, 0x00, 0x00, 0x0C, // size = 12
            b'm', b'd', b'a', b't', // type = "mdat"
            b'5', b'6', b'7', b'8', // data
        ];

        let moov = find_atom(&data, "moov");
        assert!(moov.is_some());
        assert_eq!(moov.unwrap().data, b"1234");

        let mdat = find_atom(&data, "mdat");
        assert!(mdat.is_some());
        assert_eq!(mdat.unwrap().data, b"5678");

        let none = find_atom(&data, "none");
        assert!(none.is_none());
    }

    #[test]
    fn test_nested_atoms() {
        // Create nested structure: parent -> child
        let child_atom = [
            0x00, 0x00, 0x00, 0x0C, // size = 12
            b'c', b'h', b'l', b'd', // type = "chld"
            b'X', b'Y', b'Z', b'!', // data
        ];

        let mut parent_data = vec![
            0x00, 0x00, 0x00, 0x14, // size = 20 (8 header + 12 child)
            b'p', b'a', b'r', b't', // type = "part"
        ];
        parent_data.extend_from_slice(&child_atom);

        let parent = find_atom(&parent_data, "part").unwrap();
        let child = parent.find_child("chld");
        assert!(child.is_some());
        assert_eq!(child.unwrap().data, b"XYZ!");
    }
}
