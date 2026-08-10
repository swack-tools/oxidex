//! Output formatting for JSON/CSV/human-readable formats
//!
//! This module handles formatting metadata output in various formats.
//!
//! # Examples
//!
//! ```
//! use oxidex::cli::output_formatter::{OutputFormatter, HumanReadableFormatter, JsonFormatter};
//! use oxidex::core::metadata_map::MetadataMap;
//! use oxidex::core::tag_value::TagValue;
//!
//! let mut metadata = MetadataMap::new();
//! metadata.insert("EXIF:Make", TagValue::new_string("Canon"));
//! metadata.insert("EXIF:Model", TagValue::new_string("EOS 5D"));
//!
//! // Human-readable format
//! let human_formatter = HumanReadableFormatter;
//! let output = human_formatter.format(&metadata, None);
//! println!("{}", output);
//!
//! // JSON format
//! let json_formatter = JsonFormatter;
//! let json_output = json_formatter.format(&metadata, None);
//! println!("{}", json_output);
//! ```

use crate::core::formatters::decode_gps_processing_method;
use crate::core::formatters::exiftool_rational_number;
use crate::core::metadata_map::MetadataMap;
use crate::core::tag_value::TagValue;
use crate::core::value_formatter::format_gps_reference;
use crate::parsers::tiff::tiff_enums::tiff_enum_to_string;
use csv::Writer;

/// Renders the placeholder ExifTool prints in place of binary tag data.
///
/// ExifTool builds this in two halves. The value itself is produced by
/// `ExtractBinary`, which returns the bare sentence without ever seeking:
///
/// ```text
/// ExifTool.pm:9832      return "Binary data $length bytes";
/// ```
///
/// The CLI then wraps it in parentheses and appends the `-b` hint when it is
/// about to print a scalar reference:
///
/// ```text
/// exiftool:3981         my $bOpt = $html ? '' : ', use -b option to extract';
/// exiftool:3982         if ($$obj =~ /^Binary data \d+ bytes$/) {
/// exiftool:3983             $obj = "($$obj$bOpt)";
/// ```
///
/// So the text a plain `exiftool -a -G1 -s` run prints is
/// `(Binary data N bytes, use -b option to extract)`. oxidex previously printed
/// `(Binary, N bytes)`, a form ExifTool emits nowhere: across the 4,240-file
/// sample corpus ExifTool produces `Binary data` 7,716 times and `(Binary,`
/// zero times, so no tag could ever have been matching on the short spelling.
///
/// The same wording is already hard-coded by a dozen parsers that hand back a
/// pre-rendered `TagValue::String` (`src/parsers/icc/tags.rs:112`,
/// `src/parsers/jpeg/mpf_parser.rs:284`, and others) and by the tag-comparison
/// harness's own extractor; this makes the four CLI renderers agree with them.
pub(crate) fn binary_placeholder(len: usize) -> String {
    format!("(Binary data {} bytes, use -b option to extract)", len)
}

/// Trait for formatting metadata into different output formats
///
/// This trait defines a common interface for all output formatters,
/// allowing the CLI to select the appropriate formatter based on user preferences.
pub trait OutputFormatter {
    /// Formats the given metadata into a string representation
    ///
    /// # Arguments
    ///
    /// * `metadata` - The metadata map to format
    /// * `filter_tags` - Optional list of tag names to include in output.
    ///   If None, all tags are included.
    ///
    /// # Returns
    ///
    /// A formatted string representation of the metadata
    fn format(&self, metadata: &MetadataMap, filter_tags: Option<&[String]>) -> String;
}

fn tag_matches_filter(tag_name: &str, filter: &[String]) -> bool {
    // ExifTool tag-name arguments are case-insensitive (`-make` matches IFD0:Make).
    filter.iter().any(|requested| {
        requested.eq_ignore_ascii_case(tag_name)
            || tag_name
                .rsplit(':')
                .next()
                .is_some_and(|short_name| short_name.eq_ignore_ascii_case(requested))
    })
}

/// Formats metadata in human-readable key-value format
///
/// Output format: "Tag: Value\n" for each tag, sorted alphabetically by tag name.
///
/// # Examples
///
/// ```
/// use oxidex::cli::output_formatter::{OutputFormatter, HumanReadableFormatter};
/// use oxidex::core::metadata_map::MetadataMap;
/// use oxidex::core::tag_value::TagValue;
///
/// let mut metadata = MetadataMap::new();
/// metadata.insert("EXIF:Make", TagValue::new_string("Canon"));
/// metadata.insert("EXIF:ISO", TagValue::new_integer(400));
///
/// let formatter = HumanReadableFormatter;
/// let output = formatter.format(&metadata, None);
/// // Output:
/// // EXIF:ISO: 400
/// // EXIF:Make: Canon
/// ```
pub struct HumanReadableFormatter;

impl OutputFormatter for HumanReadableFormatter {
    fn format(&self, metadata: &MetadataMap, filter_tags: Option<&[String]>) -> String {
        if metadata.is_empty() {
            return String::new();
        }

        // Collect tags into a vector for sorting
        let mut tags: Vec<_> = metadata.iter().collect();

        // Filter tags if a filter is provided
        if let Some(filter) = filter_tags {
            tags.retain(|(name, _)| tag_matches_filter(name, filter));
            if tags.is_empty() {
                return String::new();
            }
        }

        // Sort tags alphabetically by name
        tags.sort_by_key(|(name, _)| *name);

        // Check if this is a raw format by examining File:FileType tag
        // Raw formats include keywords like "Raw", "DNG", "CR2", "NEF", etc.
        let is_raw = metadata
            .get("File:FileType")
            .and_then(|v| v.as_string())
            .map(|s| {
                s.contains("Raw")
                    || s.contains("DNG")
                    || s.contains("CR2")
                    || s.contains("CR3")
                    || s.contains("NEF")
                    || s.contains("ARW")
                    || s.contains("RAF")
                    || s.contains("ORF")
                    || s.contains("PEF")
                    || s.contains("RW2")
            })
            .unwrap_or(false);

        // Format each tag as "Tag: Value\n"
        let mut output = String::new();

        // Add "Camera Raw File" header for raw formats
        if is_raw {
            output.push_str("Camera Raw File\n");
            output.push_str("---------------\n");
        }

        for (tag_name, tag_value) in tags {
            // Skip large binary data fields to prevent terminal corruption
            if let TagValue::Binary(bytes) = tag_value
                && bytes.len() > 256
            {
                // Skip large binary fields in human-readable output
                continue;
            }

            // Skip known problematic tags that contain structured binary/text data
            // These tags are typically very large and not useful in default output
            if matches!(
                tag_name.as_str(),
                "IFD0:LeafData"
                    | "IFD1:LeafData"
                    | "EXIF:MakerNoteApple"
                    | "EXIF:PrintIM"
                    | "EXIF:ApplicationNotes"
            ) {
                // Skip in human-readable output (still available in JSON/CSV if small enough)
                continue;
            }

            let formatted_value = format_tag_value(tag_name, tag_value);
            output.push_str(&format!("{}: {}\n", tag_name, formatted_value));
        }

        output
    }
}

