//! TIFF sub-reader for handling embedded TIFF data
//!
//! This module provides a FileReader wrapper that adjusts offsets to be relative
//! to a base offset. This is useful for parsing TIFF data embedded in other formats
//! (e.g., EXIF data in JPEG APP1 segments).

use crate::core::FileReader;

/// A FileReader wrapper that adjusts offsets relative to a base offset.
///
/// This is used when parsing TIFF structures embedded within other file formats,
/// where offsets in the TIFF data are relative to the start of the TIFF structure
/// rather than the start of the file.
pub struct TiffSubReader<'a> {
    reader: &'a dyn FileReader,
    base_offset: u64,
}

impl<'a> TiffSubReader<'a> {
    /// Creates a new TiffSubReader.
    ///
    /// # Arguments
    ///
    /// * `reader` - The underlying file reader
    /// * `base_offset` - The offset in the file where the TIFF data starts
    pub fn new(reader: &'a dyn FileReader, base_offset: u64) -> Self {
        Self {
            reader,
            base_offset,
        }
    }
}

impl<'a> FileReader for TiffSubReader<'a> {
    fn read(&self, offset: u64, length: usize) -> std::io::Result<&[u8]> {
        // Adjust offset to be relative to base
        self.reader.read(self.base_offset + offset, length)
    }

    fn size(&self) -> u64 {
        // Return size relative to base (remaining size from base to end)
        let total_size = self.reader.size();
        total_size.saturating_sub(self.base_offset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    struct MockReader {
        data: Vec<u8>,
    }

    impl FileReader for MockReader {
        fn read(&self, offset: u64, length: usize) -> io::Result<&[u8]> {
            let start = offset as usize;
            let end = start + length;
            if end > self.data.len() {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "EOF"));
            }
            Ok(&self.data[start..end])
        }

        fn size(&self) -> u64 {
            self.data.len() as u64
        }
    }

    #[test]
    fn test_tiff_subreader_offset_adjustment() {
        // Create a reader with 100 bytes
        let reader = MockReader {
            data: (0..100).collect(),
        };

        // Create a sub-reader starting at offset 20
        let sub_reader = TiffSubReader::new(&reader, 20);

        // Reading offset 5 from sub-reader should read offset 25 from main reader
        let data = sub_reader.read(5, 10).unwrap();
        assert_eq!(data, &[25, 26, 27, 28, 29, 30, 31, 32, 33, 34]);
    }

    #[test]
    fn test_tiff_subreader_size() {
        let reader = MockReader {
            data: (0..100).collect(),
        };

        // Sub-reader starting at offset 20 should report size as 80
        let sub_reader = TiffSubReader::new(&reader, 20);
        assert_eq!(sub_reader.size(), 80);

        // Sub-reader starting at offset 0 should report full size
        let sub_reader = TiffSubReader::new(&reader, 0);
        assert_eq!(sub_reader.size(), 100);
    }
}
