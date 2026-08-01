//! consolidated lens databases
//!
//! This file contains the lens databases for all manufacturers, consolidated
//! to reduce file clutter in the makernotes directory.

#![allow(missing_docs)]

use super::shared::{LensDatabase, StaticLensDb};

/// Canon lens database
pub mod canon {
    /// ExifTool's `%canonLensTypes` (Canon.pm:97), the `PrintConv` every Canon
    /// `LensType` tag shares -- `%Canon::CameraSettings` key 22, and the
    /// `LensType` field of all 32 `%Canon::CameraInfo*` tables.
    ///
    /// TRANSCRIBED BY SCRIPT from ExifTool's own in-memory Perl hash, not typed
    /// out by hand, and every name is additionally traced back to a literal
    /// `id => 'name',` line in Canon.pm before it is emitted.  The generator
    /// hard-errors on anything it has not seen -- a value that is a reference
    /// rather than a scalar, a key that is neither `N` nor `N.M`, a sub-entry
    /// whose base id is absent or whose `.1 .. .N` run has a hole, a key that
    /// does not fit the int16u the tag is read as, or a name with no source
    /// line -- rather than dropping it silently.
    ///
    /// # Ambiguous ids
    ///
    /// 71 of these ids are shared by several lenses.  ExifTool stores the
    /// alternatives under fractional keys (`2.1`, `33.14`, `61182.68`, ...) and
    /// only `Composite:LensID` consults them: `Canon::PrintLensID` narrows the
    /// list by focal length and maximum aperture, which needs MinFocalLength,
    /// MaxFocalLength, MaxAperture and LensModel from the same file.
    ///
    /// `Canon:LensType` -- the tag in this group, and the only one oxidex
    /// emits -- is a plain hash lookup with no such narrowing, so ExifTool
    /// prints the combined string held at the integer key verbatim: id 2
    /// prints "Canon EF 28mm f/2.8 or Sigma Lens", not the Sigma 24mm filed
    /// under 2.1, and id 61182 prints "Canon RF 50mm F1.2L USM or other Canon
    /// RF Lens" rather than one of the 68 RF lenses filed under it.  Confirmed
    /// against the corpus, where every ambiguous id that occurs prints its
    /// combined string.  The 295
    /// fractional keys are therefore deliberately not transcribed; they belong
    /// with `Composite:LensID`, which oxidex does not implement.
    ///
    /// -1 and 65535 both carry "n/a" in ExifTool; the generator refuses to
    /// emit the table if that ever stops being true, because an int16u read
    /// can only ever reach 65535.
    #[rustfmt::skip]
    pub static CANON_LENS_TYPES: [(i64, &str); 239] = [
        (-1, "n/a"),
        (1, "Canon EF 50mm f/1.8"),
        (2, "Canon EF 28mm f/2.8 or Sigma Lens"),
        (3, "Canon EF 135mm f/2.8 Soft"),
        (4, "Canon EF 35-105mm f/3.5-4.5 or Sigma Lens"),
        (5, "Canon EF 35-70mm f/3.5-4.5"),
        (6, "Canon EF 28-70mm f/3.5-4.5 or Sigma or Tokina Lens"),
        (7, "Canon EF 100-300mm f/5.6L"),
        (8, "Canon EF 100-300mm f/5.6 or Sigma or Tokina Lens"),
        (9, "Canon EF 70-210mm f/4"),
        (10, "Canon EF 50mm f/2.5 Macro or Sigma Lens"),
        (11, "Canon EF 35mm f/2"),
        (13, "Canon EF 15mm f/2.8 Fisheye"),
        (14, "Canon EF 50-200mm f/3.5-4.5L"),
        (15, "Canon EF 50-200mm f/3.5-4.5"),
        (16, "Canon EF 35-135mm f/3.5-4.5"),
        (17, "Canon EF 35-70mm f/3.5-4.5A"),
        (18, "Canon EF 28-70mm f/3.5-4.5"),
        (20, "Canon EF 100-200mm f/4.5A"),
        (21, "Canon EF 80-200mm f/2.8L"),
        (22, "Canon EF 20-35mm f/2.8L or Tokina Lens"),
        (23, "Canon EF 35-105mm f/3.5-4.5"),
        (24, "Canon EF 35-80mm f/4-5.6 Power Zoom"),
        (25, "Canon EF 35-80mm f/4-5.6 Power Zoom"),
        (26, "Canon EF 100mm f/2.8 Macro or Other Lens"),
        (27, "Canon EF 35-80mm f/4-5.6"),
        (28, "Canon EF 80-200mm f/4.5-5.6 or Tamron Lens"),
        (29, "Canon EF 50mm f/1.8 II"),
        (30, "Canon EF 35-105mm f/4.5-5.6"),
        (31, "Canon EF 75-300mm f/4-5.6 or Tamron Lens"),
        (32, "Canon EF 24mm f/2.8 or Sigma Lens"),
        (33, "Voigtlander or Carl Zeiss Lens"),
        (35, "Canon EF 35-80mm f/4-5.6"),
        (36, "Canon EF 38-76mm f/4.5-5.6"),
        (37, "Canon EF 35-80mm f/4-5.6 or Tamron Lens"),
        (38, "Canon EF 80-200mm f/4.5-5.6 II"),
        (39, "Canon EF 75-300mm f/4-5.6"),
        (40, "Canon EF 28-80mm f/3.5-5.6"),
        (41, "Canon EF 28-90mm f/4-5.6"),
        (42, "Canon EF 28-200mm f/3.5-5.6 or Tamron Lens"),
        (43, "Canon EF 28-105mm f/4-5.6"),
        (44, "Canon EF 90-300mm f/4.5-5.6"),
        (45, "Canon EF-S 18-55mm f/3.5-5.6 [II]"),
        (46, "Canon EF 28-90mm f/4-5.6"),
        (47, "Zeiss Milvus 35mm f/2 or 50mm f/2"),
        (48, "Canon EF-S 18-55mm f/3.5-5.6 IS"),
        (49, "Canon EF-S 55-250mm f/4-5.6 IS"),
        (50, "Canon EF-S 18-200mm f/3.5-5.6 IS"),
        (51, "Canon EF-S 18-135mm f/3.5-5.6 IS"),
        (52, "Canon EF-S 18-55mm f/3.5-5.6 IS II"),
        (53, "Canon EF-S 18-55mm f/3.5-5.6 III"),
        (54, "Canon EF-S 55-250mm f/4-5.6 IS II"),
        (60, "Irix 11mm f/4 or 15mm f/2.4"),
        (63, "Irix 30mm F1.4 Dragonfly"),
        (80, "Canon TS-E 50mm f/2.8L Macro"),
        (81, "Canon TS-E 90mm f/2.8L Macro"),
        (82, "Canon TS-E 135mm f/4L Macro"),
        (94, "Canon TS-E 17mm f/4L"),
        (95, "Canon TS-E 24mm f/3.5L II"),
        (103, "Samyang AF 14mm f/2.8 EF or Rokinon Lens"),
        (106, "Rokinon SP / Samyang XP 35mm f/1.2"),
        (112, "Sigma 28mm f/1.5 FF High-speed Prime or other Sigma Lens"),
        (117, "Tamron 35-150mm f/2.8-4.0 Di VC OSD (A043) or other Tamron Lens"),
        (124, "Canon MP-E 65mm f/2.8 1-5x Macro Photo"),
        (125, "Canon TS-E 24mm f/3.5L"),
        (126, "Canon TS-E 45mm f/2.8"),
        (127, "Canon TS-E 90mm f/2.8 or Tamron Lens"),
        (129, "Canon EF 300mm f/2.8L USM"),
        (130, "Canon EF 50mm f/1.0L USM"),
        (131, "Canon EF 28-80mm f/2.8-4L USM or Sigma Lens"),
        (132, "Canon EF 1200mm f/5.6L USM"),
        (134, "Canon EF 600mm f/4L IS USM"),
        (135, "Canon EF 200mm f/1.8L USM"),
        (136, "Canon EF 300mm f/2.8L USM"),
        (137, "Canon EF 85mm f/1.2L USM or Sigma or Tamron Lens"),
        (138, "Canon EF 28-80mm f/2.8-4L"),
        (139, "Canon EF 400mm f/2.8L USM"),
        (140, "Canon EF 500mm f/4.5L USM"),
        (141, "Canon EF 500mm f/4.5L USM"),
        (142, "Canon EF 300mm f/2.8L IS USM"),
        (143, "Canon EF 500mm f/4L IS USM or Sigma Lens"),
        (144, "Canon EF 35-135mm f/4-5.6 USM"),
        (145, "Canon EF 100-300mm f/4.5-5.6 USM"),
        (146, "Canon EF 70-210mm f/3.5-4.5 USM"),
        (147, "Canon EF 35-135mm f/4-5.6 USM"),
        (148, "Canon EF 28-80mm f/3.5-5.6 USM"),
        (149, "Canon EF 100mm f/2 USM"),
        (150, "Canon EF 14mm f/2.8L USM or Sigma Lens"),
        (151, "Canon EF 200mm f/2.8L USM"),
        (152, "Canon EF 300mm f/4L IS USM or Sigma Lens"),
        (153, "Canon EF 35-350mm f/3.5-5.6L USM or Sigma or Tamron Lens"),
        (154, "Canon EF 20mm f/2.8 USM or Zeiss Lens"),
        (155, "Canon EF 85mm f/1.8 USM or Sigma Lens"),
        (156, "Canon EF 28-105mm f/3.5-4.5 USM or Tamron Lens"),
        (160, "Canon EF 20-35mm f/3.5-4.5 USM or Tamron or Tokina Lens"),
        (161, "Canon EF 28-70mm f/2.8L USM or Other Lens"),
        (162, "Canon EF 200mm f/2.8L USM"),
        (163, "Canon EF 300mm f/4L"),
        (164, "Canon EF 400mm f/5.6L"),
        (165, "Canon EF 70-200mm f/2.8L USM"),
        (166, "Canon EF 70-200mm f/2.8L USM + 1.4x"),
        (167, "Canon EF 70-200mm f/2.8L USM + 2x"),
        (168, "Canon EF 28mm f/1.8 USM or Sigma Lens"),
        (169, "Canon EF 17-35mm f/2.8L USM or Sigma Lens"),
        (170, "Canon EF 200mm f/2.8L II USM or Sigma Lens"),
        (171, "Canon EF 300mm f/4L USM"),
        (172, "Canon EF 400mm f/5.6L USM or Sigma Lens"),
        (173, "Canon EF 180mm Macro f/3.5L USM or Sigma Lens"),
        (174, "Canon EF 135mm f/2L USM or Other Lens"),
        (175, "Canon EF 400mm f/2.8L USM"),
        (176, "Canon EF 24-85mm f/3.5-4.5 USM"),
        (177, "Canon EF 300mm f/4L IS USM"),
        (178, "Canon EF 28-135mm f/3.5-5.6 IS"),
        (179, "Canon EF 24mm f/1.4L USM"),
        (180, "Canon EF 35mm f/1.4L USM or Other Lens"),
        (181, "Canon EF 100-400mm f/4.5-5.6L IS USM + 1.4x or Sigma Lens"),
        (182, "Canon EF 100-400mm f/4.5-5.6L IS USM + 2x or Sigma Lens"),
        (183, "Canon EF 100-400mm f/4.5-5.6L IS USM or Sigma Lens"),
        (184, "Canon EF 400mm f/2.8L USM + 2x"),
        (185, "Canon EF 600mm f/4L IS USM"),
        (186, "Canon EF 70-200mm f/4L USM"),
        (187, "Canon EF 70-200mm f/4L USM + 1.4x"),
        (188, "Canon EF 70-200mm f/4L USM + 2x"),
        (189, "Canon EF 70-200mm f/4L USM + 2.8x"),
        (190, "Canon EF 100mm f/2.8 Macro USM"),
        (191, "Canon EF 400mm f/4 DO IS or Sigma Lens"),
        (193, "Canon EF 35-80mm f/4-5.6 USM"),
        (194, "Canon EF 80-200mm f/4.5-5.6 USM"),
        (195, "Canon EF 35-105mm f/4.5-5.6 USM"),
        (196, "Canon EF 75-300mm f/4-5.6 USM"),
        (197, "Canon EF 75-300mm f/4-5.6 IS USM or Sigma Lens"),
        (198, "Canon EF 50mm f/1.4 USM or Other Lens"),
        (199, "Canon EF 28-80mm f/3.5-5.6 USM"),
        (200, "Canon EF 75-300mm f/4-5.6 USM"),
        (201, "Canon EF 28-80mm f/3.5-5.6 USM"),
        (202, "Canon EF 28-80mm f/3.5-5.6 USM IV"),
        (208, "Canon EF 22-55mm f/4-5.6 USM"),
        (209, "Canon EF 55-200mm f/4.5-5.6"),
        (210, "Canon EF 28-90mm f/4-5.6 USM"),
        (211, "Canon EF 28-200mm f/3.5-5.6 USM"),
        (212, "Canon EF 28-105mm f/4-5.6 USM"),
        (213, "Canon EF 90-300mm f/4.5-5.6 USM or Tamron Lens"),
        (214, "Canon EF-S 18-55mm f/3.5-5.6 USM"),
        (215, "Canon EF 55-200mm f/4.5-5.6 II USM"),
        (217, "Tamron AF 18-270mm f/3.5-6.3 Di II VC PZD"),
        (220, "Yongnuo YN 50mm f/1.8"),
        (224, "Canon EF 70-200mm f/2.8L IS USM"),
        (225, "Canon EF 70-200mm f/2.8L IS USM + 1.4x"),
        (226, "Canon EF 70-200mm f/2.8L IS USM + 2x"),
        (227, "Canon EF 70-200mm f/2.8L IS USM + 2.8x"),
        (228, "Canon EF 28-105mm f/3.5-4.5 USM"),
        (229, "Canon EF 16-35mm f/2.8L USM"),
        (230, "Canon EF 24-70mm f/2.8L USM"),
        (231, "Canon EF 17-40mm f/4L USM or Sigma Lens"),
        (232, "Canon EF 70-300mm f/4.5-5.6 DO IS USM"),
        (233, "Canon EF 28-300mm f/3.5-5.6L IS USM"),
        (234, "Canon EF-S 17-85mm f/4-5.6 IS USM or Tokina Lens"),
        (235, "Canon EF-S 10-22mm f/3.5-4.5 USM"),
        (236, "Canon EF-S 60mm f/2.8 Macro USM"),
        (237, "Canon EF 24-105mm f/4L IS USM"),
        (238, "Canon EF 70-300mm f/4-5.6 IS USM"),
        (239, "Canon EF 85mm f/1.2L II USM or Rokinon Lens"),
        (240, "Canon EF-S 17-55mm f/2.8 IS USM or Sigma Lens"),
        (241, "Canon EF 50mm f/1.2L USM"),
        (242, "Canon EF 70-200mm f/4L IS USM"),
        (243, "Canon EF 70-200mm f/4L IS USM + 1.4x"),
        (244, "Canon EF 70-200mm f/4L IS USM + 2x"),
        (245, "Canon EF 70-200mm f/4L IS USM + 2.8x"),
        (246, "Canon EF 16-35mm f/2.8L II USM"),
        (247, "Canon EF 14mm f/2.8L II USM"),
        (248, "Canon EF 200mm f/2L IS USM or Sigma Lens"),
        (249, "Canon EF 800mm f/5.6L IS USM"),
        (250, "Canon EF 24mm f/1.4L II USM or Sigma Lens"),
        (251, "Canon EF 70-200mm f/2.8L IS II USM"),
        (252, "Canon EF 70-200mm f/2.8L IS II USM + 1.4x"),
        (253, "Canon EF 70-200mm f/2.8L IS II USM + 2x"),
        (254, "Canon EF 100mm f/2.8L Macro IS USM or Tamron Lens"),
        (255, "Sigma 24-105mm f/4 DG OS HSM | A or Other Lens"),
        (368, "Sigma 14-24mm f/2.8 DG HSM | A or other Sigma Lens"),
        (488, "Canon EF-S 15-85mm f/3.5-5.6 IS USM"),
        (489, "Canon EF 70-300mm f/4-5.6L IS USM"),
        (490, "Canon EF 8-15mm f/4L Fisheye USM"),
        (491, "Canon EF 300mm f/2.8L IS II USM or Tamron Lens"),
        (492, "Canon EF 400mm f/2.8L IS II USM"),
        (493, "Canon EF 500mm f/4L IS II USM or EF 24-105mm f4L IS USM"),
        (494, "Canon EF 600mm f/4L IS II USM"),
        (495, "Canon EF 24-70mm f/2.8L II USM or Sigma Lens"),
        (496, "Canon EF 200-400mm f/4L IS USM"),
        (499, "Canon EF 200-400mm f/4L IS USM + 1.4x"),
        (502, "Canon EF 28mm f/2.8 IS USM or Tamron Lens"),
        (503, "Canon EF 24mm f/2.8 IS USM"),
        (504, "Canon EF 24-70mm f/4L IS USM"),
        (505, "Canon EF 35mm f/2 IS USM"),
        (506, "Canon EF 400mm f/4 DO IS II USM"),
        (507, "Canon EF 16-35mm f/4L IS USM"),
        (508, "Canon EF 11-24mm f/4L USM or Tamron Lens"),
        (624, "Sigma 70-200mm f/2.8 DG OS HSM | S or other Sigma Lens"),
        (747, "Canon EF 100-400mm f/4.5-5.6L IS II USM or Tamron Lens"),
        (748, "Canon EF 100-400mm f/4.5-5.6L IS II USM + 1.4x or Tamron Lens"),
        (749, "Canon EF 100-400mm f/4.5-5.6L IS II USM + 2x or Tamron Lens"),
        (750, "Canon EF 35mm f/1.4L II USM or Tamron Lens"),
        (751, "Canon EF 16-35mm f/2.8L III USM"),
        (752, "Canon EF 24-105mm f/4L IS II USM"),
        (753, "Canon EF 85mm f/1.4L IS USM"),
        (754, "Canon EF 70-200mm f/4L IS II USM"),
        (757, "Canon EF 400mm f/2.8L IS III USM"),
        (758, "Canon EF 600mm f/4L IS III USM"),
        (923, "Meike/SKY 85mm f/1.8 DCM"),
        (1136, "Sigma 24-70mm f/2.8 DG OS HSM | A"),
        (4142, "Canon EF-S 18-135mm f/3.5-5.6 IS STM"),
        (4143, "Canon EF-M 18-55mm f/3.5-5.6 IS STM or Tamron Lens"),
        (4144, "Canon EF 40mm f/2.8 STM"),
        (4145, "Canon EF-M 22mm f/2 STM"),
        (4146, "Canon EF-S 18-55mm f/3.5-5.6 IS STM"),
        (4147, "Canon EF-M 11-22mm f/4-5.6 IS STM"),
        (4148, "Canon EF-S 55-250mm f/4-5.6 IS STM"),
        (4149, "Canon EF-M 55-200mm f/4.5-6.3 IS STM"),
        (4150, "Canon EF-S 10-18mm f/4.5-5.6 IS STM"),
        (4152, "Canon EF 24-105mm f/3.5-5.6 IS STM"),
        (4153, "Canon EF-M 15-45mm f/3.5-6.3 IS STM"),
        (4154, "Canon EF-S 24mm f/2.8 STM"),
        (4155, "Canon EF-M 28mm f/3.5 Macro IS STM"),
        (4156, "Canon EF 50mm f/1.8 STM"),
        (4157, "Canon EF-M 18-150mm f/3.5-6.3 IS STM"),
        (4158, "Canon EF-S 18-55mm f/4-5.6 IS STM"),
        (4159, "Canon EF-M 32mm f/1.4 STM"),
        (4160, "Canon EF-S 35mm f/2.8 Macro IS STM"),
        (4208, "Sigma 56mm f/1.4 DC DN | C or other Sigma Lens"),
        (4976, "Sigma 16-300mm F3.5-6.7 DC OS | C (025)"),
        (6512, "Sigma 12mm F1.4 DC | C"),
        (36910, "Canon EF 70-300mm f/4-5.6 IS II USM"),
        (36912, "Canon EF-S 18-135mm f/3.5-5.6 IS USM"),
        (61182, "Canon RF 50mm F1.2L USM or other Canon RF Lens"),
        (61491, "Canon CN-E 14mm T3.1 L F"),
        (61492, "Canon CN-E 24mm T1.5 L F"),
        (61494, "Canon CN-E 85mm T1.3 L F"),
        (61495, "Canon CN-E 135mm T2.2 L F"),
        (61496, "Canon CN-E 35mm T1.5 L F"),
        (65535, "n/a"),
    ];

