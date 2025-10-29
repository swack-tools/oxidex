//! QuickTime and MP4 metadata parser.
//!
//! This module provides parsing capabilities for QuickTime (.mov) and MP4 (.mp4, .m4v, .m4a)
//! files. It extracts metadata from various locations within the file structure:
//!
//! - Classic QuickTime user data atoms (©nam, ©ART, etc. in moov→udta)
//! - iTunes-style metadata (moov→udta→meta→ilst)
//! - MP4 metadata with keys/ilst (moov→meta→keys + moov→meta→ilst)
//!
//! # Example
//!
//! ```no_run
//! use exiftool_rs::core::FileReader;
//! use exiftool_rs::parsers::quicktime::parse_quicktime_metadata;
//!
//! # fn example(reader: &dyn FileReader) -> Result<(), String> {
//! let metadata = parse_quicktime_metadata(reader)?;
//!
//! // Access extracted metadata
//! if let Some(title) = metadata.get_string("QuickTime:Title") {
//!     println!("Title: {}", title);
//! }
//! # Ok(())
//! # }
//! ```
//!
//! # File Structure
//!
//! QuickTime and MP4 files are organized as a hierarchy of atoms (also called boxes).
//! Each atom has a 4-byte size, 4-byte type (FourCC), and variable-length data.
//!
//! Metadata locations:
//! - `moov.udta.©xxx`: Classic QuickTime user data
//! - `moov.udta.meta.ilst`: iTunes-style metadata
//! - `moov.meta.keys` + `moov.meta.ilst`: MP4 metadata
//!
//! # Supported Tags
//!
//! The parser extracts common metadata tags including:
//! - Title, Artist, Album, Year
//! - Copyright, Comment, Genre
//! - Encoder, Director, Producer
//! - Track and disc numbers (iTunes metadata)

use crate::core::{FileReader, MetadataMap};

mod atom_parser;
mod metadata_extractor;

pub use atom_parser::{Atom, FourCC};

/// Parse QuickTime/MP4 metadata from a file.
///
/// This function reads the file structure, navigates the atom hierarchy,
/// and extracts metadata from all supported locations.
///
/// # Arguments
///
/// * `reader` - A file reader providing access to the QuickTime/MP4 file data
///
/// # Returns
///
/// A `MetadataMap` containing all extracted metadata tags, or an error if:
/// - The file signature is invalid
/// - No metadata atoms are found
/// - The file structure is malformed
///
/// # Example
///
/// ```no_run
/// # use exiftool_rs::core::FileReader;
/// # use exiftool_rs::parsers::quicktime::parse_quicktime_metadata;
/// # fn example(reader: &dyn FileReader) -> Result<(), String> {
/// let metadata = parse_quicktime_metadata(reader)?;
///
/// for (key, value) in metadata.iter() {
///     println!("{}: {:?}", key, value);
/// }
/// # Ok(())
/// # }
/// ```
pub fn parse_quicktime_metadata(reader: &dyn FileReader) -> Result<MetadataMap, String> {
    // Validate file signature
    // QuickTime/MP4 files typically start with ftyp atom at offset 4
    // But we can also check for moov atom presence
    validate_signature(reader)?;

    // Read enough data to parse the atom structure
    // For most files, metadata is in the first few MB
    // Read up to 10MB to handle various file layouts
    let max_read_size = 10 * 1024 * 1024; // 10 MB
    let file_size = reader.size();
    let read_size = file_size.min(max_read_size as u64) as usize;

    let data = reader
        .read(0, read_size)
        .map_err(|e| format!("Failed to read file: {}", e))?;

    // Parse top-level atoms
    let atoms = atom_parser::parse_atoms(data)
        .map_err(|e| format!("Failed to parse atoms: {}", e))?
        .1;

    // Extract metadata from the atoms
    metadata_extractor::extract_metadata(&atoms)
}

