//! Lytro Light Field Picture (LFP) container reader.
//!
//! Transcribed from ExifTool's `lib/Image/ExifTool/Lytro.pm` (version 1.04).
//! Line references below are to that file.
//!
//! # Container
//!
//! An LFP file is a 16-byte header (`\x89LFP\x0d\x0a\x1a\x0a` plus a version
//! word) followed by a chain of self-describing sections (`ProcessLFP`,
//! Lytro.pm:134). Each section is a 16-byte record header whose first three
//! bytes are `\x89LF` and whose last four are a big-endian payload length, an
//! 80-byte SHA-1 identifier, the payload itself, then padding to the next
//! 16-byte boundary. A payload beginning `{` + whitespace + `"` is JSON
//! metadata; one beginning `\xff\xd8\xff` is an embedded JPEG preview.
//!
//! # Tag names
//!
//! ExifTool flattens the JSON object graph into tag names rather than
//! declaring a fixed table: `ExtractTags` (Lytro.pm:104) concatenates
//! `ucfirst` of each key down the path, so `mla.sensorOffset.x` becomes
//! `MlaSensorOffsetX`. A path that matches one of the entries transcribed in
//! [`TAG_TABLE`] takes that entry's ExifTool name and conversion; everything
//! else is emitted under a name derived by [`derive_name`]. This is why the
//! table below is short even though the format yields ~85 tags -- the table is
//! the exception list, not the tag list.
//!
//! # Numeric text
//!
//! `Image::ExifTool::Import::ReadJSONObject` returns numbers, `true`, `false`
//! and `null` as their *raw source text* (Import.pm:234-238 assembles the token
//! by `substr` and never numifies it). A tag with no ValueConv therefore prints
//! the literal characters from the file, which is how ExifTool reports
//! `AccelerometerX` as `-0.039215687662363052368` -- 21 significant digits, far
//! more than a double survives. [`JsonValue::Raw`] preserves that text verbatim
//! so those tags are copied, never reformatted. Only the handful of tags with a
//! ValueConv or PrintConv go through `f64`.

#![allow(dead_code)]

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};

/// File header magic (Lytro.pm:142).
const LFP_MAGIC: &[u8] = b"\x89LFP\x0d\x0a\x1a\x0a";

/// Section record header magic (Lytro.pm:147).
const SECTION_MAGIC: &[u8] = b"\x89LF";

/// Bytes in a section record header: 12 bytes of type/version plus a
/// big-endian u32 length (Lytro.pm:146-148).
const SECTION_HEADER_LEN: usize = 16;

/// Bytes of SHA-1 identifier following each section header (Lytro.pm:150).
const SECTION_ID_LEN: usize = 80;

/// Payload size above which ExifTool seeks past the section instead of
/// buffering it (Lytro.pm:155).
const MAX_BUFFERED_SECTION: u32 = 20_000_000;

// ---------------------------------------------------------------------------
// Minimal ordered JSON reader
// ---------------------------------------------------------------------------

/// A JSON value in the shape `ReadJSONObject` produces.
///
/// Objects keep their keys in document order because ExifTool walks them with
/// `OrderedKeys` (Lytro.pm:109), and order decides which of two same-named tags
/// survives: `modes.regionOfInterestArray` holds two objects that both define
/// `type`, and ExifTool reports the later one.
#[derive(Debug, Clone, PartialEq)]
enum JsonValue {
    /// A quoted string, with escapes already resolved.
    Str(String),
    /// A number, `true`, `false` or `null`, kept as the exact source text.
    Raw(String),
    /// An array, in document order.
    Array(Vec<JsonValue>),
    /// An object, in document order.
    Object(Vec<(String, JsonValue)>),
}