    /// Looks up a lens name from a Canon `LensType` value.
    ///
    /// The value is read as int16u wherever ExifTool uses this table, so 65535
    /// -- not -1 -- is the "n/a" key a real file can carry.
    pub fn lookup(lens_id: u16) -> Option<&'static str> {
        CANON_LENS_TYPES
            .binary_search_by_key(&i64::from(lens_id), |&(id, _)| id)
            .ok()
            .map(|i| CANON_LENS_TYPES[i].1)
    }
}

/// Sony lens database
///
/// `%Image::ExifTool::Sony::sonyLensTypes`, the A-mount `LensType` table shared
/// with Minolta. Generated from ExifTool rather than transcribed: the names
/// carry ExifTool's exact spelling ("F3.5-4.5", not "f/3.5-4.5") because a
/// comparison against its output is character-for-character.
///
/// E-mount bodies report a different table (`sonyLensTypes2`) under a
/// different tag, so an id here must not be resolved for those.
pub mod sony {
    use super::*;

    /// Lens id to name, as ExifTool's `PrintConv` for Sony `LensType`.
    pub static SONY_LENSES: [(u16, &str); 242] = [
        (0, "Minolta AF 28-85mm F3.5-4.5 New"),
        (1, "Minolta AF 80-200mm F2.8 HS-APO G"),
        (2, "Minolta AF 28-70mm F2.8 G"),
        (3, "Minolta AF 28-80mm F4-5.6"),
        (4, "Minolta AF 85mm F1.4G"),
        (5, "Minolta AF 35-70mm F3.5-4.5 [II]"),
        (6, "Minolta AF 24-85mm F3.5-4.5 [New]"),
        (
            7,
            "Minolta AF 100-300mm F4.5-5.6 APO [New] or 100-400mm or Sigma Lens",
        ),
        (8, "Minolta AF 70-210mm F4.5-5.6 [II]"),
        (9, "Minolta AF 50mm F3.5 Macro"),
        (10, "Minolta AF 28-105mm F3.5-4.5 [New]"),
        (11, "Minolta AF 300mm F4 HS-APO G"),
        (12, "Minolta AF 100mm F2.8 Soft Focus"),
        (13, "Minolta AF 75-300mm F4.5-5.6 (New or II)"),
        (14, "Minolta AF 100-400mm F4.5-6.7 APO"),
        (15, "Minolta AF 400mm F4.5 HS-APO G"),
        (16, "Minolta AF 17-35mm F3.5 G"),
        (17, "Minolta AF 20-35mm F3.5-4.5"),
        (18, "Minolta AF 28-80mm F3.5-5.6 II"),
        (19, "Minolta AF 35mm F1.4 G"),
        (20, "Minolta/Sony 135mm F2.8 [T4.5] STF"),
        (22, "Minolta AF 35-80mm F4-5.6 II"),
        (23, "Minolta AF 200mm F4 Macro APO G"),
        (
            24,
            "Minolta/Sony AF 24-105mm F3.5-4.5 (D) or Sigma or Tamron Lens",
        ),
        (25, "Minolta AF 100-300mm F4.5-5.6 APO (D) or Sigma Lens"),
        (27, "Minolta AF 85mm F1.4 G (D)"),
        (28, "Minolta/Sony AF 100mm F2.8 Macro (D) or Tamron Lens"),
        (29, "Minolta/Sony AF 75-300mm F4.5-5.6 (D)"),
        (30, "Minolta AF 28-80mm F3.5-5.6 (D) or Sigma Lens"),
        (31, "Minolta/Sony AF 50mm F2.8 Macro (D) or F3.5"),
        (32, "Minolta/Sony AF 300mm F2.8 G or 1.5x Teleconverter"),
        (33, "Minolta/Sony AF 70-200mm F2.8 G"),
        (35, "Minolta AF 85mm F1.4 G (D) Limited"),
        (36, "Minolta AF 28-100mm F3.5-5.6 (D)"),
        (38, "Minolta AF 17-35mm F2.8-4 (D)"),
        (39, "Minolta AF 28-75mm F2.8 (D)"),
        (40, "Minolta/Sony AF DT 18-70mm F3.5-5.6 (D)"),
        (41, "Minolta/Sony AF DT 11-18mm F4.5-5.6 (D) or Tamron Lens"),
        (42, "Minolta/Sony AF DT 18-200mm F3.5-6.3 (D)"),
        (43, "Sony 35mm F1.4 G (SAL35F14G)"),
        (44, "Sony 50mm F1.4 (SAL50F14)"),
        (45, "Carl Zeiss Planar T* 85mm F1.4 ZA (SAL85F14Z)"),
        (
            46,
            "Carl Zeiss Vario-Sonnar T* DT 16-80mm F3.5-4.5 ZA (SAL1680Z)",
        ),
        (47, "Carl Zeiss Sonnar T* 135mm F1.8 ZA (SAL135F18Z)"),
        (
            48,
            "Carl Zeiss Vario-Sonnar T* 24-70mm F2.8 ZA SSM (SAL2470Z) or Other Lens",
        ),
        (49, "Sony DT 55-200mm F4-5.6 (SAL55200)"),
        (50, "Sony DT 18-250mm F3.5-6.3 (SAL18250)"),
        (51, "Sony DT 16-105mm F3.5-5.6 (SAL16105)"),
        (
            52,
            "Sony 70-300mm F4.5-5.6 G SSM (SAL70300G) or G SSM II or Tamron Lens",
        ),
        (53, "Sony 70-400mm F4-5.6 G SSM (SAL70400G)"),
        (
            54,
            "Carl Zeiss Vario-Sonnar T* 16-35mm F2.8 ZA SSM (SAL1635Z) or ZA SSM II",
        ),
        (55, "Sony DT 18-55mm F3.5-5.6 SAM (SAL1855) or SAM II"),
        (56, "Sony DT 55-200mm F4-5.6 SAM (SAL55200-2)"),
        (
            57,
            "Sony DT 50mm F1.8 SAM (SAL50F18) or Tamron Lens or Commlite CM-EF-NEX adapter",
        ),
        (58, "Sony DT 30mm F2.8 Macro SAM (SAL30M28)"),
        (59, "Sony 28-75mm F2.8 SAM (SAL2875)"),
        (60, "Carl Zeiss Distagon T* 24mm F2 ZA SSM (SAL24F20Z)"),
        (61, "Sony 85mm F2.8 SAM (SAL85F28)"),
        (62, "Sony DT 35mm F1.8 SAM (SAL35F18)"),
        (63, "Sony DT 16-50mm F2.8 SSM (SAL1650)"),
        (64, "Sony 500mm F4 G SSM (SAL500F40G)"),
        (65, "Sony DT 18-135mm F3.5-5.6 SAM (SAL18135)"),
        (66, "Sony 300mm F2.8 G SSM II (SAL300F28G2)"),
        (67, "Sony 70-200mm F2.8 G SSM II (SAL70200G2)"),
        (68, "Sony DT 55-300mm F4.5-5.6 SAM (SAL55300)"),
        (69, "Sony 70-400mm F4-5.6 G SSM II (SAL70400G2)"),
        (70, "Carl Zeiss Planar T* 50mm F1.4 ZA SSM (SAL50F14Z)"),
        (128, "Tamron or Sigma Lens (128)"),
        (129, "Tamron Lens (129)"),
        (131, "Tamron 20-40mm F2.7-3.5 SP Aspherical IF"),
        (135, "Vivitar 28-210mm F3.5-5.6"),
        (136, "Tokina EMZ M100 AF 100mm F3.5"),
        (137, "Cosina 70-210mm F2.8-4 AF"),
        (138, "Soligor 19-35mm F3.5-4.5"),
        (139, "Tokina AF 28-300mm F4-6.3"),
        (142, "Cosina AF 70-300mm F4.5-5.6 MC"),
        (146, "Voigtlander Macro APO-Lanthar 125mm F2.5 SL"),
        (194, "Tamron SP AF 17-50mm F2.8 XR Di II LD Aspherical [IF]"),
        (202, "Tamron SP AF 70-200mm F2.8 Di LD [IF] Macro"),
        (203, "Tamron SP 70-200mm F2.8 Di USD"),
        (204, "Tamron SP 24-70mm F2.8 Di USD"),
        (212, "Tamron 28-300mm F3.5-6.3 Di PZD"),
        (213, "Tamron 16-300mm F3.5-6.3 Di II PZD Macro"),
        (214, "Tamron SP 150-600mm F5-6.3 Di USD"),
        (215, "Tamron SP 15-30mm F2.8 Di USD"),
        (216, "Tamron SP 45mm F1.8 Di USD"),
        (217, "Tamron SP 35mm F1.8 Di USD"),
        (218, "Tamron SP 90mm F2.8 Di Macro 1:1 USD (F017)"),
        (220, "Tamron SP 150-600mm F5-6.3 Di USD G2"),
        (224, "Tamron SP 90mm F2.8 Di Macro 1:1 USD (F004)"),
        (255, "Tamron Lens (255)"),
        (
            1868,
            "Sigma MC-11 SA-E Mount Converter with not-supported Sigma lens",
        ),
        (2550, "Minolta AF 50mm F1.7"),
        (2551, "Minolta AF 35-70mm F4 or Other Lens"),
        (2552, "Minolta AF 28-85mm F3.5-4.5 or Other Lens"),
        (2553, "Minolta AF 28-135mm F4-4.5 or Other Lens"),
        (2554, "Minolta AF 35-105mm F3.5-4.5"),
        (2555, "Minolta AF 70-210mm F4 Macro or Sigma Lens"),
        (2556, "Minolta AF 135mm F2.8"),
        (2557, "Minolta/Sony AF 28mm F2.8"),
        (2558, "Minolta AF 24-50mm F4"),
        (2560, "Minolta AF 100-200mm F4.5"),
        (2561, "Minolta AF 75-300mm F4.5-5.6 or Sigma Lens"),
        (2562, "Minolta AF 50mm F1.4 [New]"),
        (2563, "Minolta AF 300mm F2.8 APO or Sigma Lens"),
        (2564, "Minolta AF 50mm F2.8 Macro or Sigma Lens"),
        (2565, "Minolta AF 600mm F4 APO"),
        (2566, "Minolta AF 24mm F2.8 or Sigma Lens"),
        (2572, "Minolta/Sony AF 500mm F8 Reflex"),
        (2578, "Minolta/Sony AF 16mm F2.8 Fisheye or Sigma Lens"),
        (2579, "Minolta/Sony AF 20mm F2.8 or Tokina Lens"),
        (
            2581,
            "Minolta AF 100mm F2.8 Macro [New] or Sigma or Tamron Lens",
        ),
        (2585, "Minolta AF 35-105mm F3.5-4.5 New or Tamron Lens"),
        (2588, "Minolta AF 70-210mm F3.5-4.5"),
        (2589, "Minolta AF 80-200mm F2.8 APO or Tokina Lens"),
        (
            2590,
            "Minolta AF 200mm F2.8 G APO + Minolta AF 1.4x APO or Other Lens + 1.4x",
        ),
        (2591, "Minolta AF 35mm F1.4"),
        (2592, "Minolta AF 85mm F1.4 G (D)"),
        (2593, "Minolta AF 200mm F2.8 APO"),
        (2594, "Minolta AF 3x-1x F1.7-2.8 Macro"),
        (2596, "Minolta AF 28mm F2"),
        (2597, "Minolta AF 35mm F2 [New]"),
        (2598, "Minolta AF 100mm F2"),
        (
            2601,
            "Minolta AF 200mm F2.8 G APO + Minolta AF 2x APO or Other Lens + 2x",
        ),
        (2604, "Minolta AF 80-200mm F4.5-5.6"),
        (2605, "Minolta AF 35-80mm F4-5.6"),
        (2606, "Minolta AF 100-300mm F4.5-5.6"),
        (2607, "Minolta AF 35-80mm F4-5.6"),
        (2608, "Minolta AF 300mm F2.8 HS-APO G"),
        (2609, "Minolta AF 600mm F4 HS-APO G"),
        (2612, "Minolta AF 200mm F2.8 HS-APO G"),
        (2613, "Minolta AF 50mm F1.7 New"),
        (2615, "Minolta AF 28-105mm F3.5-4.5 xi"),
        (2616, "Minolta AF 35-200mm F4.5-5.6 xi"),
        (2618, "Minolta AF 28-80mm F4-5.6 xi"),
        (2619, "Minolta AF 80-200mm F4.5-5.6 xi"),
        (2620, "Minolta AF 28-70mm F2.8 G"),
        (2621, "Minolta AF 100-300mm F4.5-5.6 xi"),
        (2624, "Minolta AF 35-80mm F4-5.6 Power Zoom"),
        (2628, "Minolta AF 80-200mm F2.8 HS-APO G"),
        (2629, "Minolta AF 85mm F1.4 New"),
        (2631, "Minolta AF 100-300mm F4.5-5.6 APO"),
        (2632, "Minolta AF 24-50mm F4 New"),
        (2638, "Minolta AF 50mm F2.8 Macro New"),
        (2639, "Minolta AF 100mm F2.8 Macro"),
        (2641, "Minolta/Sony AF 20mm F2.8 New"),
        (2642, "Minolta AF 24mm F2.8 New"),
        (2644, "Minolta AF 100-400mm F4.5-6.7 APO"),
        (2662, "Minolta AF 50mm F1.4 New"),
        (2667, "Minolta AF 35mm F2 New"),
        (2668, "Minolta AF 28mm F2 New"),
        (2672, "Minolta AF 24-105mm F3.5-4.5 (D)"),
        (3046, "Metabones Canon EF Speed Booster"),
        (4567, "Tokina 70-210mm F4-5.6"),
        (4568, "Tokina AF 35-200mm F4-5.6 Zoom SD"),
        (4570, "Tamron AF 35-135mm F3.5-4.5"),
        (4571, "Vivitar 70-210mm F4.5-5.6"),
        (4574, "2x Teleconverter or Tamron or Tokina Lens"),
        (4575, "1.4x Teleconverter"),
        (4585, "Tamron SP AF 300mm F2.8 LD IF"),
        (4586, "Tamron SP AF 35-105mm F2.8 LD Aspherical IF"),
        (4587, "Tamron AF 70-210mm F2.8 SP LD"),
        (4812, "Metabones Canon EF Speed Booster Ultra"),
        (6118, "Canon EF Adapter"),
        (6528, "Sigma 16mm F2.8 Filtermatic Fisheye"),
        (6553, "E-Mount, T-Mount, Other Lens or no lens"),
        (
            18688,
            "Sigma MC-11 SA-E Mount Converter with not-supported Sigma lens",
        ),
        (25501, "Minolta AF 50mm F1.7"),
        (25511, "Minolta AF 35-70mm F4 or Other Lens"),
        (25521, "Minolta AF 28-85mm F3.5-4.5 or Other Lens"),
        (25531, "Minolta AF 28-135mm F4-4.5 or Other Lens"),
        (25541, "Minolta AF 35-105mm F3.5-4.5"),
        (25551, "Minolta AF 70-210mm F4 Macro or Sigma Lens"),
        (25561, "Minolta AF 135mm F2.8"),
        (25571, "Minolta/Sony AF 28mm F2.8"),
        (25581, "Minolta AF 24-50mm F4"),
        (25601, "Minolta AF 100-200mm F4.5"),
        (25611, "Minolta AF 75-300mm F4.5-5.6 or Sigma Lens"),
        (25621, "Minolta AF 50mm F1.4 [New]"),
        (25631, "Minolta AF 300mm F2.8 APO or Sigma Lens"),
        (25641, "Minolta AF 50mm F2.8 Macro or Sigma Lens"),
        (25651, "Minolta AF 600mm F4 APO"),
        (25661, "Minolta AF 24mm F2.8 or Sigma Lens"),
        (25721, "Minolta/Sony AF 500mm F8 Reflex"),
        (25781, "Minolta/Sony AF 16mm F2.8 Fisheye or Sigma Lens"),
        (25791, "Minolta/Sony AF 20mm F2.8 or Tokina Lens"),
        (
            25811,
            "Minolta AF 100mm F2.8 Macro [New] or Sigma or Tamron Lens",
        ),
        (25851, "Beroflex 35-135mm F3.5-4.5"),
        (25858, "Minolta AF 35-105mm F3.5-4.5 New or Tamron Lens"),
        (25881, "Minolta AF 70-210mm F3.5-4.5"),
        (25891, "Minolta AF 80-200mm F2.8 APO or Tokina Lens"),
        (
            25901,
            "Minolta AF 200mm F2.8 G APO + Minolta AF 1.4x APO or Other Lens + 1.4x",
        ),
        (25911, "Minolta AF 35mm F1.4"),
        (25921, "Minolta AF 85mm F1.4 G (D)"),
        (25931, "Minolta AF 200mm F2.8 APO"),
        (25941, "Minolta AF 3x-1x F1.7-2.8 Macro"),
        (25961, "Minolta AF 28mm F2"),
        (25971, "Minolta AF 35mm F2 [New]"),
        (25981, "Minolta AF 100mm F2"),
        (
            26011,
            "Minolta AF 200mm F2.8 G APO + Minolta AF 2x APO or Other Lens + 2x",
        ),
        (26041, "Minolta AF 80-200mm F4.5-5.6"),
        (26051, "Minolta AF 35-80mm F4-5.6"),
        (26061, "Minolta AF 100-300mm F4.5-5.6"),
        (26071, "Minolta AF 35-80mm F4-5.6"),
        (26081, "Minolta AF 300mm F2.8 HS-APO G"),
        (26091, "Minolta AF 600mm F4 HS-APO G"),
        (26121, "Minolta AF 200mm F2.8 HS-APO G"),
        (26131, "Minolta AF 50mm F1.7 New"),
        (26151, "Minolta AF 28-105mm F3.5-4.5 xi"),
        (26161, "Minolta AF 35-200mm F4.5-5.6 xi"),
        (26181, "Minolta AF 28-80mm F4-5.6 xi"),
        (26191, "Minolta AF 80-200mm F4.5-5.6 xi"),
        (26201, "Minolta AF 28-70mm F2.8 G"),
        (26211, "Minolta AF 100-300mm F4.5-5.6 xi"),
        (26241, "Minolta AF 35-80mm F4-5.6 Power Zoom"),
        (26281, "Minolta AF 80-200mm F2.8 HS-APO G"),
        (26291, "Minolta AF 85mm F1.4 New"),
        (26311, "Minolta AF 100-300mm F4.5-5.6 APO"),
        (26321, "Minolta AF 24-50mm F4 New"),
        (26381, "Minolta AF 50mm F2.8 Macro New"),
        (26391, "Minolta AF 100mm F2.8 Macro"),
        (26411, "Minolta/Sony AF 20mm F2.8 New"),
        (26421, "Minolta AF 24mm F2.8 New"),
        (26441, "Minolta AF 100-400mm F4.5-6.7 APO"),
        (26621, "Minolta AF 50mm F1.4 New"),
        (26671, "Minolta AF 35mm F2 New"),
        (26681, "Minolta AF 28mm F2 New"),
        (26721, "Minolta AF 24-105mm F3.5-4.5 (D)"),
        (30464, "Metabones Canon EF Speed Booster"),
        (45671, "Tokina 70-210mm F4-5.6"),
        (45681, "Tokina AF 35-200mm F4-5.6 Zoom SD"),
        (45701, "Tamron AF 35-135mm F3.5-4.5"),
        (45711, "Vivitar 70-210mm F4.5-5.6"),
        (45741, "2x Teleconverter or Tamron or Tokina Lens"),
        (45751, "1.4x Teleconverter"),
        (45851, "Tamron SP AF 300mm F2.8 LD IF"),
        (45861, "Tamron SP AF 35-105mm F2.8 LD Aspherical IF"),
        (45871, "Tamron AF 70-210mm F2.8 SP LD"),
        (48128, "Metabones Canon EF Speed Booster Ultra"),
        (61184, "Canon EF Adapter"),
        (65280, "Sigma 16mm F2.8 Filtermatic Fisheye"),
        (65535, "E-Mount, T-Mount, Other Lens or no lens"),
    ];

