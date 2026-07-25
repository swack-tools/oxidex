//! Video format parsers
//!
//! This module contains parsers for various video container formats.

#![allow(dead_code)]

pub mod asf;
pub mod avi;
pub mod flv;
pub mod mkv;
pub mod mp4;
pub mod mts;
pub mod mxf;
pub mod webm;

pub use asf::{AsfParser, parse_asf_metadata};
pub use avi::{AviParser, parse_avi_metadata};
pub use flv::{FlvParser, parse_flv_metadata};
pub use mkv::{MkvParser, parse_mkv_metadata};
pub use mp4::{Mp4Parser, parse_mp4_metadata};
pub use mts::{MtsParser, parse_mts_metadata};
pub use mxf::{MxfParser, parse_mxf_metadata};
pub use webm::{WebmParser, parse_webm_metadata};
