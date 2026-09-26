//! Command-line argument definitions using lexopt
//!
//! This module defines the CLI argument structure for the oxidex application.

use crate::cli::non_utf8::os_bytes;
use lexopt::prelude::*;
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

// Re-export DetectorMode from parsers module
pub use crate::parsers::DetectorMode;

impl std::str::FromStr for DetectorMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "signature" => Ok(DetectorMode::Signature),
            "magika" => Ok(DetectorMode::Magika),
            _ => Err(format!(
                "Invalid detector mode '{}'. Valid options: signature, magika",
                s
            )),
        }
    }
}

/// A modern, high-performance Rust reimplementation of ExifTool
#[derive(Debug)]
pub struct CliArgs {
    /// File type detection mode (signature or magika)
    pub detector: DetectorMode,

    /// Output in JSON format
    pub json: bool,

    /// Output in CSV format
    pub csv: bool,

    /// ExifTool's short output level (`exiftool`:1243-1244): `0` is the
    /// default layout, `1` (`-s`, `-short`) prints tag names padded to 32
    /// columns, `2` (`-s2`, `-S`, `-veryShort`, `-s -s`) prints unpadded
    /// `Name: value`, and `3` or more (`-s3`, `-s -s -s`, `-s -S`) prints
    /// values only. `-s` adds 1, `-S`/`-veryShort` add 2, and `-sN`/`-shortN`
    /// set the level to N. Rendered by
    /// `cli::tag_resolution::render_short_lines`.
    pub short_level: u8,

    /// Display every retained occurrence of a requested tag, not just the
    /// current priority winner (ExifTool's `-a`). Consumed by
    /// `cli::tag_resolution` wherever a specific `-TAG` is requested;
    /// without a tag filter, full-listing output is unaffected (Step 20's
    /// `OVERHAUL_STEP18_DESIGN.md` scope -- see the module doc there).
    pub all_tags: bool,

    /// Family numbers requested by `-G`/`-Gn`/`-Gn:m` (uppercase only), in
    /// the order given -- `None` when no `-G` flag was seen.
    ///
    /// ExifTool defaults a bare `-G` to family 0 (confirmed against the
    /// pinned oracle, not assumed: `-G -s -Make` on `t/images/ExifTool.jpg`
    /// prints `[MakerNotes]`, family 0, not `[CIFF]`, family 1). Lowercase
    /// `-g` requests ExifTool's *grouped* listing (`---- Group ----`
    /// headers, tags nested underneath) rather than an inline per-tag
    /// prefix; that is a different rendering this step does not implement,
    /// so `-g` is still accepted (as it always was, to avoid the
    /// `-j -G1 -a` regression `is_group_display_flag`'s doc comment
    /// describes) but never populates this field.
    pub group_display: Option<Vec<u8>>,

    /// Step 21 (`--extended-output`): reveals OxiDex's own non-ExifTool
    /// diagnostic/forensic namespace on an unfiltered listing -- the ten
    /// JPEG SOF diagnostic tags (`JPEG:ComponentID_N`, `JPEG:Width`/
    /// `Height`, ...), the undecoded-MakerNote hex-fallback tag
    /// (`ExifIFD:0x927C` and its kin in any IFD), and ZIP's per-entry
    /// forensic tags (`ZIP:File1:CRC32`, ...). None of these are real
    /// ExifTool tags -- default output must match ExifTool's own
    /// (AGENTS.md output-contract/8.4), so they are hidden unless this flag
    /// opts in. See `core::read_options` for the full namespace and the
    /// ExifTool.pm citations this models. `JPEGQualityEstimate` is
    /// unaffected by this flag specifically: it is a real ExifTool tag,
    /// gated on being individually requested (`-JPEGQualityEstimate`)
    /// rather than on this namespace flag -- though `--extended-output`
    /// also reveals it, matching ExifTool's own `RequestAll`-vs-`RequestTags`
    /// shape (`ExifTool.pm:7688-7689`).
    pub extended_output: bool,

    /// Recursive directory processing
    pub recursive: bool,

    /// Preserve original file modification time after writing metadata.
    /// When this flag is set, the file's modification timestamp (mtime) will be
    /// restored to its original value after metadata changes are written.
    pub preserve_file_times: bool,

    /// Create a backup copy of the file before modifying it.
    /// The backup file will have the same name with a .bak extension appended.
    /// For example: photo.jpg -> photo.jpg.bak
    pub backup: bool,

    /// Enable read-only mode to prevent any file modifications.
    /// When this flag is set, the tool will refuse to write any changes and
    /// return an error if write operations are attempted. Use this as a safety
    /// measure to prevent accidental modifications.
    pub readonly: bool,

    /// Apply ExifTool's PrintConv layer to tag values. **Defaults to `true`.**
    ///
    /// When enabled, tag values are formatted to match ExifTool's output format,
    /// including enum descriptions, unit suffixes, and precision formatting.
    ///
    /// This was opt-in (`-e`) until it was found to be the single largest source
    /// of wrong values in the product: ExifTool applies PrintConv by *default*
    /// and takes `-n` to turn it off, so an OxiDex that defaulted to raw values
    /// disagreed with it on 401 tags across the 207-file pinned corpus while
    /// every one of those conversions was already implemented and tested in
    /// `core::exiftool_compat`. The wrong ones were not obviously wrong, either
    /// -- `Flash: 24`, `FNumber: 8`, `FocalLength: 55 mm` all read like answers.
    ///
    /// `--no-print-conv` restores the raw form. It is spelled long because
    /// OxiDex's `-n` already means dry-run; ExifTool's `-n` is unavailable here.
    pub exiftool_compat: bool,

    /// Copy metadata from source file (ExifTool -TagsFromFile syntax).
    /// Use with optional tag names to copy specific tags, or without to copy all tags.
    /// Example: oxidex -TagsFromFile src.jpg dest.jpg (copy all)
    /// Example: oxidex -TagsFromFile src.jpg -EXIF:Artist -EXIF:Copyright dest.jpg
    ///
    /// A path, so kept as the bytes it was given: it need not be UTF-8.
    pub tags_from_file: Option<OsString>,

    /// Date format string for DateTime tags in filename patterns (using chrono format).
    /// Example: -d %Y%m%d_%H%M%S
    /// Common specifiers: %Y (year), %m (month), %d (day), %H (hour), %M (minute), %S (second)
    pub date_format: Option<String>,

    /// Dry-run mode: show proposed renames without executing.
    /// Prints "old_name -> new_name" for each file without actually renaming.
    pub dry_run: bool,

    /// Fail a read instead of degrading it.
    ///
    /// By default a read that hits a recoverable problem -- a truncated
    /// JPEG, a format neither a parser nor the identification tables can
    /// name -- still returns whatever it could get (filesystem tags at a
    /// minimum) tagged with a non-`Parsed` `Status`, the same way ExifTool
    /// itself keeps going and reports a `Warning` tag rather than raising
    /// an exception (`ExifTool.pm:8483`). `--strict` opts back into the
    /// older fail-fast behavior: a read that would come back as anything
    /// other than `Status: Parsed` or `Status: IdentifiedOnly` exits with
    /// an error instead.
    pub strict: bool,

    /// Tag modifications and file path. Use -TAG=VALUE to modify tags.
    /// Example: -EXIF:Artist="John Doe" -EXIF:Copyright=2025 photo.jpg
    /// The last argument must be the file path.
    ///
    /// Kept as `OsString`s because a path is bytes, not text. `parse`
    /// guarantees that an entry which is not valid UTF-8 is either a plain
    /// positional path (it does not start with `-`) or a `-TAG=VALUE`
    /// modification whose `TAG` is valid UTF-8 and whose value is not; every
    /// other non-UTF-8 argument is refused with an error there. The
    /// accessors below rely on that: they read options and tag names as
    /// UTF-8 and never see anything else.
    pub args: Vec<OsString>,

    /// Every argument after `--`, in order. These are always file paths,
    /// however they are spelled: pinned 13.59 `exiftool -s2 -Make -- a.jpg -s`
    /// reads both `a.jpg` and a file named `-s`. They are kept apart from
    /// `args`, whose accessors classify by spelling (a leading `-` means an
    /// option or tag) and by position (the last argument is the file).
    pub literal_paths: Vec<OsString>,
}

