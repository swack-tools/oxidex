//! Olympus MakerNote directory tables, ported from ExifTool's `Olympus.pm`.
//!
//! One table per directory, because Olympus reuses tag IDs across directories
//! (0x0100 is `CameraType2` in Equipment, `PreviewImageValid` in
//! CameraSettings, `WB_RBLevels` in ImageProcessing). Tags whose ExifTool
//! conversion we cannot reproduce exactly are simply left out -- a dropped tag
//! is better than a wrong one.

use super::ifd::{Conv, ElemConv, OlyVal, TagDef, ftype};
use super::lookups::{CAMERA_TYPE2, EQUIPMENT_EXTENDER, EQUIPMENT_LENS_TYPE};

// ===========================================================================
// Shared PrintConv hashes
// ===========================================================================

/// `Olympus.pm` `%filters` -- shared by ArtFilter, MagicFilter and
/// RawDevArtFilter.
static FILTERS: &[(i64, &str)] = &[
    (0, "Off"),
    (1, "Soft Focus"),
    (2, "Pop Art"),
    (3, "Pale & Light Color"),
    (4, "Light Tone"),
    (5, "Pin Hole"),
    (6, "Grainy Film"),
    (8, "Underwater"),
    (9, "Diorama"),
    (10, "Cross Process"),
    (12, "Fish Eye"),
    (13, "Drawing"),
    (14, "Gentle Sepia"),
    (15, "Pale & Light Color II"),
    (16, "Pop Art II"),
    (17, "Pin Hole II"),
    (18, "Pin Hole III"),
    (19, "Grainy Film II"),
    (20, "Dramatic Tone"),
    (21, "Punk"),
    (22, "Soft Focus 2"),
    (23, "Sparkle"),
    (24, "Watercolor"),
    (25, "Key Line"),
    (26, "Key Line II"),
    (27, "Miniature"),
    (28, "Reflection"),
    (29, "Fragmented"),
    (31, "Cross Process II"),
    (32, "Dramatic Tone II"),
    (33, "Watercolor I"),
    (34, "Watercolor II"),
    (35, "Diorama II"),
    (36, "Vintage"),
    (37, "Vintage II"),
    (38, "Vintage III"),
    (39, "Partial Color"),
    (40, "Partial Color II"),
    (41, "Partial Color III"),
];

/// `Olympus.pm` `%toneLevelType`.
static TONE_LEVEL_TYPE: &[(i64, &str)] = &[
    (0, "0"),
    (-31999, "Highlights"),
    (-31998, "Shadows"),
    (-31997, "Midtones"),
];

/// PrintConv list shared by `ContrastSetting`, `SharpnessSetting`,
/// `CustomSaturation`, `PictureMode{Saturation,Contrast,Sharpness}`:
/// ExifTool renders these as `"$v[0] (min $v[1], max $v[2])"`.
fn print_min_max(val: &OlyVal) -> Option<String> {
    let v = val.ints()?;
    if v.len() < 3 {
        return None;
    }
    Some(format!("{} (min {}, max {})", v[0], v[1], v[2]))
}

/// `Olympus.pm` `PrintAFAreas`: each non-zero 32-bit word is four bytes of
/// big-endian corner coordinates, with three well-known words named.
fn print_af_areas(val: &OlyVal) -> Option<String> {
    let v = val.ints()?;
    let mut parts: Vec<String> = Vec::new();
    for &pt in v {
        if pt == 0 {
            continue;
        }
        let w = pt as u32;
        let name = match w {
            0x3679_4285 => "Left ",
            0x7979_8585 => "Center ",
            0xBD79_C985 => "Right ",
            _ => "",
        };
        let b = w.to_be_bytes();
        parts.push(format!("{}({},{})-({},{})", name, b[0], b[1], b[2], b[3]));
    }
    if parts.is_empty() {
        return Some("none".to_string());
    }
    Some(parts.join(", "))
}

/// `Olympus.pm` Equipment 0x0104 / 0x0204 / 0x0304 firmware version:
/// `sprintf("%x")` then a decimal point three digits from the right.
fn print_firmware(val: &OlyVal) -> Option<String> {
    let v = val.first_int()?;
    let mut s = format!("{:x}", v);
    if s.len() >= 3 {
        let at = s.len() - 3;
        s.insert(at, '.');
    }
    Some(s)
}

/// Equipment 0x0201 LensType: ExifTool keys the hash on
/// `sprintf("%x %.2x %.2x", @bytes[0,2,3])`.
fn print_lens_type(val: &OlyVal) -> Option<String> {
    let v = val.ints()?;
    if v.len() < 4 {
        return None;
    }
    let key = format!("{:x} {:02x} {:02x}", v[0], v[2], v[3]);
    Some(match EQUIPMENT_LENS_TYPE.iter().find(|(k, _)| *k == key) {
        Some((_, s)) => (*s).to_string(),
        None => format!("Unknown ({})", key),
    })
}

/// Equipment 0x0301 Extender: `sprintf("%x %.2x", @bytes[0,2])`.
fn print_extender(val: &OlyVal) -> Option<String> {
    let v = val.ints()?;
    if v.len() < 3 {
        return None;
    }
    let key = format!("{:x} {:02x}", v[0], v[2]);
    Some(match EQUIPMENT_EXTENDER.iter().find(|(k, _)| *k == key) {
        Some((_, s)) => (*s).to_string(),
        None => format!("Unknown ({})", key),
    })
}

/// CameraSettings 0x0405/0x0406: ExifTool prints all-`undef` rationals as
/// `n/a` (or `n/a (x4)` for four of them) and leaves anything else raw.
fn print_flash_strength(val: &OlyVal) -> Option<String> {
    let raw = val.print_raw();
    let n = val.len();
    if raw.split(' ').all(|p| p == "undef") {
        return Some(if n == 4 {
            "n/a (x4)".to_string()
        } else {
            "n/a".to_string()
        });
    }
    Some(raw)
}

/// CameraSettings 0x0901 ManometerReading: both values are tenths, printed as
/// `"<m> m, <ft> ft"`.
fn print_manometer_reading(val: &OlyVal) -> Option<String> {
    let v = val.ints()?;
    if v.len() < 2 {
        return None;
    }
    Some(format!(
        "{} m, {} ft",
        super::ifd::fmt_g15(v[0] as f64 / 10.0),
        super::ifd::fmt_g15(v[1] as f64 / 10.0)
    ))
}

/// Main 0x0200 SpecialMode: three values -- mode, sequence number, panorama
/// direction (`Olympus.pm` `PrintConv` sub).
fn print_special_mode(val: &OlyVal) -> Option<String> {
    let v = val.ints()?;
    if v.is_empty() {
        return None;
    }
    let modes = ["Normal", "Unknown", "Fast", "Panorama"];
    let dirs = [
        "(none)",
        "Left to Right",
        "Right to Left",
        "Bottom to Top",
        "Top to Bottom",
    ];
    let mode = match usize::try_from(v[0]).ok().and_then(|i| modes.get(i)) {
        Some(m) => (*m).to_string(),
        None => format!("Unknown ({})", v[0]),
    };
    let seq = v.get(1).copied().unwrap_or(0);
    let dir_raw = v.get(2).copied().unwrap_or(0);
    let dir = match usize::try_from(dir_raw).ok().and_then(|i| dirs.get(i)) {
        Some(d) => (*d).to_string(),
        None => format!("Unknown ({})", dir_raw),
    };
    Some(format!("{}, Sequence: {}, Panorama: {}", mode, seq, dir))
}

/// Main 0x0204 DigitalZoom: ExifTool appends `.0` when the value has no
/// decimal point.
fn print_digital_zoom(val: &OlyVal) -> Option<String> {
    let raw = val.print_raw();
    Some(if raw.contains('.') {
        raw
    } else {
        format!("{}.0", raw)
    })
}

/// FocusInfo 0x1209 ManualFlash: `Off`, or `On (1/N strength)`.
fn print_manual_flash(val: &OlyVal) -> Option<String> {
    let v = val.ints()?;
    if v.is_empty() {
        return None;
    }
    if v[0] == 0 {
        return Some("Off".to_string());
    }
    let b = *v.get(1)?;
    Some(if b == 1 {
        "On (Full strength)".to_string()
    } else {
        format!("On (1/{} strength)", b)
    })
}

/// CameraSettings 0x0305 AFPointSelected: five rationals, the first ignored;
/// any `undef` (zero denominator) means `n/a`, otherwise two percentages.
fn print_af_point_selected(val: &OlyVal) -> Option<String> {
    let OlyVal::Rat(r) = val else { return None };
    if r.len() < 5 {
        return None;
    }
    let rest = &r[1..];
    if rest.iter().any(|&(_, d)| d == 0) {
        return Some("n/a".to_string());
    }
    let pct: Vec<i64> = rest
        .iter()
        .map(|&(n, d)| (n as f64 / d as f64 * 100.0) as i64)
        .collect();
    Some(format!(
        "({}%,{}%) ({}%,{}%)",
        pct[0], pct[1], pct[2], pct[3]
    ))
}