/// Recursive-descent reader for the JSON subset LFP files carry.
struct JsonReader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> JsonReader<'a> {
    fn new(text: &'a str) -> Self {
        JsonReader {
            bytes: text.as_bytes(),
            pos: 0,
        }
    }

    fn skip_ws(&mut self) {
        while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.skip_ws();
        self.bytes.get(self.pos).copied()
    }

    fn expect(&mut self, want: u8) -> Option<()> {
        if self.peek()? == want {
            self.pos += 1;
            Some(())
        } else {
            None
        }
    }

    fn parse_value(&mut self) -> Option<JsonValue> {
        match self.peek()? {
            b'{' => self.parse_object(),
            b'[' => self.parse_array(),
            b'"' => self.parse_string().map(JsonValue::Str),
            _ => self.parse_raw(),
        }
    }

    fn parse_object(&mut self) -> Option<JsonValue> {
        self.expect(b'{')?;
        let mut entries = Vec::new();
        if self.peek()? == b'}' {
            self.pos += 1;
            return Some(JsonValue::Object(entries));
        }
        loop {
            let key = self.parse_string()?;
            self.expect(b':')?;
            let value = self.parse_value()?;
            entries.push((key, value));
            match self.peek()? {
                b',' => self.pos += 1,
                b'}' => {
                    self.pos += 1;
                    return Some(JsonValue::Object(entries));
                }
                _ => return None,
            }
        }
    }

    fn parse_array(&mut self) -> Option<JsonValue> {
        self.expect(b'[')?;
        let mut items = Vec::new();
        if self.peek()? == b']' {
            self.pos += 1;
            return Some(JsonValue::Array(items));
        }
        loop {
            items.push(self.parse_value()?);
            match self.peek()? {
                b',' => self.pos += 1,
                b']' => {
                    self.pos += 1;
                    return Some(JsonValue::Array(items));
                }
                _ => return None,
            }
        }
    }

    fn parse_string(&mut self) -> Option<String> {
        self.expect(b'"')?;
        let mut out = String::new();
        loop {
            let c = *self.bytes.get(self.pos)?;
            self.pos += 1;
            match c {
                b'"' => return Some(out),
                b'\\' => {
                    let esc = *self.bytes.get(self.pos)?;
                    self.pos += 1;
                    match esc {
                        // Import.pm:222 maps exactly this set; any other
                        // escaped character stands for itself.
                        b't' => out.push('\t'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'u' => {
                            let hex = self.bytes.get(self.pos..self.pos + 4)?;
                            let hex = std::str::from_utf8(hex).ok()?;
                            let code = u32::from_str_radix(hex, 16).ok()?;
                            self.pos += 4;
                            out.push(char::from_u32(code)?);
                        }
                        other => out.push(other as char),
                    }
                }
                _ => {
                    // Copy the whole UTF-8 sequence, not just the lead byte.
                    let start = self.pos - 1;
                    let len = utf8_len(c);
                    let seq = self.bytes.get(start..start + len)?;
                    out.push_str(std::str::from_utf8(seq).ok()?);
                    self.pos = start + len;
                }
            }
        }
    }

    /// Read a number, `true`, `false` or `null` as literal text.
    ///
    /// Import.pm:235 terminates the token on whitespace, `:`, `,`, `}` or `]`
    /// and keeps everything before it, which is reproduced here so the exact
    /// digits from the file reach the tag value.
    fn parse_raw(&mut self) -> Option<JsonValue> {
        self.skip_ws();
        let start = self.pos;
        while let Some(&c) = self.bytes.get(self.pos) {
            if c.is_ascii_whitespace() || matches!(c, b':' | b',' | b'}' | b']') {
                break;
            }
            self.pos += 1;
        }
        if self.pos == start {
            return None;
        }
        let text = std::str::from_utf8(self.bytes.get(start..self.pos)?).ok()?;
        Some(JsonValue::Raw(text.to_string()))
    }
}

/// Length in bytes of the UTF-8 sequence beginning with `lead`.
fn utf8_len(lead: u8) -> usize {
    match lead {
        0x00..=0x7f => 1,
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf7 => 4,
        // A continuation or invalid byte: consume one byte so the reader
        // always advances rather than looping.
        _ => 1,
    }
}

/// Parse a complete JSON document.
fn parse_json(text: &str) -> Option<JsonValue> {
    let mut reader = JsonReader::new(text);
    reader.parse_value()
}

// ---------------------------------------------------------------------------
// Tag table
// ---------------------------------------------------------------------------

/// The value conversion a table entry applies.
///
/// Each variant names the ExifTool ValueConv/PrintConv pair it reproduces; the
/// bodies live in [`convert`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Conv {
    /// No ValueConv and no PrintConv: the source text is the value.
    Raw,
    /// `XMP::ConvertXMPDate` then `ConvertDateTime` (Lytro.pm:55-56).
    XmpDate,
    /// `Exif::PrintFNumber` (Lytro.pm:60).
    FNumber,
    /// `$val * 1000` metres to mm, printed `%.1f mm` (Lytro.pm:64-65).
    FocalLength,
    /// `sprintf("%.1f C",$val)` (Lytro.pm:69, :73).
    Celsius,
    /// `Exif::PrintExposureTime` (Lytro.pm:77, :81).
    ExposureTime,
    /// `25.4 / $val / 1000`, metres per pixel to pixels per inch (Lytro.pm:86).
    FocalPlaneRes,
    /// `sprintf("%+.1f", $val)` (Lytro.pm:90-91).
    ExposureBias,
    /// `PrintConv => { 1 => 'Horizontal (normal)' }` (Lytro.pm:95-97).
    Orientation,
}