fn normalize_exiftool_option(arg: String) -> String {
    if let Some(value) = arg.strip_prefix("-TagsFromFile=") {
        return format!("--TagsFromFile={value}");
    }

    match arg.as_str() {
        "-json" => "--json".to_string(),
        "-csv" => "--csv".to_string(),
        "-preserve-file-times" => "--preserve-file-times".to_string(),
        "-backup" => "--backup".to_string(),
        "-readonly" => "--readonly".to_string(),
        "-exiftool-compat" => "--exiftool-compat".to_string(),
        "-no-print-conv" => "--no-print-conv".to_string(),
        "-TagsFromFile" => "--TagsFromFile".to_string(),
        _ => arg,
    }
}

/// Where an argument that is not valid UTF-8 goes.
#[derive(Debug)]
enum NonUtf8Arg {
    /// A `-TAG=VALUE` whose tag name is UTF-8 and whose value oxidex can
    /// write (`cli::non_utf8::tag_value`).
    Modification(OsString),
    /// A path, positional or `--TagsFromFile=PATH`, or the next argument's
    /// value for lexopt.
    Lexopt(OsString),
}

/// Classifies an argument that is not valid UTF-8, or refuses it.
///
/// A path is bytes, so a positional argument (no leading `-`) passes through
/// untouched, as does the path in `-TagsFromFile=PATH`. A `-TAG=VALUE` is
/// accepted when `TAG` is UTF-8 and the value is one oxidex can write as
/// pinned ExifTool 13.59 does; its conversion is checked here, before any
/// file is touched. Everything else -- an option or tag name that is not
/// UTF-8, or a value oxidex would have to approximate -- is an error. The
/// oracle refuses such names too (`Invalid tag name 'Art\xffist'`,
/// `Unknown option -\xff`).
fn non_utf8_arg(arg: OsString) -> Result<NonUtf8Arg, lexopt::Error> {
    let bytes = os_bytes(&arg);
    if bytes.starts_with(b"--TagsFromFile=") {
        return Ok(NonUtf8Arg::Lexopt(arg));
    }
    if bytes.starts_with(b"-TagsFromFile=") {
        // `normalize_exiftool_option`'s rewrite, byte-wise.
        let mut long = OsString::from("-");
        long.push(&arg);
        return Ok(NonUtf8Arg::Lexopt(long));
    }
    if !bytes.starts_with(b"-") {
        return Ok(NonUtf8Arg::Lexopt(arg));
    }
    let named = !bytes.starts_with(b"--")
        && crate::cli::non_utf8::split_at_equals(&arg)
            .is_some_and(|(name, _)| name.to_str().is_some());
    if !named {
        return Err(format!(
            "invalid option or tag name {arg:?}: option and tag names must be valid UTF-8"
        )
        .into());
    }
    let Some((tag_name, value)) = CliArgs::parse_modification(&arg) else {
        return Err(format!("invalid tag name in {arg:?}").into());
    };
    crate::cli::non_utf8::tag_value(&tag_name, os_bytes(&value))
        .map_err(|error| lexopt::Error::Custom(Box::new(error)))?;
    Ok(NonUtf8Arg::Modification(arg))
}

/// The value of an option that takes text (`-d`, `--detector`), refused
/// with a clear error when it is not UTF-8.
fn text_option_value(option: &str, value: OsString) -> Result<String, lexopt::Error> {
    value
        .into_string()
        .map_err(|value| format!("the value of {option} must be valid UTF-8, got {value:?}").into())
}

/// One of ExifTool's short-output option spellings, as the `exiftool` script
/// itself parses them (13.59, `exiftool`:1243-1244):
///
/// ```perl
/// (/^S$/ or $a eq 'veryshort') and $outFormat+=2, next;
/// /^s(hort)?(\d*)$/i and $outFormat = $2 eq '' ? $outFormat + 1 : $2, next;
/// ```
///
/// `$a` is the lower-cased option, so `-veryShort`, `-SHORT` and `-S2` are
/// all accepted; only the bare uppercase `-S` means "add 2".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShortLevelOption {
    Add(u8),
    Set(u8),
}

impl ShortLevelOption {
    fn apply(self, level: u8) -> u8 {
        match self {
            ShortLevelOption::Add(n) => level.saturating_add(n),
            ShortLevelOption::Set(n) => n,
        }
    }
}

fn parse_short_level_option(arg: &str) -> Option<ShortLevelOption> {
    let body = arg.strip_prefix('-')?;
    if body == "S" || body.eq_ignore_ascii_case("veryshort") {
        return Some(ShortLevelOption::Add(2));
    }
    let lower = body.to_ascii_lowercase();
    let digits = lower
        .strip_prefix("short")
        .or_else(|| lower.strip_prefix('s'))?;
    if digits.is_empty() {
        return Some(ShortLevelOption::Add(1));
    }
    if !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    // Perl keeps the number as given; any level of 3 or more renders the
    // same values-only layout, so saturating an absurd one is exact.
    Some(ShortLevelOption::Set(digits.parse().unwrap_or(u8::MAX)))
}

fn is_flag_short_option(ch: char) -> bool {
    matches!(ch, 'h' | 'V' | 'j' | 's' | 'a' | 'r' | 'e' | 'n')
}

/// Matches ExifTool's group-display flags: `-G`, `-g`, optionally followed by
/// digits and/or colon-separated family numbers (`-G1`, `-g0`, `-G1:2`). Real
/// tag names starting with 'G' (e.g. `-GPSLatitude`) contain letters after
/// the 'G' and so never match.
fn is_group_display_flag(arg: &str) -> bool {
    let Some(body) = arg.strip_prefix('-') else {
        return false;
    };
    let Some(rest) = body.strip_prefix('G').or_else(|| body.strip_prefix('g')) else {
        return false;
    };
    rest.chars().all(|c| c.is_ascii_digit() || c == ':')
}

/// Parses an uppercase `-G`/`-Gn`/`-Gn:m:...` flag into the family numbers
/// requested, in order. Returns `None` for a lowercase `-g` flag (or
/// anything else `is_group_display_flag` matched but this function does not
/// handle) -- see [`CliArgs::group_display`]'s doc comment for why.
fn parse_group_display_families(arg: &str) -> Option<Vec<u8>> {
    let body = arg.strip_prefix('-')?;
    let rest = body.strip_prefix('G')?;
    if rest.is_empty() {
        // ExifTool's own default (confirmed against the pinned oracle, see
        // `CliArgs::group_display`'s doc comment): family 0.
        return Some(vec![0]);
    }
    let families: Option<Vec<u8>> = rest.split(':').map(|part| part.parse().ok()).collect();
    families.filter(|families| !families.is_empty())
}

fn is_lexopt_short_arg(arg: &str) -> bool {
    let Some(body) = arg.strip_prefix('-') else {
        return false;
    };
    if body.is_empty() || body.starts_with('-') {
        return false;
    }
    // Assignment-shaped args are tag modifications (e.g. -description=x), never
    // short-option clusters, even when they start with a valid cluster prefix.
    if body.contains('=') {
        return false;
    }

    for (index, ch) in body.char_indices() {
        if ch == 'd' {
            // Everything before 'd' was already confirmed to be a flag option.
            // 'd' is the date-format option only when it ends the cluster
            // (`-d`, `-srd`; format is the next argument) or is immediately
            // followed by a strftime directive (`-d%Y%m%d`). Otherwise the arg
            // is a tag name that merely starts with cluster letters
            // (`-description`, `-date`, `-shadows`) and must not be swallowed.
            let remainder = &body[index + 1..];
            return remainder.is_empty() || remainder.starts_with('%');
        }
        if !is_flag_short_option(ch) {
            return false;
        }
    }

    true
}

fn lexopt_arg_requires_next_value(arg: &str) -> bool {
    if arg == "--TagsFromFile" {
        return true;
    }

    let Some(body) = arg.strip_prefix('-') else {
        return false;
    };
    !body.starts_with('-')
        && body.ends_with('d')
        && body[..body.len() - 1].chars().all(is_flag_short_option)
}

impl CliArgs {
    /// Parse command-line arguments from the environment.
    ///
    /// This method uses lexopt to parse arguments in a way that's compatible with
    /// the original ExifTool syntax, including support for:
    /// - Single-dash long options (e.g., `-json` alongside `--json`)
    /// - Tag modification syntax (e.g., `-TAG=VALUE`)
    /// - Trailing variable arguments for files and tag modifications
    ///
    /// # Returns
    ///
    /// Returns `Ok(CliArgs)` if parsing succeeds, or `Err` if invalid arguments are provided.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - An unknown option is encountered that doesn't look like a tag modification
    /// - A required value for an option is missing
    /// - Help (`--help`, `-h`) or version (`--version`, `-V`) is requested (exits immediately)
    pub fn parse() -> Result<Self, lexopt::Error> {
        // `args_os`, never `args`: `std::env::args()` panics on the first
        // argument that is not valid UTF-8, and a path or a tag value is
        // bytes (pinned ExifTool 13.59 reads `n\xff.jpg` and writes
        // `-XPTitle=A\xed\xa0\x80B`).
        Self::parse_from(std::env::args_os().skip(1))
    }