/// CameraSettings 0x0600 DriveMode, ported from `Olympus.pm`'s PrintConv.
fn print_drive_mode(val: &OlyVal) -> Option<String> {
    let v = val.ints()?;
    if v.is_empty() {
        return None;
    }
    let (a, b, c, e, f) = (
        v[0],
        v.get(1).copied().unwrap_or(0),
        v.get(2).copied(),
        v.get(4).copied(),
        v.get(5).copied().unwrap_or(0),
    );
    let shot = if b != 0 {
        format!(", Shot {}", b)
    } else {
        String::new()
    };
    let shutter = match e {
        None | Some(4) => String::new(),
        Some(0) => "; Mechanical shutter".to_string(),
        Some(2) => "; Anti-shock".to_string(),
        Some(other) => format!("; Unknown ({})", other),
    };
    let mode = if a == 5 && c.is_some() {
        let bits = super::ifd::decode_bits(
            c.unwrap_or(0),
            &[
                (0, "AE"),
                (1, "WB"),
                (2, "FL"),
                (3, "MF"),
                (4, "ISO"),
                (5, "AE Auto"),
                (6, "Focus"),
            ],
        );
        format!("{} Bracketing", bits.replace(", ", "+"))
    } else if f != 0 {
        match DRIVE_MODE_BYTE6.iter().find(|(k, _)| *k == f) {
            Some((_, s)) => (*s).to_string(),
            None => format!("Unknown ({})", f),
        }
    } else {
        match DRIVE_MODE_BASIC.iter().find(|(k, _)| *k == a) {
            Some((_, s)) => (*s).to_string(),
            None => format!("Unknown ({})", a),
        }
    };
    Some(format!("{}{}{}", mode, shot, shutter))
}

static DRIVE_MODE_BASIC: &[(i64, &str)] = &[
    (0, "Single Shot"),
    (1, "Continuous Shooting"),
    (2, "Exposure Bracketing"),
    (3, "White Balance Bracketing"),
    (4, "Exposure+WB Bracketing"),
];

/// `Olympus.pm` DriveMode byte 6 (E-M1 and later).
static DRIVE_MODE_BYTE6: &[(i64, &str)] = &[
    (0x01, "Single Shot"),
    (0x02, "Sequential L"),
    (0x03, "Sequential H"),
    (0x07, "Sequential"),
    (0x11, "Single Shot"),
    (0x12, "Sequential L"),
    (0x13, "Sequential H"),
    (0x14, "Self-Timer 12 sec"),
    (0x15, "Self-Timer 2 sec"),
    (0x16, "Custom Self-Timer"),
    (0x17, "Sequential"),
    (0x21, "Single Shot"),
    (0x22, "Sequential L"),
    (0x23, "Sequential H"),
    (0x24, "Self-Timer 2 sec"),
    (0x25, "Self-Timer 12 sec"),
    (0x26, "Custom Self-Timer"),
    (0x27, "Sequential"),
    (0x28, "Sequential SH1"),
    (0x29, "Sequential SH2"),
    (0x30, "HighRes Shot"),
    (0x41, "ProCap H"),
    (0x42, "ProCap L"),
    (0x43, "ProCap"),
    (0x48, "ProCap SH1"),
    (0x49, "ProCap SH2"),
];

/// CameraSettings 0x0601 PanoramaMode: `Off`, or direction plus shot number.
fn print_panorama_mode(val: &OlyVal) -> Option<String> {
    let v = val.ints()?;
    if v.is_empty() {
        return None;
    }
    if v[0] == 0 {
        return Some("Off".to_string());
    }
    let dir = match v[0] {
        1 => "Left to Right".to_string(),
        2 => "Right to Left".to_string(),
        3 => "Bottom to Top".to_string(),
        4 => "Top to Bottom".to_string(),
        n => format!("Unknown ({})", n),
    };
    Some(format!("{}, Shot {}", dir, v.get(1).copied().unwrap_or(0)))
}

/// FocusInfo 0x1500 SensorTemperature, multi-value form: ExifTool drops a
/// trailing `" 0 0"` and appends ` C`. The single-value form needs the camera
/// model to pick between two conversions, so it is left to the caller.
fn print_sensor_temperature_multi(val: &OlyVal) -> Option<String> {
    let raw = val.print_raw();
    let trimmed = raw.strip_suffix(" 0 0").unwrap_or(&raw);
    Some(format!("{} C", trimmed))
}

// ===========================================================================
// Olympus::Main -- the rows the generated table cannot produce (slice I-2)
// ===========================================================================
//
// When `src/exiftool_tables/enabled_ifd.rs` lists `("Olympus", "Main")`, the
// top-level Main directory is walked by the IFD engine over the generated
// `IFD_OLYMPUS_MAIN` (`olympus.rs::parse_located`), and `MAIN` above is not
// walked at all -- walking both would insert every Main tag twice. This is
// the exact remainder: the `MAIN` rows the generator either withheld
// (`Omitted` set, so the engine never reports them) or refused to transcribe
// (no row at all), which therefore still have the hand conversion as their
// only producer. Nothing else may appear here: a row the engine does produce
// would be emitted twice, and a row ExifTool never prints (0x103f
// `FieldCount`, `Unknown => 1`, Olympus.pm:1157-1161) must stay out even
// though `MAIN` carries it. `main_residual_is_exactly_the_generated_tables_
// remainder` below pins this list against the generated table, so a
// regeneration that starts producing one of these fails loudly rather than
// double-emitting.
//
// * 0x0200 `SpecialMode` -- PrintConv is a Perl `sub` (Olympus.pm:684-694);
//   withheld as `omitted.print_conv`.
// * 0x0204 `DigitalZoom` -- PrintConv `'$val=~/\./ or $val.=".0"; $val'`
//   (Olympus.pm:752), outside the expression grammar; `omitted.print_conv`.
// * 0x0280 `PreviewImage` -- `%Image::ExifTool::previewImageTagInfo`
//   (Olympus.pm:788-793), a `RawConv` image validator; not transcribed.
// * 0x0f04 `ZoomedPreviewStart` / 0x0f05 `ZoomedPreviewLength` --
//   `OffsetPair`/`DataTag` (Olympus.pm:893-907); not transcribed (the design
//   spec's `IsOffset`/`OffsetPair`/`DataTag` refusal, section 6).
// * 0x1036 `PreviewImageStart` -- `Flags => 'IsOffset'` (Olympus.pm:1121-
//   1129); not transcribed. `olympus::absolutise_preview_image_start` still
//   reads the stored number this row yields.
//
// A second, deliberately separate class -- `ENGINE_MISRENDERS` -- holds rows
// the generated table DOES produce but renders differently from ExifTool
// today. The engine inserts its value first and the hand row here overwrites
// it (the residual walk runs after every engine walk), so the map ends with
// the hand rendering, which the pinned oracle confirms on the corpus. Each
// entry names the engine defect; the class exists so the defect is counted
// in one place and so this list can only SHRINK: when the engine is fixed
// the entry is removed here and from `ENGINE_MISRENDERS`, and the test below
// then requires the row to be engine-only again.
//
// The class is EMPTY as of the I-3 engine fixes (commit ccb0567f). It held
// three rows, kept here as the record of what the class is for:
//
// * 0x1015 `WBMode` -- `int16u[2]` with a PrintConv hash keyed by the
//   space-joined value (`'1 0' => 'Auto'`, ..., Olympus.pm:1020-1041).
//   `runtime::render`'s `StrEnum` arm took `DecodedValue::enum_key`, which
//   was `None` for a fixed-count `Array`, so the walk fell back to the raw
//   `"1 0"`; ExifTool looks the joined string up (`Auto`) and prints
//   `Unknown (1 1)` on a miss (20 corpus files). Fixed: `enum_key` keys an
//   `Array` by its elements' Perl text joined by one space (ExifTool.pm:6330,
//   3614-3616).
// * 0x0205 `FocalPlaneDiagonal` and 0x100c `ManualFocusDistance` --
//   `rational64u` with `PrintConv => '"$val mm"'` (Olympus.pm:755-760,
//   986-990). ExifTool's `$val` for a 64-bit rational is already
//   `RoundFloat($n/$d, 10)` = `sprintf("%.10g")` (`GetRational64u`,
//   ExifTool.pm:6114-6120, 5960-5964); the IFD engine handed `render`'s
//   `Expr` arm the exact quotient, which `perl_num` printed at 15 digits
//   (`OlympusFE-120.jpg`: `1.733823728e-07 mm` vs `1.73382372803657e-07
//   mm`, 5 corpus files). Fixed: `ifd_engine::round_rationals` reads every
//   nonzero-denominator rational as that ten-digit number before any
//   conversion.
//
// Retiring the three rows was measured with `conformance.py` over the
// 315-file Olympus directory against the pinned 13.59 oracle (control =
// the tree before the engine fixes, treatment = engine rendering alone);
// the numbers are in the commit that emptied the list.
pub static MAIN_RESIDUAL: &[TagDef] = &[
    TagDef::func(0x0200, "SpecialMode", print_special_mode),
    TagDef::func(0x0204, "DigitalZoom", print_digital_zoom),
    TagDef::binary(0x0280, "PreviewImage"),
    TagDef::raw(0x0F04, "ZoomedPreviewStart"),
    TagDef::raw(0x0F05, "ZoomedPreviewLength"),
    TagDef::raw(0x1036, "PreviewImageStart"),
];

/// The `MAIN_RESIDUAL` rows that override an engine rendering rather than
/// supply a withheld one -- see the second class in the comment above.
pub static ENGINE_MISRENDERS: &[u16] = &[];