/// Validate QuickTime/MP4 file signature.
///
/// QuickTime/MP4 files should contain either:
/// - An ftyp atom (file type box) near the beginning
/// - A moov atom (movie container)
/// - A wide atom followed by mdat
fn validate_signature(reader: &dyn FileReader) -> Result<(), String> {
    // Read first 16 bytes to check for ftyp or other known atoms
    let header = reader
        .read(0, 16)
        .map_err(|e| format!("Failed to read file header: {}", e))?;

    if header.len() < 16 {
        return Err("File too small to be a valid QuickTime/MP4 file".to_string());
    }

    // Check for common QuickTime/MP4 atom types at the beginning
    // Bytes 4-8 should contain a known atom type
    let atom_type = &header[4..8];

    match atom_type {
        b"ftyp" | b"moov" | b"mdat" | b"wide" | b"free" | b"skip" => Ok(()),
        _ => {
            // Some MP4 files might have a different structure
            // Try reading more to find a moov atom
            let data = reader
                .read(0, 1024)
                .map_err(|e| format!("Failed to read file: {}", e))?;

            if atom_parser::find_atom(data, "moov").is_some()
                || atom_parser::find_atom(data, "ftyp").is_some()
            {
                Ok(())
            } else {
                Err("Invalid QuickTime/MP4 file signature".to_string())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test file reader for unit testing
    struct TestReader {
        data: Vec<u8>,
    }

    impl TestReader {
        fn new(data: Vec<u8>) -> Self {
            Self { data }
        }
    }

    impl FileReader for TestReader {
        fn read(&self, offset: u64, length: usize) -> std::io::Result<&[u8]> {
            let start = offset as usize;
            let end = (start + length).min(self.data.len());
            if start >= self.data.len() {
                Ok(&[])
            } else {
                Ok(&self.data[start..end])
            }
        }

        fn size(&self) -> u64 {
            self.data.len() as u64
        }
    }

    /// Create a minimal QuickTime file structure with user data
    fn create_test_quicktime_file() -> Vec<u8> {
        let mut data = Vec::new();

        // ftyp atom (file type)
        data.extend_from_slice(&[
            0x00, 0x00, 0x00, 0x20, // size = 32
            b'f', b't', b'y', b'p', // type = ftyp
            b'q', b't', b' ', b' ', // major brand = "qt  "
            0x00, 0x00, 0x00, 0x00, // minor version
            b'q', b't', b' ', b' ', // compatible brand 1
            b'm', b'p', b'4', b'2', // compatible brand 2
            0x00, 0x00, 0x00, 0x00, // padding
            0x00, 0x00, 0x00, 0x00, // padding
        ]);

        // Create a ©nam (title) user data atom
        let title_text = b"Test Title";
        let title_data_size = 4 + title_text.len(); // 2 bytes size + 2 bytes lang + text
        let title_atom_size = 8 + title_data_size; // header + data

        let mut title_atom = Vec::new();
        title_atom.extend_from_slice(&(title_atom_size as u32).to_be_bytes());
        title_atom.extend_from_slice(b"\xa9nam"); // ©nam
        title_atom.extend_from_slice(&(title_text.len() as u16).to_be_bytes());
        title_atom.extend_from_slice(&[0x00, 0x00]); // language
        title_atom.extend_from_slice(title_text);

        // Create udta atom containing the title atom
        let udta_size = 8 + title_atom.len();
        let mut udta_atom = Vec::new();
        udta_atom.extend_from_slice(&(udta_size as u32).to_be_bytes());
        udta_atom.extend_from_slice(b"udta");
        udta_atom.extend_from_slice(&title_atom);

        // Create moov atom containing udta
        let moov_size = 8 + udta_atom.len();
        data.extend_from_slice(&(moov_size as u32).to_be_bytes());
        data.extend_from_slice(b"moov");
        data.extend_from_slice(&udta_atom);

        data
    }

    /// Create a minimal MP4 file with iTunes metadata
    fn create_test_itunes_file() -> Vec<u8> {
        let mut data = Vec::new();

        // ftyp atom
        data.extend_from_slice(&[
            0x00, 0x00, 0x00, 0x20, // size = 32
            b'f', b't', b'y', b'p', // type = ftyp
            b'M', b'4', b'A', b' ', // major brand
            0x00, 0x00, 0x00, 0x00, // minor version
            b'M', b'4', b'A', b' ', // compatible brand 1
            b'm', b'p', b'4', b'2', // compatible brand 2
            0x00, 0x00, 0x00, 0x00, // padding
            0x00, 0x00, 0x00, 0x00, // padding
        ]);

        // Create a data atom with UTF-8 text "Artist Name"
        let artist_text = b"Artist Name";
        let mut data_atom = Vec::new();
        data_atom.extend_from_slice(&((8 + 8 + artist_text.len()) as u32).to_be_bytes());
        data_atom.extend_from_slice(b"data");
        data_atom.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]); // type = UTF-8
        data_atom.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // reserved
        data_atom.extend_from_slice(artist_text);

        // Create ©ART atom containing data atom
        let artist_size = 8 + data_atom.len();
        let mut artist_atom = Vec::new();
        artist_atom.extend_from_slice(&(artist_size as u32).to_be_bytes());
        artist_atom.extend_from_slice(b"\xa9ART"); // ©ART
        artist_atom.extend_from_slice(&data_atom);

        // Create ilst atom containing artist atom
        let ilst_size = 8 + artist_atom.len();
        let mut ilst_atom = Vec::new();
        ilst_atom.extend_from_slice(&(ilst_size as u32).to_be_bytes());
        ilst_atom.extend_from_slice(b"ilst");
        ilst_atom.extend_from_slice(&artist_atom);

        // Create meta atom with hdlr and ilst
        // Meta needs a version/flags (4 bytes) and hdlr atom
        let hdlr_atom = [
            0x00, 0x00, 0x00, 0x21, // size = 33
            b'h', b'd', b'l', b'r', // type = hdlr
            0x00, 0x00, 0x00, 0x00, // version/flags
            0x00, 0x00, 0x00, 0x00, // pre-defined
            b'm', b'd', b'i', b'r', // handler type
            b'a', b'p', b'p', b'l', // reserved
            0x00, 0x00, 0x00, 0x00, // reserved
            0x00, 0x00, 0x00, 0x00, // reserved
            0x00, // name (empty)
        ];

        let meta_size = 8 + 4 + hdlr_atom.len() + ilst_atom.len();
        let mut meta_atom = Vec::new();
        meta_atom.extend_from_slice(&(meta_size as u32).to_be_bytes());
        meta_atom.extend_from_slice(b"meta");
        meta_atom.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // version/flags
        meta_atom.extend_from_slice(&hdlr_atom);
        meta_atom.extend_from_slice(&ilst_atom);

        // Create udta atom containing meta
        let udta_size = 8 + meta_atom.len();
        let mut udta_atom = Vec::new();
        udta_atom.extend_from_slice(&(udta_size as u32).to_be_bytes());
        udta_atom.extend_from_slice(b"udta");
        udta_atom.extend_from_slice(&meta_atom);

        // Create moov atom
        let moov_size = 8 + udta_atom.len();
        data.extend_from_slice(&(moov_size as u32).to_be_bytes());
        data.extend_from_slice(b"moov");
        data.extend_from_slice(&udta_atom);

        data
    }

    #[test]
    fn test_validate_signature_valid_ftyp() {
        let data = create_test_quicktime_file();
        let reader = TestReader::new(data);
        assert!(validate_signature(&reader).is_ok());
    }

    #[test]
    fn test_validate_signature_invalid() {
        let data = vec![0x00; 100];
        let reader = TestReader::new(data);
        assert!(validate_signature(&reader).is_err());
    }

    #[test]
    fn test_parse_quicktime_user_data() {
        let data = create_test_quicktime_file();
        let reader = TestReader::new(data);

        let result = parse_quicktime_metadata(&reader);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        assert!(metadata.contains_key("QuickTime:Title"));

        if let Some(title) = metadata.get_string("QuickTime:Title") {
            assert_eq!(title, "Test Title");
        } else {
            panic!("Expected QuickTime:Title to be a string");
        }
    }

    #[test]
    fn test_parse_itunes_metadata() {
        let data = create_test_itunes_file();
        let reader = TestReader::new(data);

        let result = parse_quicktime_metadata(&reader);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        assert!(metadata.contains_key("iTunes:Artist"));

        if let Some(artist) = metadata.get_string("iTunes:Artist") {
            assert_eq!(artist, "Artist Name");
        } else {
            panic!("Expected iTunes:Artist to be a string");
        }
    }

    #[test]
    fn test_empty_file() {
        let reader = TestReader::new(vec![]);
        let result = parse_quicktime_metadata(&reader);
        assert!(result.is_err());
    }
}