/// Formats metadata as JSON
///
/// Uses `serde_json` to serialize the metadata map into a JSON string.
/// The output is pretty-printed for readability.
///
/// # Examples
///
/// ```
/// use oxidex::cli::output_formatter::{OutputFormatter, JsonFormatter};
/// use oxidex::core::metadata_map::MetadataMap;
/// use oxidex::core::tag_value::TagValue;
///
/// let mut metadata = MetadataMap::new();
/// metadata.insert("EXIF:Make", TagValue::new_string("Canon"));
///
/// let formatter = JsonFormatter;
/// let json = formatter.format(&metadata, None);
/// // JSON output can be parsed by jq or any JSON parser
/// ```
pub struct JsonFormatter;

impl OutputFormatter for JsonFormatter {
    fn format(&self, metadata: &MetadataMap, filter_tags: Option<&[String]>) -> String {
        // If filter is specified, create a new filtered metadata map
        let metadata_to_filter = if let Some(filter) = filter_tags {
            let filtered: MetadataMap = metadata
                .iter()
                .filter(|(name, _)| tag_matches_filter(name, filter))
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect();
            filtered
        } else {
            metadata.clone()
        };

        // Convert MetadataMap to a simple HashMap for Perl ExifTool-compatible JSON output
        // Unwrap TagValue enum to produce flat values like {"EXIF:Make": "Canon"}
        // instead of {"EXIF:Make": {"type": "String", "value": "Canon"}}
        let mut json_map = serde_json::Map::new();

        for (tag_name, tag_value) in metadata_to_filter.iter() {
            let json_value = tag_value_to_json(Some(tag_name.as_str()), tag_value);
            json_map.insert(tag_name.clone(), json_value);
        }

        // Serialize to pretty JSON wrapped in an array for Perl ExifTool compatibility
        // Perl ExifTool outputs: [{...}] (array with one object per file)
        // This allows processing multiple files with consistent JSON structure
        match serde_json::to_string_pretty(&vec![json_map]) {
            Ok(json) => json,
            Err(e) => {
                // Fallback error message if serialization fails
                format!("[{{\"error\": \"Failed to serialize metadata: {}\"}}]", e)
            }
        }
    }
}

/// Converts a TagValue to a serde_json::Value for Perl ExifTool-compatible output
///
/// This unwraps the TagValue enum and produces simple JSON values:
/// - String → JSON string (except literal "true"/"false", which become JSON booleans)
/// - Integer → JSON number
/// - Float → JSON number
/// - Rational → JSON string "numerator/denominator"
/// - Binary → JSON string "(Binary data N bytes, use -b option to extract)"
/// - DateTime → JSON string (EXIF format: "YYYY:MM:DD HH:MM:SS")
/// - Struct → JSON object (recursive)
fn tag_value_to_json(tag_name: Option<&str>, value: &TagValue) -> serde_json::Value {
    if let Some(name) = tag_name
        && let Some(label) = friendly_enum_name(name, value)
    {
        return serde_json::Value::String(label);
    }

    match value {
        TagValue::String(s) => {
            // Mirror the `exiftool` script's JSON typing (around line 3807):
            // `return lc($str) if $str =~ /^(true|false)$/i` emits a bare
            // JSON boolean for these two literal strings, not a quoted one.
            if s.eq_ignore_ascii_case("true") {
                serde_json::Value::Bool(true)
            } else if s.eq_ignore_ascii_case("false") {
                serde_json::Value::Bool(false)
            } else {
                serde_json::Value::String(s.clone())
            }
        }
        TagValue::Integer(i) => serde_json::json!(*i),
        TagValue::Float(f) => serde_json::json!(*f),
        TagValue::Rational {
            numerator,
            denominator,
        } => {
            // Normalize rational display to match Perl ExifTool
            if *denominator == 0 {
                // Invalid rational, output as string
                serde_json::Value::String(format!("{}/0", numerator))
            } else if *denominator == 1 {
                // Output as integer string (e.g., "100/1" → "100")
                serde_json::Value::String(format!("{}", numerator))
            } else if *numerator == 0 {
                // Zero rational
                serde_json::Value::String("0".to_string())
            } else {
                // Check if this should be output as a decimal number (like Perl ExifTool does for FNumber)
                // For typical aperture/focal length values, output as decimal
                let decimal = *numerator as f64 / *denominator as f64;
                if decimal < 1000.0 && decimal.fract() != 0.0 {
                    // This looks like an aperture or similar value, output as JSON Number
                    if let Some(num) = serde_json::Number::from_f64(decimal) {
                        return serde_json::Value::Number(num);
                    }
                }
                // Otherwise keep as fraction string
                serde_json::Value::String(format!("{}/{}", numerator, denominator))
            }
        }
        TagValue::Binary(bytes) => serde_json::Value::String(binary_placeholder(bytes.len())),
        TagValue::DateTime(dt) => {
            // Format as EXIF DateTime: "YYYY:MM:DD HH:MM:SS"
            // This matches Perl ExifTool's output format
            serde_json::Value::String(dt.format("%Y:%m:%d %H:%M:%S").to_string())
        }
        TagValue::Struct(map) => {
            let mut obj = serde_json::Map::new();
            for (key, val) in map.iter() {
                obj.insert(key.clone(), tag_value_to_json(None, val));
            }
            serde_json::Value::Object(obj)
        }
        TagValue::Array(values) => {
            let array: Vec<serde_json::Value> = values
                .iter()
                .map(|v| tag_value_to_json(tag_name, v))
                .collect();
            serde_json::Value::Array(array)
        }
    }
}

/// Formats metadata as CSV
///
/// Output format: Two-column CSV with "Tag" and "Value" headers.
/// Each metadata entry becomes a row with the tag name and its formatted value.
/// The CSV is RFC 4180 compliant and parseable by standard tools (Excel, pandas).
///
/// # Examples
///
/// ```
/// use oxidex::cli::output_formatter::{OutputFormatter, CsvFormatter};
/// use oxidex::core::metadata_map::MetadataMap;
/// use oxidex::core::tag_value::TagValue;
///
/// let mut metadata = MetadataMap::new();
/// metadata.insert("EXIF:Make", TagValue::new_string("Canon"));
/// metadata.insert("EXIF:ISO", TagValue::new_integer(400));
///
/// let formatter = CsvFormatter;
/// let csv = formatter.format(&metadata, None);
/// // Output:
/// // Tag,Value
/// // EXIF:ISO,400
/// // EXIF:Make,Canon
/// ```
pub struct CsvFormatter;

impl OutputFormatter for CsvFormatter {
    fn format(&self, metadata: &MetadataMap, filter_tags: Option<&[String]>) -> String {
        if metadata.is_empty() {
            return String::new();
        }

        // Collect tags into a vector for sorting
        let mut tags: Vec<_> = metadata.iter().collect();

        // Filter tags if a filter is provided
        if let Some(filter) = filter_tags {
            tags.retain(|(name, _)| tag_matches_filter(name, filter));
            if tags.is_empty() {
                return String::new();
            }
        }

        // Sort tags alphabetically by name
        tags.sort_by_key(|(name, _)| *name);

        // Create CSV writer that writes to a Vec<u8> buffer
        let mut wtr = Writer::from_writer(vec![]);

        // Write header row
        if wtr.write_record(["Tag", "Value"]).is_err() {
            return String::from("Tag,Value\n");
        }

        // Write data rows
        for (tag_name, tag_value) in tags {
            // Skip large binary data fields to prevent CSV corruption
            if let TagValue::Binary(bytes) = tag_value
                && bytes.len() > 256
            {
                // Skip large binary fields in CSV output
                continue;
            }

            let formatted_value = format_tag_value(tag_name, tag_value);
            if wtr.write_record([tag_name, &formatted_value]).is_err() {
                // Skip this record if write fails, but continue
                continue;
            }
        }

        // Flush the writer and get the buffer
        if wtr.flush().is_err() {
            return String::from("Tag,Value\n");
        }

        let data = match wtr.into_inner() {
            Ok(buffer) => buffer,
            Err(_) => return String::from("Tag,Value\n"),
        };

        // Convert bytes to UTF-8 string
        String::from_utf8(data).unwrap_or_else(|_| String::from("Tag,Value\n"))
    }
}