/// One transcribed entry of `%Image::ExifTool::Lytro::Main`.
struct LytroTag {
    /// The flattened JSON path ExifTool uses as the table key.
    key: &'static str,
    /// The ExifTool tag name (`Name`, or the key itself when none is given).
    name: &'static str,
    /// The conversion ExifTool declares for this entry.
    conv: Conv,
}

/// `%Image::ExifTool::Lytro::Main`, Lytro.pm:42-98.
///
/// `JSONMetadata` and `EmbeddedImage` are handled by the container walk rather
/// than the JSON flattener, so they are not listed here.
///
/// Entries without a `Name` in the Perl table (the two exposure biases) keep
/// their key as the tag name, which is what ExifTool does.
static TAG_TABLE: &[LytroTag] = &[
    LytroTag {
        key: "Type",
        name: "CameraType",
        conv: Conv::Raw,
    },
    LytroTag {
        key: "CameraMake",
        name: "Make",
        conv: Conv::Raw,
    },
    LytroTag {
        key: "CameraModel",
        name: "Model",
        conv: Conv::Raw,
    },
    LytroTag {
        key: "CameraSerialNumber",
        name: "SerialNumber",
        conv: Conv::Raw,
    },
    LytroTag {
        key: "CameraFirmware",
        name: "FirmwareVersion",
        conv: Conv::Raw,
    },
    LytroTag {
        key: "DevicesAccelerometerSampleArrayTime",
        name: "AccelerometerTime",
        conv: Conv::Raw,
    },
    LytroTag {
        key: "DevicesAccelerometerSampleArrayX",
        name: "AccelerometerX",
        conv: Conv::Raw,
    },
    LytroTag {
        key: "DevicesAccelerometerSampleArrayY",
        name: "AccelerometerY",
        conv: Conv::Raw,
    },
    LytroTag {
        key: "DevicesAccelerometerSampleArrayZ",
        name: "AccelerometerZ",
        conv: Conv::Raw,
    },
    LytroTag {
        key: "DevicesClockZuluTime",
        name: "DateTimeOriginal",
        conv: Conv::XmpDate,
    },
    LytroTag {
        key: "DevicesLensFNumber",
        name: "FNumber",
        conv: Conv::FNumber,
    },
    LytroTag {
        key: "DevicesLensFocalLength",
        name: "FocalLength",
        conv: Conv::FocalLength,
    },
    LytroTag {
        key: "DevicesLensTemperature",
        name: "LensTemperature",
        conv: Conv::Celsius,
    },
    LytroTag {
        key: "DevicesSocTemperature",
        name: "SocTemperature",
        conv: Conv::Celsius,
    },
    LytroTag {
        key: "DevicesShutterFrameExposureDuration",
        name: "FrameExposureTime",
        conv: Conv::ExposureTime,
    },
    LytroTag {
        key: "DevicesShutterPixelExposureDuration",
        name: "ExposureTime",
        conv: Conv::ExposureTime,
    },
    LytroTag {
        key: "DevicesSensorPixelPitch",
        name: "FocalPlaneXResolution",
        conv: Conv::FocalPlaneRes,
    },
    LytroTag {
        key: "DevicesSensorSensorSerial",
        name: "SensorSerialNumber",
        conv: Conv::Raw,
    },
    LytroTag {
        key: "DevicesSensorIso",
        name: "ISO",
        conv: Conv::Raw,
    },
    LytroTag {
        key: "ImageLimitExposureBias",
        name: "ImageLimitExposureBias",
        conv: Conv::ExposureBias,
    },
    LytroTag {
        key: "ImageModulationExposureBias",
        name: "ImageModulationExposureBias",
        conv: Conv::ExposureBias,
    },
    LytroTag {
        key: "ImageOrientation",
        name: "Orientation",
        conv: Conv::Orientation,
    },
];

/// Look up a flattened path in the transcribed table.
fn table_entry(key: &str) -> Option<&'static LytroTag> {
    TAG_TABLE.iter().find(|t| t.key == key)
}

