//! The ambiguous half of the manufacturer `LensType` lookups: for each id that
//! several lenses share, the alternatives ExifTool files under its fractional
//! keys.
//!
//! DO NOT EDIT BY HAND. Transcribed from the pinned ExifTool tree's own
//! in-memory Perl hashes (`.exiftool-version`, 13.59) by
//! `tools/exiftool-tables/dump_lens_alternatives.pl --full-file`.
//!
//! # Why this is a separate table, and how its keys preserve identity
//!
//! [`super::super::parsers::tiff::makernotes::lens_data`] carries the *integer*
//! keys of `%Image::ExifTool::Canon::canonLensTypes` and friends -- the 239
//! entries a plain `Canon:LensType` lookup needs. Its 296 fractional keys
//! belong to `Composite:LensID`; this file carries that missing half.
//!
//! Both upstream `PrintLensID` routines retain the raw maker lens ID:
//!
//! ```text
//!     $lens =~ s/ or .*//s;    # remove everything after "or"
//!     my @lenses = ( $lens );
//!     for ($i=1; $$printConv{"$lensType.$i"}; ++$i) {
//!         push @lenses, $$printConv{"$lensType.$i"};
//!     }
//! ```
//!
//! Canon must therefore keep the raw ID, base label and alternatives together.
//! Distinct IDs can share a label while only one has alternatives. Every base
//! row is included so a caller lacking the raw ID can still determine whether
//! its label has one unambiguous candidate set; it must omit when it cannot.
//!
//! Canon RF has a separate PrintConv table. Its complete raw-ID/label rows
//! are retained independently; the producer refuses any fractional RF growth.
//!
//! Pentax retains its existing label keys only after the producer checks every
//! base label, including IDs without alternatives: each ambiguous label must
//! identify exactly one base ID. Fractional chains must be contiguous truthy
//! strings, matching the upstream loop. Unsupported shapes are refused.
/// `%Image::ExifTool::Canon::canonLensTypes`: the 239 integer
/// ids, keyed by raw ID and retaining the exact base label. Empty slices
/// mean no alternatives; nonempty slices follow ExifTool's `.1 .. .N` order.
pub static CANON_LENS_ALTERNATIVES: [(i64, &str, &[&str]); 239] = [
    // id -1
    (-1, "n/a", &[]),
    // id 1
    (1, "Canon EF 50mm f/1.8", &[]),
    // id 2
    (
        2,
        "Canon EF 28mm f/2.8 or Sigma Lens",
        &["Sigma 24mm f/2.8 Super Wide II"],
    ),
    // id 3
    (3, "Canon EF 135mm f/2.8 Soft", &[]),
    // id 4
    (
        4,
        "Canon EF 35-105mm f/3.5-4.5 or Sigma Lens",
        &["Sigma UC Zoom 35-135mm f/4-5.6"],
    ),
    // id 5
    (5, "Canon EF 35-70mm f/3.5-4.5", &[]),
    // id 6
    (
        6,
        "Canon EF 28-70mm f/3.5-4.5 or Sigma or Tokina Lens",
        &[
            "Sigma 18-50mm f/3.5-5.6 DC",
            "Sigma 18-125mm f/3.5-5.6 DC IF ASP",
            "Tokina AF 193-2 19-35mm f/3.5-4.5",
            "Sigma 28-80mm f/3.5-5.6 II Macro",
            "Sigma 28-300mm f/3.5-6.3 DG Macro",
        ],
    ),
    // id 7
    (7, "Canon EF 100-300mm f/5.6L", &[]),
    // id 8
    (
        8,
        "Canon EF 100-300mm f/5.6 or Sigma or Tokina Lens",
        &[
            "Sigma 70-300mm f/4-5.6 [APO] DG Macro",
            "Tokina AT-X 242 AF 24-200mm f/3.5-5.6",
        ],
    ),
    // id 9
    (9, "Canon EF 70-210mm f/4", &["Sigma 55-200mm f/4-5.6 DC"]),
    // id 10
    (
        10,
        "Canon EF 50mm f/2.5 Macro or Sigma Lens",
        &[
            "Sigma 50mm f/2.8 EX",
            "Sigma 28mm f/1.8",
            "Sigma 105mm f/2.8 Macro EX",
            "Sigma 70mm f/2.8 EX DG Macro EF",
        ],
    ),
    // id 11
    (11, "Canon EF 35mm f/2", &[]),
    // id 13
    (13, "Canon EF 15mm f/2.8 Fisheye", &[]),
    // id 14
    (14, "Canon EF 50-200mm f/3.5-4.5L", &[]),
    // id 15
    (15, "Canon EF 50-200mm f/3.5-4.5", &[]),
    // id 16
    (16, "Canon EF 35-135mm f/3.5-4.5", &[]),
    // id 17
    (17, "Canon EF 35-70mm f/3.5-4.5A", &[]),
    // id 18
    (18, "Canon EF 28-70mm f/3.5-4.5", &[]),
    // id 20
    (20, "Canon EF 100-200mm f/4.5A", &[]),
    // id 21
    (21, "Canon EF 80-200mm f/2.8L", &[]),
    // id 22
    (
        22,
        "Canon EF 20-35mm f/2.8L or Tokina Lens",
        &["Tokina AT-X 280 AF Pro 28-80mm f/2.8 Aspherical"],
    ),
    // id 23
    (23, "Canon EF 35-105mm f/3.5-4.5", &[]),
    // id 24
    (24, "Canon EF 35-80mm f/4-5.6 Power Zoom", &[]),
    // id 25
    (25, "Canon EF 35-80mm f/4-5.6 Power Zoom", &[]),
    // id 26
    (
        26,
        "Canon EF 100mm f/2.8 Macro or Other Lens",
        &[
            "Cosina 100mm f/3.5 Macro AF",
            "Tamron SP AF 90mm f/2.8 Di Macro",
            "Tamron SP AF 180mm f/3.5 Di Macro",
            "Carl Zeiss Planar T* 50mm f/1.4",
            "Voigtlander APO Lanthar 125mm F2.5 SL Macro",
            "Carl Zeiss Planar T 85mm f/1.4 ZE",
        ],
    ),
    // id 27
    (27, "Canon EF 35-80mm f/4-5.6", &[]),
    // id 28
    (
        28,
        "Canon EF 80-200mm f/4.5-5.6 or Tamron Lens",
        &[
            "Tamron SP AF 28-105mm f/2.8 LD Aspherical IF",
            "Tamron SP AF 28-75mm f/2.8 XR Di LD Aspherical [IF] Macro",
            "Tamron AF 70-300mm f/4-5.6 Di LD 1:2 Macro",
            "Tamron AF Aspherical 28-200mm f/3.8-5.6",
        ],
    ),
    // id 29
    (29, "Canon EF 50mm f/1.8 II", &[]),
    // id 30
    (30, "Canon EF 35-105mm f/4.5-5.6", &[]),
    // id 31
    (
        31,
        "Canon EF 75-300mm f/4-5.6 or Tamron Lens",
        &["Tamron SP AF 300mm f/2.8 LD IF"],
    ),
    // id 32
    (
        32,
        "Canon EF 24mm f/2.8 or Sigma Lens",
        &["Sigma 15mm f/2.8 EX Fisheye"],
    ),
    // id 33
    (
        33,
        "Voigtlander or Carl Zeiss Lens",
        &[
            "Voigtlander Ultron 40mm f/2 SLII Aspherical",
            "Voigtlander Color Skopar 20mm f/3.5 SLII Aspherical",
            "Voigtlander APO-Lanthar 90mm f/3.5 SLII Close Focus",
            "Carl Zeiss Distagon T* 15mm f/2.8 ZE",
            "Carl Zeiss Distagon T* 18mm f/3.5 ZE",
            "Carl Zeiss Distagon T* 21mm f/2.8 ZE",
            "Carl Zeiss Distagon T* 25mm f/2 ZE",
            "Carl Zeiss Distagon T* 28mm f/2 ZE",
            "Carl Zeiss Distagon T* 35mm f/2 ZE",
            "Carl Zeiss Distagon T* 35mm f/1.4 ZE",
            "Carl Zeiss Planar T* 50mm f/1.4 ZE",
            "Carl Zeiss Makro-Planar T* 50mm f/2 ZE",
            "Carl Zeiss Makro-Planar T* 100mm f/2 ZE",
            "Carl Zeiss Apo-Sonnar T* 135mm f/2 ZE",
        ],
    ),
    // id 35
    (35, "Canon EF 35-80mm f/4-5.6", &[]),
    // id 36
    (36, "Canon EF 38-76mm f/4.5-5.6", &[]),
    // id 37
    (
        37,
        "Canon EF 35-80mm f/4-5.6 or Tamron Lens",
        &[
            "Tamron 70-200mm f/2.8 Di LD IF Macro",
            "Tamron AF 28-300mm f/3.5-6.3 XR Di VC LD Aspherical [IF] Macro (A20)",
            "Tamron SP AF 17-50mm f/2.8 XR Di II VC LD Aspherical [IF]",
            "Tamron AF 18-270mm f/3.5-6.3 Di II VC LD Aspherical [IF] Macro",
        ],
    ),
    // id 38
    (38, "Canon EF 80-200mm f/4.5-5.6 II", &[]),
    // id 39
    (39, "Canon EF 75-300mm f/4-5.6", &[]),
    // id 40
    (40, "Canon EF 28-80mm f/3.5-5.6", &[]),
    // id 41
    (41, "Canon EF 28-90mm f/4-5.6", &[]),
    // id 42
    (
        42,
        "Canon EF 28-200mm f/3.5-5.6 or Tamron Lens",
        &["Tamron AF 28-300mm f/3.5-6.3 XR Di VC LD Aspherical [IF] Macro (A20)"],
    ),
    // id 43
    (43, "Canon EF 28-105mm f/4-5.6", &[]),
    // id 44
    (44, "Canon EF 90-300mm f/4.5-5.6", &[]),
    // id 45
    (45, "Canon EF-S 18-55mm f/3.5-5.6 [II]", &[]),
    // id 46
    (46, "Canon EF 28-90mm f/4-5.6", &[]),
    // id 47
    (
        47,
        "Zeiss Milvus 35mm f/2 or 50mm f/2",
        &["Zeiss Milvus 50mm f/2 Makro", "Zeiss Milvus 135mm f/2 ZE"],
    ),
    // id 48
    (48, "Canon EF-S 18-55mm f/3.5-5.6 IS", &[]),
    // id 49
    (49, "Canon EF-S 55-250mm f/4-5.6 IS", &[]),
    // id 50
    (50, "Canon EF-S 18-200mm f/3.5-5.6 IS", &[]),
    // id 51
    (51, "Canon EF-S 18-135mm f/3.5-5.6 IS", &[]),
    // id 52
    (52, "Canon EF-S 18-55mm f/3.5-5.6 IS II", &[]),
    // id 53
    (53, "Canon EF-S 18-55mm f/3.5-5.6 III", &[]),
    // id 54
    (54, "Canon EF-S 55-250mm f/4-5.6 IS II", &[]),
    // id 60
    (60, "Irix 11mm f/4 or 15mm f/2.4", &["Irix 15mm f/2.4"]),
    // id 63
    (63, "Irix 30mm F1.4 Dragonfly", &[]),
    // id 80
    (80, "Canon TS-E 50mm f/2.8L Macro", &[]),
    // id 81
    (81, "Canon TS-E 90mm f/2.8L Macro", &[]),
    // id 82
    (82, "Canon TS-E 135mm f/4L Macro", &[]),
    // id 94
    (94, "Canon TS-E 17mm f/4L", &[]),
    // id 95
    (95, "Canon TS-E 24mm f/3.5L II", &[]),
    // id 103
    (
        103,
        "Samyang AF 14mm f/2.8 EF or Rokinon Lens",
        &["Rokinon SP 14mm f/2.4", "Rokinon AF 14mm f/2.8 EF"],
    ),
    // id 106
    (106, "Rokinon SP / Samyang XP 35mm f/1.2", &[]),
    // id 112
    (
        112,
        "Sigma 28mm f/1.5 FF High-speed Prime or other Sigma Lens",
        &[
            "Sigma 40mm f/1.5 FF High-speed Prime",
            "Sigma 105mm f/1.5 FF High-speed Prime",
        ],
    ),
    // id 117
    (
        117,
        "Tamron 35-150mm f/2.8-4.0 Di VC OSD (A043) or other Tamron Lens",
        &["Tamron SP 35mm f/1.4 Di USD (F045)"],
    ),
    // id 124
    (124, "Canon MP-E 65mm f/2.8 1-5x Macro Photo", &[]),
    // id 125
    (125, "Canon TS-E 24mm f/3.5L", &[]),
    // id 126
    (126, "Canon TS-E 45mm f/2.8", &[]),
    // id 127
    (
        127,
        "Canon TS-E 90mm f/2.8 or Tamron Lens",
        &["Tamron 18-200mm f/3.5-6.3 Di II VC (B018)"],
    ),
    // id 129
    (129, "Canon EF 300mm f/2.8L USM", &[]),
    // id 130
    (130, "Canon EF 50mm f/1.0L USM", &[]),
    // id 131
    (
        131,
        "Canon EF 28-80mm f/2.8-4L USM or Sigma Lens",
        &[
            "Sigma 8mm f/3.5 EX DG Circular Fisheye",
            "Sigma 17-35mm f/2.8-4 EX DG Aspherical HSM",
            "Sigma 17-70mm f/2.8-4.5 DC Macro",
            "Sigma APO 50-150mm f/2.8 [II] EX DC HSM",
            "Sigma APO 120-300mm f/2.8 EX DG HSM",
            "Sigma 4.5mm f/2.8 EX DC HSM Circular Fisheye",
            "Sigma 70-200mm f/2.8 APO EX HSM",
            "Sigma 28-70mm f/2.8-4 DG",
        ],
    ),
    // id 132
    (132, "Canon EF 1200mm f/5.6L USM", &[]),
    // id 134
    (134, "Canon EF 600mm f/4L IS USM", &[]),
    // id 135
    (135, "Canon EF 200mm f/1.8L USM", &[]),
    // id 136
    (
        136,
        "Canon EF 300mm f/2.8L USM",
        &["Tamron SP 15-30mm f/2.8 Di VC USD (A012)"],
    ),
    // id 137
    (
        137,
        "Canon EF 85mm f/1.2L USM or Sigma or Tamron Lens",
        &[
            "Sigma 18-50mm f/2.8-4.5 DC OS HSM",
            "Sigma 50-200mm f/4-5.6 DC OS HSM",
            "Sigma 18-250mm f/3.5-6.3 DC OS HSM",
            "Sigma 24-70mm f/2.8 IF EX DG HSM",
            "Sigma 18-125mm f/3.8-5.6 DC OS HSM",
            "Sigma 17-70mm f/2.8-4 DC Macro OS HSM | C",
            "Sigma 17-50mm f/2.8 OS HSM",
            "Sigma 18-200mm f/3.5-6.3 DC OS HSM [II]",
            "Tamron AF 18-270mm f/3.5-6.3 Di II VC PZD (B008)",
            "Sigma 8-16mm f/4.5-5.6 DC HSM",
            "Tamron SP 17-50mm f/2.8 XR Di II VC (B005)",
            "Tamron SP 60mm f/2 Macro Di II (G005)",
            "Sigma 10-20mm f/3.5 EX DC HSM",
            "Tamron SP 24-70mm f/2.8 Di VC USD",
            "Sigma 18-35mm f/1.8 DC HSM",
            "Sigma 12-24mm f/4.5-5.6 DG HSM II",
            "Sigma 70-300mm f/4-5.6 DG OS",
        ],
    ),
    // id 138
    (138, "Canon EF 28-80mm f/2.8-4L", &[]),
    // id 139
    (139, "Canon EF 400mm f/2.8L USM", &[]),
    // id 140
    (140, "Canon EF 500mm f/4.5L USM", &[]),
    // id 141
    (141, "Canon EF 500mm f/4.5L USM", &[]),
    // id 142
    (142, "Canon EF 300mm f/2.8L IS USM", &[]),
    // id 143
    (
        143,
        "Canon EF 500mm f/4L IS USM or Sigma Lens",
        &["Sigma 17-70mm f/2.8-4 DC Macro OS HSM"],
    ),
    // id 144
    (144, "Canon EF 35-135mm f/4-5.6 USM", &[]),
    // id 145
    (145, "Canon EF 100-300mm f/4.5-5.6 USM", &[]),
    // id 146
    (146, "Canon EF 70-210mm f/3.5-4.5 USM", &[]),
    // id 147
    (147, "Canon EF 35-135mm f/4-5.6 USM", &[]),
    // id 148
    (148, "Canon EF 28-80mm f/3.5-5.6 USM", &[]),
    // id 149
    (149, "Canon EF 100mm f/2 USM", &[]),
    // id 150
    (
        150,
        "Canon EF 14mm f/2.8L USM or Sigma Lens",
        &[
            "Sigma 20mm EX f/1.8",
            "Sigma 30mm f/1.4 DC HSM",
            "Sigma 24mm f/1.8 DG Macro EX",
            "Sigma 28mm f/1.8 DG Macro EX",
            "Sigma 18-35mm f/1.8 DC HSM | A",
        ],
    ),
    // id 151
    (151, "Canon EF 200mm f/2.8L USM", &[]),
    // id 152
    (
        152,
        "Canon EF 300mm f/4L IS USM or Sigma Lens",
        &[
            "Sigma 12-24mm f/4.5-5.6 EX DG ASPHERICAL HSM",
            "Sigma 14mm f/2.8 EX Aspherical HSM",
            "Sigma 10-20mm f/4-5.6",
            "Sigma 100-300mm f/4",
            "Sigma 300-800mm f/5.6 APO EX DG HSM",
        ],
    ),
    // id 153
    (
        153,
        "Canon EF 35-350mm f/3.5-5.6L USM or Sigma or Tamron Lens",
        &[
            "Sigma 50-500mm f/4-6.3 APO HSM EX",
            "Tamron AF 28-300mm f/3.5-6.3 XR LD Aspherical [IF] Macro",
            "Tamron AF 18-200mm f/3.5-6.3 XR Di II LD Aspherical [IF] Macro (A14)",
            "Tamron 18-250mm f/3.5-6.3 Di II LD Aspherical [IF] Macro",
        ],
    ),
    // id 154
    (
        154,
        "Canon EF 20mm f/2.8 USM or Zeiss Lens",
        &[
            "Zeiss Milvus 21mm f/2.8",
            "Zeiss Milvus 15mm f/2.8 ZE",
            "Zeiss Milvus 18mm f/2.8 ZE",
        ],
    ),
    // id 155
    (
        155,
        "Canon EF 85mm f/1.8 USM or Sigma Lens",
        &["Sigma 14mm f/1.8 DG HSM | A"],
    ),
    // id 156
    (
        156,
        "Canon EF 28-105mm f/3.5-4.5 USM or Tamron Lens",
        &[
            "Tamron SP 70-300mm f/4-5.6 Di VC USD (A005)",
            "Tamron SP AF 28-105mm f/2.8 LD Aspherical IF (176D)",
        ],
    ),
    // id 160
    (
        160,
        "Canon EF 20-35mm f/3.5-4.5 USM or Tamron or Tokina Lens",
        &[
            "Tamron AF 19-35mm f/3.5-4.5",
            "Tokina AT-X 124 AF Pro DX 12-24mm f/4",
            "Tokina AT-X 107 AF DX 10-17mm f/3.5-4.5 Fisheye",
            "Tokina AT-X 116 AF Pro DX 11-16mm f/2.8",
            "Tokina AT-X 11-20 F2.8 PRO DX Aspherical 11-20mm f/2.8",
        ],
    ),
    // id 161
    (
        161,
        "Canon EF 28-70mm f/2.8L USM or Other Lens",
        &[
            "Sigma 24-70mm f/2.8 EX",
            "Sigma 28-70mm f/2.8 EX",
            "Sigma 24-60mm f/2.8 EX DG",
            "Tamron AF 17-50mm f/2.8 Di-II LD Aspherical",
            "Tamron 90mm f/2.8",
            "Tamron SP AF 17-35mm f/2.8-4 Di LD Aspherical IF (A05)",
            "Tamron SP AF 28-75mm f/2.8 XR Di LD Aspherical [IF] Macro",
            "Tokina AT-X 24-70mm f/2.8 PRO FX (IF)",
        ],
    ),
    // id 162
    (162, "Canon EF 200mm f/2.8L USM", &[]),
    // id 163
    (163, "Canon EF 300mm f/4L", &[]),
    // id 164
    (164, "Canon EF 400mm f/5.6L", &[]),
    // id 165
    (165, "Canon EF 70-200mm f/2.8L USM", &[]),
    // id 166
    (166, "Canon EF 70-200mm f/2.8L USM + 1.4x", &[]),
    // id 167
    (167, "Canon EF 70-200mm f/2.8L USM + 2x", &[]),
    // id 168
    (
        168,
        "Canon EF 28mm f/1.8 USM or Sigma Lens",
        &["Sigma 50-100mm f/1.8 DC HSM | A"],
    ),
    // id 169
    (
        169,
        "Canon EF 17-35mm f/2.8L USM or Sigma Lens",
        &[
            "Sigma 18-200mm f/3.5-6.3 DC OS",
            "Sigma 15-30mm f/3.5-4.5 EX DG Aspherical",
            "Sigma 18-50mm f/2.8 Macro",
            "Sigma 50mm f/1.4 EX DG HSM",
            "Sigma 85mm f/1.4 EX DG HSM",
            "Sigma 30mm f/1.4 EX DC HSM",
            "Sigma 35mm f/1.4 DG HSM",
            "Sigma 35mm f/1.5 FF High-Speed Prime | 017",
            "Sigma 70mm f/2.8 Macro EX DG",
        ],
    ),
    // id 170
    (
        170,
        "Canon EF 200mm f/2.8L II USM or Sigma Lens",
        &[
            "Sigma 300mm f/2.8 APO EX DG HSM",
            "Sigma 800mm f/5.6 APO EX DG HSM",
        ],
    ),
    // id 171
    (171, "Canon EF 300mm f/4L USM", &[]),
    // id 172
    (
        172,
        "Canon EF 400mm f/5.6L USM or Sigma Lens",
        &[
            "Sigma 150-600mm f/5-6.3 DG OS HSM | S",
            "Sigma 500mm f/4.5 APO EX DG HSM",
        ],
    ),
    // id 173
    (
        173,
        "Canon EF 180mm Macro f/3.5L USM or Sigma Lens",
        &[
            "Sigma 180mm EX HSM Macro f/3.5",
            "Sigma APO Macro 150mm f/2.8 EX DG HSM",
            "Sigma 10mm f/2.8 EX DC Fisheye",
            "Sigma 15mm f/2.8 EX DG Diagonal Fisheye",
            "Venus Laowa 100mm F2.8 2X Ultra Macro APO",
        ],
    ),
    // id 174
    (
        174,
        "Canon EF 135mm f/2L USM or Other Lens",
        &[
            "Sigma 70-200mm f/2.8 EX DG APO OS HSM",
            "Sigma 50-500mm f/4.5-6.3 APO DG OS HSM",
            "Sigma 150-500mm f/5-6.3 APO DG OS HSM",
            "Zeiss Milvus 100mm f/2 Makro",
            "Sigma APO 50-150mm f/2.8 EX DC OS HSM",
            "Sigma APO 120-300mm f/2.8 EX DG OS HSM",
            "Sigma 120-300mm f/2.8 DG OS HSM S013",
            "Sigma 120-400mm f/4.5-5.6 APO DG OS HSM",
            "Sigma 200-500mm f/2.8 APO EX DG",
        ],
    ),
    // id 175
    (175, "Canon EF 400mm f/2.8L USM", &[]),
    // id 176
    (176, "Canon EF 24-85mm f/3.5-4.5 USM", &[]),
    // id 177
    (177, "Canon EF 300mm f/4L IS USM", &[]),
    // id 178
    (178, "Canon EF 28-135mm f/3.5-5.6 IS", &[]),
    // id 179
    (179, "Canon EF 24mm f/1.4L USM", &[]),
    // id 180
    (
        180,
        "Canon EF 35mm f/1.4L USM or Other Lens",
        &[
            "Sigma 50mm f/1.4 DG HSM | A",
            "Sigma 24mm f/1.4 DG HSM | A",
            "Zeiss Milvus 50mm f/1.4",
            "Zeiss Milvus 85mm f/1.4",
            "Zeiss Otus 28mm f/1.4 ZE",
            "Sigma 24mm f/1.5 FF High-Speed Prime | 017",
            "Sigma 50mm f/1.5 FF High-Speed Prime | 017",
            "Sigma 85mm f/1.5 FF High-Speed Prime | 017",
            "Tokina Opera 50mm f/1.4 FF",
            "Sigma 20mm f/1.4 DG HSM | A",
        ],
    ),
    // id 181
    (
        181,
        "Canon EF 100-400mm f/4.5-5.6L IS USM + 1.4x or Sigma Lens",
        &["Sigma 150-600mm f/5-6.3 DG OS HSM | S + 1.4x"],
    ),
    // id 182
    (
        182,
        "Canon EF 100-400mm f/4.5-5.6L IS USM + 2x or Sigma Lens",
        &["Sigma 150-600mm f/5-6.3 DG OS HSM | S + 2x"],
    ),
    // id 183
    (
        183,
        "Canon EF 100-400mm f/4.5-5.6L IS USM or Sigma Lens",
        &[
            "Sigma 150mm f/2.8 EX DG OS HSM APO Macro",
            "Sigma 105mm f/2.8 EX DG OS HSM Macro",
            "Sigma 180mm f/2.8 EX DG OS HSM APO Macro",
            "Sigma 150-600mm f/5-6.3 DG OS HSM | C",
            "Sigma 150-600mm f/5-6.3 DG OS HSM | S",
            "Sigma 100-400mm f/5-6.3 DG OS HSM",
            "Sigma 180mm f/3.5 APO Macro EX DG IF HSM",
        ],
    ),
    // id 184
    (184, "Canon EF 400mm f/2.8L USM + 2x", &[]),
    // id 185
    (185, "Canon EF 600mm f/4L IS USM", &[]),
    // id 186
    (186, "Canon EF 70-200mm f/4L USM", &[]),
    // id 187
    (187, "Canon EF 70-200mm f/4L USM + 1.4x", &[]),
    // id 188
    (188, "Canon EF 70-200mm f/4L USM + 2x", &[]),
    // id 189
    (189, "Canon EF 70-200mm f/4L USM + 2.8x", &[]),
    // id 190
    (190, "Canon EF 100mm f/2.8 Macro USM", &[]),
    // id 191
    (
        191,
        "Canon EF 400mm f/4 DO IS or Sigma Lens",
        &["Sigma 500mm f/4 DG OS HSM"],
    ),
    // id 193
    (193, "Canon EF 35-80mm f/4-5.6 USM", &[]),
    // id 194
    (194, "Canon EF 80-200mm f/4.5-5.6 USM", &[]),
    // id 195
    (195, "Canon EF 35-105mm f/4.5-5.6 USM", &[]),
    // id 196
    (196, "Canon EF 75-300mm f/4-5.6 USM", &[]),
    // id 197
    (
        197,
        "Canon EF 75-300mm f/4-5.6 IS USM or Sigma Lens",
        &["Sigma 18-300mm f/3.5-6.3 DC Macro OS HSM"],
    ),
    // id 198
    (
        198,
        "Canon EF 50mm f/1.4 USM or Other Lens",
        &[
            "Zeiss Otus 55mm f/1.4 ZE",
            "Zeiss Otus 85mm f/1.4 ZE",
            "Zeiss Milvus 25mm f/1.4",
            "Zeiss Otus 100mm f/1.4",
            "Zeiss Milvus 35mm f/1.4 ZE",
            "Yongnuo YN 35mm f/2",
        ],
    ),
    // id 199
    (199, "Canon EF 28-80mm f/3.5-5.6 USM", &[]),
    // id 200
    (200, "Canon EF 75-300mm f/4-5.6 USM", &[]),
    // id 201
    (201, "Canon EF 28-80mm f/3.5-5.6 USM", &[]),
    // id 202
    (202, "Canon EF 28-80mm f/3.5-5.6 USM IV", &[]),
    // id 208
    (208, "Canon EF 22-55mm f/4-5.6 USM", &[]),
    // id 209
    (209, "Canon EF 55-200mm f/4.5-5.6", &[]),
    // id 210
    (210, "Canon EF 28-90mm f/4-5.6 USM", &[]),
    // id 211
    (211, "Canon EF 28-200mm f/3.5-5.6 USM", &[]),
    // id 212
    (212, "Canon EF 28-105mm f/4-5.6 USM", &[]),
    // id 213
    (
        213,
        "Canon EF 90-300mm f/4.5-5.6 USM or Tamron Lens",
        &[
            "Tamron SP 150-600mm f/5-6.3 Di VC USD (A011)",
            "Tamron 16-300mm f/3.5-6.3 Di II VC PZD Macro (B016)",
            "Tamron SP 35mm f/1.8 Di VC USD (F012)",
            "Tamron SP 45mm f/1.8 Di VC USD (F013)",
        ],
    ),
    // id 214
    (214, "Canon EF-S 18-55mm f/3.5-5.6 USM", &[]),
    // id 215
    (215, "Canon EF 55-200mm f/4.5-5.6 II USM", &[]),
    // id 217
    (217, "Tamron AF 18-270mm f/3.5-6.3 Di II VC PZD", &[]),
    // id 220
    (220, "Yongnuo YN 50mm f/1.8", &[]),
    // id 224
    (224, "Canon EF 70-200mm f/2.8L IS USM", &[]),
    // id 225
    (225, "Canon EF 70-200mm f/2.8L IS USM + 1.4x", &[]),
    // id 226
    (226, "Canon EF 70-200mm f/2.8L IS USM + 2x", &[]),
    // id 227
    (227, "Canon EF 70-200mm f/2.8L IS USM + 2.8x", &[]),
    // id 228
    (228, "Canon EF 28-105mm f/3.5-4.5 USM", &[]),
    // id 229
    (229, "Canon EF 16-35mm f/2.8L USM", &[]),
    // id 230
    (230, "Canon EF 24-70mm f/2.8L USM", &[]),
    // id 231
    (
        231,
        "Canon EF 17-40mm f/4L USM or Sigma Lens",
        &["Sigma 12-24mm f/4 DG HSM A016"],
    ),
    // id 232
    (232, "Canon EF 70-300mm f/4.5-5.6 DO IS USM", &[]),
    // id 233
    (233, "Canon EF 28-300mm f/3.5-5.6L IS USM", &[]),
    // id 234
    (
        234,
        "Canon EF-S 17-85mm f/4-5.6 IS USM or Tokina Lens",
        &["Tokina AT-X 12-28 PRO DX 12-28mm f/4"],
    ),
    // id 235
    (235, "Canon EF-S 10-22mm f/3.5-4.5 USM", &[]),
    // id 236
    (236, "Canon EF-S 60mm f/2.8 Macro USM", &[]),
    // id 237
    (237, "Canon EF 24-105mm f/4L IS USM", &[]),
    // id 238
    (238, "Canon EF 70-300mm f/4-5.6 IS USM", &[]),
    // id 239
    (
        239,
        "Canon EF 85mm f/1.2L II USM or Rokinon Lens",
        &["Rokinon SP 85mm f/1.2"],
    ),
    // id 240
    (
        240,
        "Canon EF-S 17-55mm f/2.8 IS USM or Sigma Lens",
        &["Sigma 17-50mm f/2.8 EX DC OS HSM"],
    ),
    // id 241
    (241, "Canon EF 50mm f/1.2L USM", &[]),
    // id 242
    (242, "Canon EF 70-200mm f/4L IS USM", &[]),
    // id 243
    (243, "Canon EF 70-200mm f/4L IS USM + 1.4x", &[]),
    // id 244
    (244, "Canon EF 70-200mm f/4L IS USM + 2x", &[]),
    // id 245
    (245, "Canon EF 70-200mm f/4L IS USM + 2.8x", &[]),
    // id 246
    (246, "Canon EF 16-35mm f/2.8L II USM", &[]),
    // id 247
    (247, "Canon EF 14mm f/2.8L II USM", &[]),
    // id 248
    (
        248,
        "Canon EF 200mm f/2L IS USM or Sigma Lens",
        &[
            "Sigma 24-35mm f/2 DG HSM | A",
            "Sigma 135mm f/2 FF High-Speed Prime | 017",
            "Sigma 24-35mm f/2.2 FF Zoom | 017",
            "Sigma 135mm f/1.8 DG HSM A017",
        ],
    ),
    // id 249
    (249, "Canon EF 800mm f/5.6L IS USM", &[]),
    // id 250
    (
        250,
        "Canon EF 24mm f/1.4L II USM or Sigma Lens",
        &[
            "Sigma 20mm f/1.4 DG HSM | A",
            "Sigma 20mm f/1.5 FF High-Speed Prime | 017",
            "Tokina Opera 16-28mm f/2.8 FF",
            "Sigma 85mm f/1.4 DG HSM A016",
        ],
    ),
    // id 251
    (
        251,
        "Canon EF 70-200mm f/2.8L IS II USM",
        &["Canon EF 70-200mm f/2.8L IS III USM"],
    ),
    // id 252
    (
        252,
        "Canon EF 70-200mm f/2.8L IS II USM + 1.4x",
        &["Canon EF 70-200mm f/2.8L IS III USM + 1.4x"],
    ),
    // id 253
    (
        253,
        "Canon EF 70-200mm f/2.8L IS II USM + 2x",
        &["Canon EF 70-200mm f/2.8L IS III USM + 2x"],
    ),
    // id 254
    (
        254,
        "Canon EF 100mm f/2.8L Macro IS USM or Tamron Lens",
        &["Tamron SP 90mm f/2.8 Di VC USD 1:1 Macro (F017)"],
    ),
    // id 255
    (
        255,
        "Sigma 24-105mm f/4 DG OS HSM | A or Other Lens",
        &[
            "Sigma 180mm f/2.8 EX DG OS HSM APO Macro",
            "Tamron SP 70-200mm f/2.8 Di VC USD",
            "Yongnuo YN 50mm f/1.8",
        ],
    ),
    // id 368
    (
        368,
        "Sigma 14-24mm f/2.8 DG HSM | A or other Sigma Lens",
        &[
            "Sigma 20mm f/1.4 DG HSM | A",
            "Sigma 50mm f/1.4 DG HSM | A",
            "Sigma 40mm f/1.4 DG HSM | A",
            "Sigma 60-600mm f/4.5-6.3 DG OS HSM | S",
            "Sigma 28mm f/1.4 DG HSM | A",
            "Sigma 150-600mm f/5-6.3 DG OS HSM | S",
            "Sigma 85mm f/1.4 DG HSM | A",
            "Sigma 105mm f/1.4 DG HSM",
            "Sigma 14-24mm f/2.8 DG HSM",
            "Sigma 35mm f/1.4 DG HSM | A",
            "Sigma 70mm f/2.8 DG Macro",
            "Sigma 18-35mm f/1.8 DC HSM | A",
            "Sigma 24-105mm f/4 DG OS HSM | A",
            "Sigma 18-300mm f/3.5-6.3 DC Macro OS HSM | C",
            "Sigma 24mm F1.4 DG HSM | A",
        ],
    ),
    // id 488
    (488, "Canon EF-S 15-85mm f/3.5-5.6 IS USM", &[]),
    // id 489
    (489, "Canon EF 70-300mm f/4-5.6L IS USM", &[]),
    // id 490
    (490, "Canon EF 8-15mm f/4L Fisheye USM", &[]),
    // id 491
    (
        491,
        "Canon EF 300mm f/2.8L IS II USM or Tamron Lens",
        &[
            "Tamron SP 70-200mm f/2.8 Di VC USD G2 (A025)",
            "Tamron 18-400mm f/3.5-6.3 Di II VC HLD (B028)",
            "Tamron 100-400mm f/4.5-6.3 Di VC USD (A035)",
            "Tamron 70-210mm f/4 Di VC USD (A034)",
            "Tamron 70-210mm f/4 Di VC USD (A034) + 1.4x",
            "Tamron SP 24-70mm f/2.8 Di VC USD G2 (A032)",
        ],
    ),
    // id 492
    (492, "Canon EF 400mm f/2.8L IS II USM", &[]),
    // id 493
    (
        493,
        "Canon EF 500mm f/4L IS II USM or EF 24-105mm f4L IS USM",
        &["Canon EF 24-105mm f/4L IS USM"],
    ),
    // id 494
    (494, "Canon EF 600mm f/4L IS II USM", &[]),
    // id 495
    (
        495,
        "Canon EF 24-70mm f/2.8L II USM or Sigma Lens",
        &["Sigma 24-70mm f/2.8 DG OS HSM | A"],
    ),
    // id 496
    (496, "Canon EF 200-400mm f/4L IS USM", &[]),
    // id 499
    (499, "Canon EF 200-400mm f/4L IS USM + 1.4x", &[]),
    // id 502
    (
        502,
        "Canon EF 28mm f/2.8 IS USM or Tamron Lens",
        &["Tamron 35mm f/1.8 Di VC USD (F012)"],
    ),
    // id 503
    (503, "Canon EF 24mm f/2.8 IS USM", &[]),
    // id 504
    (504, "Canon EF 24-70mm f/4L IS USM", &[]),
    // id 505
    (505, "Canon EF 35mm f/2 IS USM", &[]),
    // id 506
    (506, "Canon EF 400mm f/4 DO IS II USM", &[]),
    // id 507
    (507, "Canon EF 16-35mm f/4L IS USM", &[]),
    // id 508
    (
        508,
        "Canon EF 11-24mm f/4L USM or Tamron Lens",
        &["Tamron 10-24mm f/3.5-4.5 Di II VC HLD (B023)"],
    ),
    // id 624
    (
        624,
        "Sigma 70-200mm f/2.8 DG OS HSM | S or other Sigma Lens",
        &["Sigma 150-600mm f/5-6.3 | C"],
    ),
    // id 747
    (
        747,
        "Canon EF 100-400mm f/4.5-5.6L IS II USM or Tamron Lens",
        &["Tamron SP 150-600mm f/5-6.3 Di VC USD G2"],
    ),
    // id 748
    (
        748,
        "Canon EF 100-400mm f/4.5-5.6L IS II USM + 1.4x or Tamron Lens",
        &[
            "Tamron 100-400mm f/4.5-6.3 Di VC USD A035E + 1.4x",
            "Tamron 70-210mm f/4 Di VC USD (A034) + 2x",
        ],
    ),
    // id 749
    (
        749,
        "Canon EF 100-400mm f/4.5-5.6L IS II USM + 2x or Tamron Lens",
        &["Tamron 100-400mm f/4.5-6.3 Di VC USD A035E + 2x"],
    ),
    // id 750
    (
        750,
        "Canon EF 35mm f/1.4L II USM or Tamron Lens",
        &[
            "Tamron SP 85mm f/1.8 Di VC USD (F016)",
            "Tamron SP 45mm f/1.8 Di VC USD (F013)",
        ],
    ),
    // id 751
    (751, "Canon EF 16-35mm f/2.8L III USM", &[]),
    // id 752
    (752, "Canon EF 24-105mm f/4L IS II USM", &[]),
    // id 753
    (753, "Canon EF 85mm f/1.4L IS USM", &[]),
    // id 754
    (754, "Canon EF 70-200mm f/4L IS II USM", &[]),
    // id 757
    (757, "Canon EF 400mm f/2.8L IS III USM", &[]),
    // id 758
    (758, "Canon EF 600mm f/4L IS III USM", &[]),
    // id 923
    (923, "Meike/SKY 85mm f/1.8 DCM", &[]),
    // id 1136
    (1136, "Sigma 24-70mm f/2.8 DG OS HSM | A", &[]),
    // id 4142
    (4142, "Canon EF-S 18-135mm f/3.5-5.6 IS STM", &[]),
    // id 4143
    (
        4143,
        "Canon EF-M 18-55mm f/3.5-5.6 IS STM or Tamron Lens",
        &["Tamron 18-200mm f/3.5-6.3 Di III VC"],
    ),
    // id 4144
    (4144, "Canon EF 40mm f/2.8 STM", &[]),
    // id 4145
    (4145, "Canon EF-M 22mm f/2 STM", &[]),
    // id 4146
    (4146, "Canon EF-S 18-55mm f/3.5-5.6 IS STM", &[]),
    // id 4147
    (4147, "Canon EF-M 11-22mm f/4-5.6 IS STM", &[]),
    // id 4148
    (4148, "Canon EF-S 55-250mm f/4-5.6 IS STM", &[]),
    // id 4149
    (4149, "Canon EF-M 55-200mm f/4.5-6.3 IS STM", &[]),
    // id 4150
    (4150, "Canon EF-S 10-18mm f/4.5-5.6 IS STM", &[]),
    // id 4152
    (4152, "Canon EF 24-105mm f/3.5-5.6 IS STM", &[]),
    // id 4153
    (4153, "Canon EF-M 15-45mm f/3.5-6.3 IS STM", &[]),
    // id 4154
    (4154, "Canon EF-S 24mm f/2.8 STM", &[]),
    // id 4155
    (4155, "Canon EF-M 28mm f/3.5 Macro IS STM", &[]),
    // id 4156
    (4156, "Canon EF 50mm f/1.8 STM", &[]),
    // id 4157
    (4157, "Canon EF-M 18-150mm f/3.5-6.3 IS STM", &[]),
    // id 4158
    (4158, "Canon EF-S 18-55mm f/4-5.6 IS STM", &[]),
    // id 4159
    (4159, "Canon EF-M 32mm f/1.4 STM", &[]),
    // id 4160
    (4160, "Canon EF-S 35mm f/2.8 Macro IS STM", &[]),
    // id 4208
    (
        4208,
        "Sigma 56mm f/1.4 DC DN | C or other Sigma Lens",
        &["Sigma 30mm F1.4 DC DN | C"],
    ),
    // id 4976
    (4976, "Sigma 16-300mm F3.5-6.7 DC OS | C (025)", &[]),
    // id 6512
    (6512, "Sigma 12mm F1.4 DC | C", &[]),
    // id 36910
    (36910, "Canon EF 70-300mm f/4-5.6 IS II USM", &[]),
    // id 36912
    (36912, "Canon EF-S 18-135mm f/3.5-5.6 IS USM", &[]),
    // id 61182
    (
        61182,
        "Canon RF 50mm F1.2L USM or other Canon RF Lens",
        &[
            "Canon RF 24-105mm F4L IS USM",
            "Canon RF 28-70mm F2L USM",
            "Canon RF 35mm F1.8 MACRO IS STM",
            "Canon RF 85mm F1.2L USM",
            "Canon RF 85mm F1.2L USM DS",
            "Canon RF 24-70mm F2.8L IS USM",
            "Canon RF 15-35mm F2.8L IS USM",
            "Canon RF 24-240mm F4-6.3 IS USM",
            "Canon RF 70-200mm F2.8L IS USM",
            "Canon RF 85mm F2 MACRO IS STM",
            "Canon RF 600mm F11 IS STM",
            "Canon RF 600mm F11 IS STM + RF1.4x",
            "Canon RF 600mm F11 IS STM + RF2x",
            "Canon RF 800mm F11 IS STM",
            "Canon RF 800mm F11 IS STM + RF1.4x",
            "Canon RF 800mm F11 IS STM + RF2x",
            "Canon RF 24-105mm F4-7.1 IS STM",
            "Canon RF 100-500mm F4.5-7.1L IS USM",
            "Canon RF 100-500mm F4.5-7.1L IS USM + RF1.4x",
            "Canon RF 100-500mm F4.5-7.1L IS USM + RF2x",
            "Canon RF 70-200mm F4L IS USM",
            "Canon RF 100mm F2.8L MACRO IS USM",
            "Canon RF 50mm F1.8 STM",
            "Canon RF 14-35mm F4L IS USM",
            "Canon RF-S 18-45mm F4.5-6.3 IS STM",
            "Canon RF 100-400mm F5.6-8 IS USM",
            "Canon RF 100-400mm F5.6-8 IS USM + RF1.4x",
            "Canon RF 100-400mm F5.6-8 IS USM + RF2x",
            "Canon RF-S 18-150mm F3.5-6.3 IS STM",
            "Canon RF 24mm F1.8 MACRO IS STM",
            "Canon RF 16mm F2.8 STM",
            "Canon RF 400mm F2.8L IS USM",
            "Canon RF 400mm F2.8L IS USM + RF1.4x",
            "Canon RF 400mm F2.8L IS USM + RF2x",
            "Canon RF 600mm F4L IS USM",
            "Canon RF 600mm F4L IS USM + RF1.4x",
            "Canon RF 600mm F4L IS USM + RF2x",
            "Canon RF 800mm F5.6L IS USM",
            "Canon RF 800mm F5.6L IS USM + RF1.4x",
            "Canon RF 800mm F5.6L IS USM + RF2x",
            "Canon RF 1200mm F8L IS USM",
            "Canon RF 1200mm F8L IS USM + RF1.4x",
            "Canon RF 1200mm F8L IS USM + RF2x",
            "Canon RF 5.2mm F2.8L Dual Fisheye 3D VR",
            "Canon RF 15-30mm F4.5-6.3 IS STM",
            "Canon RF 135mm F1.8 L IS USM",
            "Canon RF 24-50mm F4.5-6.3 IS STM",
            "Canon RF-S 55-210mm F5-7.1 IS STM",
            "Canon RF 100-300mm F2.8L IS USM",
            "Canon RF 100-300mm F2.8L IS USM + RF1.4x",
            "Canon RF 100-300mm F2.8L IS USM + RF2x",
            "Canon RF 10-20mm F4 L IS STM",
            "Canon RF 28mm F2.8 STM",
            "Canon RF 24-105mm F2.8 L IS USM Z",
            "Canon RF-S 10-18mm F4.5-6.3 IS STM",
            "Canon RF 35mm F1.4 L VCM",
            "Canon RF 70-200mm F2.8 L IS USM Z",
            "Canon RF 70-200mm F2.8 L IS USM Z + RF1.4x",
            "Canon RF 70-200mm F2.8 L IS USM Z + RF2x",
            "Canon RF 16-28mm F2.8 IS STM",
            "Canon RF-S 14-30mm F4-6.3 IS STM PZ",
            "Canon RF 50mm F1.4 L VCM",
            "Canon RF 24mm F1.4 L VCM",
            "Canon RF 20mm F1.4 L VCM",
            "Canon RF 85mm F1.4 L VCM",
            "Canon RF 20-50mm F4 L IS USM PZ",
            "Canon RF 45mm F1.2 STM",
            "Canon RF 7-14mm F2.8-3.5 L FISHEYE STM",
            "Canon RF 14mm F1.4 L VCM",
        ],
    ),
    // id 61491
    (61491, "Canon CN-E 14mm T3.1 L F", &[]),
    // id 61492
    (61492, "Canon CN-E 24mm T1.5 L F", &[]),
    // id 61494
    (61494, "Canon CN-E 85mm T1.3 L F", &[]),
    // id 61495
    (61495, "Canon CN-E 135mm T2.2 L F", &[]),
    // id 61496
    (61496, "Canon CN-E 35mm T1.5 L F", &[]),
    // id 65535
    (65535, "n/a", &[]),
];