/// Formats metadata in short (compact) format
///
/// Output format: "ShortTagName: Value" with family prefix stripped
/// and long values truncated. This provides a more concise view of metadata.
///
/// # Examples
///
/// ```
/// use oxidex::cli::output_formatter::{OutputFormatter, ShortFormatter};
/// use oxidex::core::metadata_map::MetadataMap;
/// use oxidex::core::tag_value::TagValue;
///
/// let mut metadata = MetadataMap::new();
/// metadata.insert("EXIF:Make", TagValue::new_string("Canon"));
/// metadata.insert("EXIF:ISO", TagValue::new_integer(400));
///
/// let formatter = ShortFormatter;
/// let output = formatter.format(&metadata, None);
/// // Output:
/// // Make: Canon
/// // ISO: 400
/// ```
pub struct ShortFormatter;

impl OutputFormatter for ShortFormatter {
    fn format(&self, metadata: &MetadataMap, filter_tags: Option<&[String]>) -> String {
        if metadata.is_empty() {
            return String::new();
        }

        // Collect tags into a vector for sorting
        let mut tags: Vec<_> = metadata.iter().collect();

        // Filter tags if a filter is provided
        if let Some(filter) = filter_tags {
            tags.retain(|(name, _)| tag_matches_filter(name, filter));
            if tags.is_empty() {
                return String::new();
            }
        }

        // Sort tags alphabetically by name
        tags.sort_by_key(|(name, _)| *name);

        // Format each tag in short format
        let mut output = String::new();
        for (tag_name, tag_value) in tags {
            // Skip large binary data fields
            if let TagValue::Binary(bytes) = tag_value
                && bytes.len() > 256
            {
                continue;
            }

            // Skip known problematic tags
            if matches!(
                tag_name.as_str(),
                "IFD0:LeafData"
                    | "IFD1:LeafData"
                    | "EXIF:MakerNoteApple"
                    | "EXIF:PrintIM"
                    | "EXIF:ApplicationNotes"
            ) {
                continue;
            }

            // Extract short name (after last colon)
            let short_name = tag_name.rsplit(':').next().unwrap_or(tag_name);
            let formatted_value = format_tag_value_short(tag_name, tag_value);
            output.push_str(&format!("{}: {}\n", short_name, formatted_value));
        }

        output
    }
}

/// Helper function to format a TagValue for short format display
///
/// Similar to `format_tag_value`, but with the compact numeric/array rendering
/// used by the `-s` output.
///
/// Values are deliberately **not** truncated. ExifTool's `-s` shortens tag
/// *names* (it prints the tag name instead of the long description); it never
/// shortens tag *values*. `exiftool -s` on the sample corpus prints values in
/// full at any length -- e.g. the 290-character `JSONInfo` on ExifTool.jpg and
/// the 196-character `AFAreaXPositions` on Canon1DmkIII.jpg both come out
/// whole. Only binary blobs are summarised, and that is independent of `-s`.
///
/// The previous implementation cut strings at `&s[..47]`, which both diverged
/// from ExifTool and panicked outright whenever byte 47 landed inside a
/// multi-byte UTF-8 sequence. That is not a rare case: any lossy-decoded
/// MakerNote/GPS text is full of 3-byte U+FFFD replacement characters, and 14
/// files in the sample corpus (Samsung and Canon JPEGs, via
/// `GPS:GPSProcessingMethod`) killed the whole `oxidex -e -s` invocation on it.
fn format_tag_value_short(tag_name: &str, value: &TagValue) -> String {
    if let Some(label) = friendly_enum_name(tag_name, value) {
        return label;
    }

    match value {
        TagValue::String(s) => s.clone(),
        TagValue::Integer(i) => i.to_string(),
        TagValue::Float(f) => format!("{:.2}", f), // Limit decimal places
        TagValue::Rational {
            numerator,
            denominator,
        } => {
            if tag_name == "GPS:GPSDOP" && *denominator != 0 {
                exiftool_rational_number(*numerator as f64 / *denominator as f64)
            } else if *denominator == 1 {
                numerator.to_string()
            } else if *denominator == 0 {
                "0".to_string()
            } else {
                format!("{}/{}", numerator, denominator)
            }
        }
        // Same wording as the other three renderers. `-s` shortens tag *names*,
        // not values, so there is no reason for it to describe a binary blob
        // differently from `-e`, `-j` and `-csv` -- `ExifIFD:FileSource` came
        // out as `(Binary, 1 bytes)` everywhere else and `(1 bytes)` here.
        TagValue::Binary(bytes) => binary_placeholder(bytes.len()),
        TagValue::DateTime(dt) => dt.format("%Y:%m:%d %H:%M:%S").to_string(),
        TagValue::Struct(_) => "(struct)".to_string(),
        // `[4 items]` was the one place this renderer still shortened a value,
        // contradicting the rule stated above -- and it was unrecoverable, since
        // the items themselves never reached the output. PhotoMechanic.jpg's
        // IPTC Keywords printed `[4 items]` where ExifTool prints
        // `Keywords, one, three, two`.
        TagValue::Array(values) => join_list(
            values
                .iter()
                .map(|v| format_tag_value_short(tag_name, v))
                .collect(),
        ),
    }
}

/// ExifTool's rendering of a list-valued tag: the items, joined, unbracketed.
///
/// `ListSep` defaults to `', '` (ExifTool.pm:1173) and is applied as a plain
/// `join $$self{OPTIONS}{ListSep}, @$value` (ExifTool.pm:3736). There are no
/// enclosing brackets and no element count -- `XMP-dc:Subject` of three items
/// prints `ExifTool, Test, XMP`, not `[ExifTool, Test, XMP]`.
///
/// The bracketed form was a debug rendering of the `TagValue::Array` variant
/// that reached real output: every list tag in the corpus (`Subject`,
/// `Keywords`, `Creator`, `SupplementalCategories`) carried the brackets, and
/// each one is a value no consumer could parse back into what ExifTool reports.
///
/// This is display only. `-j` still emits a JSON array via `tag_value_to_json`,
/// which is what `exiftool -j` does for a list tag.
fn join_list(items: Vec<String>) -> String {
    items.join(", ")
}