    /// Shared lookup structure over [`SONY_LENSES`].
    pub static LENS_DB: StaticLensDb = StaticLensDb::new(&SONY_LENSES);

    /// Resolves an A-mount lens id, or `None` when ExifTool has no name for it.
    pub fn lookup(lens_id: u16) -> Option<&'static str> {
        LENS_DB.lookup(lens_id)
    }
}

/// Pentax lens database
pub mod pentax {
    /// ExifTool's `%pentaxLensTypes` (Pentax.pm:75), keyed there by the string
    /// `"series sub_id"` -- the two bytes the LensType tag holds (0x003f
    /// LensRec, and the LensType field of LensInfo/LensInfo2/etc).  It is keyed
    /// on that pair and nothing else; there is no single-number Pentax lens
    /// table in ExifTool.
    /// Only the base (non fractional-disambiguated) entries are included;
    /// ExifTool uses extra heuristics (focal length, etc) to disambiguate a
    /// handful of ambiguous IDs which share the exact same (series, sub_id)
    /// pair, and those variants are omitted here.
    pub static PENTAX_LENS_TYPES: [(u8, u16, &str); 265] = [
        (0, 0, "M-42 or No Lens"),
        (1, 0, "K or M Lens"),
        (2, 0, "A Series Lens"),
        (3, 0, "Sigma"),
        (3, 17, "smc PENTAX-FA SOFT 85mm F2.8"),
        (3, 18, "smc PENTAX-F 1.7X AF ADAPTER"),
        (3, 19, "smc PENTAX-F 24-50mm F4"),
        (3, 20, "smc PENTAX-F 35-80mm F4-5.6"),
        (3, 21, "smc PENTAX-F 80-200mm F4.7-5.6"),
        (3, 22, "smc PENTAX-F FISH-EYE 17-28mm F3.5-4.5"),
        (3, 23, "smc PENTAX-F 100-300mm F4.5-5.6 or Sigma Lens"),
        (3, 24, "smc PENTAX-F 35-135mm F3.5-4.5"),
        (
            3,
            25,
            "smc PENTAX-F 35-105mm F4-5.6 or Sigma or Tokina Lens",
        ),
        (3, 26, "smc PENTAX-F* 250-600mm F5.6 ED[IF]"),
        (3, 27, "smc PENTAX-F 28-80mm F3.5-4.5 or Tokina Lens"),
        (3, 28, "smc PENTAX-F 35-70mm F3.5-4.5 or Tokina Lens"),
        (3, 29, "PENTAX-F 28-80mm F3.5-4.5 or Sigma or Tokina Lens"),
        (3, 30, "PENTAX-F 70-200mm F4-5.6"),
        (
            3,
            31,
            "smc PENTAX-F 70-210mm F4-5.6 or Tokina or Takumar Lens",
        ),
        (3, 32, "smc PENTAX-F 50mm F1.4"),
        (3, 33, "smc PENTAX-F 50mm F1.7"),
        (3, 34, "smc PENTAX-F 135mm F2.8 [IF]"),
        (3, 35, "smc PENTAX-F 28mm F2.8"),
        (3, 36, "Sigma 20mm F1.8 EX DG Aspherical RF"),
        (3, 38, "smc PENTAX-F* 300mm F4.5 ED[IF]"),
        (3, 39, "smc PENTAX-F* 600mm F4 ED[IF]"),
        (3, 40, "smc PENTAX-F Macro 100mm F2.8"),
        (3, 41, "smc PENTAX-F Macro 50mm F2.8 or Sigma Lens"),
        (3, 42, "Sigma 300mm F2.8 EX DG APO IF"),
        (3, 44, "Sigma or Tamron Lens (3 44)"),
        (3, 46, "Sigma or Samsung Lens (3 46)"),
        (3, 50, "smc PENTAX-FA 28-70mm F4 AL"),
        (3, 51, "Sigma 28mm F1.8 EX DG Aspherical Macro"),
        (
            3,
            52,
            "smc PENTAX-FA 28-200mm F3.8-5.6 AL[IF] or Tamron Lens",
        ),
        (3, 53, "smc PENTAX-FA 28-80mm F3.5-5.6 AL"),
        (3, 247, "smc PENTAX-DA FISH-EYE 10-17mm F3.5-4.5 ED[IF]"),
        (3, 248, "smc PENTAX-DA 12-24mm F4 ED AL[IF]"),
        (3, 250, "smc PENTAX-DA 50-200mm F4-5.6 ED"),
        (3, 251, "smc PENTAX-DA 40mm F2.8 Limited"),
        (3, 252, "smc PENTAX-DA 18-55mm F3.5-5.6 AL"),
        (3, 253, "smc PENTAX-DA 14mm F2.8 ED[IF]"),
        (3, 254, "smc PENTAX-DA 16-45mm F4 ED AL"),
        (3, 255, "Sigma Lens (3 255)"),
        (4, 1, "smc PENTAX-FA SOFT 28mm F2.8"),
        (4, 2, "smc PENTAX-FA 80-320mm F4.5-5.6"),
        (4, 3, "smc PENTAX-FA 43mm F1.9 Limited"),
        (4, 6, "smc PENTAX-FA 35-80mm F4-5.6"),
        (4, 7, "Irix 45mm F1.4"),
        (4, 8, "Irix 150mm F2.8 Macro"),
        (4, 9, "Irix 11mm F4 Firefly"),
        (4, 10, "Irix 15mm F2.4"),
        (4, 12, "smc PENTAX-FA 50mm F1.4"),
        (4, 15, "smc PENTAX-FA 28-105mm F4-5.6 [IF]"),
        (4, 16, "Tamron AF 80-210mm F4-5.6 (178D)"),
        (4, 19, "Tamron SP AF 90mm F2.8 (172E)"),
        (4, 20, "smc PENTAX-FA 28-80mm F3.5-5.6"),
        (4, 21, "Cosina AF 100-300mm F5.6-6.7"),
        (4, 22, "Tokina 28-80mm F3.5-5.6"),
        (4, 23, "smc PENTAX-FA 20-35mm F4 AL"),
        (4, 24, "smc PENTAX-FA 77mm F1.8 Limited"),
        (4, 25, "Tamron SP AF 14mm F2.8"),
        (4, 26, "smc PENTAX-FA Macro 100mm F3.5 or Cosina Lens"),
        (
            4,
            27,
            "Tamron AF 28-300mm F3.5-6.3 LD Aspherical[IF] Macro (185D/285D)",
        ),
        (4, 28, "smc PENTAX-FA 35mm F2 AL"),
        (
            4,
            29,
            "Tamron AF 28-200mm F3.8-5.6 LD Super II Macro (371D)",
        ),
        (4, 34, "smc PENTAX-FA 24-90mm F3.5-4.5 AL[IF]"),
        (4, 35, "smc PENTAX-FA 100-300mm F4.7-5.8"),
        (4, 36, "Tamron AF 70-300mm F4-5.6 LD Macro 1:2"),
        (4, 37, "Tamron SP AF 24-135mm F3.5-5.6 AD AL (190D)"),
        (4, 38, "smc PENTAX-FA 28-105mm F3.2-4.5 AL[IF]"),
        (4, 39, "smc PENTAX-FA 31mm F1.8 AL Limited"),
        (
            4,
            41,
            "Tamron AF 28-200mm Super Zoom F3.8-5.6 Aspherical XR [IF] Macro (A03)",
        ),
        (4, 43, "smc PENTAX-FA 28-90mm F3.5-5.6"),
        (4, 44, "smc PENTAX-FA J 75-300mm F4.5-5.8 AL"),
        (4, 45, "Tamron Lens (4 45)"),
        (4, 46, "smc PENTAX-FA J 28-80mm F3.5-5.6 AL"),
        (4, 47, "smc PENTAX-FA J 18-35mm F4-5.6 AL"),
        (
            4,
            49,
            "Tamron SP AF 28-75mm F2.8 XR Di LD Aspherical [IF] Macro",
        ),
        (4, 51, "smc PENTAX-D FA 50mm F2.8 Macro"),
        (4, 52, "smc PENTAX-D FA 100mm F2.8 Macro"),
        (4, 55, "Samsung/Schneider D-XENOGON 35mm F2"),
        (4, 56, "Samsung/Schneider D-XENON 100mm F2.8 Macro"),
        (4, 75, "Tamron SP AF 70-200mm F2.8 Di LD [IF] Macro (A001)"),
        (4, 214, "smc PENTAX-DA 35mm F2.4 AL"),
        (4, 229, "smc PENTAX-DA 18-55mm F3.5-5.6 AL II"),
        (4, 230, "Tamron SP AF 17-50mm F2.8 XR Di II"),
        (4, 231, "smc PENTAX-DA 18-250mm F3.5-6.3 ED AL [IF]"),
        (4, 237, "Samsung/Schneider D-XENOGON 10-17mm F3.5-4.5"),
        (4, 239, "Samsung/Schneider D-XENON 12-24mm F4 ED AL [IF]"),
        (
            4,
            242,
            "smc PENTAX-DA* 16-50mm F2.8 ED AL [IF] SDM (SDM unused)",
        ),
        (4, 243, "smc PENTAX-DA 70mm F2.4 Limited"),
        (4, 244, "smc PENTAX-DA 21mm F3.2 AL Limited"),
        (4, 245, "Samsung/Schneider D-XENON 50-200mm F4-5.6"),
        (4, 246, "Samsung/Schneider D-XENON 18-55mm F3.5-5.6"),
        (4, 247, "smc PENTAX-DA FISH-EYE 10-17mm F3.5-4.5 ED[IF]"),
        (4, 248, "smc PENTAX-DA 12-24mm F4 ED AL [IF]"),
        (4, 249, "Tamron XR DiII 18-200mm F3.5-6.3 (A14)"),
        (4, 250, "smc PENTAX-DA 50-200mm F4-5.6 ED"),
        (4, 251, "smc PENTAX-DA 40mm F2.8 Limited"),
        (4, 252, "smc PENTAX-DA 18-55mm F3.5-5.6 AL"),
        (4, 253, "smc PENTAX-DA 14mm F2.8 ED[IF]"),
        (4, 254, "smc PENTAX-DA 16-45mm F4 ED AL"),
        (5, 1, "smc PENTAX-FA* 24mm F2 AL[IF]"),
        (5, 2, "smc PENTAX-FA 28mm F2.8 AL"),
        (5, 3, "smc PENTAX-FA 50mm F1.7"),
        (5, 4, "smc PENTAX-FA 50mm F1.4"),
        (5, 5, "smc PENTAX-FA* 600mm F4 ED[IF]"),
        (5, 6, "smc PENTAX-FA* 300mm F4.5 ED[IF]"),
        (5, 7, "smc PENTAX-FA 135mm F2.8 [IF]"),
        (5, 8, "smc PENTAX-FA Macro 50mm F2.8"),
        (5, 9, "smc PENTAX-FA Macro 100mm F2.8"),
        (5, 10, "smc PENTAX-FA* 85mm F1.4 [IF]"),
        (5, 11, "smc PENTAX-FA* 200mm F2.8 ED[IF]"),
        (5, 12, "smc PENTAX-FA 28-80mm F3.5-4.7"),
        (5, 13, "smc PENTAX-FA 70-200mm F4-5.6"),
        (5, 14, "smc PENTAX-FA* 250-600mm F5.6 ED[IF]"),
        (5, 15, "smc PENTAX-FA 28-105mm F4-5.6"),
        (5, 16, "smc PENTAX-FA 100-300mm F4.5-5.6"),
        (5, 98, "smc PENTAX-FA 100-300mm F4.5-5.6"),
        (6, 1, "smc PENTAX-FA* 85mm F1.4 [IF]"),
        (6, 2, "smc PENTAX-FA* 200mm F2.8 ED[IF]"),
        (6, 3, "smc PENTAX-FA* 300mm F2.8 ED[IF]"),
        (6, 4, "smc PENTAX-FA* 28-70mm F2.8 AL"),
        (6, 5, "smc PENTAX-FA* 80-200mm F2.8 ED[IF]"),
        (6, 6, "smc PENTAX-FA* 28-70mm F2.8 AL"),
        (6, 7, "smc PENTAX-FA* 80-200mm F2.8 ED[IF]"),
        (6, 8, "smc PENTAX-FA 28-70mm F4AL"),
        (6, 9, "smc PENTAX-FA 20mm F2.8"),
        (6, 10, "smc PENTAX-FA* 400mm F5.6 ED[IF]"),
        (6, 13, "smc PENTAX-FA* 400mm F5.6 ED[IF]"),
        (6, 14, "smc PENTAX-FA* Macro 200mm F4 ED[IF]"),
        (7, 0, "smc PENTAX-DA 21mm F3.2 AL Limited"),
        (7, 58, "smc PENTAX-D FA Macro 100mm F2.8 WR"),
        (7, 75, "Tamron SP AF 70-200mm F2.8 Di LD [IF] Macro (A001)"),
        (7, 201, "smc Pentax-DA L 50-200mm F4-5.6 ED WR"),
        (7, 202, "smc PENTAX-DA L 18-55mm F3.5-5.6 AL WR"),
        (7, 203, "HD PENTAX-DA 55-300mm F4-5.8 ED WR"),
        (7, 204, "HD PENTAX-DA 15mm F4 ED AL Limited"),
        (7, 205, "HD PENTAX-DA 35mm F2.8 Macro Limited"),
        (7, 206, "HD PENTAX-DA 70mm F2.4 Limited"),
        (7, 207, "HD PENTAX-DA 21mm F3.2 ED AL Limited"),
        (7, 208, "HD PENTAX-DA 40mm F2.8 Limited"),
        (7, 212, "smc PENTAX-DA 50mm F1.8"),
        (7, 213, "smc PENTAX-DA 40mm F2.8 XS"),
        (7, 214, "smc PENTAX-DA 35mm F2.4 AL"),
        (7, 216, "smc PENTAX-DA L 55-300mm F4-5.8 ED"),
        (7, 217, "smc PENTAX-DA 50-200mm F4-5.6 ED WR"),
        (7, 218, "smc PENTAX-DA 18-55mm F3.5-5.6 AL WR"),
        (
            7,
            220,
            "Tamron SP AF 10-24mm F3.5-4.5 Di II LD Aspherical [IF]",
        ),
        (7, 221, "smc PENTAX-DA L 50-200mm F4-5.6 ED"),
        (7, 222, "smc PENTAX-DA L 18-55mm F3.5-5.6"),
        (7, 223, "Samsung/Schneider D-XENON 18-55mm F3.5-5.6 II"),
        (7, 224, "smc PENTAX-DA 15mm F4 ED AL Limited"),
        (7, 225, "Samsung/Schneider D-XENON 18-250mm F3.5-6.3"),
        (7, 226, "smc PENTAX-DA* 55mm F1.4 SDM (SDM unused)"),
        (7, 227, "smc PENTAX-DA* 60-250mm F4 [IF] SDM (SDM unused)"),
        (7, 228, "Samsung 16-45mm F4 ED"),
        (7, 229, "smc PENTAX-DA 18-55mm F3.5-5.6 AL II"),
        (7, 230, "Tamron AF 17-50mm F2.8 XR Di-II LD (Model A16)"),
        (7, 231, "smc PENTAX-DA 18-250mm F3.5-6.3 ED AL [IF]"),
        (7, 233, "smc PENTAX-DA 35mm F2.8 Macro Limited"),
        (7, 234, "smc PENTAX-DA* 300mm F4 ED [IF] SDM (SDM unused)"),
        (7, 235, "smc PENTAX-DA* 200mm F2.8 ED [IF] SDM (SDM unused)"),
        (7, 236, "smc PENTAX-DA 55-300mm F4-5.8 ED"),
        (
            7,
            238,
            "Tamron AF 18-250mm F3.5-6.3 Di II LD Aspherical [IF] Macro",
        ),
        (
            7,
            241,
            "smc PENTAX-DA* 50-135mm F2.8 ED [IF] SDM (SDM unused)",
        ),
        (
            7,
            242,
            "smc PENTAX-DA* 16-50mm F2.8 ED AL [IF] SDM (SDM unused)",
        ),
        (7, 243, "smc PENTAX-DA 70mm F2.4 Limited"),
        (7, 244, "smc PENTAX-DA 21mm F3.2 AL Limited"),
        (8, 0, "Sigma 50-150mm F2.8 II APO EX DC HSM"),
        (8, 3, "Sigma 18-125mm F3.8-5.6 DC HSM"),
        (8, 4, "Sigma 50mm F1.4 EX DG HSM"),
        (8, 6, "Sigma 4.5mm F2.8 EX DC Fisheye"),
        (8, 7, "Sigma 24-70mm F2.8 IF EX DG HSM"),
        (8, 8, "Sigma 18-250mm F3.5-6.3 DC OS HSM"),
        (8, 11, "Sigma 10-20mm F3.5 EX DC HSM"),
        (8, 12, "Sigma 70-300mm F4-5.6 DG OS"),
        (8, 13, "Sigma 120-400mm F4.5-5.6 APO DG OS HSM"),
        (8, 14, "Sigma 17-70mm F2.8-4.0 DC Macro OS HSM"),
        (8, 15, "Sigma 150-500mm F5-6.3 APO DG OS HSM"),
        (8, 16, "Sigma 70-200mm F2.8 EX DG Macro HSM II"),
        (8, 17, "Sigma 50-500mm F4.5-6.3 DG OS HSM"),
        (8, 18, "Sigma 8-16mm F4.5-5.6 DC HSM"),
        (8, 20, "Sigma 18-50mm F2.8-4.5 DC HSM"),
        (8, 21, "Sigma 17-50mm F2.8 EX DC OS HSM"),
        (8, 22, "Sigma 85mm F1.4 EX DG HSM"),
        (8, 23, "Sigma 70-200mm F2.8 APO EX DG OS HSM"),
        (8, 24, "Sigma 17-70mm F2.8-4 DC Macro OS HSM"),
        (8, 25, "Sigma 17-50mm F2.8 EX DC HSM"),
        (8, 27, "Sigma 18-200mm F3.5-6.3 II DC HSM"),
        (8, 28, "Sigma 18-250mm F3.5-6.3 DC Macro HSM"),
        (8, 29, "Sigma 35mm F1.4 DG HSM"),
        (8, 30, "Sigma 17-70mm F2.8-4 DC Macro HSM | C"),
        (8, 31, "Sigma 18-35mm F1.8 DC HSM"),
        (8, 32, "Sigma 30mm F1.4 DC HSM | A"),
        (8, 33, "Sigma 18-200mm F3.5-6.3 DC Macro HSM"),
        (8, 34, "Sigma 18-300mm F3.5-6.3 DC Macro HSM"),
        (8, 59, "HD PENTAX-D FA 150-450mm F4.5-5.6 ED DC AW"),
        (8, 60, "HD PENTAX-D FA* 70-200mm F2.8 ED DC AW"),
        (8, 61, "HD PENTAX-D FA 28-105mm F3.5-5.6 ED DC WR"),
        (8, 62, "HD PENTAX-D FA 24-70mm F2.8 ED SDM WR"),
        (8, 63, "HD PENTAX-D FA 15-30mm F2.8 ED SDM WR"),
        (8, 64, "HD PENTAX-D FA* 50mm F1.4 SDM AW"),
        (8, 65, "HD PENTAX-D FA 70-210mm F4 ED SDM WR"),
        (8, 66, "HD PENTAX-D FA 85mm F1.4 ED SDM AW"),
        (8, 67, "HD PENTAX-D FA 21mm F2.4 ED Limited DC WR"),
        (8, 195, "HD PENTAX DA* 16-50mm F2.8 ED PLM AW"),
        (8, 196, "HD PENTAX-DA* 11-18mm F2.8 ED DC AW"),
        (8, 197, "HD PENTAX-DA 55-300mm F4.5-6.3 ED PLM WR RE"),
        (8, 198, "smc PENTAX-DA L 18-50mm F4-5.6 DC WR RE"),
        (8, 199, "HD PENTAX-DA 18-50mm F4-5.6 DC WR RE"),
        (8, 200, "HD PENTAX-DA 16-85mm F3.5-5.6 ED DC WR"),
        (8, 209, "HD PENTAX-DA 20-40mm F2.8-4 ED Limited DC WR"),
        (8, 210, "smc PENTAX-DA 18-270mm F3.5-6.3 ED SDM"),
        (8, 211, "HD PENTAX-DA 560mm F5.6 ED AW"),
        (8, 215, "smc PENTAX-DA 18-135mm F3.5-5.6 ED AL [IF] DC WR"),
        (8, 226, "smc PENTAX-DA* 55mm F1.4 SDM"),
        (8, 227, "smc PENTAX-DA* 60-250mm F4 [IF] SDM"),
        (8, 232, "smc PENTAX-DA 17-70mm F4 AL [IF] SDM"),
        (8, 234, "smc PENTAX-DA* 300mm F4 ED [IF] SDM"),
        (8, 235, "smc PENTAX-DA* 200mm F2.8 ED [IF] SDM"),
        (8, 241, "smc PENTAX-DA* 50-135mm F2.8 ED [IF] SDM"),
        (8, 242, "smc PENTAX-DA* 16-50mm F2.8 ED AL [IF] SDM"),
        (8, 255, "Sigma Lens (8 255)"),
        (9, 0, "645 Manual Lens"),
        (9, 3, "HD PENTAX-FA 43mm F1.9 Limited"),
        (9, 24, "HD PENTAX-FA 77mm F1.8 Limited"),
        (9, 39, "HD PENTAX-FA 31mm F1.8 AL Limited"),
        (9, 247, "HD PENTAX-DA FISH-EYE 10-17mm F3.5-4.5 ED [IF]"),
        (10, 0, "645 A Series Lens"),
        (11, 1, "smc PENTAX-FA 645 75mm F2.8"),
        (11, 2, "smc PENTAX-FA 645 45mm F2.8"),
        (11, 3, "smc PENTAX-FA* 645 300mm F4 ED [IF]"),
        (11, 4, "smc PENTAX-FA 645 45-85mm F4.5"),
        (11, 5, "smc PENTAX-FA 645 400mm F5.6 ED [IF]"),
        (11, 7, "smc PENTAX-FA 645 Macro 120mm F4"),
        (11, 8, "smc PENTAX-FA 645 80-160mm F4.5"),
        (11, 9, "smc PENTAX-FA 645 200mm F4 [IF]"),
        (11, 10, "smc PENTAX-FA 645 150mm F2.8 [IF]"),
        (11, 11, "smc PENTAX-FA 645 35mm F3.5 AL [IF]"),
        (11, 12, "smc PENTAX-FA 645 300mm F5.6 ED [IF]"),
        (11, 14, "smc PENTAX-FA 645 55-110mm F5.6"),
        (11, 16, "smc PENTAX-FA 645 33-55mm F4.5 AL"),
        (11, 17, "smc PENTAX-FA 645 150-300mm F5.6 ED [IF]"),
        (11, 21, "HD PENTAX-D FA 645 35mm F3.5 AL [IF]"),
        (13, 18, "smc PENTAX-D FA 645 55mm F2.8 AL [IF] SDM AW"),
        (13, 19, "smc PENTAX-D FA 645 25mm F4 AL [IF] SDM AW"),
        (13, 20, "HD PENTAX-D FA 645 90mm F2.8 ED AW SR"),
        (13, 253, "HD PENTAX-DA 645 28-45mm F4.5 ED AW SR"),
        (13, 254, "smc PENTAX-DA 645 25mm F4 AL [IF] SDM AW"),
        (20, 0, "Pentax Q Manual Lens (Q, Q10)"),
        (21, 0, "Pentax Q Manual Lens"),
        (21, 1, "01 Standard Prime 8.5mm F1.9"),
        (21, 2, "02 Standard Zoom 5-15mm F2.8-4.5"),
        (22, 3, "03 Fish-eye 3.2mm F5.6"),
        (22, 4, "04 Toy Lens Wide 6.3mm F7.1"),
        (22, 5, "05 Toy Lens Telephoto 18mm F8"),
        (21, 6, "06 Telephoto Zoom 15-45mm F2.8"),
        (21, 7, "07 Mount Shield 11.5mm F9"),
        (21, 8, "08 Wide Zoom 3.8-5.9mm F3.7-4"),
        (21, 233, "Adapter Q for K-mount Lens"),
        (31, 1, "18.3mm F2.8"),
        (31, 4, "26.1mm F2.8"),
        (31, 5, "26.1mm F2.8 GT-2 TC"),
        (31, 8, "18.3mm F2.8"),
    ];

