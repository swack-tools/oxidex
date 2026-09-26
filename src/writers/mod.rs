//! Infrastructure: Metadata serializers
//!
//! This module contains format-specific metadata writers and serializers.

#![allow(dead_code)]

// WriteExif's %crossDelete: an IFD0/ExifIFD set deletes the other copy.
pub mod atomic_writer;
pub(crate) mod exif_cross_delete;
// The mandatory entries of an ExifIFD a TIFF write creates.
pub(crate) mod exif_ifd_creation;
pub mod exif_inplace;
pub mod exif_surgical;
// Source-selected addresses remain internal until public file parity is proved.
pub(crate) mod generated_public_write;
pub(crate) mod generated_setnewvalue_address_rules;
pub(crate) mod generated_setnewvalue_public_migration_rules;
pub(crate) mod generated_write_address;
pub(crate) mod generated_write_dispatch;
// Shared source-derived helpers are not yet connected to public writes.
pub(crate) mod generated_convinv;
// Static source rows remain inactive until complete file parity is proved.
pub(crate) mod generated_convinv_rows;
pub(crate) mod generated_convinv_rules;
pub(crate) mod generated_sanitize;
pub(crate) mod generated_sanitize_rules;
// Source-derived new-directory defaults remain inactive until public creation routing lands.
pub(crate) mod generated_fresh_jpeg_byte_order;
pub(crate) mod generated_mandatory_defaults;
// Raw source properties are separate from displayed metadata values.
pub(crate) mod generated_raw_jfif;
#[allow(dead_code)]
pub(crate) mod generated_scalar;
pub(crate) mod generated_scalar_rules;
pub(crate) mod mandatory_defaults_runtime;
pub(crate) mod raw_segment_properties;
// Table validation composition remains inactive until the public write route lands.
pub(crate) mod generated_checkexif;
pub(crate) mod generated_checkexif_rules;
// Absolute trailer offsets (AFCP) re-based after a JPEG length change.
pub(crate) mod jpeg_trailer;
pub mod jpeg_writer;
pub mod pdf_writer;
pub mod png_writer;
// Source-selected final scalar stage remains internal until file parity is proved.
pub(crate) mod generated_tiff_scalar_final_rules;
pub(crate) mod tiff_scalar_final_stage;
pub mod tiff_surgical;
pub mod tiff_writer;
// Exif.pm ValueConvInv for 0x9c9b-0x9c9f, shared by every EXIF serializer.
pub(crate) mod xp_strings;

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