/// Canon FileInfo RFLensType: all raw IDs and labels; no fractional alternatives.
pub static CANON_RF_LENS_ALTERNATIVES: [(i64, &str, &[&str]); 76] = [
    // id 0
    (0, "n/a", &[]),
    // id 257
    (257, "Canon RF 50mm F1.2L USM", &[]),
    // id 258
    (258, "Canon RF 24-105mm F4L IS USM", &[]),
    // id 259
    (259, "Canon RF 28-70mm F2L USM", &[]),
    // id 260
    (260, "Canon RF 35mm F1.8 MACRO IS STM", &[]),
    // id 261
    (261, "Canon RF 85mm F1.2L USM", &[]),
    // id 262
    (262, "Canon RF 85mm F1.2L USM DS", &[]),
    // id 263
    (263, "Canon RF 24-70mm F2.8L IS USM", &[]),
    // id 264
    (264, "Canon RF 15-35mm F2.8L IS USM", &[]),
    // id 265
    (265, "Canon RF 24-240mm F4-6.3 IS USM", &[]),
    // id 266
    (266, "Canon RF 70-200mm F2.8L IS USM", &[]),
    // id 267
    (267, "Canon RF 85mm F2 MACRO IS STM", &[]),
    // id 268
    (268, "Canon RF 600mm F11 IS STM", &[]),
    // id 269
    (269, "Canon RF 600mm F11 IS STM + RF1.4x", &[]),
    // id 270
    (270, "Canon RF 600mm F11 IS STM + RF2x", &[]),
    // id 271
    (271, "Canon RF 800mm F11 IS STM", &[]),
    // id 272
    (272, "Canon RF 800mm F11 IS STM + RF1.4x", &[]),
    // id 273
    (273, "Canon RF 800mm F11 IS STM + RF2x", &[]),
    // id 274
    (274, "Canon RF 24-105mm F4-7.1 IS STM", &[]),
    // id 275
    (275, "Canon RF 100-500mm F4.5-7.1L IS USM", &[]),
    // id 276
    (276, "Canon RF 100-500mm F4.5-7.1L IS USM + RF1.4x", &[]),
    // id 277
    (277, "Canon RF 100-500mm F4.5-7.1L IS USM + RF2x", &[]),
    // id 278
    (278, "Canon RF 70-200mm F4L IS USM", &[]),
    // id 279
    (279, "Canon RF 100mm F2.8L MACRO IS USM", &[]),
    // id 280
    (280, "Canon RF 50mm F1.8 STM", &[]),
    // id 281
    (281, "Canon RF 14-35mm F4L IS USM", &[]),
    // id 282
    (282, "Canon RF-S 18-45mm F4.5-6.3 IS STM", &[]),
    // id 283
    (283, "Canon RF 100-400mm F5.6-8 IS USM", &[]),
    // id 284
    (284, "Canon RF 100-400mm F5.6-8 IS USM + RF1.4x", &[]),
    // id 285
    (285, "Canon RF 100-400mm F5.6-8 IS USM + RF2x", &[]),
    // id 286
    (286, "Canon RF-S 18-150mm F3.5-6.3 IS STM", &[]),
    // id 287
    (287, "Canon RF 24mm F1.8 MACRO IS STM", &[]),
    // id 288
    (288, "Canon RF 16mm F2.8 STM", &[]),
    // id 289
    (289, "Canon RF 400mm F2.8L IS USM", &[]),
    // id 290
    (290, "Canon RF 400mm F2.8L IS USM + RF1.4x", &[]),
    // id 291
    (291, "Canon RF 400mm F2.8L IS USM + RF2x", &[]),
    // id 292
    (292, "Canon RF 600mm F4L IS USM", &[]),
    // id 293
    (293, "Canon RF 600mm F4L IS USM + RF1.4x", &[]),
    // id 294
    (294, "Canon RF 600mm F4L IS USM + RF2x", &[]),
    // id 295
    (295, "Canon RF 800mm F5.6L IS USM", &[]),
    // id 296
    (296, "Canon RF 800mm F5.6L IS USM + RF1.4x", &[]),
    // id 297
    (297, "Canon RF 800mm F5.6L IS USM + RF2x", &[]),
    // id 298
    (298, "Canon RF 1200mm F8L IS USM", &[]),
    // id 299
    (299, "Canon RF 1200mm F8L IS USM + RF1.4x", &[]),
    // id 300
    (300, "Canon RF 1200mm F8L IS USM + RF2x", &[]),
    // id 301
    (301, "Canon RF 5.2mm F2.8L Dual Fisheye 3D VR", &[]),
    // id 302
    (302, "Canon RF 15-30mm F4.5-6.3 IS STM", &[]),
    // id 303
    (303, "Canon RF 135mm F1.8 L IS USM", &[]),
    // id 304
    (304, "Canon RF 24-50mm F4.5-6.3 IS STM", &[]),
    // id 305
    (305, "Canon RF-S 55-210mm F5-7.1 IS STM", &[]),
    // id 306
    (306, "Canon RF 100-300mm F2.8L IS USM", &[]),
    // id 307
    (307, "Canon RF 100-300mm F2.8L IS USM + RF1.4x", &[]),
    // id 308
    (308, "Canon RF 100-300mm F2.8L IS USM + RF2x", &[]),
    // id 309
    (309, "Canon RF 200-800mm F6.3-9 IS USM", &[]),
    // id 310
    (310, "Canon RF 200-800mm F6.3-9 IS USM + RF1.4x", &[]),
    // id 311
    (311, "Canon RF 200-800mm F6.3-9 IS USM + RF2x", &[]),
    // id 312
    (312, "Canon RF 10-20mm F4 L IS STM", &[]),
    // id 313
    (313, "Canon RF 28mm F2.8 STM", &[]),
    // id 314
    (314, "Canon RF 24-105mm F2.8 L IS USM Z", &[]),
    // id 315
    (315, "Canon RF-S 10-18mm F4.5-6.3 IS STM", &[]),
    // id 316
    (316, "Canon RF 35mm F1.4 L VCM", &[]),
    // id 317
    (317, "Canon RF-S 3.9mm F3.5 STM DUAL FISHEYE", &[]),
    // id 318
    (318, "Canon RF 28-70mm F2.8 IS STM", &[]),
    // id 319
    (319, "Canon RF 70-200mm F2.8 L IS USM Z", &[]),
    // id 320
    (320, "Canon RF 70-200mm F2.8 L IS USM Z + RF1.4x", &[]),
    // id 321
    (321, "Canon RF 70-200mm F2.8 L IS USM Z + RF2x", &[]),
    // id 323
    (323, "Canon RF 16-28mm F2.8 IS STM", &[]),
    // id 324
    (324, "Canon RF-S 14-30mm F4-6.3 IS STM PZ", &[]),
    // id 325
    (325, "Canon RF 50mm F1.4 L VCM", &[]),
    // id 326
    (326, "Canon RF 24mm F1.4 L VCM", &[]),
    // id 327
    (327, "Canon RF 20mm F1.4 L VCM", &[]),
    // id 328
    (328, "Canon RF 85mm F1.4 L VCM", &[]),
    // id 329
    (329, "Canon RF 20-50mm F4 L IS USM PZ", &[]),
    // id 330
    (330, "Canon RF 45mm F1.2 STM", &[]),
    // id 331
    (331, "Canon RF 7-14mm F2.8-3.5 L FISHEYE STM", &[]),
    // id 332
    (332, "Canon RF 14mm F1.4 L VCM", &[]),
];