    /// Looks up a lens name by its (series, sub_id) pair, matching ExifTool's
    /// `%pentaxLensTypes` PrintConv used for the LensType tag.
    pub fn lookup_lens_type(series: u8, sub_id: u16) -> Option<&'static str> {
        PENTAX_LENS_TYPES
            .iter()
            .find(|(s, i, _)| *s == series && *i == sub_id)
            .map(|(_, _, name)| *name)
    }
}

/// Leica lens database
pub mod leica {
    //! ExifTool's `%Image::ExifTool::Panasonic::leicaLensTypes` (Panasonic.pm:46),
    //! the `PrintConv` shared by every Leica `LensType` tag: `Leica2` 0x0310
    //! (Panasonic.pm:1648), `Subdir` 0x3405 (Panasonic.pm:1894) and `Data1`
    //! 0x0016 (Panasonic.pm:1977).
    //!
    //! TRANSCRIBED BY SCRIPT (`scripts/gen_leica_lens_types.pl`) from ExifTool's
    //! own in-memory Perl hash, and every name is additionally traced back to a
    //! literal `key => 'name',` line in Panasonic.pm -- quoted beside each entry
    //! -- before it is emitted.  The generator hard-errors on anything it has
    //! not seen (a value that is a reference rather than a scalar, a key that is
    //! neither `N` nor `N M`, a half outside the range the reader can produce, or
    //! a name with no source line) rather than dropping it silently.
    //!
    //! # Key shape
    //!
    //! The tag is an `int32u`, and ExifTool's `ValueConv` splits it into two
    //! integers -- `($val >> 2) . " " . ($val & 0x3)`.  The second number is the
    //! M8/M9 frame-selector position, and only a handful of lenses are
    //! distinguished by it; ExifTool's `OTHER` handler strips it and retries with
    //! the first number alone for everything else.  `Some(sub)` here is an entry
    //! ExifTool files under the two-number key, `None` one it files under the
    //! bare id.