// ===========================================================================
// Olympus::Main, re-entered via the MainInfoIFD sub-IFD (0x4000)
// ===========================================================================
//
// ExifTool's 0x4000 tag is model-conditional between an inline variant and
// `Name => 'MainInfoIFD', Flags => 'SubIFD', SubDirectory => { TagTable =>
// 'Image::ExifTool::Olympus::Main', Start => '$val' }` (Olympus.pm) -- a
// second, later-processed walk of the *same* Main table at a different
// offset. Neither Main's nor Equipment's 0x0104 entry sets `Priority`, so
// both default to priority 1 and ExifTool's FoundTag() (ExifTool.pm) lets the
// later-encountered value win outright, overwriting the earlier one under
// the plain `BodyFirmwareVersion` key. Since 0x4000 > 0x2010, MainInfoIFD is
// always walked after Equipment, so its copy wins whenever both are present
// -- e.g. OlympusSH-1.jpg: Equipment 0x0104 = 0x1003 ("1.003") is superseded
// by MainInfo 0x0104 = 0 ("0"), and `exiftool -G1 -s` prints only the latter.
//
// This table intentionally does NOT reuse the full `MAIN` table above for
// this second walk. Several of its entries -- ShutterSpeedValue (0x1000),
// ISOValue (0x1001), ApertureValue (0x1002), BrightnessValue (0x1003) and
// Sharpness (0x100F) -- carry `Priority => 0` in Olympus.pm, which flips
// FoundTag()'s resolution: a priority-0 duplicate does NOT overwrite an
// existing priority-1 value, so for those tags the *first* (top-level)
// occurrence wins, not the last. Reusing `MAIN` wholesale here would get
// that backwards for any file whose MainInfoIFD carries a different value
// for one of them. Until each such tag is individually verified, only the
// one tag actually in scope is re-declared.
pub static MAIN_INFO: &[TagDef] = &[TagDef::raw(0x0104, "BodyFirmwareVersion")];

// ===========================================================================
// Olympus::Equipment -- the rows the generated table cannot produce (I-3)
// ===========================================================================
//
// The same construction as `MAIN_RESIDUAL` above, for the `("Olympus",
// "Equipment")` line in `src/exiftool_tables/enabled_ifd.rs`. With that line
// in force the IFD engine walks the 0x2010 directory DURING the Main walk
// (`ifd_engine::descend` follows `IFD_OLYMPUS_MAIN`'s 0x2010 edge in entry
// order, which is ExifTool's order, Exif.pm:6919-7102), and
// `olympus.rs::parse_located`'s sub-IFD loop walks this list where it used to
// walk `EQUIPMENT` -- walking both would insert every Equipment tag twice.
// `equipment_residual_is_exactly_the_generated_tables_remainder` pins the
// list against `IFD_OLYMPUS_EQUIPMENT`, in both directions.
//
// Withheld by the generator (`Omitted` set, the engine reports nothing):
//
// * 0x0000 `EquipmentVersion` -- `RawConv => '$val=~s/\0+$//; $val'`
//   (Olympus.pm:1601), not the one `$$self{X} = $val` RawConv shape the
//   schema carries; `omitted.raw_conv`. The hand `text` row cuts at the
//   first NUL, which is the same string for a NUL-padded `undef[4]`.
// * 0x0101 `SerialNumber`, 0x0202 `LensSerialNumber` -- `PrintConv =>
//   '$val=~s/\s+$//;$val'` (Olympus.pm:1615, 1666), a substitution the
//   expression grammar does not compile; `omitted.print_conv`.
// * 0x0104 `BodyFirmwareVersion`, 0x0204 `LensFirmwareVersion`, 0x0304
//   `ExtenderFirmwareVersion`, 0x1002 `FlashFirmwareVersion` -- `PrintConv =>
//   '$val=sprintf("%x",$val);$val=~s/(.{3})$/\.$1/;$val'` (Olympus.pm:1633,
//   1673, 1730, 1770); `omitted.print_conv`. `print_firmware`.
// * 0x0201 `LensType`, 0x0301 `Extender` -- `ValueConv => 'my @a=split("
//   ",$val); sprintf("%x %.2x %.2x",@a[0,2,3])'` / `sprintf("%x
//   %.2x",@a[0,2])` (Olympus.pm:1649, 1716) feeding a string-keyed hash;
//   `omitted.value_conv`. `print_lens_type` / `print_extender`.
//
// RETIRED (slice I-3 integration): the engine reads a 64-bit rational as
// `RoundFloat($n/$d, 10)` before any conversion since commit ccb0567f, so the
// row left this residual and the list below is empty (measured identical;
// see the retiring commit). The record above stays as the class's history.
// Override (`EQUIPMENT_ENGINE_MISRENDERS`, the second class of the
// `MAIN_RESIDUAL` comment: the engine reports the row, the hand rendering
// lands last):
//
// * 0x0103 `FocalPlaneDiagonal` -- `rational64u` with `PrintConv => '"$val
//   mm"'` (Olympus.pm:1627): the same defect as `Main` 0x0205 above (the
//   engine interpolates the exact quotient at 15 digits, ExifTool
//   `RoundFloat`s it to 10 first, ExifTool.pm:6114-6120). No corpus carrier
//   stores a non-terminating quotient in THIS table (every Equipment
//   FocalPlaneDiagonal under `combined-samples/Olympus` is a short decimal
//   such as 21.6 mm or 9.25 mm, `exiftool-pinned.sh -a -G1 -s`; the build
//   with all six override rows removed moved 0 rows for this tag under
//   `conformance.py`), so like Main's 0x100c the entry stands on the shape
//   alone. It also settles an
//   ORDER: `MAIN_RESIDUAL`'s 0x0205 override runs after the engine walk and
//   would otherwise leave the Main copy over Equipment's, where ExifTool --
//   reaching 0x2010 after 0x0205 -- reports Equipment's last
//   (`t/images/OlympusE1.jpg` carries both, 21.6 mm each).
pub static EQUIPMENT_RESIDUAL: &[TagDef] = &[
    TagDef::text(0x0000, "EquipmentVersion"),
    // 0x0101 SerialNumber and 0x0202 LensSerialNumber carry
    // `PrintConv => '$val=~s/\s+$//;$val'` (Olympus.pm:1615, 1666); slice
    // I-3's trim-end form compiles it, the i7 oracle approved it, and the
    // regenerated table produces both rows, so their hand rows are gone.
    TagDef::func(0x0104, "BodyFirmwareVersion", print_firmware),
    TagDef::func(0x0201, "LensType", print_lens_type),
    TagDef::func(0x0204, "LensFirmwareVersion", print_firmware),
    TagDef::func(0x0301, "Extender", print_extender),
    TagDef::func(0x0304, "ExtenderFirmwareVersion", print_firmware),
    TagDef::func(0x1002, "FlashFirmwareVersion", print_firmware),
];

/// The `EQUIPMENT_RESIDUAL` rows that override an engine rendering rather
/// than supply a withheld one -- see the comment above.
pub static EQUIPMENT_ENGINE_MISRENDERS: &[u16] = &[];

// ===========================================================================
// Olympus::CameraSettings (0x2020)
// ===========================================================================

static CS_FOCUS_MODE_1: &[(i64, &str)] = &[
    (0, "Single AF"),
    (1, "Sequential shooting AF"),
    (2, "Continuous AF"),
    (3, "Multi AF"),
    (4, "Face Detect"),
    (10, "MF"),
];

static CS_FOCUS_MODE_BITS: &[(u32, &str)] = &[
    (0, "S-AF"),
    (2, "C-AF"),
    (4, "MF"),
    (5, "Face Detect"),
    (6, "Imager AF"),
    (7, "Live View Magnification Frame"),
    (8, "AF sensor"),
    (9, "Starry Sky AF"),
];

static CS_PICTURE_MODE: &[(i64, &str)] = &[
    (1, "Vivid"),
    (2, "Natural"),
    (3, "Muted"),
    (4, "Portrait"),
    (5, "i-Enhance"),
    (6, "e-Portrait"),
    (7, "Color Creator"),
    (8, "Underwater"),
    (9, "Color Profile 1"),
    (10, "Color Profile 2"),
    (11, "Color Profile 3"),
    (12, "Monochrome Profile 1"),
    (13, "Monochrome Profile 2"),
    (14, "Monochrome Profile 3"),
    (17, "Art Mode"),
    (18, "Monochrome Profile 4"),
    (256, "Monotone"),
    (512, "Sepia"),
];

static CS_GRADATION_HEAD: &[(&str, &str)] = &[
    ("0 0 0", "n/a"),
    ("-1 -1 1", "Low Key"),
    ("0 -1 1", "Normal"),
    ("1 -1 1", "High Key"),
];

static CS_ART_FILTER_LIST: &[ElemConv] = &[ElemConv::Map(FILTERS)];

/// `CameraSettings` 0x0821 `ISOAutoSettings` -- "2 numbers: 1. Default
/// sensitivty, 2. Maximum sensitivity", each converted through the same
/// sensitivity hash (`Olympus.pm:2683` declares it twice, once per element).
static CS_ISO_AUTO_SENSITIVITY: &[(i64, &str)] = &[
    (0x0000, "n/a"),
    (0x0600, "200"),
    (0x0655, "250"),
    (0x06aa, "320"),
    (0x0700, "400"),
    (0x0755, "500"),
    (0x07aa, "640"),
    (0x0800, "800"),
    (0x0855, "1000"),
    (0x08aa, "1250"),
    (0x0900, "1600"),
    (0x0955, "2000"),
    (0x09aa, "2500"),
    (0x0a00, "3200"),
    (0x0a55, "4000"),
    (0x0aaa, "5000"),
    (0x0b00, "6400"),
    (0x0b55, "8000"),
    (0x0baa, "10000"),
    (0x0c00, "12800"),
    (0x0c55, "16000"),
    (0x0caa, "20000"),
    (0x0d00, "25600"),
    (0x0d55, "32000"),
    (0x0daa, "40000"),
    (0x0e00, "51200"),
    (0x0e55, "64000"),
    (0x0eaa, "80000"),
    (0x0f00, "102400"),
];

static CS_ISO_AUTO_SETTINGS_LIST: &[ElemConv] = &[
    ElemConv::Map(CS_ISO_AUTO_SENSITIVITY),
    ElemConv::Map(CS_ISO_AUTO_SENSITIVITY),
];

