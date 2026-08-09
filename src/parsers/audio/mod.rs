//! Audio format parsers
//!
//! This module contains parsers for various audio formats.

#![allow(dead_code)]

pub mod aac;
pub mod aiff;
pub mod ape;
pub mod dss;
pub mod flac;
pub mod mp3;
pub mod ogg;
pub mod opus;
pub mod ram;
pub mod wav;

pub use aac::AacParser;
pub use aiff::AiffParser;
pub use ape::ApeParser;
pub use flac::FlacParser;
pub use mp3::Mp3Parser;
pub use ogg::OggParser;
pub use opus::OpusParser;
pub use wav::WavParser;