/// Helper function to format a TagValue for human-readable display
///
/// Converts each TagValue variant into a clean string representation
/// without the enum structure (e.g., "Canon" instead of "String(\"Canon\")").
fn format_tag_value(tag_name: &str, value: &TagValue) -> String {
    if let Some(label) = friendly_enum_name(tag_name, value) {
        return label;
    }

    match value {
        TagValue::String(s) => s.clone(),
        TagValue::Integer(i) => i.to_string(),
        TagValue::Float(f) => f.to_string(),
        TagValue::Rational {
            numerator,
            denominator,
        } if tag_name == "GPS:GPSDOP" && *denominator != 0 => {
            exiftool_rational_number(*numerator as f64 / *denominator as f64)
        }
        TagValue::Rational {
            numerator,
            denominator,
        } => format!("{}/{}", numerator, denominator),
        TagValue::Binary(bytes) => binary_placeholder(bytes.len()),
        // ExifTool's date form, which is what `-s` and `-j` already print and
        // what ExifTool itself prints: `2005:11:07 11:06:52`. This renderer
        // backs the default output *and* `-csv`, and it was emitting RFC 3339
        // (`2005-11-07T11:06:52+00:00`) -- a shape ExifTool never produces --
        // so `oxidex -e` and `oxidex -e -s` disagreed about the same tag in the
        // same file, 1142 times across 546 corpus files. tag-comparison reads
        // TagValue in-process and never reaches a formatter, so no coverage
        // number moved.
        TagValue::DateTime(dt) => dt.format("%Y:%m:%d %H:%M:%S").to_string(),
        TagValue::Struct(_) => "(Structured data)".to_string(),
        TagValue::Array(values) => join_list(
            values
                .iter()
                .map(|v| format_tag_value(tag_name, v))
                .collect(),
        ),
    }
}

/// Resolves TIFF enumeration names and GPS reference values to human-readable text.
///
/// This function handles two types of value formatting:
/// 1. TIFF enums: Looks up the tag descriptor to retrieve the numeric tag ID and uses
///    the TIFF enum table to translate well-known values (e.g., Orientation).
/// 2. GPS reference values: Converts single-character codes to human-readable descriptions
///    (e.g., "N" -> "North", "T" -> "True North").
fn friendly_enum_name(tag_name: &str, value: &TagValue) -> Option<String> {
    if tag_name.rsplit(':').next() == Some("ApertureValue")
        && let TagValue::String(formatted) =
            crate::core::exiftool_compat::format_tag_value(tag_name, value)
    {
        return Some(formatted);
    }

    if tag_name.rsplit(':').next() == Some("GPSProcessingMethod")
        && let TagValue::Binary(bytes) = value
    {
        let decoded = decode_gps_processing_method(bytes);
        if !decoded.is_empty() {
            return Some(decoded);
        }
    }

    // First, check if this is a GPS reference value (string-based)
    if let TagValue::String(s) = value
        && let Some(formatted) = format_gps_reference(tag_name, s)
    {
        return Some(formatted);
    }

    // Also handle GPS reference values that may be stored as integers
    // (e.g., GPSAltitudeRef 0/1 or GPSDifferential 0/1)
    if let TagValue::Integer(i) = value
        && let Some(formatted) = format_gps_reference(tag_name, &i.to_string())
    {
        return Some(formatted);
    }

    // GPSAltitudeRef is TIFF BYTE[1]. The parsers preserve BYTE values as
    // binary, so resolve this one enum before the generic binary placeholder.
    if tag_name.rsplit(':').next() == Some("GPSAltitudeRef")
        && let TagValue::Binary(bytes) = value
        && let [byte @ (0 | 1)] = bytes.as_slice()
    {
        return format_gps_reference(tag_name, &byte.to_string());
    }

    // Then try TIFF enum lookup for integer values
    let tag_id = lookup_tiff_enum_tag_id(tag_name)?;

    match value {
        TagValue::Integer(i) => tiff_enum_to_string(tag_id, *i),
        _ => None,
    }
}