    pub static LEICA_LENS_TYPES: [(u16, Option<u8>, &str); 58] = [
        (0, Some(0), "Uncoded lens"),                   // Panasonic.pm:71
        (1, None, "Elmarit-M 21mm f/2.8"),              // Panasonic.pm:76
        (3, None, "Elmarit-M 28mm f/2.8 (III)"),        // Panasonic.pm:77
        (4, None, "Tele-Elmarit-M 90mm f/2.8 (II)"),    // Panasonic.pm:78
        (5, None, "Summilux-M 50mm f/1.4 (II)"),        // Panasonic.pm:79
        (6, None, "Summicron-M 35mm f/2 (IV)"),         // Panasonic.pm:80
        (6, Some(0), "Summilux-M 35mm f/1.4"),          // Panasonic.pm:81
        (7, None, "Summicron-M 90mm f/2 (II)"),         // Panasonic.pm:82
        (9, None, "Elmarit-M 135mm f/2.8 (I/II)"),      // Panasonic.pm:83
        (9, Some(0), "Apo-Telyt-M 135mm f/3.4"),        // Panasonic.pm:84
        (11, None, "Summaron-M 28mm f/5.6"),            // Panasonic.pm:85
        (12, None, "Thambar-M 90mm f/2.2"),             // Panasonic.pm:86
        (16, None, "Tri-Elmar-M 16-18-21mm f/4 ASPH."), // Panasonic.pm:87
        (16, Some(1), "Tri-Elmar-M 16-18-21mm f/4 ASPH. (at 16mm)"), // Panasonic.pm:88
        (16, Some(2), "Tri-Elmar-M 16-18-21mm f/4 ASPH. (at 18mm)"), // Panasonic.pm:89
        (16, Some(3), "Tri-Elmar-M 16-18-21mm f/4 ASPH. (at 21mm)"), // Panasonic.pm:90
        (23, None, "Summicron-M 50mm f/2 (III)"),       // Panasonic.pm:91
        (24, None, "Elmarit-M 21mm f/2.8 ASPH."),       // Panasonic.pm:92
        (25, None, "Elmarit-M 24mm f/2.8 ASPH."),       // Panasonic.pm:93
        (26, None, "Summicron-M 28mm f/2 ASPH."),       // Panasonic.pm:94
        (27, None, "Elmarit-M 28mm f/2.8 (IV)"),        // Panasonic.pm:95
        (28, None, "Elmarit-M 28mm f/2.8 ASPH."),       // Panasonic.pm:96
        (29, None, "Summilux-M 35mm f/1.4 ASPH."),      // Panasonic.pm:97
        (29, Some(0), "Summilux-M 35mm f/1.4 ASPHERICAL"), // Panasonic.pm:98
        (30, None, "Summicron-M 35mm f/2 ASPH."),       // Panasonic.pm:99
        (31, None, "Noctilux-M 50mm f/1"),              // Panasonic.pm:100
        (31, Some(0), "Noctilux-M 50mm f/1.2"),         // Panasonic.pm:101
        (32, None, "Summilux-M 50mm f/1.4 ASPH."),      // Panasonic.pm:102
        (33, None, "Summicron-M 50mm f/2 (IV, V)"),     // Panasonic.pm:103
        (34, None, "Elmar-M 50mm f/2.8"),               // Panasonic.pm:104
        (35, None, "Summilux-M 75mm f/1.4"),            // Panasonic.pm:105
        (36, None, "Apo-Summicron-M 75mm f/2 ASPH."),   // Panasonic.pm:106
        (37, None, "Apo-Summicron-M 90mm f/2 ASPH."),   // Panasonic.pm:107
        (38, None, "Elmarit-M 90mm f/2.8"),             // Panasonic.pm:108
        (39, None, "Macro-Elmar-M 90mm f/4"),           // Panasonic.pm:109
        (39, Some(0), "Tele-Elmar-M 135mm f/4 (II)"),   // Panasonic.pm:110
        (40, None, "Macro-Adapter M"),                  // Panasonic.pm:111
        (41, None, "Apo-Summicron-M 50mm f/2 ASPH."),   // Panasonic.pm:112
        (41, Some(3), "Apo-Summicron-M 50mm f/2 ASPH."), // Panasonic.pm:113
        (42, None, "Tri-Elmar-M 28-35-50mm f/4 ASPH."), // Panasonic.pm:114
        (42, Some(1), "Tri-Elmar-M 28-35-50mm f/4 ASPH. (at 28mm)"), // Panasonic.pm:115
        (42, Some(2), "Tri-Elmar-M 28-35-50mm f/4 ASPH. (at 35mm)"), // Panasonic.pm:116
        (42, Some(3), "Tri-Elmar-M 28-35-50mm f/4 ASPH. (at 50mm)"), // Panasonic.pm:117
        (43, None, "Summarit-M 35mm f/2.5"),            // Panasonic.pm:118
        (44, None, "Summarit-M 50mm f/2.5"),            // Panasonic.pm:119
        (45, None, "Summarit-M 75mm f/2.5"),            // Panasonic.pm:120
        (46, None, "Summarit-M 90mm f/2.5"),            // Panasonic.pm:121
        (47, None, "Summilux-M 21mm f/1.4 ASPH."),      // Panasonic.pm:122
        (48, None, "Summilux-M 24mm f/1.4 ASPH."),      // Panasonic.pm:123
        (49, None, "Noctilux-M 50mm f/0.95 ASPH."),     // Panasonic.pm:124
        (50, None, "Elmar-M 24mm f/3.8 ASPH."),         // Panasonic.pm:125
        (51, None, "Super-Elmar-M 21mm f/3.4 Asph"),    // Panasonic.pm:126
        (51, Some(2), "Super-Elmar-M 14mm f/3.8 Asph"), // Panasonic.pm:127
        (52, None, "Apo-Telyt-M 18mm f/3.8 ASPH."),     // Panasonic.pm:128
        (53, None, "Apo-Telyt-M 135mm f/3.4"),          // Panasonic.pm:129
        (53, Some(2), "Apo-Telyt-M 135mm f/3.4"),       // Panasonic.pm:130
        (53, Some(3), "Apo-Summicron-M 50mm f/2 (VI)"), // Panasonic.pm:131
        (58, None, "Noctilux-M 75mm f/1.25 ASPH."),     // Panasonic.pm:132
    ];

