//! Tag registry modules for MakerNote parsers
//!
//! This module contains TagRegistry definitions for each manufacturer,
//! providing declarative tag and array schema definitions.

// (no `google` registry: its 15 tag ids were invented -- none of the names it
// declared is a tag ExifTool reports for any Google file, and Google's own
// MakerNote is not the numeric IFD that registry assumed. It was never
// declared here, so it never compiled; see `makernotes::google` for the real
// parser. Likewise no `nikon` registry: `makernotes::nikon` and its submodules
// carry the real per-table id mapping, and the registry copy was never
// declared either.)
pub mod apple;
pub mod canon;
pub mod captureone; // Capture One migration complete (Batch 4, Task 4.2)
pub mod nikoncapture;

// Batch 1: Traditional Camera Manufacturers
pub mod fujifilm; // Fujifilm migration (Batch 1, Task 1.4)
// (no `leica` registry: it duplicated the Leica MakerNote parser's own tag
// dispatch under fabricated, non-ExifTool tag ids, was never called from
// anywhere but its own tests, and has been deleted -- see `makernotes::leica`
// for the real per-table id mapping.)
pub mod olympus; // Olympus migration (Batch 1, Task 1.1)
pub mod panasonic; // Panasonic migration (Batch 1, Task 1.2)
pub mod pentax; // Pentax migration (Batch 1, Task 1.3) // Leica migration (Batch 1, Task 1.5)

// Batch 2: Smartphone manufacturers
pub mod samsung; // Samsung migration complete (Batch 2, Task 2.2)

// (no `microsoft` registry: MakerNotes.pm has no MakerNoteMicrosoft TIFF-IFD
// dispatch entry -- ExifTool has no TIFF-IFD MakerNote path for Microsoft at
// all. Microsoft's only MakerNotes-group table (Microsoft::Stitch) is binary
// data read from EXIF tag 0x4748, not a MakerNote IFD. This registry's tag
// ids and names (AutoHDR, CreativeEffect, DynamicFlash, LensType,
// OpticalStabilization, PanoramaMode, PureViewMode, Refocus, RichCapture,
// RichCaptureMode, RichRecordingAudio, Video4K) appear in zero ExifTool
// source files -- see `makernotes::microsoft` deletion for the same finding.)

// (no `qualcomm` registry: ExifTool's Qualcomm.pm has no TIFF-IFD MakerNote
// table -- its two tables (`Main`, `DualCamera`) are string-id-keyed and
// reached only from JPEG APP7/APP4 segments, never from a Make="Qualcomm"
// MakerNote IFD. This registry's numeric ids and tag names (ClearSight,
// ChromaFlash, OptiZoom, ...) appear in zero ExifTool source files -- see
// `makernotes::qualcomm` deletion for the same finding.)

// Batch 3: Specialty Device Manufacturers
pub mod dji; // DJI migration complete (Batch 3, Task 3.1)
pub mod flir; // FLIR migration (Batch 3, Task 3.3)
pub mod gopro; // GoPro migration (Batch 3, Task 3.2)

// Batch 5: Legacy and Niche Manufacturers
// Sub-Batch 5.1: Traditional Camera Manufacturers
pub mod casio; // Casio migration (Batch 5, Sub-Batch 5.1)
pub mod kodak;
pub mod ricoh; // Ricoh migration (Batch 5, Sub-Batch 5.1)
// (no `sigma` registry: Sigma has one table, in `makernotes::sigma`, because a
// Sigma MakerNote's value offsets address the enclosing TIFF and so cannot be
// resolved from an id->name registry over the payload alone)

// Sub-Batch 5.2: Medium Format and Specialty Manufacturers
pub mod parrot;
pub mod phaseone; // Phase One migration (Batch 5, Sub-Batch 5.2)
pub mod red; // RED migration (Batch 5, Sub-Batch 5.2) // Parrot migration (Batch 5, Sub-Batch 5.2)

// Sub-Batch 5.3: Consumer and Specialty Manufacturers
pub mod ge;
pub mod hp;
pub mod infiray;
pub mod jvc;
pub mod motorola;
pub mod nintendo;
pub mod sanyo;

// Sub-Batch 5.4: Software and Post-Processing Applications
pub mod fotostation;
pub mod gimp;
pub mod indesign;
pub mod reconyx;
pub mod scalado;

pub use apple::apple_registry;
pub use canon::canon_registry;

// Batch 1 exports
pub use fujifilm::fujifilm_registry;
pub use olympus::olympus_registry;
pub use panasonic::panasonic_registry;
pub use pentax::pentax_registry;

// Batch 2 exports
pub use samsung::samsung_registry;

// Batch 3 exports
pub use dji::dji_registry;
pub use flir::flir_registry;
pub use gopro::gopro_registry;

// Batch 5 Sub-Batch 5.1 exports
pub use casio::casio_registry;
pub use kodak::kodak_registry;
pub use ricoh::ricoh_registry;

// Batch 5 Sub-Batch 5.2 exports
pub use parrot::parrot_registry;
pub use phaseone::{phaseone_tag_name, sensor_calibration_tag_name};
pub use red::red_registry;

// Batch 5 Sub-Batch 5.3 exports
pub use ge::ge_registry;
pub use hp::hp_registry;
pub use infiray::infiray_registry;
pub use jvc::jvc_registry;
pub use motorola::motorola_registry;
pub use nintendo::nintendo_registry;
pub use sanyo::sanyo_registry;

// Batch 5 Sub-Batch 5.4 exports
pub use fotostation::fotostation_registry;
pub use gimp::gimp_registry;
pub use indesign::indesign_registry;
pub use reconyx::reconyx_registry;
pub use scalado::scalado_registry;