static CS_ART_FILTER_EFFECT_LIST: &[ElemConv] = &[
    ElemConv::Map(FILTERS),
    ElemConv::Raw,
    ElemConv::Raw,
    ElemConv::Prefix("Partial Color"),
    ElemConv::Map(&[
        (0x0000, "No Effect"),
        (0x8010, "Star Light"),
        (0x8020, "Pin Hole"),
        (0x8030, "Frame"),
        (0x8040, "Soft Focus"),
        (0x8050, "White Edge"),
        (0x8060, "B&W"),
        (0x8080, "Blur Top and Bottom"),
        (0x8081, "Blur Left and Right"),
    ]),
    ElemConv::Raw,
    ElemConv::Map(&[
        (0, "No Color Filter"),
        (1, "Yellow Color Filter"),
        (2, "Orange Color Filter"),
        (3, "Red Color Filter"),
        (4, "Green Color Filter"),
    ]),
];

static CS_TONE_LEVEL_LIST: &[ElemConv] = &[
    ElemConv::Map(TONE_LEVEL_TYPE),
    ElemConv::Raw,
    ElemConv::Raw,
    ElemConv::Raw,
    ElemConv::Map(TONE_LEVEL_TYPE),
    ElemConv::Raw,
    ElemConv::Raw,
    ElemConv::Raw,
    ElemConv::Map(TONE_LEVEL_TYPE),
    ElemConv::Raw,
    ElemConv::Raw,
    ElemConv::Raw,
    ElemConv::Map(TONE_LEVEL_TYPE),
    ElemConv::Raw,
    ElemConv::Raw,
    ElemConv::Raw,
    ElemConv::Map(TONE_LEVEL_TYPE),
    ElemConv::Raw,
    ElemConv::Raw,
    ElemConv::Raw,
    ElemConv::Map(TONE_LEVEL_TYPE),
    ElemConv::Raw,
    ElemConv::Raw,
    ElemConv::Raw,
    ElemConv::Map(TONE_LEVEL_TYPE),
    ElemConv::Raw,
    ElemConv::Raw,
    ElemConv::Raw,
];

static CS_COLOR_CREATOR_LIST: &[ElemConv] = &[
    ElemConv::Prefix("Color"),
    ElemConv::Raw,
    ElemConv::Raw,
    ElemConv::Prefix("Strength"),
    ElemConv::Raw,
    ElemConv::Raw,
];

static CS_MONO_PROFILE_LIST: &[ElemConv] = &[
    ElemConv::Map(&[
        (0, "No Filter"),
        (1, "Yellow Filter"),
        (2, "Orange Filter"),
        (3, "Red Filter"),
        (4, "Magenta Filter"),
        (5, "Blue Filter"),
        (6, "Cyan Filter"),
        (7, "Green Filter"),
        (8, "Yellow-green Filter"),
    ]),
    ElemConv::Raw,
    ElemConv::Raw,
    ElemConv::Prefix("Strength"),
    ElemConv::Raw,
    ElemConv::Raw,
];

static CS_COLOR_PROFILE_LIST: &[ElemConv] = &[
    ElemConv::Prefix("Min"),
    ElemConv::Prefix("Max"),
    ElemConv::Prefix("Yellow"),
    ElemConv::Prefix("Orange"),
    ElemConv::Prefix("Orange-red"),
    ElemConv::Prefix("Red"),
    ElemConv::Prefix("Magenta"),
    ElemConv::Prefix("Violet"),
    ElemConv::Prefix("Blue"),
    ElemConv::Prefix("Blue-cyan"),
    ElemConv::Prefix("Cyan"),
    ElemConv::Prefix("Green-cyan"),
    ElemConv::Prefix("Green"),
    ElemConv::Prefix("Yellow-green"),
];

static CS_FOCUS_MODE_LIST: &[ElemConv] = &[
    ElemConv::Map(CS_FOCUS_MODE_1),
    ElemConv::Bits {
        map: &[(0, "(none)")],
        bits: CS_FOCUS_MODE_BITS,
    },
];

static CS_FOCUS_PROCESS_LIST: &[ElemConv] = &[ElemConv::Map(&[(0, "AF Not Used"), (1, "AF Used")])];

static CS_PICTURE_MODE_LIST: &[ElemConv] = &[ElemConv::Map(CS_PICTURE_MODE)];

static CS_GRADATION_CONVS: &[ElemConv] = &[
    ElemConv::StrMap(CS_GRADATION_HEAD),
    ElemConv::Map(&[(0, "User-Selected"), (1, "Auto-Override")]),
];

/// Olympus.pm:2019-2024, `CameraSettings` 0x0404 `FlashControlMode` -- the
/// first element's hash; the rest print raw.
static CS_FLASH_CONTROL_MODE_LIST: &[ElemConv] = &[ElemConv::Map(&[
    (0, "Off"),
    (1, "TTL"),
    (2, "Auto"),
    (3, "Manual"),
])];

// ===========================================================================
// Olympus::CameraSettings -- the rows the generated table cannot produce (I-3)
// ===========================================================================
//
// For the `("Olympus", "CameraSettings")` line; see `EQUIPMENT_RESIDUAL` for
// the construction. `camera_settings_residual_is_exactly_the_generated_
// tables_remainder` pins it against `IFD_OLYMPUS_CAMERASETTINGS`.
//
// Not transcribed at all (no row in the generated table):
//
// * 0x0101 `PreviewImageStart`, 0x0102 `PreviewImageLength` -- `Flags =>
//   'IsOffset'` / `OffsetPair` (Olympus.pm:1793-1809), the design spec's
//   section-6 refusal. The stored number this row yields is what
//   `olympus::absolutise_preview_image_start` rebases, exactly as before.
//
// Withheld (`Omitted` set):
//
// * 0x0000 `CameraSettingsVersion` -- `RawConv => '$val=~s/\0+$//; $val'`
//   (Olympus.pm:1785); `omitted.raw_conv`.
// * 0x0301 `FocusMode`, 0x0302 `FocusProcess`, 0x0404 `FlashControlMode`,
//   0x0520 `PictureMode`, 0x0529 `ArtFilter`, 0x052c `MagicFilter`, 0x052e
//   `ToneLevel`, 0x052f `ArtFilterEffect`, 0x0532 `ColorCreatorEffect`,
//   0x0537 `MonochromeProfileSettings`, 0x0539 `ColorProfileSettings`,
//   0x0821 `ISOAutoSettings` -- `PrintConv => [ ... ]`, ExifTool's
//   per-element list form (Olympus.pm:1857, 1883, 2019, 2252, 2346, 2354,
//   2369, 2406, 2437, 2450, 2483, 2683), which the generator does not
//   model; `omitted.print_conv`. `Conv::List`.
// * 0x050f `Gradation` -- the list form under `Relist => [ [0..2], 3 ]`
//   (Olympus.pm:2236-2237); `Conv::Relist`.
// * 0x0304 `AFAreas` -- `PrintConv => 'Image::ExifTool::Olympus::
//   PrintAFAreas($val)'` (Olympus.pm:1905), a Perl sub; `print_af_areas`.
// * 0x0305 `AFPointSelected` -- `ValueConv => '$val =~ s/\S* //; $val'` and a
//   `q{}` PrintConv (Olympus.pm:1912-1917); `print_af_point_selected`.
// * 0x0405 `FlashIntensity`, 0x0406 `ManualFlashStrength` -- `PrintConv =>
//   { OTHER => sub {...} }` (Olympus.pm:2031-2036, 2042-2047);
//   `print_flash_strength`.
// * 0x0503 `CustomSaturation` -- a `q{}` PrintConv that reads
//   `$$self{Model}` (Olympus.pm:2092-2099); `print_min_max` (the hand row
//   renders the non-E-1 branch only, a pre-existing gap this slice does not
//   change: `scripts/compare_file.py` reports it WRONG on both E-1 carriers
//   before and after).
// * 0x0600 `DriveMode`, 0x0601 `PanoramaMode` -- `q{}` PrintConvs
//   (Olympus.pm:2525, 2599); `print_drive_mode` / `print_panorama_mode`.
// * 0x0901 `ManometerReading` -- `ValueConv => 'my @a=split(" ",$val); $_ /=
//   10 foreach @a; "@a"'` (Olympus.pm:2758); `print_manometer_reading`.
//
// Withheld rows the hand table never had (0x0804 `StackedImage`, 0x0903
// `RollAngle`, 0x0904 `PitchAngle`) stay MISSING: no hand conversion exists
// and none is guessed at. The two sub-directory rows (0x030a
// `AFTargetInfo`, 0x030b `SubjectDetectInfo`, Olympus.pm:1962-1975) are
// edges to tables no allowlist carries, so the engine refuses them exactly
// as the hand walk ignored them.
//
// RETIRED (slice I-3 integration): `runtime::render`'s `StrEnum` arm keys a
// fixed-count `Array` by its space-joined elements since commit ccb0567f, so
// the engine renders these rows as ExifTool does; the rows left this residual
// and the list below is empty (measured identical on the Olympus directory,
// see the retiring commit). The record above stays as the class's history.
// Overrides (`CAMERA_SETTINGS_ENGINE_MISRENDERS`):
//
// * 0x0527 `NoiseFilter`, 0x052d `PictureModeEffect` -- `int16s[3]` with a
//   PrintConv hash keyed by the space-joined value (`'0 -2 1' => 'Standard'`,
//   Olympus.pm:2333-2340, 2360-2366): the `Main` 0x1015 `WBMode` defect
//   (`runtime::render`'s `StrEnum` arm has no key for a fixed-count
//   `Array`, so the raw joined value stands in). Measured, `conformance.py`
//   over the 315-file Olympus directory against the pinned 13.59 oracle,
//   treatment = a build with all six override rows of this slice removed
//   (the engine's rendering alone; the first such build silently failed to
//   strip -- a reflowed anchor, the script exited before writing and the
//   build ran on the unchanged source, byte-identical to the treatment by
//   md5 -- and was caught by a debug probe of the engine's emitted value,
//   then redone with every removal verified):
//     NoiseFilter        83 files: Standard -> "0 -2 1" x41, n/a -> "0 0 0"
//                        x29, Off -> "-2 -2 1" x10, Low -> "-1 -2 1" x2,
//                        High -> "1 -2 1" x1 (OlympusAIR-A01.jpg,
//                        OlympusE-3.jpg, OlympusE-30.jpg, ...)
//     PictureModeEffect  51 files: Standard -> "0 -1 1" x40, n/a -> "0 0 0"
//                        x11 (OlympusAIR-A01.jpg, OlympusE-5.jpg,
//                        OlympusE-M1.jpg, ...)
//   With the rows in place both are 0 new VALUE. `Conv::ListLookup` is that
//   hash lookup, and renders a miss as ExifTool.pm:3624-3631 does,
//   `Unknown (<joined>)`.
pub static CAMERA_SETTINGS_RESIDUAL: &[TagDef] = &[
    TagDef::text(0x0000, "CameraSettingsVersion"),
    TagDef::raw(0x0101, "PreviewImageStart"),
    TagDef::raw(0x0102, "PreviewImageLength"),
    TagDef {
        id: 0x0301,
        name: "FocusMode",
        force_type: None,
        conv: Conv::List(CS_FOCUS_MODE_LIST),
    },
    TagDef {
        id: 0x0302,
        name: "FocusProcess",
        force_type: None,
        conv: Conv::List(CS_FOCUS_PROCESS_LIST),
    },
    TagDef::func(0x0304, "AFAreas", print_af_areas),
    TagDef::func(0x0305, "AFPointSelected", print_af_point_selected),
    TagDef {
        id: 0x0404,
        name: "FlashControlMode",
        force_type: None,
        conv: Conv::List(CS_FLASH_CONTROL_MODE_LIST),
    },
    TagDef::func(0x0405, "FlashIntensity", print_flash_strength),
    TagDef::func(0x0406, "ManualFlashStrength", print_flash_strength),
    TagDef::func(0x0503, "CustomSaturation", print_min_max),
    TagDef {
        id: 0x050F,
        name: "Gradation",
        force_type: None,
        conv: Conv::Relist {
            group: 3,
            convs: CS_GRADATION_CONVS,
        },
    },
    TagDef {
        id: 0x0520,
        name: "PictureMode",
        force_type: None,
        conv: Conv::List(CS_PICTURE_MODE_LIST),
    },
    TagDef {
        id: 0x0529,
        name: "ArtFilter",
        force_type: None,
        conv: Conv::List(CS_ART_FILTER_LIST),
    },
    TagDef {
        id: 0x052C,
        name: "MagicFilter",
        force_type: None,
        conv: Conv::List(CS_ART_FILTER_LIST),
    },
    TagDef {
        id: 0x052E,
        name: "ToneLevel",
        force_type: None,
        conv: Conv::List(CS_TONE_LEVEL_LIST),
    },
    TagDef {
        id: 0x052F,
        name: "ArtFilterEffect",
        force_type: None,
        conv: Conv::List(CS_ART_FILTER_EFFECT_LIST),
    },
    TagDef {
        id: 0x0532,
        name: "ColorCreatorEffect",
        force_type: None,
        conv: Conv::List(CS_COLOR_CREATOR_LIST),
    },
    TagDef {
        id: 0x0537,
        name: "MonochromeProfileSettings",
        force_type: None,
        conv: Conv::List(CS_MONO_PROFILE_LIST),
    },
    TagDef {
        id: 0x0539,
        name: "ColorProfileSettings",
        force_type: None,
        conv: Conv::List(CS_COLOR_PROFILE_LIST),
    },
    TagDef::func(0x0600, "DriveMode", print_drive_mode),
    TagDef::func(0x0601, "PanoramaMode", print_panorama_mode),
    TagDef {
        id: 0x0821,
        name: "ISOAutoSettings",
        force_type: None,
        conv: Conv::List(CS_ISO_AUTO_SETTINGS_LIST),
    },
    TagDef::func(0x0901, "ManometerReading", print_manometer_reading),
];