    /// [`CliArgs::parse`] over an explicit argument list (without the program
    /// name).
    ///
    /// # Errors
    ///
    /// As [`CliArgs::parse`], and also for an argument that is not valid
    /// UTF-8 where only text can appear: an option or tag name, a
    /// `-TAG=VALUE` whose value oxidex cannot write as ExifTool would (see
    /// `cli::non_utf8`), or the value of `-d`/`--detector`. A path may be any
    /// bytes.
    pub fn parse_from<I>(raw_args: I) -> Result<Self, lexopt::Error>
    where
        I: IntoIterator<Item = OsString>,
    {
        // Initialize with default values
        let mut detector = DetectorMode::default();
        let mut json = false;
        let mut csv = false;
        let mut short_level: u8 = 0;
        let mut all_tags = false;
        let mut group_display: Option<Vec<u8>> = None;
        let mut extended_output = false;
        let mut recursive = false;
        let mut preserve_file_times = false;
        let mut backup = false;
        let mut readonly = false;
        // ExifTool's default, and now OxiDex's. See the field's own docs.
        let mut exiftool_compat = true;
        let mut tags_from_file = None;
        let mut date_format = None;
        let mut dry_run = false;
        let mut strict = false;
        let mut args: Vec<OsString> = Vec::new();

        // Pre-process arguments to handle tag modifications that look like flags
        // e.g., "-EXIF:Artist=value" starts with '-' but isn't a regular flag
        let mut lexopt_args: Vec<OsString> = Vec::new();
        let mut tag_modifications: Vec<OsString> = Vec::new();
        let mut next_arg_is_lexopt_value = false;

        let mut options_ended = false;
        let mut literal_paths = Vec::new();

        for raw_arg in raw_args {
            if next_arg_is_lexopt_value {
                lexopt_args.push(raw_arg);
                next_arg_is_lexopt_value = false;
                continue;
            }

            // `--` ends option recognition: everything after it is a path,
            // however it is spelled (pinned 13.59 `exiftool -Make -- a.jpg -s`
            // reads `a.jpg` and a file named `-s`). They go to
            // `literal_paths`, never through the option/tag recognition below
            // or into `args`, where a leading `-` would read as a tag.
            if options_ended {
                literal_paths.push(raw_arg);
                continue;
            }
            if raw_arg == "--" {
                options_ended = true;
                continue;
            }

            // An argument that is not valid UTF-8 can only be a path or the
            // value of a `-TAG=VALUE`; see `non_utf8_arg`.
            let raw_arg = match raw_arg.into_string() {
                Ok(arg) => arg,
                Err(raw_arg) => {
                    match non_utf8_arg(raw_arg)? {
                        NonUtf8Arg::Modification(arg) => tag_modifications.push(arg),
                        NonUtf8Arg::Lexopt(arg) => lexopt_args.push(arg),
                    }
                    continue;
                }
            };

            let arg = normalize_exiftool_option(raw_arg);

            // ExifTool's short-output levels. Handled here, in argument
            // order, because `-s2`/`-s3`/`-S`/`-veryShort`/`-short3` are not
            // lexopt clusters and used to fall through to the specific-tag
            // branch below as requests for tags literally named `s3`, `S`,
            // ... -- `-s3 -Make` printed the full `Group:Tag: value` line
            // instead of the value alone. Every spelling that sets the level
            // is handled here, in this one ordered pass, so `-s -S` and
            // `-S -s` both reach level 3 and a later `-s1` always wins.
            if let Some(option) = parse_short_level_option(&arg) {
                short_level = option.apply(short_level);
                continue;
            }

            // ExifTool's group-display flags (-G, -G0..-G8, -g, -g0..-g8, and
            // colon-separated family lists like -G1:2) must not fall through
            // to the tag-modification/specific-tag branch below, where an
            // unrecognized "-G1" was treated as a request for a tag
            // literally named "G1" -- matching nothing and silently
            // emptying the entire tag set (`-j -G1 -a` returned `[{}]`
            // instead of the full extraction). Step 20 stops discarding the
            // family list outright (`-G*` was a pure no-op before this):
            // `-G`/`-Gn`/`-Gn:m` now populate `group_display`, consumed by
            // `cli::tag_resolution` wherever a specific `-TAG` is requested.
            // `-g` continues to be swallowed without effect -- see
            // `CliArgs::group_display`'s doc comment for why.
            if is_group_display_flag(&arg) {
                if let Some(families) = parse_group_display_families(&arg) {
                    group_display = Some(families);
                }
                continue;
            }

            // Keep tag modifications, date shifts, and specific tag extraction out of lexopt.
            // Supported short options and clusters still flow through lexopt.
            if arg.starts_with('-')
                && !arg.starts_with("--")
                && !is_lexopt_short_arg(&arg)
                && (arg.contains('=')
                    || arg.ends_with("+=")
                    || arg.ends_with("-=")
                    || arg.contains(':')
                    || arg.len() > 1)
            {
                // This is a tag modification, date shift, or specific tag - don't pass to lexopt
                tag_modifications.push(arg.into());
            } else {
                next_arg_is_lexopt_value = lexopt_arg_requires_next_value(&arg);
                // Regular argument - pass to lexopt
                lexopt_args.push(arg.into());
            }
        }

        // Create parser from filtered arguments
        let mut parser = lexopt::Parser::from_args(lexopt_args);

        // Add pre-identified tag modifications to args list
        args.extend(tag_modifications);

        // Process each argument
        loop {
            // Handle parser.next() errors specially for tag modifications
            let arg = match parser.next() {
                Ok(Some(arg)) => arg,
                Ok(None) => break, // No more arguments
                Err(e) => {
                    // Handle lexopt errors - these might be tag modifications or date shifts
                    // that lexopt tries to parse as flags
                    let error_msg = e.to_string();
                    if let Some(arg_str) = extract_arg_from_error(&error_msg) {
                        args.push(arg_str.into());
                    } else {
                        // If we can't extract the argument, return the error
                        return Err(e);
                    }
                    // Collect remaining arguments. They are kept as given:
                    // a path need not be UTF-8, and dropping one that is
                    // not would silently process a different file list.
                    if let Ok(raw) = parser.raw_args() {
                        args.extend(raw);
                    }
                    // (raw_args() can fail, but we already collected the
                    // main arg, so we can continue.)
                    break;
                }
            };

            match arg {
                // Help flag
                Short('h') | Long("help") => {
                    print_help();
                    std::process::exit(0);
                }
                // Version flag
                Short('V') | Long("version") => {
                    print_version();
                    std::process::exit(0);
                }
                // JSON output
                Short('j') | Long("json") => {
                    json = true;
                }
                // CSV output
                Long("csv") => {
                    csv = true;
                }
                // An `s` inside an option cluster (`-sa`, `-ss`, `-sr`) never
                // changes the short level. ExifTool has no single-letter
                // clustering: pinned 13.59 reads `-sa`/`-ss`/`-sS` as unknown
                // tag names, so `-sa -s1` is level 1 and `-s2 -ss` level 2.
                // Counting it here, after pre-processing had already applied
                // every standalone spelling, also replayed it out of order.
                // The cluster's other letters keep their OxiDex meaning.
                Short('s') => {}
                // All tags
                Short('a') => {
                    all_tags = true;
                }
                // Recursive
                Short('r') => {
                    recursive = true;
                }
                // Preserve file times
                Long("preserve-file-times") => {
                    preserve_file_times = true;
                }
                // Backup
                Long("backup") => {
                    backup = true;
                }
                // Readonly
                Long("readonly") => {
                    readonly = true;
                }
                // Strict: fail a degraded read instead of returning partial
                // output. See the field doc on `CliArgs::strict`.
                Long("strict") => {
                    strict = true;
                }
                // Step 21: reveal OxiDex's own diagnostic/forensic
                // namespace. See the field doc on `CliArgs::extended_output`.
                Long("extended-output") => {
                    extended_output = true;
                }
                // ExifTool compatibility mode. Now the default, so this is a
                // no-op; it stays accepted because scripts pass it, and because
                // silently rejecting a flag that used to matter is worse than
                // honouring one that no longer needs to.
                Short('e') | Long("exiftool-compat") => {
                    exiftool_compat = true;
                }
                // ExifTool's `-n`, which OxiDex cannot spell that way: `-n` is
                // already dry-run here.
                Long("no-print-conv") => {
                    exiftool_compat = false;
                }
                // TagsFromFile (copy metadata from source file). A path: any
                // bytes.
                Long("TagsFromFile") => {
                    tags_from_file = Some(parser.value()?);
                }
                // Date format
                Short('d') => {
                    date_format = Some(text_option_value("-d", parser.value()?)?);
                }
                // Dry-run
                Short('n') => {
                    dry_run = true;
                }
                // Detector mode (signature or magika)
                Long("detector") => {
                    let value_str = text_option_value("--detector", parser.value()?)?;
                    detector = value_str.parse().map_err(|e| {
                        lexopt::Error::Custom(Box::new(std::io::Error::new(
                            std::io::ErrorKind::InvalidInput,
                            e,
                        )))
                    })?;
                }
                // Value argument (file path or positional argument): kept as
                // given, since a path need not be UTF-8.
                Value(val) => {
                    args.push(val);
                }
                // Unknown short or long option
                // This could be a tag modification like -EXIF:Artist=value
                // or a date shift operation like -AllDates+=1:0:0
                // Collect it as a trailing argument by accessing the raw value
                Short(_) | Long(_) => {
                    // Get the raw argument by using parser.raw_args()
                    // Since we can't go back, we need to handle this differently
                    // We'll use the unexpected error to extract the option

                    // For unknown options, we want to collect them as tag modifications
                    // This is a bit tricky with lexopt, so we need to handle it specially
                    // The arg.unexpected() will give us an error, but we want to collect
                    // the raw string instead

                    // Unfortunately, lexopt doesn't give us direct access to the raw string
                    // in the error case, so we need a different approach
                    // We'll collect remaining arguments using raw_args()

                    // Collect all remaining raw arguments (including this one)
                    // First, we need to reconstruct the current argument
                    let current_arg = format!("{}", arg.unexpected());

                    // Extract the actual argument from the error message
                    // Error format is typically "unexpected argument '--option'"
                    // or "unexpected option '-o'"
                    if let Some(arg_str) = extract_arg_from_error(&current_arg) {
                        args.push(arg_str.into());
                    }

                    // Collect all remaining arguments, as given (paths need
                    // not be UTF-8).
                    args.extend(parser.raw_args()?);

                    // Break out of the loop since we've consumed all arguments
                    break;
                }
            }
        }

        Ok(CliArgs {
            detector,
            json,
            csv,
            short_level,
            all_tags,
            group_display,
            extended_output,
            recursive,
            preserve_file_times,
            backup,
            readonly,
            exiftool_compat,
            tags_from_file,
            date_format,
            dry_run,
            strict,
            args,
            literal_paths,
        })
    }