    /// `%leicaLensTypes` lookup for a raw `LensType` int32u.
    ///
    /// Mirrors ExifTool exactly: the pair `(val >> 2, val & 3)` is tried first,
    /// then the `OTHER` fallback (`return undef if ... not $val =~ s/ .*//;`)
    /// drops the second number and retries with the first alone.
    pub fn lookup(raw: u32) -> Option<&'static str> {
        let id = raw >> 2;
        let sub = (raw & 0x3) as u8;
        LEICA_LENS_TYPES
            .iter()
            .find(|(i, s, _)| u32::from(*i) == id && *s == Some(sub))
            .or_else(|| {
                LEICA_LENS_TYPES
                    .iter()
                    .find(|(i, s, _)| u32::from(*i) == id && s.is_none())
            })
            .map(|(_, _, name)| *name)
    }

    /// ExifTool's `ValueConv` string for a raw `LensType`, which is what it
    /// prints inside `Unknown (...)` when the hash has no entry.
    pub fn value_conv(raw: u32) -> String {
        format!("{} {}", raw >> 2, raw & 0x3)
    }
}

/// Minolta lens database
///
/// `%minoltaLensTypes`, the `PrintConv` for Minolta `LensType`. It is the same
/// A-mount table Sony inherited - a strict subset of `%sonyLensTypes`, which
/// adds only later E-mount adapter ids - and ExifTool builds Sony's from this
/// one. Generated from ExifTool, so the spellings match its output exactly.
pub mod minolta {
    use super::*;