/// The `CAMERA_SETTINGS_RESIDUAL` rows that override an engine rendering --
/// see the comment above.
pub static CAMERA_SETTINGS_ENGINE_MISRENDERS: &[u16] = &[];

// ===========================================================================
// Olympus::RawDevelopment (0x2030)
// ===========================================================================

// ===========================================================================
// Olympus::RawDevelopment -- the rows the generated table cannot produce (I-3)
// ===========================================================================
//
// For the `("Olympus", "RawDevelopment")` line; see `EQUIPMENT_RESIDUAL` for
// the construction. One row: 0x0000 `RawDevVersion`, `RawConv =>
// '$val=~s/\0+$//; $val'` (Olympus.pm:2864), `omitted.raw_conv`. Every other
// row is plain or an integer hash (`RawDevNoiseReduction` / `RawDevSettings`
// are `BITMASK` hashes the engine decodes, Olympus.pm:2892-2934) and is the
// engine's. No override: none was needed on any of the 103 corpus carriers
// (`exiftool-pinned.sh -a -G1 -s -RawDevEditStatus` census). A body that
// writes both 0x2030 and 0x2031 (`OlympusXZ-1.jpg`, `OlympusE-M1.jpg`) has
// the RawDevelopment2 copy of every shared name reported last, in ExifTool's
// order, because the engine walks 0x2031 after 0x2030 and the residual loop
// does the same.
pub static RAW_DEVELOPMENT_RESIDUAL: &[TagDef] = &[TagDef::text(0x0000, "RawDevVersion")];

/// No `RAW_DEVELOPMENT_RESIDUAL` row overrides an engine rendering.
pub static RAW_DEVELOPMENT_ENGINE_MISRENDERS: &[u16] = &[];

// ===========================================================================
// Olympus::RawDevelopment2 -- the rows the generated table cannot produce (I-3)
// ===========================================================================
//
// For the `("Olympus", "RawDevelopment2")` line; see `EQUIPMENT_RESIDUAL`.
//
// * 0x0000 `RawDevVersion` -- `RawConv => '$val=~s/\0+$//; $val'`
//   (Olympus.pm:2944); `omitted.raw_conv`.
// * 0x0121 `RawDevArtFilter` -- `PrintConv => [ \%filters ]` (Olympus.pm:
//   3037), the list form; `omitted.print_conv`. `Conv::List`.
//
// 0x8000 `RawDevSubIFD` (Olympus.pm:3039-3049, `Flags => 'SubIFD'`, `FixFormat
// => 'ifd'`) is an edge to `Olympus::RawDevSubIFD`, which no allowlist
// carries; the engine refuses it and the hand table never had a row for it.
//
// RETIRED (slice I-3 integration): `ifd_engine::read_plan`/`decode_plan` now
// read a zero-count entry as ExifTool.pm:6296-6297 do (`return '' if defined
// $count`), so the engine reports the empty value that overwrites the
// RawDevelopment copy; the row left this residual and the list below is empty
// (measured identical; see the retiring commit).
// Override (`RAW_DEVELOPMENT2_ENGINE_MISRENDERS`):
//
// * 0x0108 `RawDevMemoryColorEmphasis` -- `Writable => 'int16u'`, no
//   conversion (Olympus.pm:2962). Both corpus carriers of this table write
//   the entry with COUNT 0, which ExifTool reads as the empty string
//   (`ReadValue`, ExifTool.pm:6297: `return '' if defined $count`) and
//   reports; the engine withholds a zero-count entry instead
//   (`ifd_engine::read_plan`, `if count == 0 { return None }`). On its own
//   that would be one MISSING, but the same bodies also write 0x2030
//   `RawDevelopment`, whose 0x0105 row of the same name (`int16u`, count 1)
//   the engine reports first -- so the plain name kept `0` where ExifTool's
//   last-wins order puts the 0x2031 copy's ``. Measured, `conformance.py`
//   over the 315-file Olympus directory against the pinned 13.59 oracle,
//   treatment = the engine alone for this row: 2 new VALUE rows,
//   `OlympusXZ-1.jpg` and `OlympusE-M1.jpg`, oracle `''`, oxidex `0` (the
//   same two rows again in the build with all six override rows removed);
//   with this row the hand decode (`decode_entry_with_floor` yields an empty
//   integer list, printed as ``) lands last again and both files match, as
//   they did before the line. The engine defect is the zero-count
//   withholding, reported with this slice; when `read_plan` returns the
//   empty value the entry comes out of both lists.
pub static RAW_DEVELOPMENT2_RESIDUAL: &[TagDef] = &[
    TagDef::text(0x0000, "RawDevVersion"),
    TagDef {
        id: 0x0121,
        name: "RawDevArtFilter",
        force_type: None,
        conv: Conv::List(CS_ART_FILTER_LIST),
    },
];

/// The `RAW_DEVELOPMENT2_RESIDUAL` row that overrides an engine
/// (non-)rendering -- see the comment above.
pub static RAW_DEVELOPMENT2_ENGINE_MISRENDERS: &[u16] = &[];

// ===========================================================================
// Olympus::ImageProcessing (0x2040)
// ===========================================================================

/// Olympus.pm:3186-3193, `ImageProcessing` 0x101c `MultipleExposureMode` --
/// the first element's hash; the rest print raw.
static IP_MULTIPLE_EXPOSURE_MODE_LIST: &[ElemConv] = &[ElemConv::Map(&[
    (0, "Off"),
    (1, "Live Composite"),
    (2, "On (2 frames)"),
    (3, "On (3 frames)"),
])];

