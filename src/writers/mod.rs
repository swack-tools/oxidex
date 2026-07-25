//! Infrastructure: Metadata serializers
//!
//! This module contains format-specific metadata writers and serializers.

#![allow(dead_code)]

pub mod atomic_writer;
pub mod exif_inplace;
pub mod exif_surgical;
pub mod jpeg_writer;
pub mod pdf_writer;
pub mod png_writer;
pub mod tiff_writer;

#[cfg(test)]
pub(crate) mod exif_surgical_test_support {
    /// Returns the TIFF slice of a JPEG's EXIF APP1 segment.
    pub fn tiff_slice(jpeg: &[u8]) -> &[u8] {
        // Minimal scan: find FFE1 whose payload starts with "Exif\0\0"
        let mut i = 2; // skip SOI
        while i + 4 <= jpeg.len() {
            let marker = u16::from_be_bytes([jpeg[i], jpeg[i + 1]]);
            let len = u16::from_be_bytes([jpeg[i + 2], jpeg[i + 3]]) as usize;
            let data = &jpeg[i + 4..i + 2 + len];
            if marker == 0xFFE1 && data.starts_with(b"Exif\0\0") {
                return &data[6..];
            }
            i += 2 + len;
        }
        panic!("no EXIF segment in test JPEG");
    }
}
