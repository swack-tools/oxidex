//! Pentax lens database
//!
//! This module is a compatibility shim for the consolidated lens data.
//! Prefer using `oxidex::parsers::tiff::makernotes::lens_data::pentax` directly.

#![allow(missing_docs)]

use super::lens_data::pentax;

pub fn lookup_lens_name(lens_id: u16) -> Option<String> {
    pentax::lookup(lens_id).map(|s| s.to_string())
}

/// Looks up a Pentax lens name by its (series, sub_id) pair, as used by the
/// LensType tag (0x003f LensRec, and the LensType field of LensInfo/etc).
pub fn lookup_lens_type_pair(series: u8, sub_id: u16) -> Option<String> {
    pentax::lookup_lens_type(series, sub_id).map(|s| s.to_string())
}