// ===========================================================================
// Olympus::ImageProcessing -- the rows the generated table cannot produce (I-3)
// ===========================================================================
//
// For the `("Olympus", "ImageProcessing")` line; see `EQUIPMENT_RESIDUAL`.
//
// Withheld:
//
// * 0x0000 `ImageProcessingVersion` -- `RawConv => '$val=~s/\0+$//; $val'`
//   (Olympus.pm:3067); `omitted.raw_conv`.
// * 0x101c `MultipleExposureMode` -- `PrintConv => [{...}]` (Olympus.pm:
//   3186), the list form; `omitted.print_conv`. `Conv::List`.
//
// The four `Unknown => 1` rows (0x0635/0x0636/0x1103/0x1104 `UnknownBlockN`)
// are never reported without `-u` and have no hand row. 0x1306
// `CameraTemperature`'s `ValueConv => '$val ? $val : undef'` (Olympus.pm:
// 3265) IS compiled: the engine withholds the tag on a Perl `undef`, which
// is what the hand `typed_func` did for 0.
//
// RETIRED (slice I-3 integration): `runtime::render`'s `StrEnum` arm keys a
// fixed-count `Array` by its space-joined elements since commit ccb0567f, so
// the engine renders these rows as ExifTool does; the rows left this residual
// and the list below is empty (measured identical on the Olympus directory,
// see the retiring commit). The record above stays as the class's history.
// Overrides (`IMAGE_PROCESSING_ENGINE_MISRENDERS`):
//
// * 0x1112 `AspectRatio`, 0x1900 `KeystoneCompensation` -- `int8u[2]` with a
//   PrintConv hash keyed by the space-joined value (`'1 1' => '4:3'`, `'0 0'
//   => 'Off'`, Olympus.pm:3211-3227, 3272-3275): the `Main` 0x1015 `WBMode`
//   defect (`runtime::render`'s `StrEnum` arm has no key for a fixed-count
//   `Array`). Measured the same way as the CameraSettings overrides (the
//   build with all six override rows removed, `conformance.py`, 315 files):
//     AspectRatio           78 files: 4:3 -> "1 1" x72, 3:2 -> "2 2" x3,
//                           Unknown (0 0) -> "0 0" x2, 16:9 -> "3 3" x1
//                           (OlympusAIR-A01.jpg, OlympusE-30.jpg,
//                           OlympusE-5.jpg, ...)
//     KeystoneCompensation  18 files: Off -> "0 0" x18 (OlympusAIR-A01.jpg,
//                           OlympusE-M10MarkII.jpg, ...)
//   With the rows in place both are 0 new VALUE.
pub static IMAGE_PROCESSING_RESIDUAL: &[TagDef] = &[
    TagDef::text(0x0000, "ImageProcessingVersion"),
    TagDef {
        id: 0x101C,
        name: "MultipleExposureMode",
        force_type: None,
        conv: Conv::List(IP_MULTIPLE_EXPOSURE_MODE_LIST),
    },
];

/// The `IMAGE_PROCESSING_RESIDUAL` rows that override an engine rendering --
/// see the comment above.
pub static IMAGE_PROCESSING_ENGINE_MISRENDERS: &[u16] = &[];

/// FocusInfo 0x1500 SensorTemperature has two ExifTool variants selected by
/// the model and value count; only the multi-value one is unambiguous from
/// the MakerNote alone.
pub static FOCUS_INFO_SENSOR_TEMP: TagDef =
    TagDef::func(0x1500, "SensorTemperature", print_sensor_temperature_multi);

// ===========================================================================
// Olympus::FocusInfo -- the rows the generated table cannot produce (I-3)
// ===========================================================================
//
// For the `("Olympus", "FocusInfo")` line; see `EQUIPMENT_RESIDUAL` for the
// construction. Two of `FOCUS_INFO`'s twelve rows are withheld by
// `IFD_OLYMPUS_FOCUSINFO`:
//
// * 0x0000 `FocusInfoVersion` -- `RawConv => '$val=~s/\0+$//; $val'`
//   (Olympus.pm:3310), `omitted.raw_conv`.
// * 0x1209 `ManualFlash` -- a `q{}` PrintConv over `int16u[2]`
//   (Olympus.pm:3568-3573), `omitted.print_conv`; `print_manual_flash`.
//
// The other ten (`SceneDetect`, the four step counts, `ExternalFlashBounce`,
// `ExternalFlashZoom`, `MacroLED`, and the two joined-key hashes
// `ExternalFlash` -- `Count => 2`, `'0 0' => 'Off'`, Olympus.pm:3529-3536 --
// and `InternalFlash` -- `Count => -1`, Olympus.pm:3552-3561) are the
// engine's: `runtime::enum_key` keys a fixed-count `Array` by its
// space-joined elements since the I-3 engine fixes (the `Main` `WBMode`
// record above), so neither needs an override; the corpus A/B in
// `enabled_ifd.rs` is the check (101 `ExternalFlash` / 101 `InternalFlash`
// carriers, 0 new VALUE). So is 0x2100 `AntiShockWaitingTime`
// (Olympus.pm:3631), a row the hand table never had: 21 corpus carriers
// move MISSING -> matched. 0x0328 `AFInfo` (Olympus.pm:3524) is an edge to
// `Olympus::AFInfo`, which no line carries: refused, as the hand walk never
// followed it. 0x1600 `ImageStabilization` (`Condition => 'not defined
// $$self{ImageStabilization}'` and a `q{}` PrintConv over `undef` bytes,
// Olympus.pm:3618-3629) is withheld (`omitted.condition`, `print_conv`) and
// never had a hand row, so it stays MISSING where a body writes it and no
// CameraSettings 0x0604 (`On, Mode 1` / `On, Mode 2` on 21 corpus files and
// `t/images/Olympus2.jpg`; `Off` on some of a further 15) -- a follow-up,
// not this line. The three `Unknown => 1` rows (0x0209, 0x0211, 0x0212) and
// 0x1203 are not reported without `-u`.
//
// The four model-conditional rows are not `TagDef` rows at all and are not
// in this list: 0x0305 `FocusDistance` (`omitted.value_conv` and
// `print_conv`, Olympus.pm:3338-3357), the `_variants` groups 0x0308
// `AFPoint` (Olympus.pm:3359-3461) and 0x031b `AFPointDetails`
// (Olympus.pm:3463-3523), and 0x1500 `SensorTemperature` (Olympus.pm:3580-
// 3617) are produced by `olympus.rs`'s `parse_focus_info_model_conditional`
// and `parse_focus_info_sensor_temperature`, which run after this residual
// on the same directory. Two alternatives of those groups the engine DOES
// report -- `AFPoint`'s fourth (E-Mxxx / OM-x bodies, `Writable =>
// 'int16u'` and nothing else) and `AFPointDetails`' second (every other
// body), both printed raw -- and the hand pass skips exactly those when the
// engine walked the table (`olympus::focus_info_engine_reports`, pinned
// against the generated `omitted` flags by
// `focus_info_hand_pass_emits_exactly_the_withheld_alternatives`).
pub static FOCUS_INFO_RESIDUAL: &[TagDef] = &[
    TagDef::text(0x0000, "FocusInfoVersion"),
    TagDef::func(0x1209, "ManualFlash", print_manual_flash),
];

/// No `FOCUS_INFO_RESIDUAL` row overrides an engine rendering.
pub static FOCUS_INFO_ENGINE_MISRENDERS: &[u16] = &[];

// ===========================================================================
// Olympus::RawInfo -- the rows the generated table cannot produce (I-3)
// ===========================================================================
//
// For the `("Olympus", "RawInfo")` line; see `EQUIPMENT_RESIDUAL`. One row:
// 0x0000 `RawInfoVersion`, `RawConv => '$val=~s/\0+$//; $val'` (Olympus.pm:
// 3661), `omitted.raw_conv`. The generated table carries 14 rows the hand
// table never had (0x1000-0x2023, `LightSource` through `CMSharpness`,
// Olympus.pm:3701-3759), which the engine now reports. No JPEG under
// `combined-samples/Olympus` or `t/images` writes a 0x3000 directory (it is
// an ORF-only block: `exiftool-pinned.sh -a -G1 -s -RawInfoVersion` over the
// 315 files finds none), so this line is measured on nothing and can move
// nothing in the corpus A/B; it is listed for the ORF path and pinned here
// so a regeneration cannot silently double-emit.
pub static RAW_INFO_RESIDUAL: &[TagDef] = &[TagDef::text(0x0000, "RawInfoVersion")];

/// No `RAW_INFO_RESIDUAL` row overrides an engine rendering.
pub static RAW_INFO_ENGINE_MISRENDERS: &[u16] = &[];

