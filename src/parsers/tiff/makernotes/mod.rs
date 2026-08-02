//! MakerNote parsers for camera manufacturers

/// Where a MakerNote sits in its enclosing TIFF block, and how far a decoder
/// may read from it
pub mod makernote_context;

// Traditional camera manufacturers
pub mod canon;
pub mod canon_lens_database;
pub mod fujifilm;
pub mod leica;
pub mod nikon;
pub mod nikon_capture_data;
pub mod olympus;
pub mod panasonic;
pub mod pentax;
pub mod pentax_lens_database;
pub mod pentax_supplement;
pub mod phaseone;
pub mod registries;
pub mod shared;
pub mod sigma;
pub mod sony;
pub mod sony_lens_database;

// Smartphone manufacturers (Phase 3)
pub mod apple;
pub mod google;
pub mod microsoft;
pub mod samsung;

// (no `qualcomm` parser: fabricated tag table with no ExifTool source --
// see `registries::mod` for the finding. ExifTool's real Qualcomm.pm tables
// are read from JPEG APP7/APP4 segments, not a TIFF MakerNote IFD.)

// Legacy camera manufacturers (Phase 4)
pub mod casio;
pub mod ge;
pub mod hp;
pub mod jvc;
pub mod kodak;
pub mod lens_data;
pub mod minolta;
/// DSLR-A100 binary tables, generated from ExifTool's Minolta module
pub mod minolta_a100_tables;
pub mod minolta_lens_database;
/// Minolta binary-data tables shared with the Sony DSLR-A100
pub mod minolta_tables;
pub mod motorola;
pub mod ricoh;
pub mod sanyo;

// Specialty devices (Phase 5)
pub mod dji; // DJI drones (Mavic, Phantom, Inspire)
pub mod flir; // FLIR thermal imaging cameras
pub mod gopro; // GoPro action cameras
pub mod infiray; // InfiRay thermal cameras
pub mod nintendo; // Nintendo 3DS cameras
pub mod parrot; // Parrot drones (Anafi, Bebop)
pub mod reconyx; // Reconyx wildlife/trail cameras
pub mod red; // RED cinema cameras (KOMODO, V-RAPTOR)

// Software applications (Phase 6 - FINAL)
pub mod captureone; // Capture One Pro - styles, color grading, lens corrections
pub mod fotostation; // FotoStation/FotoWare - asset management, workflow
pub mod gimp; // GIMP - layers, filters, adjustments
pub mod indesign; // Adobe InDesign - document layout, embedded images
pub mod nikoncapture; // Nikon Capture NX-D - Picture Control, Active D-Lighting
pub mod photomechanic; // Photo Mechanic - IPTC workflow, keywords, ratings
pub mod photoshop; // Adobe Photoshop - layers, adjustments, filters
pub mod scalado; // Scalado - mobile photo editor, filters
