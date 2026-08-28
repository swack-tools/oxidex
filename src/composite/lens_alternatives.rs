//! The ambiguous half of the manufacturer `LensType` lookups: for each id that
//! several lenses share, the alternatives ExifTool files under its fractional
//! keys.
//!
//! DO NOT EDIT BY HAND. Transcribed from the pinned ExifTool tree's own
//! in-memory Perl hashes (`.exiftool-version`, 13.59) by
//! `tools/exiftool-tables/dump_lens_alternatives.pl`.
//!
//! # Why this is a separate table, and why it is keyed by a string
//!
//! [`super::super::parsers::tiff::makernotes::lens_data`] carries the *integer*
//! keys of `%Image::ExifTool::Canon::canonLensTypes` and friends -- the 239
//! entries a plain `Canon:LensType` lookup needs. Its doc comment records that
//! the 296 fractional keys (`2.1`, `33.14`, `61182.68`, ...) were deliberately
//! left out, because "they belong with `Composite:LensID`, which oxidex does
//! not implement". This file is that missing half.
//!
//! Both ExifTool routines that consult the fractional keys build their
//! candidate list the same way -- `Canon.pm:10191-10196` and
//! `Exif.pm:5963-5970`:
//!
//! ```text
//!     $lens =~ s/ or .*//s;    # remove everything after "or"
//!     my @lenses = ( $lens );
//!     for ($i=1; $$printConv{"$lensType.$i"}; ++$i) {
//!         push @lenses, $$printConv{"$lensType.$i"};
//!     }
//! ```
//!
//! so the only handle either needs on the integer key is the string stored
//! there -- which is exactly what oxidex already emits as `Canon:LensType` /
//! `Pentax:LensType`. Keying this table by that string rather than by the
//! numeric id therefore loses nothing, and it means `Composite:LensID` needs no
//! new numeric plumbing through the makernote parsers, whose insert shim stores
//! the print form only.
//!
//! The keying is exact rather than convenient: `alternatives_keys_are_unique`
//! below pins that no two ambiguous ids share an integer-key string, in either
//! table, so the string determines the id's alternative list uniquely. (Canon
//! has 13 *unambiguous* ids that duplicate another id's string -- 129 and 151
//! both read "Canon EF 300mm f/2.8L USM" and so on -- but none of them has any
//! fractional key, and `PrintLensID` returns the shared string itself for all
//! of them, so they are indistinguishable by construction and this table never
//! sees them.)
/// `%Image::ExifTool::Canon::canonLensTypes` (Canon.pm:97): the 71 integer
/// ids that carry at least one `.N` alternative, keyed by the string at the
/// integer key, with the alternatives in ExifTool's own `.1 .. .N` order.
pub static CANON_LENS_ALTERNATIVES: [(&str, &[&str]); 71] = [
    // id 2
    (
        "Canon EF 28mm f/2.8 or Sigma Lens",
        &["Sigma 24mm f/2.8 Super Wide II"],
    ),
    // id 4
    (
        "Canon EF 35-105mm f/3.5-4.5 or Sigma Lens",
        &["Sigma UC Zoom 35-135mm f/4-5.6"],
    ),
    // id 6
    (
        "Canon EF 28-70mm f/3.5-4.5 or Sigma or Tokina Lens",
        &[
            "Sigma 18-50mm f/3.5-5.6 DC",
            "Sigma 18-125mm f/3.5-5.6 DC IF ASP",
            "Tokina AF 193-2 19-35mm f/3.5-4.5",
            "Sigma 28-80mm f/3.5-5.6 II Macro",
            "Sigma 28-300mm f/3.5-6.3 DG Macro",
        ],
    ),
    // id 8
    (
        "Canon EF 100-300mm f/5.6 or Sigma or Tokina Lens",
        &[
            "Sigma 70-300mm f/4-5.6 [APO] DG Macro",
            "Tokina AT-X 242 AF 24-200mm f/3.5-5.6",
        ],
    ),
    // id 9
    ("Canon EF 70-210mm f/4", &["Sigma 55-200mm f/4-5.6 DC"]),
    // id 10
    (
        "Canon EF 50mm f/2.5 Macro or Sigma Lens",
        &[
            "Sigma 50mm f/2.8 EX",
            "Sigma 28mm f/1.8",
            "Sigma 105mm f/2.8 Macro EX",
            "Sigma 70mm f/2.8 EX DG Macro EF",
        ],
    ),
    // id 22
    (
        "Canon EF 20-35mm f/2.8L or Tokina Lens",
        &["Tokina AT-X 280 AF Pro 28-80mm f/2.8 Aspherical"],
    ),
    // id 26
    (
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
    // id 28
    (
        "Canon EF 80-200mm f/4.5-5.6 or Tamron Lens",
        &[
            "Tamron SP AF 28-105mm f/2.8 LD Aspherical IF",
            "Tamron SP AF 28-75mm f/2.8 XR Di LD Aspherical [IF] Macro",
            "Tamron AF 70-300mm f/4-5.6 Di LD 1:2 Macro",
            "Tamron AF Aspherical 28-200mm f/3.8-5.6",
        ],
    ),
    // id 31
    (
        "Canon EF 75-300mm f/4-5.6 or Tamron Lens",
        &["Tamron SP AF 300mm f/2.8 LD IF"],
    ),
    // id 32
    (
        "Canon EF 24mm f/2.8 or Sigma Lens",
        &["Sigma 15mm f/2.8 EX Fisheye"],
    ),
    // id 33
    (
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
    // id 37
    (
        "Canon EF 35-80mm f/4-5.6 or Tamron Lens",
        &[
            "Tamron 70-200mm f/2.8 Di LD IF Macro",
            "Tamron AF 28-300mm f/3.5-6.3 XR Di VC LD Aspherical [IF] Macro (A20)",
            "Tamron SP AF 17-50mm f/2.8 XR Di II VC LD Aspherical [IF]",
            "Tamron AF 18-270mm f/3.5-6.3 Di II VC LD Aspherical [IF] Macro",
        ],
    ),
    // id 42
    (
        "Canon EF 28-200mm f/3.5-5.6 or Tamron Lens",
        &["Tamron AF 28-300mm f/3.5-6.3 XR Di VC LD Aspherical [IF] Macro (A20)"],
    ),
    // id 47
    (
        "Zeiss Milvus 35mm f/2 or 50mm f/2",
        &["Zeiss Milvus 50mm f/2 Makro", "Zeiss Milvus 135mm f/2 ZE"],
    ),
    // id 60
    ("Irix 11mm f/4 or 15mm f/2.4", &["Irix 15mm f/2.4"]),
    // id 103
    (
        "Samyang AF 14mm f/2.8 EF or Rokinon Lens",
        &["Rokinon SP 14mm f/2.4", "Rokinon AF 14mm f/2.8 EF"],
    ),
    // id 112
    (
        "Sigma 28mm f/1.5 FF High-speed Prime or other Sigma Lens",
        &[
            "Sigma 40mm f/1.5 FF High-speed Prime",
            "Sigma 105mm f/1.5 FF High-speed Prime",
        ],
    ),
    // id 117
    (
        "Tamron 35-150mm f/2.8-4.0 Di VC OSD (A043) or other Tamron Lens",
        &["Tamron SP 35mm f/1.4 Di USD (F045)"],
    ),
    // id 127
    (
        "Canon TS-E 90mm f/2.8 or Tamron Lens",
        &["Tamron 18-200mm f/3.5-6.3 Di II VC (B018)"],
    ),
    // id 131
    (
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
    // id 136
    (
        "Canon EF 300mm f/2.8L USM",
        &["Tamron SP 15-30mm f/2.8 Di VC USD (A012)"],
    ),
    // id 137
    (
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
    // id 143
    (
        "Canon EF 500mm f/4L IS USM or Sigma Lens",
        &["Sigma 17-70mm f/2.8-4 DC Macro OS HSM"],
    ),
    // id 150
    (
        "Canon EF 14mm f/2.8L USM or Sigma Lens",
        &[
            "Sigma 20mm EX f/1.8",
            "Sigma 30mm f/1.4 DC HSM",
            "Sigma 24mm f/1.8 DG Macro EX",
            "Sigma 28mm f/1.8 DG Macro EX",
            "Sigma 18-35mm f/1.8 DC HSM | A",
        ],
    ),
    // id 152
    (
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
        "Canon EF 20mm f/2.8 USM or Zeiss Lens",
        &[
            "Zeiss Milvus 21mm f/2.8",
            "Zeiss Milvus 15mm f/2.8 ZE",
            "Zeiss Milvus 18mm f/2.8 ZE",
        ],
    ),
    // id 155
    (
        "Canon EF 85mm f/1.8 USM or Sigma Lens",
        &["Sigma 14mm f/1.8 DG HSM | A"],
    ),
    // id 156
    (
        "Canon EF 28-105mm f/3.5-4.5 USM or Tamron Lens",
        &[
            "Tamron SP 70-300mm f/4-5.6 Di VC USD (A005)",
            "Tamron SP AF 28-105mm f/2.8 LD Aspherical IF (176D)",
        ],
    ),
    // id 160
    (
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
    // id 168
    (
        "Canon EF 28mm f/1.8 USM or Sigma Lens",
        &["Sigma 50-100mm f/1.8 DC HSM | A"],
    ),
    // id 169
    (
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
        "Canon EF 200mm f/2.8L II USM or Sigma Lens",
        &[
            "Sigma 300mm f/2.8 APO EX DG HSM",
            "Sigma 800mm f/5.6 APO EX DG HSM",
        ],
    ),
    // id 172
    (
        "Canon EF 400mm f/5.6L USM or Sigma Lens",
        &[
            "Sigma 150-600mm f/5-6.3 DG OS HSM | S",
            "Sigma 500mm f/4.5 APO EX DG HSM",
        ],
    ),
    // id 173
    (
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
    // id 180
    (
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
        "Canon EF 100-400mm f/4.5-5.6L IS USM + 1.4x or Sigma Lens",
        &["Sigma 150-600mm f/5-6.3 DG OS HSM | S + 1.4x"],
    ),
    // id 182
    (
        "Canon EF 100-400mm f/4.5-5.6L IS USM + 2x or Sigma Lens",
        &["Sigma 150-600mm f/5-6.3 DG OS HSM | S + 2x"],
    ),
    // id 183
    (
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
    // id 191
    (
        "Canon EF 400mm f/4 DO IS or Sigma Lens",
        &["Sigma 500mm f/4 DG OS HSM"],
    ),
    // id 197
    (
        "Canon EF 75-300mm f/4-5.6 IS USM or Sigma Lens",
        &["Sigma 18-300mm f/3.5-6.3 DC Macro OS HSM"],
    ),
    // id 198
    (
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
    // id 213
    (
        "Canon EF 90-300mm f/4.5-5.6 USM or Tamron Lens",
        &[
            "Tamron SP 150-600mm f/5-6.3 Di VC USD (A011)",
            "Tamron 16-300mm f/3.5-6.3 Di II VC PZD Macro (B016)",
            "Tamron SP 35mm f/1.8 Di VC USD (F012)",
            "Tamron SP 45mm f/1.8 Di VC USD (F013)",
        ],
    ),
    // id 231
    (
        "Canon EF 17-40mm f/4L USM or Sigma Lens",
        &["Sigma 12-24mm f/4 DG HSM A016"],
    ),
    // id 234
    (
        "Canon EF-S 17-85mm f/4-5.6 IS USM or Tokina Lens",
        &["Tokina AT-X 12-28 PRO DX 12-28mm f/4"],
    ),
    // id 239
    (
        "Canon EF 85mm f/1.2L II USM or Rokinon Lens",
        &["Rokinon SP 85mm f/1.2"],
    ),
    // id 240
    (
        "Canon EF-S 17-55mm f/2.8 IS USM or Sigma Lens",
        &["Sigma 17-50mm f/2.8 EX DC OS HSM"],
    ),
    // id 248
    (
        "Canon EF 200mm f/2L IS USM or Sigma Lens",
        &[
            "Sigma 24-35mm f/2 DG HSM | A",
            "Sigma 135mm f/2 FF High-Speed Prime | 017",
            "Sigma 24-35mm f/2.2 FF Zoom | 017",
            "Sigma 135mm f/1.8 DG HSM A017",
        ],
    ),
    // id 250
    (
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
        "Canon EF 70-200mm f/2.8L IS II USM",
        &["Canon EF 70-200mm f/2.8L IS III USM"],
    ),
    // id 252
    (
        "Canon EF 70-200mm f/2.8L IS II USM + 1.4x",
        &["Canon EF 70-200mm f/2.8L IS III USM + 1.4x"],
    ),
    // id 253
    (
        "Canon EF 70-200mm f/2.8L IS II USM + 2x",
        &["Canon EF 70-200mm f/2.8L IS III USM + 2x"],
    ),
    // id 254
    (
        "Canon EF 100mm f/2.8L Macro IS USM or Tamron Lens",
        &["Tamron SP 90mm f/2.8 Di VC USD 1:1 Macro (F017)"],
    ),
    // id 255
    (
        "Sigma 24-105mm f/4 DG OS HSM | A or Other Lens",
        &[
            "Sigma 180mm f/2.8 EX DG OS HSM APO Macro",
            "Tamron SP 70-200mm f/2.8 Di VC USD",
            "Yongnuo YN 50mm f/1.8",
        ],
    ),
    // id 368
    (
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
    // id 491
    (
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
    // id 493
    (
        "Canon EF 500mm f/4L IS II USM or EF 24-105mm f4L IS USM",
        &["Canon EF 24-105mm f/4L IS USM"],
    ),
    // id 495
    (
        "Canon EF 24-70mm f/2.8L II USM or Sigma Lens",
        &["Sigma 24-70mm f/2.8 DG OS HSM | A"],
    ),
    // id 502
    (
        "Canon EF 28mm f/2.8 IS USM or Tamron Lens",
        &["Tamron 35mm f/1.8 Di VC USD (F012)"],
    ),
    // id 508
    (
        "Canon EF 11-24mm f/4L USM or Tamron Lens",
        &["Tamron 10-24mm f/3.5-4.5 Di II VC HLD (B023)"],
    ),
    // id 624
    (
        "Sigma 70-200mm f/2.8 DG OS HSM | S or other Sigma Lens",
        &["Sigma 150-600mm f/5-6.3 | C"],
    ),
    // id 747
    (
        "Canon EF 100-400mm f/4.5-5.6L IS II USM or Tamron Lens",
        &["Tamron SP 150-600mm f/5-6.3 Di VC USD G2"],
    ),
    // id 748
    (
        "Canon EF 100-400mm f/4.5-5.6L IS II USM + 1.4x or Tamron Lens",
        &[
            "Tamron 100-400mm f/4.5-6.3 Di VC USD A035E + 1.4x",
            "Tamron 70-210mm f/4 Di VC USD (A034) + 2x",
        ],
    ),
    // id 749
    (
        "Canon EF 100-400mm f/4.5-5.6L IS II USM + 2x or Tamron Lens",
        &["Tamron 100-400mm f/4.5-6.3 Di VC USD A035E + 2x"],
    ),
    // id 750
    (
        "Canon EF 35mm f/1.4L II USM or Tamron Lens",
        &[
            "Tamron SP 85mm f/1.8 Di VC USD (F016)",
            "Tamron SP 45mm f/1.8 Di VC USD (F013)",
        ],
    ),
    // id 4143
    (
        "Canon EF-M 18-55mm f/3.5-5.6 IS STM or Tamron Lens",
        &["Tamron 18-200mm f/3.5-6.3 Di III VC"],
    ),
    // id 4208
    (
        "Sigma 56mm f/1.4 DC DN | C or other Sigma Lens",
        &["Sigma 30mm F1.4 DC DN | C"],
    ),
    // id 61182
    (
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
];

/// `%Image::ExifTool::Pentax::pentaxLensTypes` (Pentax.pm:118): the 14 ids
/// that carry at least one `.N` alternative, same shape as the Canon table.
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