// ---------------------------------------------------------------------------
// Name derivation for paths not in the table
// ---------------------------------------------------------------------------

/// The literal ExifTool strips from generated names (Lytro.pm:116).
const VENDOR_INFIX: &str = "ParametersVendorContentComLytroTags";

/// Derive the tag name for a flattened path with no table entry.
///
/// Reproduces Lytro.pm:115-118:
///
/// ```text
/// ($name = $tag) =~ s/[^-_a-zA-Z0-9](.?)/\U$1/g;
/// $name =~ s/ParametersVendorContentComLytroTags//;
/// $tagInfo{Groups} = { 2 => 'Image' } unless $name =~ s/^Devices//;
/// ```
///
/// The first substitution deletes each character outside `[-_a-zA-Z0-9]` and
/// upper-cases whatever follows it, turning the JSON key `com.lytro.tags` into
/// `ComLytroTags`. The `Devices` prefix is stripped by the third line's
/// side effect, which is why `devices.mla.lensPitch` reports as `MlaLensPitch`.
fn derive_name(path: &str) -> String {
    let chars: Vec<char> = path.chars().collect();
    let mut name = String::with_capacity(path.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '-' || c == '_' || c.is_ascii_alphanumeric() {
            name.push(c);
            i += 1;
            continue;
        }
        // Drop the separator and absorb the next character upper-cased.
        // Perl's `.` does not match a newline, so a newline is left in place
        // for the next iteration to drop on its own.
        i += 1;
        if let Some(&next) = chars.get(i)
            && next != '\n'
        {
            name.extend(next.to_uppercase());
            i += 1;
        }
    }

    if let Some(at) = name.find(VENDOR_INFIX) {
        name.replace_range(at..at + VENDOR_INFIX.len(), "");
    }
    if let Some(rest) = name.strip_prefix("Devices") {
        name = rest.to_string();
    }
    name
}

