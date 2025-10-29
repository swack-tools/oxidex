//! TIFF format parser
//!
//! Handles Image File Directory (IFD) parsing, TIFF tag extraction, and maker notes.

#![allow(dead_code)]

pub mod ifd_parser;
pub mod tag_parser;
pub mod makernote_parser;