    /// The arguments that can be options, tags or modifications: all of
    /// `args` when `--` supplied the paths, otherwise every argument but the
    /// last, which is the file.
    fn option_args(&self) -> &[OsString] {
        if !self.literal_paths.is_empty() {
            &self.args
        } else if self.args.is_empty() {
            &[]
        } else {
            &self.args[..self.args.len() - 1]
        }
    }

    /// Extracts the file path from the arguments: the last path given after
    /// `--`, otherwise the last argument.
    pub fn file(&self) -> Option<PathBuf> {
        self.literal_paths
            .last()
            .or(self.args.last())
            .map(PathBuf::from)
    }

    /// Extracts every file/directory path from the arguments, preserving
    /// input order.
    ///
    /// Every other accessor on this struct (`tag_modifications`,
    /// `date_shift_operations`, `specific_tags`, ...) already treats any
    /// argument starting with '-' as an option rather than a path, so the
    /// same rule is used here: a plain positional argument is a path,
    /// everything else is not. This is what lets `oxidex -j a.jpg b.jpg`
    /// see both files instead of only the last positional argument.
    ///
    /// Falls back to `file()` when the filter finds nothing, so an edge
    /// case like a single dash-prefixed filename still resolves the same
    /// way it did before this method existed.
    ///
    /// Every argument after `--` is a path regardless of spelling, and comes
    /// after the plain positional ones -- which is also its command-line
    /// order, since `--` ends the options.
    pub fn files(&self) -> Vec<PathBuf> {
        let files: Vec<PathBuf> = self
            .args
            .iter()
            .filter(|arg| !os_bytes(arg).starts_with(b"-"))
            .chain(self.literal_paths.iter())
            .map(PathBuf::from)
            .collect();

        if files.is_empty() {
            self.file().into_iter().collect()
        } else {
            files
        }
    }

    /// Parses tag modification arguments (all args except the last one)
    /// Returns a vector of (tag_name, value) tuples.
    ///
    /// The value is an `OsString` because it need not be UTF-8: `parse`
    /// accepted it only if `cli::non_utf8::tag_value` can write it (an XP
    /// string), and `value_parser::parse_cli_tag_value_os` converts it.
    pub fn tag_modifications(&self) -> Vec<(String, OsString)> {
        let mut modifications = Vec::new();
        // Process every option argument (never the file, see `option_args`)
        for arg in self.option_args() {
            if let Some((tag, value)) = Self::parse_modification(arg) {
                modifications.push((tag, value));
            }
        }
        modifications
    }

    /// Parses a single modification argument in the form -TAG=VALUE. A
    /// value that is not UTF-8 goes through the same steps byte-wise
    /// (`cli::non_utf8::unquote`); the tag name always is UTF-8 (`parse`).
    fn parse_modification(arg: &OsStr) -> Option<(String, OsString)> {
        let Some(arg) = arg.to_str() else {
            let (name, value) = crate::cli::non_utf8::split_at_equals(arg)?;
            let name = name.to_str()?;
            if !name.starts_with('-') {
                return None;
            }
            let tag_name = name.trim_start_matches('-').trim();
            if tag_name.is_empty() {
                return None;
            }
            return Some((
                tag_name.to_string(),
                crate::cli::non_utf8::unquote(value).to_os_string(),
            ));
        };
        Self::parse_text_modification(arg).map(|(tag, value)| (tag, value.into()))
    }

    /// [`CliArgs::parse_modification`] for an argument that is valid UTF-8.
    fn parse_text_modification(arg: &str) -> Option<(String, String)> {
        // Check if it starts with '-' and contains '='
        if !arg.starts_with('-') || !arg.contains('=') {
            return None;
        }

        // Split on first '=' to handle values that contain '='
        let parts: Vec<&str> = arg.splitn(2, '=').collect();
        if parts.len() != 2 {
            return None;
        }

        // Extract tag name (remove leading '-')
        let tag_name = parts[0].trim_start_matches('-').trim();

        // An empty tag name is not a modification, and treating it as one was
        // worse than the parse failure it looks like: `oxidex '-=' photo.jpg`
        // and `oxidex '-  =  ' photo.jpg` produced a modification of tag ""
        // with value "", which the writer accepted -- reporting "1 image files
        // updated" and rewriting the file. ExifTool refuses ("Unknown option
        // -=", "Invalid TAG name") and leaves the file untouched.
        if tag_name.is_empty() {
            return None;
        }

        // Extract value and remove surrounding quotes if present
        let value = Self::unquote(parts[1]);

        Some((tag_name.to_string(), value))
    }

    /// Removes surrounding quotes from a string if present
    fn unquote(s: &str) -> String {
        let trimmed = s.trim();
        // A lone quote satisfies both `starts_with` and `ends_with` at once, so
        // without the length check `oxidex '-EXIF:Artist="' file.jpg` sliced
        // `[1..0]` and aborted the whole invocation with "byte range starts at
        // 1 but ends at 0". Two quotes are needed before there is a pair to
        // strip. (The indices themselves are char-boundary safe: both ends are
        // pinned to a single-byte ASCII quote.)
        if trimmed.len() >= 2
            && ((trimmed.starts_with('"') && trimmed.ends_with('"'))
                || (trimmed.starts_with('\'') && trimmed.ends_with('\'')))
        {
            trimmed[1..trimmed.len() - 1].to_string()
        } else {
            s.to_string()
        }
    }