/// `%Image::ExifTool::Pentax::pentaxLensTypes`: the 14 ids
/// that carry at least one `.N` alternative, keyed by validated unique base label.
pub static PENTAX_LENS_ALTERNATIVES: [(&str, &[&str]); 14] = [
    // id 3 23
    (
        "smc PENTAX-F 100-300mm F4.5-5.6 or Sigma Lens",
        &[
            "Sigma AF 28-300mm F3.5-5.6 DL IF",
            "Sigma AF 28-300mm F3.5-6.3 DG IF Macro",
            "Tokina 80-200mm F2.8 ATX-Pro",
        ],
    ),
    // id 3 25
    (
        "smc PENTAX-F 35-105mm F4-5.6 or Sigma or Tokina Lens",
        &[
            "Sigma 55-200mm F4-5.6 DC",
            "Sigma AF 28-300mm F3.5-5.6 DL IF",
            "Sigma AF 28-300mm F3.5-6.3 DL IF",
            "Sigma AF 28-300mm F3.5-6.3 DG IF Macro",
            "Tokina 80-200mm F2.8 ATX-Pro",
        ],
    ),
    // id 3 255
    (
        "Sigma Lens (3 255)",
        &[
            "Sigma 18-200mm F3.5-6.3 DC",
            "Sigma DL-II 35-80mm F4-5.6",
            "Sigma DL Zoom 75-300mm F4-5.6",
            "Sigma DF EX Aspherical 28-70mm F2.8",
            "Sigma AF Tele 400mm F5.6 Multi-coated",
            "Sigma 24-60mm F2.8 EX DG",
            "Sigma 70-300mm F4-5.6 Macro",
            "Sigma 55-200mm F4-5.6 DC",
            "Sigma 18-50mm F2.8 EX DC",
        ],
    ),
    // id 3 27
    (
        "smc PENTAX-F 28-80mm F3.5-4.5 or Tokina Lens",
        &["Tokina AT-X Pro AF 28-70mm F2.6-2.8"],
    ),
    // id 3 28
    (
        "smc PENTAX-F 35-70mm F3.5-4.5 or Tokina Lens",
        &["Tokina 19-35mm F3.5-4.5 AF", "Tokina AT-X AF 400mm F5.6"],
    ),
    // id 3 29
    (
        "PENTAX-F 28-80mm F3.5-4.5 or Sigma or Tokina Lens",
        &[
            "Sigma AF 18-125mm F3.5-5.6 DC",
            "Tokina AT-X PRO 28-70mm F2.6-2.8",
        ],
    ),
    // id 3 31
    (
        "smc PENTAX-F 70-210mm F4-5.6 or Tokina or Takumar Lens",
        &[
            "Tokina AF 730 75-300mm F4.5-5.6",
            "Takumar-F 70-210mm F4-5.6",
        ],
    ),
    // id 3 41
    (
        "smc PENTAX-F Macro 50mm F2.8 or Sigma Lens",
        &["Sigma 50mm F2.8 Macro"],
    ),
    // id 3 44
    (
        "Sigma or Tamron Lens (3 44)",
        &[
            "Sigma AF 10-20mm F4-5.6 EX DC",
            "Sigma 12-24mm F4.5-5.6 EX DG",
            "Sigma 17-70mm F2.8-4.5 DC Macro",
            "Sigma 18-50mm F3.5-5.6 DC",
            "Sigma 17-35mm F2.8-4 EX DG",
            "Tamron 35-90mm F4-5.6 AF",
            "Sigma AF 18-35mm F3.5-4.5 Aspherical",
        ],
    ),
    // id 3 46
    (
        "Sigma or Samsung Lens (3 46)",
        &[
            "Sigma APO 70-200mm F2.8 EX",
            "Sigma EX APO 100-300mm F4 IF",
            "Samsung/Schneider D-XENON 50-200mm F4-5.6 ED",
        ],
    ),
    // id 3 52
    (
        "smc PENTAX-FA 28-200mm F3.8-5.6 AL[IF] or Tamron Lens",
        &["Tamron AF LD 28-200mm F3.8-5.6 [IF] Aspherical (171D)"],
    ),
    // id 4 26
    (
        "smc PENTAX-FA Macro 100mm F3.5 or Cosina Lens",
        &["Cosina 100mm F3.5 Macro"],
    ),
    // id 4 45
    (
        "Tamron Lens (4 45)",
        &[
            "Tamron 28-300mm F3.5-6.3 Ultra zoom XR",
            "Tamron AF 28-300mm F3.5-6.3 XR Di LD Aspherical [IF] Macro",
        ],
    ),
    // id 8 255
    (
        "Sigma Lens (8 255)",
        &[
            "Sigma 70-200mm F2.8 EX DG Macro HSM II",
            "Sigma 150-500mm F5-6.3 DG APO [OS] HSM",
            "Sigma 50-150mm F2.8 II APO EX DC HSM",
            "Sigma 4.5mm F2.8 EX DC HSM Circular Fisheye",
            "Sigma 50-200mm F4-5.6 DC OS",
            "Sigma 24-70mm F2.8 EX DG HSM",
        ],
    ),
];