/// Perl's `ucfirst`: upper-case the first character, leave the rest alone.
fn ucfirst(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

// ---------------------------------------------------------------------------
// Conversions
// ---------------------------------------------------------------------------

/// The print form of a tag plus, when they differ, the full-precision
/// ValueConv form the Composite layer must consume.
struct Converted {
    /// What ExifTool prints.
    print: String,
    /// ExifTool's ValueConv value, when it is not the printed string.
    ///
    /// `Composite:FocalLength35efl` multiplies the *unrounded* focal length by
    /// the scale factor; feeding it the printed `6.4 mm` instead of
    /// 6.4499998092651363 shifts the 35 mm equivalent from 43.0 mm to 42.6 mm.
    value: Option<String>,
}

impl Converted {
    fn print_only(print: String) -> Self {
        Converted { print, value: None }
    }

    fn with_value(print: String, value: String) -> Self {
        Converted {
            print,
            value: Some(value),
        }
    }
}

/// Format a double the way Perl stringifies one: `%.15g`, trailing zeros
/// trimmed.
///
/// `FocalPlaneXResolution` has a ValueConv and no PrintConv, so ExifTool prints
/// the raw NV and Perl's default stringification decides the digits --
/// `18142.8574518282`, not the 17 digits Rust's `to_string` would emit.
fn perl_number(value: f64) -> String {
    if !value.is_finite() {
        return value.to_string();
    }
    if value == 0.0 {
        return "0".to_string();
    }

    let exponent = value.abs().log10().floor() as i32;
    // %g switches to exponential outside [-4, precision).
    if exponent < -4 || exponent >= 15 {
        let mantissa = format!("{:.*e}", 14, value);
        let (digits, exp) = mantissa.split_once('e').unwrap_or((mantissa.as_str(), "0"));
        let digits = trim_fraction(digits);
        let exp: i32 = exp.parse().unwrap_or(0);
        return format!(
            "{digits}e{}{:02}",
            if exp < 0 { '-' } else { '+' },
            exp.abs()
        );
    }

    let decimals = (14 - exponent).max(0) as usize;
    trim_fraction(&format!("{value:.decimals$}"))
}

/// Strip the trailing zeros (and then a bare trailing dot) from a decimal.
fn trim_fraction(s: &str) -> String {
    if !s.contains('.') {
        return s.to_string();
    }
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// `Image::ExifTool::XMP::ConvertXMPDate` (XMP.pm:3383).
///
/// Rewrites `2012-04-12T14:10:55.000Z` as `2012:04:12 14:10:55.000Z`. The
/// PrintConv is `$self->ConvertDateTime($val)`, which without `-d` returns its
/// argument unchanged, so this single step is the whole conversion.
fn convert_xmp_date(val: &str) -> String {
    // ^(\d{4})-(\d{2})-(\d{2})[T ](\d{2}:\d{2})(:\d{2})?\s*(\S*)$
    let b = val.as_bytes();
    let digits = |from: usize, n: usize| {
        b.get(from..from + n)
            .is_some_and(|s| s.iter().all(u8::is_ascii_digit))
    };
    let matches_shape = b.len() >= 16
        && digits(0, 4)
        && b[4] == b'-'
        && digits(5, 2)
        && b[7] == b'-'
        && digits(8, 2)
        && (b[10] == b'T' || b[10] == b' ')
        && digits(11, 2)
        && b[13] == b':'
        && digits(14, 2);
    if !matches_shape {
        // ExifTool's second branch: a bare date has its separators swapped.
        if b.len() >= 4 && digits(0, 4) {
            return val.replace('-', ":");
        }
        return val.to_string();
    }

    let mut rest = &val[16..];
    let mut seconds = "";
    if rest.len() >= 3
        && rest.as_bytes()[0] == b':'
        && rest.as_bytes()[1].is_ascii_digit()
        && rest.as_bytes()[2].is_ascii_digit()
    {
        seconds = &rest[..3];
        rest = &rest[3..];
    }
    // `\s*(\S*)$` keeps only a trailing run with no interior whitespace.
    let trailing = rest.trim_start();
    if trailing.chars().any(char::is_whitespace) {
        return val.to_string();
    }
    format!(
        "{}:{}:{} {}{}{}",
        &val[0..4],
        &val[5..7],
        &val[8..10],
        &val[11..16],
        seconds,
        trailing
    )
}

/// `Image::ExifTool::Exif::PrintFNumber`: `%.2f` below 1, `%.1f` at or above.
fn print_f_number(val: f64) -> String {
    if val > 0.0 && val < 1.0 {
        format!("{val:.2}")
    } else {
        format!("{val:.1}")
    }
}

/// `Image::ExifTool::Exif::PrintExposureTime`, the shared port. A private
/// copy used to live here; it agreed with the shared one byte-for-byte on
/// every probe of the consolidation sweep, so it is gone.
fn print_exposure_time(secs: f64) -> String {
    crate::core::formatters::print_exposure_time(secs)
}

/// Apply a table entry's conversion to the raw JSON text.
///
/// A value that is not the number the conversion expects is passed through
/// untouched, which is what Perl's numeric operators would effectively do for
/// a string, and keeps a malformed file from producing a fabricated number.
fn convert(conv: Conv, raw: &str) -> Converted {
    let number = raw.parse::<f64>().ok();
    match (conv, number) {
        (Conv::Raw, _) => Converted::print_only(raw.to_string()),
        (Conv::XmpDate, _) => Converted::print_only(convert_xmp_date(raw)),
        (Conv::FNumber, Some(v)) => Converted::with_value(print_f_number(v), raw.to_string()),
        (Conv::FocalLength, Some(v)) => {
            let mm = v * 1000.0;
            // The value form never reaches output, so it keeps every bit
            // rather than Perl's 15 printed digits.
            Converted::with_value(format!("{mm:.1} mm"), mm.to_string())
        }
        (Conv::Celsius, Some(v)) => Converted::print_only(format!("{v:.1} C")),
        (Conv::ExposureTime, Some(v)) => {
            Converted::with_value(print_exposure_time(v), raw.to_string())
        }
        (Conv::FocalPlaneRes, Some(v)) if v != 0.0 => {
            let ppi = 25.4 / v / 1000.0;
            Converted::print_only(perl_number(ppi))
        }
        (Conv::ExposureBias, Some(v)) => Converted::print_only(format!("{v:+.1}")),
        (Conv::Orientation, _) => Converted::print_only(match raw {
            "1" => "Horizontal (normal)".to_string(),
            // ExifTool's default for a PrintConv hash with no matching key.
            other => format!("Unknown ({other})"),
        }),
        // Numeric conversion wanted but the text is not a number.
        (_, None) | (Conv::FocalPlaneRes, Some(_)) => Converted::print_only(raw.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Flattening
// ---------------------------------------------------------------------------

/// Accumulates tags in document order, applying ExifTool's List semantics.
#[derive(Default)]
struct Collector {
    /// `Lytro:Name` keys in first-seen order.
    order: Vec<String>,
    /// Every value recorded for a key, in order, alongside whether that key
    /// is `List => 1` (`ExtractTags` sets it per key from whether *that*
    /// JSON value is itself an array, Lytro.pm:118 -- not from whether the
    /// flattened name happens to repeat, which it does whenever an array of
    /// *objects* shares a field name, e.g. `Modes.RegionOfInterestArray[]
    /// .type` flattening to `ModesRegionOfInterestArrayType` once per
    /// element).
    values: Vec<(String, bool, Vec<TagValue>)>,
    /// The ValueConv form of the most recent value, when it differs.
    forms: Vec<(String, String)>,
}

impl Collector {
    fn slot(&mut self, key: &str) -> usize {
        if let Some(i) = self.values.iter().position(|(k, _, _)| k == key) {
            return i;
        }
        self.order.push(key.to_string());
        self.values.push((key.to_string(), false, Vec::new()));
        self.values.len() - 1
    }

    /// Record one value.
    ///
    /// A `List => 1` key's repeats really are multiple values of *one*
    /// occurrence (Lytro.pm:118, `finish` below joins them into a
    /// `TagValue::Array`); a non-`List` key's repeats -- like the
    /// `RegionOfInterestArray` example above -- are ordinary duplicate
    /// *occurrences* of a scalar tag, which ExifTool's `HandleTag` retains
    /// individually (last one visible by default, every one visible under
    /// `-a`) rather than collapsing. This used to overwrite the whole vec on
    /// every non-list push (`self.values[i].1 = vec![value]`), which is the
    /// right *default-view* answer but discarded every earlier occurrence
    /// outright -- `duplicate_loss_scan.py` scored
    /// `Lytro:ModesRegionOfInterestArrayType` PARTIAL because of it (oracle's
    /// `-a` shows `exposure` then `creative`, oxidex only `creative`).
    /// Keeping every value here and letting `finish` insert non-list ones
    /// one at a time lets [`crate::core::tag_sink::TagSink::record`] retain
    /// every occurrence while still projecting the last as the winner.
    fn push(&mut self, key: &str, value: TagValue, form: Option<String>, list: bool) {
        let i = self.slot(key);
        self.values[i].1 = list;
        self.values[i].2.push(value);
        self.forms.retain(|(k, _)| k != key);
        if let Some(form) = form {
            self.forms.push((key.to_string(), form));
        }
    }

    /// Write the collected tags into a metadata map.
    ///
    /// A List tag holding a single value is a scalar in ExifTool, not a
    /// one-element array; only a second value promotes it. `JSONMetadata` (3
    /// blocks here) is an array, `PictureDerivationArray` (1 entry) is not. A
    /// non-List key's values are inserted one at a time instead, so every
    /// occurrence reaches the sink (see `push`'s own doc comment).
    fn finish(self, metadata: &mut MetadataMap) {
        for (key, list, values) in self.values {
            if !list {
                for value in values {
                    metadata.insert(key.clone(), value);
                }
                continue;
            }
            let mut values = values;
            let value = match values.len() {
                0 => continue,
                1 => values.remove(0),
                _ => TagValue::Array(values),
            };
            metadata.insert(key.clone(), value);
        }
        for (key, form) in self.forms {
            metadata.set_value_form(key, form);
        }
    }
}

/// `Image::ExifTool::Lytro::ExtractTags` (Lytro.pm:104).
///
/// Walks the object graph, concatenating `ucfirst` of each key onto `parent`.
/// An array of objects recurses once per element under the same path, which is
/// how `frameArray[0].frame.metadataRef` flattens to
/// `PictureFrameArrayFrameMetadataRef` with no index in the name.
fn extract_tags(node: &JsonValue, parent: &str, out: &mut Collector) {
    let JsonValue::Object(entries) = node else {
        return;
    };
    for (key, value) in entries {
        let path = format!("{parent}{}", ucfirst(key));
        let (items, is_list): (&[JsonValue], bool) = match value {
            JsonValue::Array(items) => (items.as_slice(), true),
            other => (std::slice::from_ref(other), false),
        };
        for item in items {
            if matches!(item, JsonValue::Object(_)) {
                extract_tags(item, &path, out);
                continue;
            }
            let raw = match item {
                JsonValue::Str(s) => s.as_str(),
                JsonValue::Raw(s) => s.as_str(),
                // An array of arrays has no ExifTool representation.
                _ => continue,
            };
            emit(&path, raw, is_list, out);
        }
    }
}

/// Resolve one flattened path to its ExifTool name and value, then record it.
fn emit(path: &str, raw: &str, is_list: bool, out: &mut Collector) {
    let (name, converted) = match table_entry(path) {
        Some(tag) => (tag.name.to_string(), convert(tag.conv, raw)),
        None => (derive_name(path), Converted::print_only(raw.to_string())),
    };
    let key = format!("Lytro:{name}");
    out.push(
        &key,
        TagValue::String(converted.print),
        converted.value,
        is_list,
    );
}

// ---------------------------------------------------------------------------
// Container walk
// ---------------------------------------------------------------------------

/// Does this payload look like the JSON metadata ExifTool accepts?
///
/// Lytro.pm:160 tests `/^\{\s+"/` -- an opening brace, at least one space, then
/// a quote. An embedded JPEG is recognised separately by its SOI marker.
fn is_json_payload(payload: &[u8]) -> bool {
    let Some(rest) = payload.strip_prefix(b"{") else {
        return false;
    };
    let spaces = rest
        .iter()
        .position(|c| !c.is_ascii_whitespace())
        .unwrap_or(rest.len());
    spaces > 0 && rest.get(spaces) == Some(&b'"')
}

/// Parser for Lytro Light Field Picture files.
pub struct LytroParser;

impl LytroParser {
    /// Check the 8-byte file header (Lytro.pm:142).
    pub fn verify_signature(reader: &dyn FileReader) -> Result<bool> {
        if reader.size() < LFP_MAGIC.len() as u64 {
            return Ok(false);
        }
        let header = reader.read(0, LFP_MAGIC.len())?;
        Ok(header == LFP_MAGIC)
    }
}

impl FormatParser for LytroParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        if !Self::verify_signature(reader)? {
            return Err(ExifToolError::parse_error("Invalid LFP signature"));
        }

        let size = reader.size() as usize;
        let data = reader.read(0, size)?;
        let mut metadata = MetadataMap::new();
        let mut collector = Collector::default();

        // The 16-byte file header is followed directly by the first section.
        let mut offset = SECTION_HEADER_LEN;
        while offset + SECTION_HEADER_LEN <= data.len() {
            let Some(header) = data.get(offset..offset + SECTION_HEADER_LEN) else {
                break;
            };
            if !header.starts_with(SECTION_MAGIC) {
                // ExifTool warns 'LFP format error' and stops.
                break;
            }
            let length = u32::from_be_bytes([header[12], header[13], header[14], header[15]]);
            if length & 0x8000_0000 != 0 {
                // 'Invalid LFP segment size' (Lytro.pm:149).
                break;
            }
            offset += SECTION_HEADER_LEN;

            // The 80-byte SHA-1 identifier is read and discarded.
            if offset + SECTION_ID_LEN > data.len() {
                break;
            }
            offset += SECTION_ID_LEN;

            // ExifTool seeks past an oversized section rather than buffering
            // it (Lytro.pm:155); such a section is image data, never metadata.
            let buffered = length <= MAX_BUFFERED_SECTION;
            let length = length as usize;
            if buffered {
                let Some(payload) = data.get(offset..offset + length) else {
                    break;
                };
                read_section(payload, &mut collector);
            }
            offset += length;

            // Sections are padded up to the next 16-byte boundary.
            let pad = SECTION_HEADER_LEN - (length % SECTION_HEADER_LEN);
            if pad != SECTION_HEADER_LEN {
                offset += pad;
            }
        }

        collector.finish(&mut metadata);
        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        format == FileFormat::LFP
    }
}

/// Handle one section payload (Lytro.pm:160-167).
fn read_section(payload: &[u8], collector: &mut Collector) {
    if is_json_payload(payload) {
        collector.push(
            "Lytro:JSONMetadata",
            TagValue::Binary(payload.to_vec()),
            None,
            true,
        );
        if let Ok(text) = std::str::from_utf8(payload)
            && let Some(root) = parse_json(text)
        {
            extract_tags(&root, "", collector);
        }
    } else if payload.starts_with(b"\xff\xd8\xff") {
        collector.push(
            "Lytro:EmbeddedImage",
            TagValue::Binary(payload.to_vec()),
            None,
            false,
        );
    }
}

/// Parse metadata from a Lytro LFP file.
///
/// # Errors
///
/// Returns an error string if the file does not carry the LFP signature or
/// cannot be read.
pub fn parse_lytro_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    LytroParser.parse(reader).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_names_the_way_exiftool_does() {
        // Lytro.pm:115 collapses the separator and upper-cases what follows.
        assert_eq!(
            derive_name("PictureFrameArrayParametersVendorContentCom.lytro.tagsDarkFrame"),
            "PictureFrameArrayDarkFrame"
        );
        // Lytro.pm:118 strips the `Devices` prefix.
        assert_eq!(derive_name("DevicesMlaLensPitch"), "MlaLensPitch");
        assert_eq!(derive_name("DevicesSensorMosaicTile"), "SensorMosaicTile");
        // A path with neither feature is unchanged.
        assert_eq!(derive_name("ImageWidth"), "ImageWidth");
    }

    #[test]
    fn ucfirst_matches_perl() {
        assert_eq!(ucfirst("lensPitch"), "LensPitch");
        assert_eq!(ucfirst("com.lytro.tags"), "Com.lytro.tags");
        assert_eq!(ucfirst(""), "");
    }

    #[test]
    fn json_reader_keeps_number_text_verbatim() {
        // The point of the reader: 21 significant digits survive, because
        // ExifTool never numifies them either.
        let doc = parse_json("{\n\t\"x\" : -0.039215687662363052368\n}").expect("parses");
        let JsonValue::Object(entries) = doc else {
            panic!("expected an object");
        };
        assert_eq!(
            entries,
            vec![(
                "x".to_string(),
                JsonValue::Raw("-0.039215687662363052368".to_string())
            )]
        );
    }

    #[test]
    fn json_reader_preserves_key_order_and_empty_containers() {
        let doc = parse_json("{\"b\":[],\"a\":{},\"c\":[1,2]}").expect("parses");
        let JsonValue::Object(entries) = doc else {
            panic!("expected an object");
        };
        let keys: Vec<&str> = entries.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, vec!["b", "a", "c"]);
        assert_eq!(entries[0].1, JsonValue::Array(vec![]));
        assert_eq!(entries[1].1, JsonValue::Object(vec![]));
    }

    #[test]
    fn converts_the_zulu_timestamp_like_convert_xmp_date() {
        assert_eq!(
            convert_xmp_date("2012-04-12T14:10:55.000Z"),
            "2012:04:12 14:10:55.000Z"
        );
        // Seconds are optional in the pattern.
        assert_eq!(convert_xmp_date("2012-04-12T14:10Z"), "2012:04:12 14:10Z");
        // Anything else falls through the second branch or is left alone.
        assert_eq!(convert_xmp_date("not a date"), "not a date");
    }

    #[test]
    fn perl_number_uses_fifteen_significant_digits() {
        // ExifTool prints FocalPlaneXResolution straight from the NV.
        assert_eq!(
            perl_number(25.4 / 1.3999999761581419596e-06 / 1000.0),
            "18142.8574518282"
        );
        assert_eq!(perl_number(1.0), "1");
        assert_eq!(perl_number(0.0), "0");
        assert_eq!(perl_number(1.5e-7), "1.5e-07");
    }

    #[test]
    fn print_conversions_match_exiftool() {
        assert_eq!(print_f_number(1.9099999666213989258), "1.9");
        assert_eq!(print_f_number(0.95), "0.95");
        assert_eq!(print_exposure_time(0.0040000001899898052216), "1/250");
        assert_eq!(print_exposure_time(2.0), "2");
        assert_eq!(print_exposure_time(1.5), "1.5");
    }

    #[test]
    fn exposure_bias_keeps_its_sign() {
        assert_eq!(convert(Conv::ExposureBias, "0").print, "+0.0");
        assert_eq!(
            convert(Conv::ExposureBias, "-1.152003169059753418").print,
            "-1.2"
        );
    }

    #[test]
    fn focal_length_keeps_a_full_precision_value_form() {
        // The printed value rounds to one decimal; the Composite layer needs
        // the unrounded millimetres or FocalLength35efl lands on 42.6 mm.
        let c = convert(Conv::FocalLength, "0.0064499998092651363371");
        assert_eq!(c.print, "6.4 mm");
        assert_eq!(c.value.as_deref(), Some("6.449999809265137"));
    }

    #[test]
    fn json_payload_gate_matches_the_perl_regex() {
        assert!(is_json_payload(b"{\n\t\"picture\" : {"));
        assert!(is_json_payload(b"{ \"a\":1}"));
        // No whitespace between the brace and the quote: ExifTool skips it.
        assert!(!is_json_payload(b"{\"a\":1}"));
        assert!(!is_json_payload(b"\xff\xd8\xffnot json"));
    }
}