    /// Extracts tag names to copy when using -TagsFromFile.
    /// Returns None if -TagsFromFile is not set.
    /// Returns Some(Vec) of tag names if tags are specified (args starting with '-' but not '=').
    /// Returns Some(empty Vec) if no specific tags are specified (copy all).
    pub fn copy_tag_filters(&self) -> Option<Vec<String>> {
        // If -TagsFromFile is not set, return None
        self.tags_from_file.as_ref()?;

        let mut tag_names = Vec::new();

        // Process every option argument (never the destination file). One
        // that is not UTF-8 is a path or a modification (`parse`), neither
        // of which is a tag name to copy.
        for arg in self.option_args().iter().filter_map(|arg| arg.to_str()) {
            // Check if it's a tag name (starts with '-' but does NOT contain '=')
            if arg.starts_with('-') && !arg.contains('=') {
                // Extract tag name (remove leading '-')
                let tag_name = arg.trim_start_matches('-').to_string();
                tag_names.push(tag_name);
            }
        }

        // Return empty vec if no tags specified (means copy all)
        // Return vec with tag names if specific tags were specified
        Some(tag_names)
    }

    /// Extracts specific tag names to display when reading metadata.
    /// Returns None if no specific tags are requested (show all tags).
    /// Returns Some(Vec) of tag names if specific tags are requested.
    ///
    /// This enables `-TAG` syntax for filtering output:
    /// - `oxidex -Make photo.jpg` → shows only Make tag
    /// - `oxidex -Make -Model photo.jpg` → shows Make and Model tags
    ///
    /// # Examples
    ///
    /// ```
    /// # use oxidex::cli::args::CliArgs;
    /// // If args are: ["-Make", "-Model", "photo.jpg"]
    /// // Returns: Some(vec!["Make", "Model"])
    /// ```
    pub fn specific_tags(&self) -> Option<Vec<String>> {
        // Don't apply in copy mode (tags_from_file handles its own filtering)
        if self.tags_from_file.is_some() {
            return None;
        }

        // Don't apply in write mode (has tag modifications with '=')
        let has_modifications = self.args.iter().any(|arg| os_bytes(arg).contains(&b'='));
        if has_modifications {
            return None;
        }

        let mut tag_names = Vec::new();

        // Process every option argument (never a file path, see `option_args`).
        // One that is not UTF-8 is a path or a modification (`parse`).
        for arg in self.option_args().iter().filter_map(|arg| arg.to_str()) {
            // Tag extraction: starts with '-', does NOT contain '='
            if arg.starts_with('-') && !arg.contains('=') {
                let tag_name = arg.trim_start_matches('-').to_string();
                tag_names.push(tag_name);
            }
        }

        if tag_names.is_empty() {
            None
        } else {
            Some(tag_names)
        }
    }

    /// Checks if the user wants to clear all metadata (`-all=` syntax).
    ///
    /// This implements the ExifTool `-all=` command for removing all metadata
    /// from a file for privacy purposes.
    ///
    /// # Examples
    ///
    /// - `oxidex -all= photo.jpg` → clears all metadata
    /// - `oxidex -ALL= photo.jpg` → clears all metadata (case-insensitive)
    pub fn is_clear_all_metadata(&self) -> bool {
        self.args.iter().filter_map(|arg| arg.to_str()).any(|arg| {
            let lower = arg.to_lowercase();
            lower == "-all=" || lower == "--all="
        })
    }

    /// Returns whether ExifTool's PrintConv layer is applied. Default: `true`.
    ///
    /// When enabled, tag values are formatted to match ExifTool's output format,
    /// including enum descriptions, unit suffixes, and precision formatting.
    ///
    /// # Examples
    ///
    /// - `oxidex photo.jpg` → ExifTool-compatible output (the default)
    /// - `oxidex -e photo.jpg` → same; `-e` is retained but no longer needed
    /// - `oxidex --no-print-conv photo.jpg` → raw stored values
    pub fn exiftool_compat(&self) -> bool {
        self.exiftool_compat
    }

    /// Extracts the filename pattern from -FileName<pattern> argument.
    /// Returns None if no -FileName argument is found.
    /// Returns Some(pattern) with the pattern after the '<' character.
    ///
    /// Example: '-FileName<DateTimeOriginal' -> Some("DateTimeOriginal")
    /// Example: '-FileName<${EXIF:Make}_${EXIF:Model}' -> Some("${EXIF:Make}_${EXIF:Model}")
    pub fn filename_pattern(&self) -> Option<String> {
        for arg in self.args.iter().filter_map(|arg| arg.to_str()) {
            // Check if this is a -FileName argument
            if arg.starts_with("-FileName") || arg.starts_with("'FileName") {
                // Find the '<' character that separates -FileName from the pattern
                if let Some(pos) = arg.find('<') {
                    // Extract everything after '<'
                    let pattern = &arg[pos + 1..];
                    // Remove trailing quote if present (from '-FileName<pattern')
                    let pattern = pattern.trim_end_matches('\'');
                    return Some(pattern.to_string());
                }
            }
        }
        None
    }

    /// Parses date shift arguments (e.g., "-AllDates+=1:0:0 0:0:0" or "-EXIF:DateTime-=0:1:0 0:0:0")
    /// Returns a vector of (tag_pattern, operation, offset_or_value) tuples
    ///
    /// # Format
    ///
    /// Date shift arguments follow the format: `-TAG_PATTERN{+= | -= | =}OFFSET`
    /// - TAG_PATTERN: "AllDates" or specific tag name (e.g., "EXIF:DateTime")
    /// - Operation: `+=` (add), `-=` (subtract), `=` (set absolute)
    /// - OFFSET: For += and -=: "Y:M:D H:M:S" format
    ///   For =: "YYYY:MM:DD HH:MM:SS" absolute datetime format
    ///
    /// # Examples
    ///
    /// - `-AllDates+=1:0:0 0:0:0` -> Add 1 year to all date tags
    /// - `-EXIF:DateTime-=0:1:0 0:0:0` -> Subtract 1 month from DateTime
    /// - `-EXIF:DateTime=2025:01:15 10:30:00` -> Set DateTime to specific value
    pub fn date_shift_operations(&self) -> Vec<(String, String, String)> {
        let mut operations = Vec::new();

        // Process every option argument (never the file, see `option_args`).
        // A non-UTF-8 value is never a date (`parse` refuses one for every
        // tag but the XP strings).
        for arg in self.option_args().iter().filter_map(|arg| arg.to_str()) {
            if let Some((tag, op, value)) = Self::parse_date_shift(arg) {
                operations.push((tag, op, value));
            }
        }

        operations
    }