    /// Lens id to name, as ExifTool's `PrintConv` for Minolta `LensType`.
    pub static MINOLTA_LENSES: [(u16, &str); 167] = [
        (0, "Minolta AF 28-85mm F3.5-4.5 New"),
        (1, "Minolta AF 80-200mm F2.8 HS-APO G"),
        (2, "Minolta AF 28-70mm F2.8 G"),
        (3, "Minolta AF 28-80mm F4-5.6"),
        (4, "Minolta AF 85mm F1.4G"),
        (5, "Minolta AF 35-70mm F3.5-4.5 [II]"),
        (6, "Minolta AF 24-85mm F3.5-4.5 [New]"),
        (
            7,
            "Minolta AF 100-300mm F4.5-5.6 APO [New] or 100-400mm or Sigma Lens",
        ),
        (8, "Minolta AF 70-210mm F4.5-5.6 [II]"),
        (9, "Minolta AF 50mm F3.5 Macro"),
        (10, "Minolta AF 28-105mm F3.5-4.5 [New]"),
        (11, "Minolta AF 300mm F4 HS-APO G"),
        (12, "Minolta AF 100mm F2.8 Soft Focus"),
        (13, "Minolta AF 75-300mm F4.5-5.6 (New or II)"),
        (14, "Minolta AF 100-400mm F4.5-6.7 APO"),
        (15, "Minolta AF 400mm F4.5 HS-APO G"),
        (16, "Minolta AF 17-35mm F3.5 G"),
        (17, "Minolta AF 20-35mm F3.5-4.5"),
        (18, "Minolta AF 28-80mm F3.5-5.6 II"),
        (19, "Minolta AF 35mm F1.4 G"),
        (20, "Minolta/Sony 135mm F2.8 [T4.5] STF"),
        (22, "Minolta AF 35-80mm F4-5.6 II"),
        (23, "Minolta AF 200mm F4 Macro APO G"),
        (
            24,
            "Minolta/Sony AF 24-105mm F3.5-4.5 (D) or Sigma or Tamron Lens",
        ),
        (25, "Minolta AF 100-300mm F4.5-5.6 APO (D) or Sigma Lens"),
        (27, "Minolta AF 85mm F1.4 G (D)"),
        (28, "Minolta/Sony AF 100mm F2.8 Macro (D) or Tamron Lens"),
        (29, "Minolta/Sony AF 75-300mm F4.5-5.6 (D)"),
        (30, "Minolta AF 28-80mm F3.5-5.6 (D) or Sigma Lens"),
        (31, "Minolta/Sony AF 50mm F2.8 Macro (D) or F3.5"),
        (32, "Minolta/Sony AF 300mm F2.8 G or 1.5x Teleconverter"),
        (33, "Minolta/Sony AF 70-200mm F2.8 G"),
        (35, "Minolta AF 85mm F1.4 G (D) Limited"),
        (36, "Minolta AF 28-100mm F3.5-5.6 (D)"),
        (38, "Minolta AF 17-35mm F2.8-4 (D)"),
        (39, "Minolta AF 28-75mm F2.8 (D)"),
        (40, "Minolta/Sony AF DT 18-70mm F3.5-5.6 (D)"),
        (41, "Minolta/Sony AF DT 11-18mm F4.5-5.6 (D) or Tamron Lens"),
        (42, "Minolta/Sony AF DT 18-200mm F3.5-6.3 (D)"),
        (43, "Sony 35mm F1.4 G (SAL35F14G)"),
        (44, "Sony 50mm F1.4 (SAL50F14)"),
        (45, "Carl Zeiss Planar T* 85mm F1.4 ZA (SAL85F14Z)"),
        (
            46,
            "Carl Zeiss Vario-Sonnar T* DT 16-80mm F3.5-4.5 ZA (SAL1680Z)",
        ),
        (47, "Carl Zeiss Sonnar T* 135mm F1.8 ZA (SAL135F18Z)"),
        (
            48,
            "Carl Zeiss Vario-Sonnar T* 24-70mm F2.8 ZA SSM (SAL2470Z) or Other Lens",
        ),
        (49, "Sony DT 55-200mm F4-5.6 (SAL55200)"),
        (50, "Sony DT 18-250mm F3.5-6.3 (SAL18250)"),
        (51, "Sony DT 16-105mm F3.5-5.6 (SAL16105)"),
        (
            52,
            "Sony 70-300mm F4.5-5.6 G SSM (SAL70300G) or G SSM II or Tamron Lens",
        ),
        (53, "Sony 70-400mm F4-5.6 G SSM (SAL70400G)"),
        (
            54,
            "Carl Zeiss Vario-Sonnar T* 16-35mm F2.8 ZA SSM (SAL1635Z) or ZA SSM II",
        ),
        (55, "Sony DT 18-55mm F3.5-5.6 SAM (SAL1855) or SAM II"),
        (56, "Sony DT 55-200mm F4-5.6 SAM (SAL55200-2)"),
        (
            57,
            "Sony DT 50mm F1.8 SAM (SAL50F18) or Tamron Lens or Commlite CM-EF-NEX adapter",
        ),
        (58, "Sony DT 30mm F2.8 Macro SAM (SAL30M28)"),
        (59, "Sony 28-75mm F2.8 SAM (SAL2875)"),
        (60, "Carl Zeiss Distagon T* 24mm F2 ZA SSM (SAL24F20Z)"),
        (61, "Sony 85mm F2.8 SAM (SAL85F28)"),
        (62, "Sony DT 35mm F1.8 SAM (SAL35F18)"),
        (63, "Sony DT 16-50mm F2.8 SSM (SAL1650)"),
        (64, "Sony 500mm F4 G SSM (SAL500F40G)"),
        (65, "Sony DT 18-135mm F3.5-5.6 SAM (SAL18135)"),
        (66, "Sony 300mm F2.8 G SSM II (SAL300F28G2)"),
        (67, "Sony 70-200mm F2.8 G SSM II (SAL70200G2)"),
        (68, "Sony DT 55-300mm F4.5-5.6 SAM (SAL55300)"),
        (69, "Sony 70-400mm F4-5.6 G SSM II (SAL70400G2)"),
        (70, "Carl Zeiss Planar T* 50mm F1.4 ZA SSM (SAL50F14Z)"),
        (128, "Tamron or Sigma Lens (128)"),
        (129, "Tamron Lens (129)"),
        (131, "Tamron 20-40mm F2.7-3.5 SP Aspherical IF"),
        (135, "Vivitar 28-210mm F3.5-5.6"),
        (136, "Tokina EMZ M100 AF 100mm F3.5"),
        (137, "Cosina 70-210mm F2.8-4 AF"),
        (138, "Soligor 19-35mm F3.5-4.5"),
        (139, "Tokina AF 28-300mm F4-6.3"),
        (142, "Cosina AF 70-300mm F4.5-5.6 MC"),
        (146, "Voigtlander Macro APO-Lanthar 125mm F2.5 SL"),
        (194, "Tamron SP AF 17-50mm F2.8 XR Di II LD Aspherical [IF]"),
        (202, "Tamron SP AF 70-200mm F2.8 Di LD [IF] Macro"),
        (203, "Tamron SP 70-200mm F2.8 Di USD"),
        (204, "Tamron SP 24-70mm F2.8 Di USD"),
        (212, "Tamron 28-300mm F3.5-6.3 Di PZD"),
        (213, "Tamron 16-300mm F3.5-6.3 Di II PZD Macro"),
        (214, "Tamron SP 150-600mm F5-6.3 Di USD"),
        (215, "Tamron SP 15-30mm F2.8 Di USD"),
        (216, "Tamron SP 45mm F1.8 Di USD"),
        (217, "Tamron SP 35mm F1.8 Di USD"),
        (218, "Tamron SP 90mm F2.8 Di Macro 1:1 USD (F017)"),
        (220, "Tamron SP 150-600mm F5-6.3 Di USD G2"),
        (224, "Tamron SP 90mm F2.8 Di Macro 1:1 USD (F004)"),
        (255, "Tamron Lens (255)"),
        (
            18688,
            "Sigma MC-11 SA-E Mount Converter with not-supported Sigma lens",
        ),
        (25501, "Minolta AF 50mm F1.7"),
        (25511, "Minolta AF 35-70mm F4 or Other Lens"),
        (25521, "Minolta AF 28-85mm F3.5-4.5 or Other Lens"),
        (25531, "Minolta AF 28-135mm F4-4.5 or Other Lens"),
        (25541, "Minolta AF 35-105mm F3.5-4.5"),
        (25551, "Minolta AF 70-210mm F4 Macro or Sigma Lens"),
        (25561, "Minolta AF 135mm F2.8"),
        (25571, "Minolta/Sony AF 28mm F2.8"),
        (25581, "Minolta AF 24-50mm F4"),
        (25601, "Minolta AF 100-200mm F4.5"),
        (25611, "Minolta AF 75-300mm F4.5-5.6 or Sigma Lens"),
        (25621, "Minolta AF 50mm F1.4 [New]"),
        (25631, "Minolta AF 300mm F2.8 APO or Sigma Lens"),
        (25641, "Minolta AF 50mm F2.8 Macro or Sigma Lens"),
        (25651, "Minolta AF 600mm F4 APO"),
        (25661, "Minolta AF 24mm F2.8 or Sigma Lens"),
        (25721, "Minolta/Sony AF 500mm F8 Reflex"),
        (25781, "Minolta/Sony AF 16mm F2.8 Fisheye or Sigma Lens"),
        (25791, "Minolta/Sony AF 20mm F2.8 or Tokina Lens"),
        (
            25811,
            "Minolta AF 100mm F2.8 Macro [New] or Sigma or Tamron Lens",
        ),
        (25851, "Beroflex 35-135mm F3.5-4.5"),
        (25858, "Minolta AF 35-105mm F3.5-4.5 New or Tamron Lens"),
        (25881, "Minolta AF 70-210mm F3.5-4.5"),
        (25891, "Minolta AF 80-200mm F2.8 APO or Tokina Lens"),
        (
            25901,
            "Minolta AF 200mm F2.8 G APO + Minolta AF 1.4x APO or Other Lens + 1.4x",
        ),
        (25911, "Minolta AF 35mm F1.4"),
        (25921, "Minolta AF 85mm F1.4 G (D)"),
        (25931, "Minolta AF 200mm F2.8 APO"),
        (25941, "Minolta AF 3x-1x F1.7-2.8 Macro"),
        (25961, "Minolta AF 28mm F2"),
        (25971, "Minolta AF 35mm F2 [New]"),
        (25981, "Minolta AF 100mm F2"),
        (
            26011,
            "Minolta AF 200mm F2.8 G APO + Minolta AF 2x APO or Other Lens + 2x",
        ),
        (26041, "Minolta AF 80-200mm F4.5-5.6"),
        (26051, "Minolta AF 35-80mm F4-5.6"),
        (26061, "Minolta AF 100-300mm F4.5-5.6"),
        (26071, "Minolta AF 35-80mm F4-5.6"),
        (26081, "Minolta AF 300mm F2.8 HS-APO G"),
        (26091, "Minolta AF 600mm F4 HS-APO G"),
        (26121, "Minolta AF 200mm F2.8 HS-APO G"),
        (26131, "Minolta AF 50mm F1.7 New"),
        (26151, "Minolta AF 28-105mm F3.5-4.5 xi"),
        (26161, "Minolta AF 35-200mm F4.5-5.6 xi"),
        (26181, "Minolta AF 28-80mm F4-5.6 xi"),
        (26191, "Minolta AF 80-200mm F4.5-5.6 xi"),
        (26201, "Minolta AF 28-70mm F2.8 G"),
        (26211, "Minolta AF 100-300mm F4.5-5.6 xi"),
        (26241, "Minolta AF 35-80mm F4-5.6 Power Zoom"),
        (26281, "Minolta AF 80-200mm F2.8 HS-APO G"),
        (26291, "Minolta AF 85mm F1.4 New"),
        (26311, "Minolta AF 100-300mm F4.5-5.6 APO"),
        (26321, "Minolta AF 24-50mm F4 New"),
        (26381, "Minolta AF 50mm F2.8 Macro New"),
        (26391, "Minolta AF 100mm F2.8 Macro"),
        (26411, "Minolta/Sony AF 20mm F2.8 New"),
        (26421, "Minolta AF 24mm F2.8 New"),
        (26441, "Minolta AF 100-400mm F4.5-6.7 APO"),
        (26621, "Minolta AF 50mm F1.4 New"),
        (26671, "Minolta AF 35mm F2 New"),
        (26681, "Minolta AF 28mm F2 New"),
        (26721, "Minolta AF 24-105mm F3.5-4.5 (D)"),
        (30464, "Metabones Canon EF Speed Booster"),
        (45671, "Tokina 70-210mm F4-5.6"),
        (45681, "Tokina AF 35-200mm F4-5.6 Zoom SD"),
        (45701, "Tamron AF 35-135mm F3.5-4.5"),
        (45711, "Vivitar 70-210mm F4.5-5.6"),
        (45741, "2x Teleconverter or Tamron or Tokina Lens"),
        (45751, "1.4x Teleconverter"),
        (45851, "Tamron SP AF 300mm F2.8 LD IF"),
        (45861, "Tamron SP AF 35-105mm F2.8 LD Aspherical IF"),
        (45871, "Tamron AF 70-210mm F2.8 SP LD"),
        (48128, "Metabones Canon EF Speed Booster Ultra"),
        (61184, "Canon EF Adapter"),
        (65280, "Sigma 16mm F2.8 Filtermatic Fisheye"),
        (65535, "E-Mount, T-Mount, Other Lens or no lens"),
    ];

    /// Shared lookup structure over [`MINOLTA_LENSES`].
    pub static LENS_DB: StaticLensDb = StaticLensDb::new(&MINOLTA_LENSES);

    /// Resolves a Minolta lens id, or `None` when ExifTool has no name for it.
    pub fn lookup(lens_id: u16) -> Option<&'static str> {
        LENS_DB.lookup(lens_id)
    }
}