/// `Olympus::Main` rows the generated table WITHHOLDS and this build does not
/// supply from a RESIDUAL ROW. Three classes, named per entry: a SubDirectory
/// edge (no value of its own); a row a named hand PASS produces instead; and a
/// row nothing reports, which is an honest absence. See
/// [`assert_residual_matches_the_generated_withholding`]. Pinned so a
/// regeneration that starts withholding a row now reported fails loudly.
static MAIN_UNSUPPLIED: &[u16] = &[
    0x0001, // a Minolta cross-reference, no value of its own
    0x0003, // MinoltaCameraSettings, a SubDirectory
    0x0201, // Quality -- `parse_camera_type_and_quality` (its PrintConv reads the CameraType data member)
    0x0207, // CameraType -- the same pass (a DataMember behind a Condition)
    0x0208, // TextInfo -- the same pass, then `text_info::parse`
    0x0400, // a SubDirectory
    0x0401, // BlackLevel, withheld with no hand conversion
    0x0e00, // PrintIM, which has its own parser
    0x2010, // the Equipment SubIFD edge
    0x2020, // the CameraSettings edge
    0x2030, // the RawDevelopment edge
    0x2031, // the RawDevelopment2 edge
    0x2040, // the ImageProcessing edge
    0x2100, // a SubDirectory edge
    0x2200, // a SubDirectory edge
    0x2300, // a SubDirectory edge
    0x2400, // a SubDirectory edge
    0x2500, // a SubDirectory edge
    0x2600, // a SubDirectory edge
    0x2700, // a SubDirectory edge
    0x2800, // a SubDirectory edge
    0x2900, // a SubDirectory edge
    0x3000, // the RawInfo edge
    0x4000, // the MainInfo edge, re-walked by `main_info_directory`
    0x5000, // a SubDirectory edge
];

/// `Olympus::Equipment` rows the generated table WITHHOLDS and this build does not
/// supply from a RESIDUAL ROW. Three classes, named per entry: a SubDirectory
/// edge (no value of its own); a row a named hand PASS produces instead; and a
/// row nothing reports, which is an honest absence. See
/// [`assert_residual_matches_the_generated_withholding`]. Pinned so a
/// regeneration that starts withholding a row now reported fails loudly.
static EQUIPMENT_UNSUPPLIED: &[u16] = &[];

/// `Olympus::CameraSettings` rows the generated table WITHHOLDS and this build does not
/// supply from a RESIDUAL ROW. Three classes, named per entry: a SubDirectory
/// edge (no value of its own); a row a named hand PASS produces instead; and a
/// row nothing reports, which is an honest absence. See
/// [`assert_residual_matches_the_generated_withholding`]. Pinned so a
/// regeneration that starts withholding a row now reported fails loudly.
static CAMERA_SETTINGS_UNSUPPLIED: &[u16] = &[
    0x030a, // withheld, no hand conversion
    0x030b, // SubjectDetectInfo, a SubDirectory
    0x0804, // withheld, no hand conversion
    0x0903, // withheld, no hand conversion
    0x0904, // withheld, no hand conversion
];

/// `Olympus::RawDevelopment` rows the generated table WITHHOLDS and this build does not
/// supply from a RESIDUAL ROW. Three classes, named per entry: a SubDirectory
/// edge (no value of its own); a row a named hand PASS produces instead; and a
/// row nothing reports, which is an honest absence. See
/// [`assert_residual_matches_the_generated_withholding`]. Pinned so a
/// regeneration that starts withholding a row now reported fails loudly.
static RAW_DEVELOPMENT_UNSUPPLIED: &[u16] = &[];

/// `Olympus::RawDevelopment2` rows the generated table WITHHOLDS and this build does not
/// supply from a RESIDUAL ROW. Three classes, named per entry: a SubDirectory
/// edge (no value of its own); a row a named hand PASS produces instead; and a
/// row nothing reports, which is an honest absence. See
/// [`assert_residual_matches_the_generated_withholding`]. Pinned so a
/// regeneration that starts withholding a row now reported fails loudly.
static RAW_DEVELOPMENT2_UNSUPPLIED: &[u16] = &[
    0x8000, // RawDevSubIFD, a SubDirectory whose target is not enabled
];

/// `Olympus::ImageProcessing` rows the generated table WITHHOLDS and this build does not
/// supply from a RESIDUAL ROW. Three classes, named per entry: a SubDirectory
/// edge (no value of its own); a row a named hand PASS produces instead; and a
/// row nothing reports, which is an honest absence. See
/// [`assert_residual_matches_the_generated_withholding`]. Pinned so a
/// regeneration that starts withholding a row now reported fails loudly.
static IMAGE_PROCESSING_UNSUPPLIED: &[u16] = &[];

/// `Olympus::FocusInfo` rows the generated table WITHHOLDS and this build does not
/// supply from a RESIDUAL ROW. Three classes, named per entry: a SubDirectory
/// edge (no value of its own); a row a named hand PASS produces instead; and a
/// row nothing reports, which is an honest absence. See
/// [`assert_residual_matches_the_generated_withholding`]. Pinned so a
/// regeneration that starts withholding a row now reported fails loudly.
static FOCUS_INFO_UNSUPPLIED: &[u16] = &[
    0x0305, // FocusDistance -- `parse_focus_info_model_conditional`, which also feeds the `value_forms` channel Composite DOF/FOV read
    0x0328, // AFInfo, a SubDirectory whose target is not enabled
    0x1500, // SensorTemperature -- `parse_focus_info_sensor_temperature` (two model/count-conditional conversions)
    0x1600, // ImageStabilization -- withheld by a single-entry Condition and never hand-written; MISSING on 36 corpus files, a recorded follow-up
];