    /// Parses a single date shift argument
    /// Returns (tag_pattern, operation, offset_or_value) or None if not a date shift argument
    ///
    /// Supports three operation types:
    /// - `+=`: Add offset (e.g., "-AllDates+=1:0:0 0:0:0")
    /// - `-=`: Subtract offset (e.g., "-EXIF:DateTime-=0:1:0 0:0:0")
    /// - `=`: Set absolute (e.g., "-EXIF:DateTime=2025:01:15 10:30:00")
    fn parse_date_shift(arg: &str) -> Option<(String, String, String)> {
        // Date shift args must start with '-'
        if !arg.starts_with('-') {
            return None;
        }

        // Every branch below slices `arg[1..pos]` to drop the leading '-', so
        // an operator found *at* index 0 asks for `[1..0]` and aborts the whole
        // invocation. The leading '-' is itself the '-' of `-=`, so
        // `oxidex '-=' photo.jpg` and `oxidex '-=1:0:0 0:0:0' photo.jpg` both
        // died with "byte range starts at 1 but ends at 0" -- the same defect
        // as the `unquote` panic (issue #261), in the other parser.
        //
        // `pos > 1` rather than `pos >= 1`: at index 1 the slice is legal but
        // empty, which is the no-tag-name case `--=x` and no more meaningful
        // than the panic was.
        //
        // (The indices are char-boundary safe: index 1 follows the ASCII '-',
        // and `find` on an ASCII needle only reports boundaries.)
        let tag_end = |pos: usize| (pos > 1).then_some(pos);

        // Check for += operator first (must check before single =)
        if let Some(pos) = arg.find("+=").and_then(tag_end) {
            let tag = arg[1..pos].to_string(); // Remove leading '-'
            let value = arg[pos + 2..].to_string();
            return Some((tag, "+=".to_string(), value));
        }

        // Check for -= operator
        if let Some(pos) = arg.find("-=").and_then(tag_end) {
            let tag = arg[1..pos].to_string();
            let value = arg[pos + 2..].to_string();
            return Some((tag, "-=".to_string(), value));
        }

        // Check for = operator (but not if it's part of += or -=)
        // Also need to distinguish from regular tag modifications
        if let Some(pos) = arg.find('=').and_then(tag_end) {
            let tag = arg[1..pos].to_string();
            let value = arg[pos + 1..].to_string();

            // CreateDate and DateTimeOriginal are normal writable EXIF tags. Routing an absolute
            // assignment through the date-shift path makes it impossible to
            // create tags 0x9004/0x9003 when absent, because that path only
            // patches existing date entries. Keep it in the ordinary write
            // path, which can add a new ExifIFD entry like ExifTool does.
            if matches!(
                tag
                .rsplit_once(':')
                .map_or(tag.as_str(), |(_, name)| name),
                name if name.eq_ignore_ascii_case("CreateDate")
                    || name.eq_ignore_ascii_case("DateTimeOriginal")
            ) {
                return None;
            }
            // So is any date set named by its IFD0 or ExifIFD group, and
            // `EXIF:ModifyDate` (written to IFD0, as 13.59 does): the shift
            // path patches the entry the name resolves to wherever it is, so
            // `-ExifIFD:ModifyDate=<date>` was refused ("not supported for
            // JPEG") and `-IFD0:ModifyDate=<date>` left the ExifIFD copy that
            // pinned ExifTool 13.59 deletes (`writers::exif_cross_delete`).
            if tag.split_once(':').is_some_and(|(group, name)| {
                group.eq_ignore_ascii_case("IFD0")
                    || group.eq_ignore_ascii_case("ExifIFD")
                    || (group.eq_ignore_ascii_case("EXIF")
                        && name.eq_ignore_ascii_case("ModifyDate"))
            }) {
                return None;
            }

            // Check if this looks like a date shift operation
            // Date shifts should have either:
            // - "AllDates" as the tag pattern (case-insensitive)
            // - A tag containing a date-related keyword (DateTime, Date, CreateDate, etc.)
            // - A value in date format (contains colons and spaces like "Y:M:D H:M:S" or "YYYY:MM:DD HH:MM:SS")

            let tag_lower = tag.to_lowercase();
            let is_date_tag =
                tag_lower == "alldates" || tag_lower.contains("date") || tag_lower.contains("time");

            let is_date_value = value.contains(':') && value.contains(' ');

            // Only treat as date shift if both tag and value look date-related
            if is_date_tag && is_date_value {
                return Some((tag, "=".to_string(), value));
            }
        }

        None
    }
}

/// Helper function to extract the actual argument from a lexopt error message
///
/// Lexopt error messages have the format: "unexpected argument '--option'" or "unexpected option '-o'"
/// or "unexpected argument for option '-E': \"XIF:Artist=TestValue\"" (when a tag looks like a flag)
/// This function extracts the actual argument string from the error message.
///
/// # Arguments
///
/// * `error_msg` - The error message from lexopt
///
/// # Returns
///
/// The extracted argument string, or the original string if parsing fails
fn extract_arg_from_error(error_msg: &str) -> Option<String> {
    // Handle the "unexpected argument for option '-X': \"value\"" format
    // This occurs when lexopt parses "-EXIF:Artist=value" as "-E" with unexpected value
    if error_msg.contains("unexpected argument for option") {
        // Extract the option part (e.g., '-E')
        if let Some(start) = error_msg.find('\'')
            && let Some(end) = error_msg[start + 1..].find('\'')
        {
            let option = &error_msg[start + 1..start + 1 + end];

            // Extract the value part (after the colon and space, between quotes)
            if let Some(value_start) = error_msg.find(": \"")
                && let Some(value_end) = error_msg[value_start + 3..].find('"')
            {
                let value = &error_msg[value_start + 3..value_start + 3 + value_end];
                // Reconstruct the full argument by combining option and value
                // e.g., '-E' + 'XIF:Artist=value' = '-EXIF:Artist=value'
                return Some(format!("{}{}", option, value));
            }
        }
    }

    // Try to find quoted text in the error message
    if let Some(start) = error_msg.find('\'')
        && let Some(end) = error_msg[start + 1..].find('\'')
    {
        return Some(error_msg[start + 1..start + 1 + end].to_string());
    }

    // Try double quotes as fallback
    if let Some(start) = error_msg.find('"')
        && let Some(end) = error_msg[start + 1..].find('"')
    {
        return Some(error_msg[start + 1..start + 1 + end].to_string());
    }

    None
}

/// Prints help text for the CLI application
///
/// This function displays comprehensive usage information including:
/// - Application description
/// - Usage syntax
/// - Available options with short and long forms
/// - Examples of common use cases
fn print_help() {
    println!("oxidex {}", env!("CARGO_PKG_VERSION"));
    println!("A modern, high-performance Rust reimplementation of ExifTool");
    println!();
    println!("USAGE:");
    println!("    oxidex [OPTIONS] [-TAG=VALUE ...] FILE|DIRECTORY");
    println!();
    println!("OPTIONS:");
    println!("    -h, --help                  Print help information");
    println!("    -V, --version               Print version information");
    println!("    -j, --json                  Output in JSON format");
    println!("        --csv                   Output in CSV format");
    println!(
        "    -s, -short                  Short output: tag names, padded (-s2/-S: unpadded, -s3: values only)"
    );
    println!("    -a                          Display all tags (default behavior)");
    println!("    -r                          Recursive directory processing");
    println!(
        "        --preserve-file-times   Preserve original file modification time after writing"
    );
    println!(
        "        --backup                Create backup copy before modifying file (.bak extension)"
    );
    println!("        --readonly              Enable read-only mode to prevent file modifications");
    println!(
        "        --strict                Fail a damaged/unidentifiable read instead of returning partial output"
    );
    println!(
        "        --extended-output       Show OxiDex's own diagnostic/forensic tags (no ExifTool counterpart), hidden by default"
    );
    println!(
        "        --detector VALUE        File detection mode: signature (default) or magika (AI-powered)"
    );
    println!(
        "    -e, --exiftool-compat       Format output for ExifTool compatibility (now the default)"
    );
    println!(
        "        --no-print-conv         Report raw stored values (ExifTool's -n; -n is dry-run here)"
    );
    println!("        --TagsFromFile VALUE    Copy metadata from source file");
    println!(
        "    -d VALUE                    Date format string for DateTime tags in filename patterns"
    );
    println!(
        "    -n                          Dry-run mode: show proposed renames without executing"
    );
    println!();
    println!("EXAMPLES:");
    println!("    # Read metadata from a file");
    println!("    oxidex photo.jpg");
    println!();
    println!("    # Output metadata in JSON format");
    println!("    oxidex -j photo.jpg");
    println!();
    println!("    # Modify a single tag");
    println!("    oxidex -EXIF:Artist=\"John Doe\" photo.jpg");
    println!();
    println!("    # Copy metadata from one file to another");
    println!("    oxidex --TagsFromFile source.jpg destination.jpg");
    println!();
    println!("    # Rename file based on metadata");
    println!("    oxidex '-FileName<DateTimeOriginal' -d %Y%m%d_%H%M%S photo.jpg");
    println!();
    println!("For more information, visit: https://github.com/oxidex/oxidex");
}

