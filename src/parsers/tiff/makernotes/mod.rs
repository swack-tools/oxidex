//! MakerNote parsers for camera manufacturers

// Traditional camera manufacturers
pub mod canon;
pub mod canon_lens_database;
pub mod fujifilm;
pub mod fujifilm_lens_database;
pub mod leica;
pub mod leica_lens_database;
pub mod nikon;
pub mod nikon_lens_database;
pub mod olympus;
pub mod olympus_lens_database;
pub mod panasonic;
pub mod panasonic_lens_database;
pub mod pentax;
pub mod pentax_lens_database;
pub mod phaseone;
pub mod phaseone_lens_database;
pub mod shared;
pub mod sigma;
pub mod sigma_lens_database;
pub mod sony;
pub mod sony_lens_database;

// Smartphone manufacturers (Phase 3)
pub mod apple;
pub mod google;
pub mod microsoft;
pub mod qualcomm;
pub mod samsung;

// Legacy camera manufacturers (Phase 4)
pub mod casio;
pub mod ge;
pub mod hp;
pub mod jvc;
pub mod kodak;
pub mod leaf;
pub mod leaf_lens_database;
pub mod minolta;
pub mod minolta_lens_database;
pub mod motorola;
pub mod ricoh;
pub mod sanyo;

// Specialty devices (Phase 5)
pub mod dji; // DJI drones (Mavic, Phantom, Inspire)
pub mod flir; // FLIR thermal imaging cameras
pub mod gopro; // GoPro action cameras
pub mod infiray; // InfiRay thermal cameras
pub mod lytro; // Lytro light field cameras
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