/// `Olympus::RawInfo` rows the generated table WITHHOLDS and this build does not
/// supply from a RESIDUAL ROW. Three classes, named per entry: a SubDirectory
/// edge (no value of its own); a row a named hand PASS produces instead; and a
/// row nothing reports, which is an honest absence. See
/// [`assert_residual_matches_the_generated_withholding`]. Pinned so a
/// regeneration that starts withholding a row now reported fails loudly.
static RAW_INFO_UNSUPPLIED: &[u16] = &[];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsers::tiff::makernotes::olympus::ifd::apply_conv;

    fn conv_of(table: &'static [TagDef], id: u16) -> &'static TagDef {
        table.iter().find(|d| d.id == id).expect("tag in table")
    }

    /// The generated table for `(Olympus, table)`, reached through
    /// `ALL_IFD_TABLES` rather than a literal `find_ifd_table` so that
    /// `tools/exiftool-tables/reachability.py`'s call-site census counts
    /// only the live calls in `olympus.rs`.
    fn generated(table: &str) -> &'static crate::exiftool_tables::IfdTable {
        crate::exiftool_tables::ALL_IFD_TABLES
            .iter()
            .find(|t| t.module == "Olympus" && t.table == table)
            .copied()
            .unwrap_or_else(|| panic!("Olympus::{table} is generated"))
    }

    /// The residual against the GENERATED table alone.
    ///
    /// Slice I-2/I-3 checked the residual against the full hand table it
    /// replaced ("is the residual exactly the remainder?"), which slice I-4
    /// deleted. This is the successor invariant, and it is stronger for
    /// resting on no hand-written tag knowledge at all -- every row of it
    /// comes from `ifd_tables.rs`:
    ///
    /// 1. no residual row may duplicate a row the engine REPORTS, or the
    ///    walk would insert that tag twice (the residual walk runs after the
    ///    engine's); the only exception is a declared `misrenders` override,
    ///    whose whole purpose is to overwrite the engine's rendering.
    /// 2. every row the generated table WITHHOLDS is either supplied by a
    ///    residual row or named in `unsupplied` -- so a regeneration that
    ///    starts withholding a row this build reports today fails HERE
    ///    rather than silently dropping a tag from the output. That is the
    ///    protection the deleted hand table used to give.
    /// 3. `unsupplied` may not name a row that is supplied or reported: a
    ///    stale entry would blind rule 2.
    ///
    /// A residual row with no generated tag at all is legitimate and
    /// expected: the generator never transcribed it (`Main` 0x0280
    /// `PreviewImage`, 0x0f04/0x0f05 `ZoomedPreview*`, 0x1036
    /// `PreviewImageStart`; `CameraSettings` 0x0101/0x0102), so the hand row
    /// is its only producer.
    fn assert_residual_matches_the_generated_withholding(
        table: &str,
        residual: &[TagDef],
        misrenders: &[u16],
        unsupplied: &[u16],
    ) {
        let generated = generated(table);
        // Every id the table declares, and whether ANY alternative reports
        // it: a variant group's alternatives are `IfdTag`s too, and a group
        // whose every alternative is withheld reports nothing.
        let mut reported: Vec<u16> = Vec::new();
        let mut withheld: Vec<u16> = Vec::new();
        for tag in generated.tags {
            if tag.omitted.any() {
                withheld.push(tag.id)
            } else {
                reported.push(tag.id)
            }
        }
        for group in generated.variants {
            if group.alternatives.iter().any(|(_, t)| !t.omitted.any()) {
                reported.push(group.id);
            } else {
                withheld.push(group.id);
            }
        }
        withheld.retain(|id| !reported.contains(id));

        for row in residual {
            if misrenders.contains(&row.id) {
                continue;
            }
            assert!(
                !reported.contains(&row.id),
                "{:#06x} {} is reported by the generated Olympus::{table}; a residual row would insert it twice",
                row.id,
                row.name
            );
        }
        for id in &withheld {
            let supplied = residual.iter().any(|r| r.id == *id);
            assert!(
                supplied || unsupplied.contains(id),
                "{id:#06x} is withheld by the generated Olympus::{table} and no residual row supplies it: \
                 either add the hand row or name it in the table's UNSUPPLIED list with the reason"
            );
        }
        for id in unsupplied {
            assert!(
                withheld.contains(id),
                "{id:#06x} is in Olympus::{table}'s UNSUPPLIED list but the generated table does not withhold it (stale entry)"
            );
            assert!(
                !residual.iter().any(|r| r.id == *id),
                "{id:#06x} is in Olympus::{table}'s UNSUPPLIED list and also a residual row"
            );
        }
    }

    fn assert_misrenders_are_overrides(table: &str, residual: &[TagDef], misrenders: &[u16]) {
        let generated = generated(table);
        for id in misrenders {
            let row = residual.iter().find(|r| r.id == *id).unwrap_or_else(|| {
                panic!("{id:#06x} is an Olympus::{table} misrender but not a residual row")
            });
            let tag = generated.tag(*id).unwrap_or_else(|| {
                panic!(
                    "{id:#06x} {} is not in the generated Olympus::{table}; it is a withheld-class residual, not an override",
                    row.name
                )
            });
            assert!(
                !tag.omitted.any() && !tag.flags.unknown && tag.subdir.is_none(),
                "{id:#06x} {}: the engine does not report this Olympus::{table} row, so there is nothing to override",
                row.name
            );
            assert_eq!(
                tag.name, row.name,
                "{id:#06x} (Olympus::{table}): an override must keep the generated name"
            );
        }
    }

    /// `MAIN_RESIDUAL` against the generated `Olympus::Main` (slice I-2).
    #[test]
    fn main_residual_is_exactly_the_generated_tables_remainder() {
        assert_residual_matches_the_generated_withholding(
            "Main",
            MAIN_RESIDUAL,
            ENGINE_MISRENDERS,
            MAIN_UNSUPPLIED,
        );
    }

    #[test]
    fn engine_misrenders_are_overrides_of_rows_the_engine_reports() {
        assert_misrenders_are_overrides("Main", MAIN_RESIDUAL, ENGINE_MISRENDERS);
    }

    // Slice I-3: the same two pins for each sub-table its allowlist line
    // moves onto the engine. One test per table so a failure names the
    // table in its own name.

    #[test]
    fn equipment_residual_is_exactly_the_generated_tables_remainder() {
        assert_residual_matches_the_generated_withholding(
            "Equipment",
            EQUIPMENT_RESIDUAL,
            EQUIPMENT_ENGINE_MISRENDERS,
            EQUIPMENT_UNSUPPLIED,
        );
        assert_misrenders_are_overrides(
            "Equipment",
            EQUIPMENT_RESIDUAL,
            EQUIPMENT_ENGINE_MISRENDERS,
        );
    }

    #[test]
    fn camera_settings_residual_is_exactly_the_generated_tables_remainder() {
        assert_residual_matches_the_generated_withholding(
            "CameraSettings",
            CAMERA_SETTINGS_RESIDUAL,
            CAMERA_SETTINGS_ENGINE_MISRENDERS,
            CAMERA_SETTINGS_UNSUPPLIED,
        );
        assert_misrenders_are_overrides(
            "CameraSettings",
            CAMERA_SETTINGS_RESIDUAL,
            CAMERA_SETTINGS_ENGINE_MISRENDERS,
        );
    }

    #[test]
    fn raw_development_residual_is_exactly_the_generated_tables_remainder() {
        assert_residual_matches_the_generated_withholding(
            "RawDevelopment",
            RAW_DEVELOPMENT_RESIDUAL,
            RAW_DEVELOPMENT_ENGINE_MISRENDERS,
            RAW_DEVELOPMENT_UNSUPPLIED,
        );
        assert_misrenders_are_overrides(
            "RawDevelopment",
            RAW_DEVELOPMENT_RESIDUAL,
            RAW_DEVELOPMENT_ENGINE_MISRENDERS,
        );
    }

    #[test]
    fn raw_development2_residual_is_exactly_the_generated_tables_remainder() {
        assert_residual_matches_the_generated_withholding(
            "RawDevelopment2",
            RAW_DEVELOPMENT2_RESIDUAL,
            RAW_DEVELOPMENT2_ENGINE_MISRENDERS,
            RAW_DEVELOPMENT2_UNSUPPLIED,
        );
        assert_misrenders_are_overrides(
            "RawDevelopment2",
            RAW_DEVELOPMENT2_RESIDUAL,
            RAW_DEVELOPMENT2_ENGINE_MISRENDERS,
        );
    }

    #[test]
    fn image_processing_residual_is_exactly_the_generated_tables_remainder() {
        assert_residual_matches_the_generated_withholding(
            "ImageProcessing",
            IMAGE_PROCESSING_RESIDUAL,
            IMAGE_PROCESSING_ENGINE_MISRENDERS,
            IMAGE_PROCESSING_UNSUPPLIED,
        );
        assert_misrenders_are_overrides(
            "ImageProcessing",
            IMAGE_PROCESSING_RESIDUAL,
            IMAGE_PROCESSING_ENGINE_MISRENDERS,
        );
    }

    #[test]
    fn raw_info_residual_is_exactly_the_generated_tables_remainder() {
        assert_residual_matches_the_generated_withholding(
            "RawInfo",
            RAW_INFO_RESIDUAL,
            RAW_INFO_ENGINE_MISRENDERS,
            RAW_INFO_UNSUPPLIED,
        );
        assert_misrenders_are_overrides("RawInfo", RAW_INFO_RESIDUAL, RAW_INFO_ENGINE_MISRENDERS);
    }

    /// The seventh sub-table line. `FocusInfo` was the one that stayed
    /// hand-walked after the first six: slice I-3's condition forms
    /// (`$count != 1`, `not defined $$self{X}`) made `IFD_OLYMPUS_FOCUSINFO`
    /// pass gate A, and its line, residual and measurement followed as their
    /// own commit. The `_variants` rows the residual does not carry are
    /// pinned in `olympus.rs`
    /// (`focus_info_hand_pass_emits_exactly_the_withheld_alternatives`).
    #[test]
    fn focus_info_residual_is_exactly_the_generated_tables_remainder() {
        assert_residual_matches_the_generated_withholding(
            "FocusInfo",
            FOCUS_INFO_RESIDUAL,
            FOCUS_INFO_ENGINE_MISRENDERS,
            FOCUS_INFO_UNSUPPLIED,
        );
        assert_misrenders_are_overrides(
            "FocusInfo",
            FOCUS_INFO_RESIDUAL,
            FOCUS_INFO_ENGINE_MISRENDERS,
        );
    }

    #[test]
    fn focus_mode_uses_exiftool_list_printconv() {
        // E-M1: raw "0 65" -> "Single AF; S-AF, Imager AF"
        let def = conv_of(CAMERA_SETTINGS_RESIDUAL, 0x0301);
        let v = OlyVal::Int(vec![0, 65]);
        assert_eq!(apply_conv(def, &v).unwrap(), "Single AF; S-AF, Imager AF");
    }

    #[test]
    fn focus_process_prints_extra_elements_raw() {
        let def = conv_of(CAMERA_SETTINGS_RESIDUAL, 0x0302);
        let v = OlyVal::Int(vec![1, 64]);
        assert_eq!(apply_conv(def, &v).unwrap(), "AF Used; 64");
    }

    #[test]
    fn gradation_relists_the_first_three_values() {
        let def = conv_of(CAMERA_SETTINGS_RESIDUAL, 0x050F);
        let v = OlyVal::Int(vec![0, -1, 1, 0]);
        assert_eq!(apply_conv(def, &v).unwrap(), "Normal; User-Selected");
    }

    #[test]
    fn tone_level_names_every_fourth_element() {
        let def = conv_of(CAMERA_SETTINGS_RESIDUAL, 0x052E);
        let v = OlyVal::Int(vec![-31999, 0, -7, 7, -31998, 0, -7, 7, 0, 0, 0, 0]);
        assert_eq!(
            apply_conv(def, &v).unwrap(),
            "Highlights; 0; -7; 7; Shadows; 0; -7; 7; 0; 0; 0; 0"
        );
    }

    #[test]
    fn color_creator_effect_uses_label_templates() {
        let def = conv_of(CAMERA_SETTINGS_RESIDUAL, 0x0532);
        let v = OlyVal::Int(vec![0, 0, 29, 0, -4, 3]);
        assert_eq!(
            apply_conv(def, &v).unwrap(),
            "Color 0; 0; 29; Strength 0; -4; 3"
        );
    }

    #[test]
    fn lens_type_keys_on_bytes_0_2_3() {
        let def = conv_of(EQUIPMENT_RESIDUAL, 0x0201);
        let v = OlyVal::Int(vec![0, 0, 0x15, 0, 0, 0]);
        assert_eq!(
            apply_conv(def, &v).unwrap(),
            "Olympus Zuiko Digital ED 7-14mm F4.0"
        );
    }

    #[test]
    fn body_firmware_version_is_hex_with_an_inserted_point() {
        let def = conv_of(EQUIPMENT_RESIDUAL, 0x0104);
        assert_eq!(
            apply_conv(def, &OlyVal::Int(vec![0x1005])).unwrap(),
            "1.005"
        );
    }

    #[test]
    fn main_info_body_firmware_version_is_raw_not_hex() {
        // Olympus::Main's 0x0104 (Olympus.pm: `Writable => 'string'`, no
        // PrintConv) is a different tag definition than Equipment's 0x0104
        // above, even though both are named BodyFirmwareVersion. MainInfoIFD
        // (0x4000) recurses into Main, not Equipment, so its copy must print
        // the stored int32u as-is -- e.g. OlympusSH-1.jpg's MainInfo 0x0104
        // is literally 0, which `exiftool -G1 -s` prints as "0", not "0.000"
        // or any hex-derived form.
        let def = conv_of(MAIN_INFO, 0x0104);
        assert_eq!(apply_conv(def, &OlyVal::Int(vec![0])).unwrap(), "0");
        assert_eq!(apply_conv(def, &OlyVal::Int(vec![4099])).unwrap(), "4099");
    }

    #[test]
    fn af_areas_renders_corner_pairs() {
        let def = conv_of(CAMERA_SETTINGS_RESIDUAL, 0x0304);
        let v = OlyVal::Int(vec![0xC845_E56B, 0, 0]);
        assert_eq!(apply_conv(def, &v).unwrap(), "(200,69)-(229,107)");
        assert_eq!(apply_conv(def, &OlyVal::Int(vec![0, 0])).unwrap(), "none");
    }
}