/// Prints version information for the CLI application
///
/// Displays the application name and version number from Cargo package metadata.
fn print_version() {
    println!("oxidex {}", env!("CARGO_PKG_VERSION"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_display_flags_are_recognized() {
        assert!(is_group_display_flag("-G"));
        assert!(is_group_display_flag("-g"));
        assert!(is_group_display_flag("-G0"));
        assert!(is_group_display_flag("-G1"));
        assert!(is_group_display_flag("-g2"));
        assert!(is_group_display_flag("-G1:2"));
    }

    #[test]
    fn tag_names_starting_with_g_are_not_group_display_flags() {
        // Regression: `-G1` fell through to the specific-tag branch and was
        // treated as a request for a tag literally named "G1", matching
        // nothing and silently emptying the whole extraction.
        assert!(!is_group_display_flag("-GPSLatitude"));
        assert!(!is_group_display_flag("-GPSLatitude=1"));
        assert!(!is_group_display_flag("photo.jpg"));
        assert!(!is_group_display_flag("--json"));
    }

    fn cli_args(args: &[&str], literal_paths: &[&str]) -> CliArgs {
        CliArgs {
            detector: DetectorMode::default(),
            json: false,
            csv: false,
            short_level: 2,
            all_tags: false,
            group_display: None,
            extended_output: false,
            recursive: false,
            preserve_file_times: false,
            backup: false,
            readonly: false,
            exiftool_compat: true,
            tags_from_file: None,
            date_format: None,
            dry_run: false,
            strict: false,
            args: args.iter().map(OsString::from).collect(),
            literal_paths: literal_paths.iter().map(OsString::from).collect(),
        }
    }

    /// Arguments that are not valid UTF-8 used to panic inside
    /// `std::env::args()`. A path is bytes and survives parsing untouched in
    /// every position a path can take; an XP value is accepted; every other
    /// non-UTF-8 name or value is an error, not a panic.
    #[cfg(unix)]
    #[test]
    fn non_utf8_paths_survive_parsing_in_every_position() {
        use std::os::unix::ffi::OsStrExt;
        let os = |bytes: &[u8]| OsStr::from_bytes(bytes).to_os_string();
        let parse = |argv: &[&[u8]]| CliArgs::parse_from(argv.iter().map(|a| os(a)));
        let path = |bytes: &[u8]| PathBuf::from(os(bytes));

        let args = parse(&[b"-s2", b"-Make", b"n\xff.jpg"]).unwrap();
        assert_eq!(args.file(), Some(path(b"n\xff.jpg")));
        assert_eq!(args.files(), [path(b"n\xff.jpg")]);
        assert_eq!(args.specific_tags(), Some(vec!["Make".to_string()]));

        let args = parse(&[b"-j", b"a\xfe.jpg", b"b\xff.jpg"]).unwrap();
        assert_eq!(args.files(), [path(b"a\xfe.jpg"), path(b"b\xff.jpg")]);

        let args = parse(&[b"-Make", b"--", b"-\xff.jpg"]).unwrap();
        assert_eq!(args.files(), [path(b"-\xff.jpg")]);

        for argv in [
            &[&b"-TagsFromFile"[..], b"s\xff.jpg", b"d.jpg"][..],
            &[b"--TagsFromFile", b"s\xff.jpg", b"d.jpg"],
            &[b"-TagsFromFile=s\xff.jpg", b"d.jpg"],
            &[b"--TagsFromFile=s\xff.jpg", b"d.jpg"],
        ] {
            let args = parse(argv).unwrap();
            assert_eq!(args.tags_from_file, Some(os(b"s\xff.jpg")), "{argv:?}");
            assert_eq!(args.file(), Some(path(b"d.jpg")), "{argv:?}");
        }

        // An XP value is kept as bytes, and a modification that follows it is
        // still one (it is not swallowed as a path).
        let args = parse(&[
            b"-IFD0:XPTitle=A\xed\xa0\x80B",
            b"-IFD0:Model=X",
            b"d\xff.jpg",
        ])
        .unwrap();
        assert_eq!(
            args.tag_modifications(),
            vec![
                ("IFD0:XPTitle".to_string(), os(b"A\xed\xa0\x80B")),
                ("IFD0:Model".to_string(), os(b"X")),
            ]
        );
        assert_eq!(args.specific_tags(), None);
        assert_eq!(args.file(), Some(path(b"d\xff.jpg")));

        // Quotes come off a non-UTF-8 value as they do off a UTF-8 one.
        let args = parse(&[b"-EXIF:XPTitle=\"A\xed\xa0\x80\"", b"d.jpg"]).unwrap();
        assert_eq!(
            args.tag_modifications(),
            vec![("EXIF:XPTitle".to_string(), os(b"A\xed\xa0\x80"))]
        );
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_names_and_unwritable_values_are_errors() {
        use std::os::unix::ffi::OsStrExt;
        let os = |bytes: &[u8]| OsStr::from_bytes(bytes).to_os_string();
        let parse = |argv: &[&[u8]]| CliArgs::parse_from(argv.iter().map(|a| os(a)));
        for (argv, expected) in [
            (
                &[&b"-Art\xffist=x"[..], b"a.jpg"][..],
                "must be valid UTF-8",
            ),
            (&[b"-\xff", b"a.jpg"], "must be valid UTF-8"),
            (&[b"--js\xffon", b"a.jpg"], "must be valid UTF-8"),
            (&[b"-Mak\xffe", b"a.jpg"], "must be valid UTF-8"),
            (&[b"-=\xff", b"a.jpg"], "invalid tag name"),
            (&[b"-IFD0:Artist=A\xffB", b"a.jpg"], "not valid UTF-8"),
            (&[b"-IFD0:XPTitle=A\xffB", b"a.jpg"], "Malformed UTF-8"),
            (&[b"-XPTitle=A\xed\xa0\x80B", b"a.jpg"], "-IFD0:XPTitle="),
            (&[b"-d", b"%Y\xff", b"a.jpg"], "-d must be valid UTF-8"),
            (
                &[b"--detector", b"\xff", b"a.jpg"],
                "--detector must be valid UTF-8",
            ),
        ] {
            let error = parse(argv).unwrap_err().to_string();
            assert!(error.contains(expected), "{argv:?}: {error}");
        }
    }

    /// Pinned 13.59 reads every argument after `--` as a file:
    /// `-s2 -Make -- normal.jpg -s` and `-s2 -Make normal.jpg -- -s` both
    /// read `normal.jpg` then `-s`, and `-- -a -b` reads `-a` then `-b`.
    #[test]
    fn paths_after_double_dash_are_files_for_every_accessor() {
        let paths = |args: &CliArgs| {
            args.files()
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
        };

        let args = cli_args(&["-Make"], &["normal.jpg", "-s"]);
        assert_eq!(paths(&args), ["normal.jpg", "-s"]);
        assert_eq!(args.file(), Some(PathBuf::from("-s")));
        assert_eq!(args.specific_tags(), Some(vec!["Make".to_string()]));
        assert!(args.tag_modifications().is_empty());

        let args = cli_args(&["-Make", "normal.jpg"], &["-s"]);
        assert_eq!(paths(&args), ["normal.jpg", "-s"]);
        assert_eq!(args.specific_tags(), Some(vec!["Make".to_string()]));

        let args = cli_args(&["-Make"], &["-a", "-b"]);
        assert_eq!(paths(&args), ["-a", "-b"]);
        assert_eq!(args.file(), Some(PathBuf::from("-b")));
        assert_eq!(args.specific_tags(), Some(vec!["Make".to_string()]));

        // A path that looks like a modification or date shift is still a path.
        let args = cli_args(&["-Make"], &["-all=", "-AllDates+=1"]);
        assert!(args.tag_modifications().is_empty());
        assert!(args.date_shift_operations().is_empty());
        assert!(!args.is_clear_all_metadata());
        assert_eq!(args.specific_tags(), Some(vec!["Make".to_string()]));

        // Without `--`, the last argument is still the file.
        let args = cli_args(&["-Make", "photo.jpg"], &[]);
        assert_eq!(paths(&args), ["photo.jpg"]);
        assert_eq!(args.specific_tags(), Some(vec!["Make".to_string()]));
    }

    /// Mirrors `exiftool`:1243-1244 (13.59): `-S`/`-veryShort` add 2,
    /// `-s`/`-short` add 1, `-sN`/`-shortN` (any case) set the level.
    #[test]
    fn short_level_option_spellings_match_exiftool() {
        use ShortLevelOption::{Add, Set};
        for (arg, expected) in [
            ("-s", Some(Add(1))),
            ("-short", Some(Add(1))),
            ("-SHORT", Some(Add(1))),
            ("-S", Some(Add(2))),
            ("-veryShort", Some(Add(2))),
            ("-veryshort", Some(Add(2))),
            ("-s0", Some(Set(0))),
            ("-s2", Some(Set(2))),
            ("-S2", Some(Set(2))),
            ("-s3", Some(Set(3))),
            ("-short3", Some(Set(3))),
            ("-s999", Some(Set(u8::MAX))),
            ("-sa", None),
            ("-sep", None),
            ("-shortx", None),
            ("-Sharpness", None),
            ("-ShutterSpeed", None),
            ("-EXIF:s3", None),
            ("s3", None),
        ] {
            assert_eq!(parse_short_level_option(arg), expected, "{arg}");
        }
        let level = [Add(1), Add(2)].iter().fold(0, |l, o| o.apply(l));
        assert_eq!(level, 3, "-s -S is level 3");
        assert_eq!(Set(2).apply(Add(1).apply(0)), 2, "-s -s2 is level 2");
    }

    #[test]
    fn short_option_clusters_are_lexopt_args() {
        assert!(is_lexopt_short_arg("-s"));
        assert!(is_lexopt_short_arg("-sr"));
        assert!(is_lexopt_short_arg("-d"));
        // Attached date-format values stay with lexopt's -d option.
        assert!(is_lexopt_short_arg("-d%Y%m%d"));
        assert!(is_lexopt_short_arg("-srd%Y"));
    }

    #[test]
    fn assignment_shaped_args_are_never_lexopt_short_args() {
        // Regression: these were consumed as `-d` with an attached value,
        // silently dropping the requested tag write.
        assert!(!is_lexopt_short_arg("-description=test"));
        assert!(!is_lexopt_short_arg(
            "-datetimeoriginal=2020:01:01 10:00:00"
        ));
        assert!(!is_lexopt_short_arg("-d=broken"));
    }

    #[test]
    fn non_cluster_args_are_not_lexopt_short_args() {
        assert!(!is_lexopt_short_arg("--json"));
        assert!(!is_lexopt_short_arg("-EXIF:Model"));
        assert!(!is_lexopt_short_arg("photo.jpg"));
        assert!(!is_lexopt_short_arg("-"));
    }

    #[test]
    fn tag_names_starting_with_cluster_letters_are_not_swallowed() {
        // Regression: these bare (no '=') filter/extraction args start with
        // valid cluster letters but are tag names, not option clusters. The
        // 'd'-prefixed ones were parsed as `-d` with an attached value.
        assert!(!is_lexopt_short_arg("-description"));
        assert!(!is_lexopt_short_arg("-datetimeoriginal"));
        assert!(!is_lexopt_short_arg("-date"));
        assert!(!is_lexopt_short_arg("-shadows")); // s,h,a all flags, then 'd'
        assert!(!is_lexopt_short_arg("-redbalance")); // r,e flags, then 'd'
        assert!(!is_lexopt_short_arg("-address")); // a flag, then 'd'
    }

    #[test]
    fn genuine_date_option_forms_still_reach_lexopt() {
        assert!(is_lexopt_short_arg("-d")); // format is the next argument
        assert!(is_lexopt_short_arg("-d%Y%m%d")); // attached strftime format
        assert!(is_lexopt_short_arg("-srd")); // cluster ending in -d
        assert!(is_lexopt_short_arg("-nd%H%M")); // cluster + attached format
    }

    /// Regression: `oxidex '-EXIF:Artist="' photo.jpg` aborted the whole
    /// invocation with "byte range starts at 1 but ends at 0" -- a lone quote
    /// satisfies `starts_with` and `ends_with` simultaneously, so `unquote`
    /// sliced `[1..0]`.
    #[test]
    fn unquote_handles_a_lone_quote_without_panicking() {
        assert_eq!(CliArgs::unquote("\""), "\"");
        assert_eq!(CliArgs::unquote("'"), "'");
        assert_eq!(CliArgs::unquote(" \" "), " \" ");
        assert_eq!(
            CliArgs::parse_text_modification("-EXIF:Artist=\""),
            Some(("EXIF:Artist".to_string(), "\"".to_string()))
        );

        // Still strips real pairs, and leaves everything else alone.
        assert_eq!(CliArgs::unquote("\"\""), "");
        assert_eq!(CliArgs::unquote("\"Ansel Adams\""), "Ansel Adams");
        assert_eq!(CliArgs::unquote("'Ansel Adams'"), "Ansel Adams");
        assert_eq!(CliArgs::unquote("Ansel Adams"), "Ansel Adams");
        // Non-ASCII values survive unquoting untouched: the slice indices are
        // pinned to single-byte quotes, but assert it rather than assume it.
        assert_eq!(CliArgs::unquote("\"日本語の写真家\""), "日本語の写真家");
        assert_eq!(CliArgs::unquote("日本語の写真家"), "日本語の写真家");
    }

    /// Regression: an argument with no tag name before the `=` was parsed as a
    /// modification of tag `""`, and the writer carried it out --
    /// `oxidex '-=' photo.jpg` printed "1 image files updated" and rewrote the
    /// file. ExifTool rejects these outright and leaves the file alone.
    #[test]
    fn empty_tag_name_is_not_a_modification() {
        assert_eq!(CliArgs::parse_text_modification("-="), None);
        assert_eq!(CliArgs::parse_text_modification("-=x"), None);
        assert_eq!(CliArgs::parse_text_modification("-=\""), None);
        assert_eq!(CliArgs::parse_text_modification("-  =  "), None);
        assert_eq!(CliArgs::parse_text_modification("--="), None);

        // Real modifications are unaffected.
        assert_eq!(
            CliArgs::parse_text_modification("-EXIF:Artist=Ansel Adams"),
            Some(("EXIF:Artist".to_string(), "Ansel Adams".to_string()))
        );
        // Clearing a tag with an empty value is still a modification: the tag
        // name is what has to be present, not the value.
        assert_eq!(
            CliArgs::parse_text_modification("-EXIF:Artist="),
            Some(("EXIF:Artist".to_string(), String::new()))
        );
    }

    /// Regression: the same `[1..0]` slice as above, in the *other* parser.
    /// `oxidex '-=' photo.jpg` and `oxidex '-=1:0:0 0:0:0' photo.jpg` aborted
    /// with "byte range starts at 1 but ends at 0" -- the leading '-' every
    /// branch strips is itself the '-' of the `-=` operator, so the operator
    /// sits at index 0 and there is no tag name to slice out.
    #[test]
    fn date_shift_operator_at_index_zero_does_not_panic() {
        assert_eq!(CliArgs::parse_date_shift("-="), None);
        assert_eq!(CliArgs::parse_date_shift("-=1:0:0 0:0:0"), None);
        assert_eq!(CliArgs::parse_date_shift("-"), None);
        // Legal slice, but an empty tag name -- no more meaningful than the
        // panic it replaced.
        assert_eq!(CliArgs::parse_date_shift("--=x"), None);
        assert_eq!(CliArgs::parse_date_shift("-+=x"), None);

        // Real date shifts still parse.
        assert_eq!(
            CliArgs::parse_date_shift("-AllDates+=1:0:0 0:0:0"),
            Some((
                "AllDates".to_string(),
                "+=".to_string(),
                "1:0:0 0:0:0".to_string()
            ))
        );
        assert_eq!(
            CliArgs::parse_date_shift("-EXIF:DateTime-=0:1:0 0:0:0"),
            Some((
                "EXIF:DateTime".to_string(),
                "-=".to_string(),
                "0:1:0 0:0:0".to_string()
            ))
        );
        assert_eq!(
            CliArgs::parse_date_shift("-EXIF:DateTime=2025:01:15 10:30:00"),
            Some((
                "EXIF:DateTime".to_string(),
                "=".to_string(),
                "2025:01:15 10:30:00".to_string()
            ))
        );
    }

    #[test]
    fn create_date_assignment_is_a_normal_tag_write() {
        assert_eq!(
            CliArgs::parse_date_shift("-CreateDate=2024:02:03 04:05:06"),
            None
        );
        assert_eq!(
            CliArgs::parse_date_shift("-ExifIFD:CreateDate=2024:02:03 04:05:06"),
            None
        );
    }

    /// A date set named by its IFD0 or ExifIFD group is an ordinary write,
    /// so it can move the tag between the two directories as pinned
    /// ExifTool 13.59 does (`writers::exif_cross_delete`); a shift is still
    /// a shift, and an ungrouped or `EXIF:` set keeps its route.
    #[test]
    fn ifd0_and_exififd_date_assignments_are_normal_tag_writes() {
        for arg in [
            "-IFD0:ModifyDate=2020:01:02 03:04:05",
            "-ExifIFD:ModifyDate=2020:01:02 03:04:05",
            "-exififd:modifydate=2020:01:02 03:04:05",
            "-EXIF:ModifyDate=2020:01:02 03:04:05",
        ] {
            assert_eq!(CliArgs::parse_date_shift(arg), None, "{arg}");
        }
        assert_eq!(
            CliArgs::parse_date_shift("-ExifIFD:ModifyDate+=1:0:0"),
            Some((
                "ExifIFD:ModifyDate".to_string(),
                "+=".to_string(),
                "1:0:0".to_string()
            ))
        );
        assert!(CliArgs::parse_date_shift("-ModifyDate=2020:01:02 03:04:05").is_some());
    }

    #[test]
    fn date_time_original_assignment_is_a_normal_tag_write() {
        assert_eq!(
            CliArgs::parse_date_shift("-DateTimeOriginal=2024:02:03 04:05:06"),
            None
        );
        assert_eq!(
            CliArgs::parse_date_shift("-ExifIFD:DateTimeOriginal=2024:02:03 04:05:06"),
            None
        );
    }
}