/// Maps canonical tag names to their numeric TIFF tag IDs for enum resolution.
fn lookup_tiff_enum_tag_id(tag_name: &str) -> Option<u16> {
    match tag_name {
        // Orientation (tag 0x0112)
        "IFD0:Orientation" | "IFD1:Orientation" | "IFD2:Orientation" | "EXIF:Orientation" => {
            Some(0x0112)
        }

        // Compression (tag 0x0103)
        "IFD0:Compression" | "IFD1:Compression" | "IFD2:Compression" | "EXIF:Compression" => {
            Some(0x0103)
        }

        // PhotometricInterpretation (tag 0x0106)
        "IFD0:PhotometricInterpretation"
        | "IFD1:PhotometricInterpretation"
        | "IFD2:PhotometricInterpretation"
        | "EXIF:PhotometricInterpretation" => Some(0x0106),

        // PlanarConfiguration (tag 0x011C)
        "IFD0:PlanarConfiguration"
        | "IFD1:PlanarConfiguration"
        | "IFD2:PlanarConfiguration"
        | "EXIF:PlanarConfiguration" => Some(0x011C),

        // ResolutionUnit (tag 0x0128)
        "IFD0:ResolutionUnit"
        | "IFD1:ResolutionUnit"
        | "IFD2:ResolutionUnit"
        | "EXIF:ResolutionUnit" => Some(0x0128),

        // FillOrder (tag 0x010A)
        "IFD0:FillOrder" | "IFD1:FillOrder" | "IFD2:FillOrder" | "EXIF:FillOrder" => Some(0x010A),

        // SampleFormat (tag 0x0153)
        "IFD0:SampleFormat" | "IFD1:SampleFormat" | "IFD2:SampleFormat" | "EXIF:SampleFormat" => {
            Some(0x0153)
        }

        // YCbCrPositioning (tag 0x0213)
        "IFD0:YCbCrPositioning"
        | "IFD1:YCbCrPositioning"
        | "IFD2:YCbCrPositioning"
        | "EXIF:YCbCrPositioning" => Some(0x0213),

        // ExtraSamples (tag 0x0152)
        "IFD0:ExtraSamples" | "IFD1:ExtraSamples" | "IFD2:ExtraSamples" | "EXIF:ExtraSamples" => {
            Some(0x0152)
        }

        // SubfileType (tag 0x00FE)
        "IFD0:SubfileType" | "IFD1:SubfileType" | "IFD2:SubfileType" | "EXIF:SubfileType" => {
            Some(0x00FE)
        }

        // ColorSpace (tag 0xA001)
        "ExifIFD:ColorSpace" | "EXIF:ColorSpace" => Some(0xA001),

        // SceneType (tag 0xA301)
        // Note: Often stored as binary data, but may appear as integer in some files
        "ExifIFD:SceneType" | "EXIF:SceneType" => Some(0xA301),

        // SensitivityType (tag 0x8830)
        "ExifIFD:SensitivityType" | "EXIF:SensitivityType" => Some(0x8830),

        // CompositeImage (tag 0xA460)
        "ExifIFD:CompositeImage" | "EXIF:CompositeImage" => Some(0xA460),

        // MakerNoteSafety (DNG tag 0xC635)
        "IFD0:MakerNoteSafety" | "EXIF:MakerNoteSafety" => Some(0xC635),

        // MeteringMode (tag 0x9207)
        "ExifIFD:MeteringMode" | "EXIF:MeteringMode" => Some(0x9207),

        // SensingMethod (tag 0xA217)
        "ExifIFD:SensingMethod" | "EXIF:SensingMethod" => Some(0xA217),

        // CustomRendered (tag 0xA401)
        "ExifIFD:CustomRendered" | "EXIF:CustomRendered" => Some(0xA401),

        // ExposureMode (tag 0xA402)
        "ExifIFD:ExposureMode" | "EXIF:ExposureMode" => Some(0xA402),

        // WhiteBalance (tag 0xA403)
        "ExifIFD:WhiteBalance" | "EXIF:WhiteBalance" => Some(0xA403),

        // SceneCaptureType (tag 0xA406)
        "ExifIFD:SceneCaptureType" | "EXIF:SceneCaptureType" => Some(0xA406),

        // ExposureProgram (tag 0x8822)
        "ExifIFD:ExposureProgram" | "EXIF:ExposureProgram" => Some(0x8822),

        // LightSource (tag 0x9208)
        "ExifIFD:LightSource" | "EXIF:LightSource" => Some(0x9208),

        // GainControl (tag 0xA407)
        "ExifIFD:GainControl" | "EXIF:GainControl" => Some(0xA407),

        // Contrast (tag 0xA408)
        "ExifIFD:Contrast" | "EXIF:Contrast" => Some(0xA408),

        // Saturation (tag 0xA409)
        "ExifIFD:Saturation" | "EXIF:Saturation" => Some(0xA409),

        // Sharpness (tag 0xA40A)
        "ExifIFD:Sharpness" | "EXIF:Sharpness" => Some(0xA40A),

        // SubjectDistanceRange (tag 0xA40C)
        "ExifIFD:SubjectDistanceRange" | "EXIF:SubjectDistanceRange" => Some(0xA40C),

        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    #[test]
    fn test_human_readable_formatter_empty_metadata() {
        let metadata = MetadataMap::new();
        let formatter = HumanReadableFormatter;
        let output = formatter.format(&metadata, None);
        assert_eq!(output, "");
    }

    #[test]
    fn test_human_readable_formatter_single_tag() {
        let mut metadata = MetadataMap::new();
        metadata.insert("EXIF:Make", TagValue::new_string("Canon"));

        let formatter = HumanReadableFormatter;
        let output = formatter.format(&metadata, None);
        assert_eq!(output, "EXIF:Make: Canon\n");
    }

    #[test]
    fn test_human_readable_formatter_multiple_tags_sorted() {
        let mut metadata = MetadataMap::new();
        metadata.insert("EXIF:Model", TagValue::new_string("EOS 5D"));
        metadata.insert("EXIF:Make", TagValue::new_string("Canon"));
        metadata.insert("EXIF:ISO", TagValue::new_integer(400));

        let formatter = HumanReadableFormatter;
        let output = formatter.format(&metadata, None);

        // Tags should be sorted alphabetically
        assert_eq!(
            output,
            "EXIF:ISO: 400\nEXIF:Make: Canon\nEXIF:Model: EOS 5D\n"
        );
    }

    #[test]
    fn test_human_readable_formatter_all_value_types() {
        let mut metadata = MetadataMap::new();
        metadata.insert("EXIF:Make", TagValue::new_string("Canon"));
        metadata.insert("EXIF:ISO", TagValue::new_integer(800));
        metadata.insert("EXIF:FNumber", TagValue::new_float(2.8));
        metadata.insert("EXIF:ExposureTime", TagValue::new_rational(1, 100));
        metadata.insert(
            "EXIF:ThumbnailData",
            TagValue::new_binary(vec![0xFF, 0xD8, 0xFF, 0xE0]),
        );

        let dt = Utc.with_ymd_and_hms(2023, 6, 15, 12, 30, 0).unwrap();
        metadata.insert("EXIF:DateTime", TagValue::new_datetime(dt));

        let formatter = HumanReadableFormatter;
        let output = formatter.format(&metadata, None);

        // Verify all types are formatted correctly
        assert!(output.contains("EXIF:Make: Canon"));
        assert!(output.contains("EXIF:ISO: 800"));
        assert!(output.contains("EXIF:FNumber: 2.8"));
        assert!(output.contains("EXIF:ExposureTime: 1/100"));
        assert!(
            output.contains("EXIF:ThumbnailData: (Binary data 4 bytes, use -b option to extract)")
        );
        // ExifTool's date form, matching `-s` and `-j`; this renderer used to
        // print RFC 3339 here, which ExifTool never emits.
        assert!(output.contains("EXIF:DateTime: 2023:06:15 12:30:00"));
    }

    #[test]
    fn test_human_readable_formatter_with_filter() {
        let mut metadata = MetadataMap::new();
        metadata.insert("EXIF:Make", TagValue::new_string("Canon"));
        metadata.insert("EXIF:Model", TagValue::new_string("EOS 5D"));
        metadata.insert("EXIF:ISO", TagValue::new_integer(400));

        let formatter = HumanReadableFormatter;
        let filter = vec!["EXIF:Make".to_string(), "EXIF:ISO".to_string()];
        let output = formatter.format(&metadata, Some(&filter));

        // Only filtered tags should appear
        assert!(output.contains("EXIF:Make: Canon"));
        assert!(output.contains("EXIF:ISO: 400"));
        assert!(!output.contains("EXIF:Model"));
    }

    #[test]
    fn test_human_readable_formatter_resolves_orientation_enum() {
        let mut metadata = MetadataMap::new();
        metadata.insert("IFD0:Orientation", TagValue::new_integer(6));

        let formatter = HumanReadableFormatter;
        let output = formatter.format(&metadata, None);

        assert!(output.contains("IFD0:Orientation: Rotate 90 CW"));
    }

    #[test]
    fn test_human_readable_formatter_filter_nonexistent_tag() {
        let mut metadata = MetadataMap::new();
        metadata.insert("EXIF:Make", TagValue::new_string("Canon"));

        let formatter = HumanReadableFormatter;
        let filter = vec!["EXIF:NonExistent".to_string()];
        let output = formatter.format(&metadata, Some(&filter));

        // No matching tags, should return empty string
        assert_eq!(output, "");
    }

    #[test]
    fn test_json_formatter_empty_metadata() {
        let metadata = MetadataMap::new();
        let formatter = JsonFormatter;
        let output = formatter.format(&metadata, None);

        // Should be valid JSON array (Perl ExifTool compatibility)
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert!(parsed.is_array());
        assert_eq!(parsed.as_array().unwrap().len(), 1);
        assert!(parsed[0].is_object());
        assert_eq!(parsed[0].as_object().unwrap().len(), 0);
    }

    #[test]
    fn test_json_formatter_basic() {
        let mut metadata = MetadataMap::new();
        metadata.insert("EXIF:Make", TagValue::new_string("Canon"));
        metadata.insert("EXIF:ISO", TagValue::new_integer(400));

        let formatter = JsonFormatter;
        let output = formatter.format(&metadata, None);

        // Verify it's valid JSON array
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert!(parsed.is_array());
        assert_eq!(parsed.as_array().unwrap().len(), 1);

        // Verify content (note: TagValue serializes with type/value structure)
        let obj = parsed[0].as_object().unwrap();
        assert!(obj.contains_key("EXIF:Make"));
        assert!(obj.contains_key("EXIF:ISO"));
    }

    #[test]
    fn test_json_formatter_valid_json_structure() {
        let mut metadata = MetadataMap::new();
        metadata.insert("EXIF:Make", TagValue::new_string("Canon"));
        metadata.insert("EXIF:Model", TagValue::new_string("EOS R5"));
        metadata.insert("EXIF:ISO", TagValue::new_integer(800));
        metadata.insert("EXIF:FNumber", TagValue::new_float(2.8));

        let formatter = JsonFormatter;
        let output = formatter.format(&metadata, None);

        // Verify it's parseable by serde_json (same as jq would use)
        let result: Result<serde_json::Value, _> = serde_json::from_str(&output);
        assert!(result.is_ok(), "JSON should be valid and parseable");

        let parsed = result.unwrap();
        assert!(parsed.is_array());
        assert_eq!(parsed.as_array().unwrap().len(), 1);
        assert_eq!(parsed[0].as_object().unwrap().len(), 4);
    }

    #[test]
    fn test_json_formatter_applies_enum_print_conversion() {
        let mut metadata = MetadataMap::new();
        metadata.insert("IFD0:Orientation", TagValue::new_integer(1));

        let formatter = JsonFormatter;
        let output = formatter.format(&metadata, None);

        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        let value = parsed[0]
            .as_object()
            .and_then(|obj| obj.get("IFD0:Orientation"))
            .and_then(|v| v.as_str());

        assert_eq!(value, Some("Horizontal (normal)"));
    }

    #[test]
    fn test_json_formatter_with_filter() {
        let mut metadata = MetadataMap::new();
        metadata.insert("EXIF:Make", TagValue::new_string("Canon"));
        metadata.insert("EXIF:Model", TagValue::new_string("EOS 5D"));
        metadata.insert("EXIF:ISO", TagValue::new_integer(400));

        let formatter = JsonFormatter;
        let filter = vec!["EXIF:Make".to_string()];
        let output = formatter.format(&metadata, Some(&filter));

        // Verify only filtered tag is in JSON
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert!(parsed.is_array());
        let obj = parsed[0].as_object().unwrap();
        assert_eq!(obj.len(), 1);
        assert!(obj.contains_key("EXIF:Make"));
        assert!(!obj.contains_key("EXIF:Model"));
        assert!(!obj.contains_key("EXIF:ISO"));
    }

    #[test]
    fn test_json_formatter_filter_empty_result() {
        let mut metadata = MetadataMap::new();
        metadata.insert("EXIF:Make", TagValue::new_string("Canon"));

        let formatter = JsonFormatter;
        let filter = vec!["EXIF:NonExistent".to_string()];
        let output = formatter.format(&metadata, Some(&filter));

        // Should be valid JSON array with empty object
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert!(parsed.is_array());
        assert_eq!(parsed.as_array().unwrap().len(), 1);
        assert_eq!(parsed[0].as_object().unwrap().len(), 0);
    }

    #[test]
    fn test_format_tag_value_string() {
        let value = TagValue::new_string("Test String");
        assert_eq!(format_tag_value("EXIF:Make", &value), "Test String");
    }

    #[test]
    fn test_format_tag_value_integer() {
        let value = TagValue::new_integer(42);
        assert_eq!(format_tag_value("EXIF:ISO", &value), "42");
    }

    #[test]
    fn test_format_tag_value_float() {
        let value = TagValue::new_float(2.8);
        assert_eq!(format_tag_value("EXIF:FNumber", &value), "2.8");
    }

    #[test]
    fn test_format_tag_value_rational() {
        let value = TagValue::new_rational(1, 125);
        assert_eq!(format_tag_value("EXIF:ExposureTime", &value), "1/125");
    }

    #[test]
    fn gps_dop_uses_exiftool_decimal_rendering() {
        // ExifTool 13.59 GPS.pm declares GPSDOP as rational64u without a
        // PrintConv. GetRational64u renders the quotient, not the stored
        // numerator/denominator pair: 3/2 is displayed as 1.5.
        let value = TagValue::new_rational(3, 2);
        assert_eq!(format_tag_value("GPS:GPSDOP", &value), "1.5");
        assert_eq!(format_tag_value_short("GPS:GPSDOP", &value), "1.5");
    }

    #[test]
    fn aperture_value_uses_apex_conversion_in_all_cli_renderers() {
        let stored = TagValue::new_rational(9515, 8133);
        assert_eq!(format_tag_value("ExifIFD:ApertureValue", &stored), "1.5");
        assert_eq!(
            format_tag_value_short("ExifIFD:ApertureValue", &stored),
            "1.5"
        );
        assert_eq!(
            tag_value_to_json(Some("ExifIFD:ApertureValue"), &stored),
            serde_json::Value::String("1.5".to_string())
        );
    }

    #[test]
    fn test_format_tag_value_binary() {
        let value = TagValue::new_binary(vec![0x00, 0x01, 0x02, 0x03, 0x04]);
        assert_eq!(
            format_tag_value("EXIF:MakerNote", &value),
            "(Binary data 5 bytes, use -b option to extract)"
        );
    }

    #[test]
    fn test_format_tag_value_orientation_enum() {
        let value = TagValue::new_integer(1);
        assert_eq!(
            format_tag_value("IFD0:Orientation", &value),
            "Horizontal (normal)"
        );
    }

    /// GPS.pm 13.59 tag 0x001b applies `ConvertExifText`, so the EXIF text
    /// header is removed and the method is printed as text, not as a binary
    /// placeholder.
    #[test]
    fn gps_processing_method_is_decoded_in_cli_renderers() {
        let value = TagValue::new_binary(b"ASCII\0\0\0GPS".to_vec());

        assert_eq!(format_tag_value("GPS:GPSProcessingMethod", &value), "GPS");
        assert_eq!(
            format_tag_value_short("GPS:GPSProcessingMethod", &value),
            "GPS"
        );
        assert_eq!(
            tag_value_to_json(Some("GPS:GPSProcessingMethod"), &value),
            serde_json::Value::String("GPS".to_string())
        );
    }

    /// The default renderer used to print RFC 3339
    /// (`2023-12-25T10:30:45+00:00`), a shape ExifTool never produces, while
    /// `-s` and `-j` printed ExifTool's form for the same tag in the same file.
    #[test]
    fn test_format_tag_value_datetime() {
        let dt = Utc.with_ymd_and_hms(2023, 12, 25, 10, 30, 45).unwrap();
        let value = TagValue::new_datetime(dt);
        assert_eq!(
            format_tag_value("EXIF:DateTime", &value),
            "2023:12:25 10:30:45"
        );
    }

    /// All four renderers describe the same value the same way, and word it the
    /// way ExifTool does. `-s` shortens tag *names*, not values, so it has no
    /// reason to word a binary blob differently -- `ExifIFD:FileSource` printed
    /// `(Binary, 1 bytes)` under `-e`/`-j`/`-csv` and `(1 bytes)` under `-s`,
    /// and ExifTool prints neither: see [`binary_placeholder`] for the two
    /// source lines that build `(Binary data N bytes, use -b option to
    /// extract)`.
    #[test]
    fn every_renderer_agrees_on_datetime_and_binary() {
        let dt = Utc.with_ymd_and_hms(2005, 11, 7, 11, 6, 52).unwrap();
        let datetime = TagValue::new_datetime(dt);
        assert_eq!(
            format_tag_value("IFD0:ModifyDate", &datetime),
            format_tag_value_short("IFD0:ModifyDate", &datetime)
        );
        assert_eq!(
            tag_value_to_json(Some("IFD0:ModifyDate"), &datetime),
            serde_json::Value::String("2005:11:07 11:06:52".to_string())
        );

        let binary = TagValue::Binary(vec![3]);
        assert_eq!(
            format_tag_value("ExifIFD:FileSource", &binary),
            format_tag_value_short("ExifIFD:FileSource", &binary)
        );
        assert_eq!(
            tag_value_to_json(Some("ExifIFD:FileSource"), &binary),
            serde_json::Value::String(
                "(Binary data 1 bytes, use -b option to extract)".to_string()
            )
        );
        // `-s` included: its historic wording was `(1 bytes)`.
        assert_eq!(
            format_tag_value_short("ExifIFD:FileSource", &binary),
            "(Binary data 1 bytes, use -b option to extract)"
        );
    }

    /// ExifTool's `exiftool` script emits a bare JSON boolean for a tag whose
    /// text value is literally "true"/"false" (case-insensitive), while `-s`
    /// and the human-readable/CSV renderers still print the plain string.
    #[test]
    fn test_tag_value_to_json_boolean_string() {
        let value = TagValue::new_string("false");
        assert_eq!(
            tag_value_to_json(Some("MPEG:CopyrightFlag"), &value),
            serde_json::Value::Bool(false)
        );
        assert_eq!(format_tag_value("MPEG:CopyrightFlag", &value), "false");

        let value = TagValue::new_string("True");
        assert_eq!(
            tag_value_to_json(Some("MPEG:OriginalMedia"), &value),
            serde_json::Value::Bool(true)
        );

        let value = TagValue::new_string("falsehood");
        assert_eq!(
            tag_value_to_json(Some("EXIF:Make"), &value),
            serde_json::Value::String("falsehood".to_string())
        );
    }

    #[test]
    fn test_format_tag_value_struct() {
        use std::collections::HashMap;
        let mut map = HashMap::new();
        map.insert("key".to_string(), TagValue::new_string("value"));
        let value = TagValue::new_struct(map);
        assert_eq!(
            format_tag_value("XMP-dc:Subject", &value),
            "(Structured data)"
        );
    }

    /// A list tag prints its items joined by `, `, with no brackets.
    ///
    /// `TagValue::Array` had no test at all, which is how a debug rendering of
    /// the enum variant reached real output and stayed there: every list tag in
    /// the corpus printed `[ExifTool, Test, XMP]` against ExifTool's
    /// `ExifTool, Test, XMP` (`ListSep` defaults to `', '`, ExifTool.pm:1173).
    #[test]
    fn list_values_join_without_brackets() {
        let value = TagValue::Array(vec![
            TagValue::new_string("ExifTool"),
            TagValue::new_string("Test"),
            TagValue::new_string("XMP"),
        ]);
        assert_eq!(
            format_tag_value("XMP-dc:Subject", &value),
            "ExifTool, Test, XMP"
        );
        assert_eq!(
            format_tag_value_short("XMP-dc:Subject", &value),
            "ExifTool, Test, XMP"
        );
    }

    /// `-s` shortens tag *names*, never tag *values* -- including long lists.
    ///
    /// The short renderer collapsed any list of more than three items to
    /// `[N items]`, discarding the items entirely. PhotoMechanic.jpg's IPTC
    /// Keywords printed `[4 items]` where ExifTool prints all four.
    #[test]
    fn long_lists_are_not_abbreviated_to_a_count() {
        let value = TagValue::Array(
            ["Keywords", "one", "three", "two"]
                .iter()
                .map(|s| TagValue::new_string(*s))
                .collect(),
        );
        assert_eq!(
            format_tag_value_short("IPTC:Keywords", &value),
            "Keywords, one, three, two"
        );
        assert_eq!(
            format_tag_value("IPTC:Keywords", &value),
            "Keywords, one, three, two"
        );
    }

    /// A single-item list is just the item, and an empty list is empty.
    #[test]
    fn degenerate_lists_render_without_separators() {
        let one = TagValue::Array(vec![TagValue::new_string("solo")]);
        assert_eq!(format_tag_value("XMP-dc:Subject", &one), "solo");
        assert_eq!(format_tag_value_short("XMP-dc:Subject", &one), "solo");

        let none = TagValue::Array(vec![]);
        assert_eq!(format_tag_value("XMP-dc:Subject", &none), "");
        assert_eq!(format_tag_value_short("XMP-dc:Subject", &none), "");
    }

    // CSV Formatter Tests
    #[test]
    fn test_csv_formatter_empty_metadata() {
        let metadata = MetadataMap::new();
        let formatter = CsvFormatter;
        let output = formatter.format(&metadata, None);
        assert_eq!(output, "");
    }

    #[test]
    fn test_csv_formatter_single_tag() {
        let mut metadata = MetadataMap::new();
        metadata.insert("EXIF:Make", TagValue::new_string("Canon"));

        let formatter = CsvFormatter;
        let output = formatter.format(&metadata, None);

        // Verify CSV format
        assert!(output.starts_with("Tag,Value\n"));
        assert!(output.contains("EXIF:Make,Canon"));

        // Verify it's parseable as CSV
        let mut rdr = csv::Reader::from_reader(output.as_bytes());
        let records: Vec<_> = rdr.records().collect();
        assert_eq!(records.len(), 1);
    }

    #[test]
    fn test_csv_formatter_multiple_tags_sorted() {
        let mut metadata = MetadataMap::new();
        metadata.insert("EXIF:Model", TagValue::new_string("EOS 5D"));
        metadata.insert("EXIF:Make", TagValue::new_string("Canon"));
        metadata.insert("EXIF:ISO", TagValue::new_integer(400));

        let formatter = CsvFormatter;
        let output = formatter.format(&metadata, None);

        // Verify header
        assert!(output.starts_with("Tag,Value\n"));

        // Parse CSV to verify structure
        let mut rdr = csv::Reader::from_reader(output.as_bytes());
        let records: Vec<_> = rdr.records().map(|r| r.unwrap()).collect();
        assert_eq!(records.len(), 3);

        // Verify tags are sorted alphabetically
        assert_eq!(records[0].get(0), Some("EXIF:ISO"));
        assert_eq!(records[0].get(1), Some("400"));
        assert_eq!(records[1].get(0), Some("EXIF:Make"));
        assert_eq!(records[1].get(1), Some("Canon"));
        assert_eq!(records[2].get(0), Some("EXIF:Model"));
        assert_eq!(records[2].get(1), Some("EOS 5D"));
    }

    #[test]
    fn test_csv_formatter_resolves_orientation_enum() {
        let mut metadata = MetadataMap::new();
        metadata.insert("IFD0:Orientation", TagValue::new_integer(3));

        let formatter = CsvFormatter;
        let output = formatter.format(&metadata, None);

        assert!(output.contains("IFD0:Orientation,Rotate 180"));
    }

    #[test]
    fn test_csv_formatter_all_value_types() {
        let mut metadata = MetadataMap::new();
        metadata.insert("EXIF:Make", TagValue::new_string("Canon"));
        metadata.insert("EXIF:ISO", TagValue::new_integer(800));
        metadata.insert("EXIF:FNumber", TagValue::new_float(2.8));
        metadata.insert("EXIF:ExposureTime", TagValue::new_rational(1, 100));
        metadata.insert(
            "EXIF:ThumbnailData",
            TagValue::new_binary(vec![0xFF, 0xD8, 0xFF, 0xE0]),
        );

        let dt = Utc.with_ymd_and_hms(2023, 6, 15, 12, 30, 0).unwrap();
        metadata.insert("EXIF:DateTime", TagValue::new_datetime(dt));

        let formatter = CsvFormatter;
        let output = formatter.format(&metadata, None);

        // Verify all types are formatted correctly in CSV
        assert!(output.contains("EXIF:Make,Canon"));
        assert!(output.contains("EXIF:ISO,800"));
        assert!(output.contains("EXIF:FNumber,2.8"));
        assert!(output.contains("EXIF:ExposureTime,1/100"));
        assert!(
            output
                .contains("EXIF:ThumbnailData,\"(Binary data 4 bytes, use -b option to extract)\"")
        );
        // `-csv` shares the default renderer, so it inherited the same RFC 3339
        // divergence from `-s` and `-j` and from ExifTool itself.
        assert!(output.contains("EXIF:DateTime,2023:06:15 12:30:00"));

        // Verify it's valid parseable CSV
        let mut rdr = csv::Reader::from_reader(output.as_bytes());
        let records: Vec<_> = rdr.records().collect();
        assert_eq!(records.len(), 6);
    }

    #[test]
    fn test_csv_formatter_with_filter() {
        let mut metadata = MetadataMap::new();
        metadata.insert("EXIF:Make", TagValue::new_string("Canon"));
        metadata.insert("EXIF:Model", TagValue::new_string("EOS 5D"));
        metadata.insert("EXIF:ISO", TagValue::new_integer(400));

        let formatter = CsvFormatter;
        let filter = vec!["EXIF:Make".to_string(), "EXIF:ISO".to_string()];
        let output = formatter.format(&metadata, Some(&filter));

        // Verify only filtered tags appear
        assert!(output.contains("EXIF:Make,Canon"));
        assert!(output.contains("EXIF:ISO,400"));
        assert!(!output.contains("EXIF:Model"));

        // Verify CSV structure
        let mut rdr = csv::Reader::from_reader(output.as_bytes());
        let records: Vec<_> = rdr.records().collect();
        assert_eq!(records.len(), 2);
    }

    #[test]
    fn test_csv_formatter_filter_nonexistent_tag() {
        let mut metadata = MetadataMap::new();
        metadata.insert("EXIF:Make", TagValue::new_string("Canon"));

        let formatter = CsvFormatter;
        let filter = vec!["EXIF:NonExistent".to_string()];
        let output = formatter.format(&metadata, Some(&filter));

        // No matching tags, should return empty string
        assert_eq!(output, "");
    }

    #[test]
    fn test_csv_formatter_special_characters() {
        let mut metadata = MetadataMap::new();
        // Test comma in value (should be quoted by csv crate)
        metadata.insert("EXIF:Artist", TagValue::new_string("Doe, John"));
        // Test quotes in value (should be escaped)
        metadata.insert("EXIF:Copyright", TagValue::new_string("Copyright \"2023\""));

        let formatter = CsvFormatter;
        let output = formatter.format(&metadata, None);

        // Verify CSV handles special characters correctly
        let mut rdr = csv::Reader::from_reader(output.as_bytes());
        let records: Vec<_> = rdr.records().map(|r| r.unwrap()).collect();
        assert_eq!(records.len(), 2);

        // CSV reader should correctly parse values with commas and quotes
        assert!(records.iter().any(|r| r.get(1) == Some("Doe, John")));
        assert!(
            records
                .iter()
                .any(|r| r.get(1) == Some("Copyright \"2023\""))
        );
    }

    #[test]
    fn test_csv_formatter_valid_csv_structure() {
        let mut metadata = MetadataMap::new();
        metadata.insert("EXIF:Make", TagValue::new_string("Canon"));
        metadata.insert("EXIF:Model", TagValue::new_string("EOS R5"));
        metadata.insert("EXIF:ISO", TagValue::new_integer(800));
        metadata.insert("EXIF:FNumber", TagValue::new_float(2.8));

        let formatter = CsvFormatter;
        let output = formatter.format(&metadata, None);

        // Verify it's parseable by csv crate (same as Excel/pandas would use)
        // Check headers
        let mut rdr = csv::Reader::from_reader(output.as_bytes());
        let headers = rdr.headers().unwrap();
        assert_eq!(headers.len(), 2);
        assert_eq!(headers.get(0), Some("Tag"));
        assert_eq!(headers.get(1), Some("Value"));

        // Check records
        let records: Vec<_> = rdr.records().map(|r| r.unwrap()).collect();
        assert_eq!(records.len(), 4);
    }

    /// Regression: `oxidex -e -s` used to abort the entire invocation with
    /// "byte index 47 is not a char boundary" whenever a tag value ran past 50
    /// bytes and byte 47 landed inside a multi-byte UTF-8 sequence.
    ///
    /// The value below is the real `GPS:GPSProcessingMethod` oxidex extracts
    /// from `Samsung/SamsungL73.jpg`: undecodable bytes lossy-decoded into
    /// 3-byte U+FFFD replacement characters, one of which spans bytes 45..48.
    /// 14 files in the sample corpus produced values of this shape.
    ///
    /// Note this *must* use genuinely multi-byte text -- an ASCII string of the
    /// same length takes the identical code path and never reproduces the bug.
    #[test]
    fn test_short_formatter_does_not_split_multibyte_values() {
        // Verbatim payload of GPS tag 0x001b in Samsung/SamsungL73.jpg, after
        // the 8-byte "JIS     " character-code prefix (`exiftool -v3`).
        let raw: &[u8] = &[
            0x00, 0x00, 0x00, 0x00, 0x32, 0x30, 0x30, 0x37, 0x3a, 0x30, 0x31, 0x3a, 0x30, 0x31,
            0x00, 0x00, 0xd0, 0xff, 0xbd, 0x27, 0x24, 0x00, 0xb5, 0xaf, 0x19, 0x80, 0x15, 0x3c,
            0x18, 0x00, 0xb2, 0xaf, 0x50, 0xd4, 0xa2, 0x8e, 0x2c, 0x00,
        ];
        let gps_processing_method = String::from_utf8_lossy(raw).into_owned();
        // Guard the guard: if this ever stops straddling byte 47 the test has
        // stopped covering the bug it was written for.
        assert!(gps_processing_method.len() > 50);
        assert!(!gps_processing_method.is_char_boundary(47));

        let mut metadata = MetadataMap::new();
        metadata.insert(
            "GPS:GPSProcessingMethod",
            TagValue::new_string(&gps_processing_method),
        );
        // CJK: the corpus carries Japanese/Korean/Chinese values too, and
        // 3-byte CJK characters straddle byte 47 just as readily.
        let cjk = "日本語のテキストです。これは五十バイトを超える長い値です。";
        assert!(cjk.len() > 50);
        assert!(!cjk.is_char_boundary(47));
        metadata.insert("XMP:Description", TagValue::new_string(cjk));

        let output = ShortFormatter.format(&metadata, None);

        // Rendered whole, exactly as `exiftool -s` renders over-long values.
        assert!(output.contains(&format!("GPSProcessingMethod: {}\n", gps_processing_method)));
        assert!(output.contains(&format!("Description: {}\n", cjk)));
        assert!(
            !output.contains("..."),
            "ExifTool's -s shortens tag names, never values: {output:?}"
        );
    }

    /// The same hazard exists on the enum-label branch of the short formatter,
    /// which also used to cut at byte 47.
    #[test]
    fn test_short_formatter_multibyte_value_via_enum_branch() {
        let mut metadata = MetadataMap::new();
        // GPSLatitudeRef resolves through `friendly_enum_name`; a non-ASCII
        // value must fall through it and be emitted untouched, not sliced.
        let value = "北緯です。これは五十バイトを超える非常に長い値になります。";
        assert!(value.len() > 50);
        assert!(!value.is_char_boundary(47));
        metadata.insert("GPS:GPSLatitudeRef", TagValue::new_string(value));

        let output = ShortFormatter.format(&metadata, None);
        assert_eq!(output, format!("GPSLatitudeRef: {}\n", value));
    }

    #[test]
    fn test_short_formatter_resolves_binary_gps_altitude_ref() {
        let mut metadata = MetadataMap::new();
        metadata.insert("GPS:GPSAltitudeRef", TagValue::Binary(vec![1]));

        let output = ShortFormatter.format(&metadata, None);

        assert_eq!(output, "GPSAltitudeRef: Below Sea Level\n");
    }
}
