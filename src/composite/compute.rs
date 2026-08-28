//! Hand-ported implementations of ExifTool's Composite conversions.
//!
//! The dependency graph in [`super::tables`] is generated; the arithmetic here
//! is not. Each function is a deliberate port of one ExifTool `ValueConv` /
//! `PrintConv` pair, with the original quoted above it so a reviewer can check
//! the translation without opening `Exif.pm`.
//!
//! A composite with no entry in [`compute`] never fires. That is the same rule
//! the binary-table generator follows: absent beats approximate, because a
//! wrong `Aperture` looks exactly like a right one to everything downstream.
//!
//! Adding one function here fixes that tag for *every* format at once, which is
//! why this layer is worth building before chasing per-format gaps.
use crate::core::formatters::duration::convert_duration;
use crate::core::formatters::exif_print_conv::print_exposure_time;
use crate::core::formatters::numeric_precision::perl_number;
use crate::parsers::tiff::makernotes::panasonic::SHOOTING_MODE;

/// Inputs to a composite: `require` values followed by `desire` values, in the
/// order ExifTool declares them, so indices line up with its `$val[N]`.
pub type Inputs<'a> = &'a [Option<&'a str>];

/// The two forms ExifTool keeps for every tag.
///
/// `value` is the `ValueConv` result -- full precision, and what dependent
/// composites consume. `print` is the `PrintConv` result, rounded for display.
///
/// Keeping them apart is not cosmetic. `HyperfocalDistance` divides by
/// `CircleOfConfusion`; feeding it the printed `0.019 mm` instead of the
/// unrounded 0.01926 yields 4.35 m where ExifTool says 4.37 m. Collapsing the
/// two forms silently loses a digit at every link in the chain.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Computed {
    pub value: String,
    pub print: String,
}

impl Computed {
    /// A tag whose display form is its value form.
    pub(super) fn same(v: impl Into<String>) -> Option<Self> {
        let v = v.into();
        Some(Computed {
            print: v.clone(),
            value: v,
        })
    }

    /// Distinct value and display forms.
    pub(super) fn new(value: impl Into<String>, print: impl Into<String>) -> Option<Self> {
        Some(Computed {
            value: value.into(),
            print: print.into(),
        })
    }
}

/// ExifTool's `join(' ', @list)` over `ValueConv` numbers, returning both the
/// joined text *and* the numbers its `PrintConv` will actually be handed.
///
/// Most composites hand their `PrintConv` a Perl NV, which `sprintf` formats
/// at full double precision. A few do not. When a `ValueConv` packs several
/// numbers into one scalar with `join(' ', ...)` and the matching `PrintConv`
/// unpacks them with `split`, the numbers make a round trip through decimal
/// text on the way: `join` stringifies each with Perl's `%.15g`, and `split`
/// hands back strings that `sprintf` re-numifies. Fifteen significant digits
/// is fewer than a double carries, so what reaches `%.2f` is not what the
/// arithmetic produced, and on a rounding boundary the two round opposite
/// ways.
///
/// `Composite:FOV` on `Olympus/OlympusTG-610.jpg` is the worked example.
/// `2*FocusDistance*tan(fd2)` is 2.8350000000000004, which `%.2f` rounds up
/// to 2.84 -- but `join` renders it `2.835`, and re-parsing that text gives
/// 2.8349999999999999, which rounds *down*. ExifTool prints `2.83 m`.
///
/// This models a data flow, so it belongs exactly where ExifTool's `ValueConv`
/// returns a joined string and nowhere else. `HyperfocalDistance`, `DOF`'s
/// sibling `CircleOfConfusion` and `FocalLength35efl` return bare numbers --
/// their `PrintConv` sees the unrounded double, and routing them through here
/// would introduce the very divergence it exists to remove.
///
/// Returning the text and the numbers from one call is deliberate: they are
/// two views of a single Perl scalar, and computing them at separate call
/// sites is how they drift apart.
fn perl_join(values: &[f64]) -> (String, Vec<f64>) {
    let rendered: Vec<String> = values.iter().copied().map(perl_number).collect();
    let reparsed = rendered
        .iter()
        .zip(values)
        .map(|(text, original)| text.parse().unwrap_or(*original))
        .collect();
    (rendered.join(" "), reparsed)
}

/// Parse a value ExifTool would have fed to `ToFloat`.
///
/// Handles the rational forms that reach composites unconverted (`1/200`) and
/// trailing units (`50.0 mm`), because the inputs are print-formatted values
/// rather than raw ones.
// `pub(super)`, not private: `generated_compute.rs` (the auto-derived
// $val[N]-expression sibling of this file, codegen_composite.py's output)
// reuses this exact parser rather than duplicating it, so a rational-input
// or unit-suffix fix made here does not silently drift out of sync with the
// generated arms.
pub(super) fn f(v: Option<&str>) -> Option<f64> {
    let s = v?.trim();
    if s.is_empty() {
        return None;
    }
    if let Some((n, d)) = s.split_once('/') {
        let (n, d) = (n.trim().parse::<f64>().ok()?, d.trim().parse::<f64>().ok()?);
        return if d == 0.0 { None } else { Some(n / d) };
    }
    // Take the leading numeric run so "50.0 mm" and "2.8" both work.
    let end = s
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+'))
        .unwrap_or(s.len());
    s[..end].parse::<f64>().ok()
}

pub(super) fn get<'a>(i: Inputs<'a>, n: usize) -> Option<&'a str> {
    i.get(n).copied().flatten()
}

fn perl_truthy(value: Option<&str>) -> bool {
    matches!(value.map(str::trim), Some(value) if !value.is_empty() && value != "0")
}

fn printed_integer(value: &str) -> Option<i64> {
    value.trim().parse().ok().or_else(|| {
        value
            .trim()
            .strip_prefix("Unknown (")?
            .strip_suffix(')')?
            .parse()
            .ok()
    })
}

/// ExifTool's `ConvertBitrate`: scale by 1000 through bps/kbps/Mbps/Gbps,
/// then use `%.3g` below 100 and `%.0f` otherwise.
fn convert_bitrate(bitrate: f64) -> String {
    const UNITS: [&str; 4] = ["bps", "kbps", "Mbps", "Gbps"];
    let mut value = bitrate;
    for (index, unit) in UNITS.iter().enumerate() {
        if value >= 1000.0 && index + 1 < UNITS.len() {
            value /= 1000.0;
            continue;
        }
        return if value < 100.0 {
            format!("{} {unit}", format_significant_3(value))
        } else {
            format!("{value:.0} {unit}")
        };
    }
    unreachable!("the bitrate unit list is non-empty")
}

/// Perl's `%.3g`, without exponential notation for the values ConvertBitrate
/// receives after unit scaling.
fn format_significant_3(value: f64) -> String {
    format_significant(value, 3)
}

/// Perl's `%.*g` for the non-exponential range these composites live in:
/// `digits` significant figures, with trailing zeros (and a bare decimal
/// point) stripped, which is what `%g` does and `%f` does not.
fn format_significant(value: f64, digits: i32) -> String {
    if value == 0.0 {
        return "0".to_string();
    }
    let magnitude = value.abs().log10().floor() as i32;
    let decimals = (digits - 1 - magnitude).max(0) as usize;
    let rendered = format!("{value:.decimals$}");
    if rendered.contains('.') {
        rendered
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    } else {
        rendered
    }
}

fn canon_exposure_mode(value: &str) -> Option<i64> {
    Some(match value {
        "Easy" => 0,
        "Program AE" => 1,
        "Shutter speed priority AE" => 2,
        "Aperture-priority AE" => 3,
        "Manual" => 4,
        "Depth-of-field AE" => 5,
        "M-Dep" => 6,
        "Bulb" => 7,
        "Flexible-priority AE" => 8,
        _ => printed_integer(value)?,
    })
}

fn canon_easy_mode(value: &str) -> Option<i64> {
    // Only values whose reverse mapping is unambiguous are accepted. Unknown
    // labels are refused rather than assigned a plausible scene-mode number.
    Some(match value {
        "Full auto" => 0,
        "Manual" => 1,
        "Landscape" => 2,
        "Fast shutter" => 3,
        "Slow shutter" => 4,
        "Night" => 5,
        "Gray Scale" => 6,
        "Sepia" => 7,
        "Portrait" => 8,
        "Sports" => 9,
        "Macro" => 10,
        "Black & White" => 11,
        "Pan focus" => 12,
        "Vivid" => 13,
        "Neutral" => 14,
        "Flash Off" => 15,
        "Long Shutter" => 16,
        "Super Macro" => 17,
        "Foliage" => 18,
        "Indoor" => 19,
        "Fireworks" => 20,
        "Beach" => 21,
        "Underwater" => 22,
        "Snow" => 23,
        "Kids & Pets" => 24,
        "Night Snapshot" => 25,
        "Digital Macro" => 26,
        "My Colors" => 27,
        "Movie Snap" => 28,
        "Super Macro 2" => 29,
        "Color Accent" => 30,
        "Color Swap" => 31,
        "Aquarium" => 32,
        "ISO 3200" => 33,
        "ISO 6400" => 34,
        "Creative Light Effect" => 35,
        "Easy" => 36,
        "Quick Shot" => 37,
        "Creative Auto" => 38,
        "Zoom Blur" => 39,
        "Low Light" => 40,
        "Nostalgic" => 41,
        "Super Vivid" => 42,
        "Poster Effect" => 43,
        "Face Self-timer" => 44,
        "Smile" => 45,
        "Wink Self-timer" => 46,
        "Fisheye Effect" => 47,
        "Miniature Effect" => 48,
        "High-speed Burst" => 49,
        "Best Image Selection" => 50,
        "High Dynamic Range" => 51,
        "Handheld Night Scene" => 52,
        "Movie Digest" => 53,
        "Live View Control" => 54,
        "Discreet" => 55,
        "Blur Reduction" => 56,
        "Monochrome" => 57,
        "Toy Camera Effect" => 58,
        "Scene Intelligent Auto" => 59,
        "High-speed Burst HQ" => 60,
        "Smooth Skin" => 61,
        "Soft Focus" => 62,
        "Food" => 68,
        "HDR Art Standard" => 84,
        "HDR Art Vivid" => 85,
        "HDR Art Bold" => 93,
        "Spotlight" => 257,
        "Night 2" => 258,
        "Night+" => 259,
        "Super Night" => 260,
        "Sunset" => 261,
        "Night Scene" => 263,
        "Surface" => 264,
        "Low Light 2" => 265,
        _ => printed_integer(value)?,
    })
}

fn canon_flash_mode(value: &str) -> Option<i64> {
    Some(match value {
        "n/a" => -1,
        "Off" => 0,
        "Auto" => 1,
        "On" => 2,
        "Red-eye reduction" => 3,
        "Slow-sync" => 4,
        "Red-eye reduction (Auto)" => 5,
        "Red-eye reduction (On)" => 6,
        "External flash" => 16,
        _ => printed_integer(value)?,
    })
}

fn focal_range(short: f64, long: f64, scale: f64) -> String {
    if short == long {
        format!("{:.1} mm", short * scale)
    } else {
        format!("{:.1} - {:.1} mm", short * scale, long * scale)
    }
}

fn gps_degrees(value: &str) -> Option<f64> {
    let value = value.trim();
    if value.is_empty() {
        return Some(0.0);
    }
    let numbers: Vec<f64> = value
        .split(|character: char| {
            !(character.is_ascii_digit() || matches!(character, '.' | '-' | '+'))
        })
        .filter(|part| !part.is_empty())
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    let degrees = *numbers.first()?;
    Some(
        degrees
            + numbers.get(1).copied().unwrap_or(0.0) / 60.0
            + numbers.get(2).copied().unwrap_or(0.0) / 3600.0,
    )
}

fn gps_print(mut degrees: f64, positive_reference: char) -> String {
    let negative_reference = match positive_reference {
        'N' => 'S',
        'E' => 'W',
        _ => unreachable!("GPS reference must be N or E"),
    };
    let reference = if degrees < 0.0 {
        degrees = -degrees;
        negative_reference
    } else {
        positive_reference
    };
    let mut whole_degrees = degrees.floor() as u32;
    let minutes = (degrees - f64::from(whole_degrees)) * 60.0;
    let mut whole_minutes = minutes.floor() as u32;
    let mut seconds = (minutes - f64::from(whole_minutes)) * 60.0;
    seconds = (seconds * 100.0).round() / 100.0;
    if seconds >= 60.0 {
        seconds -= 60.0;
        whole_minutes += 1;
    }
    if whole_minutes >= 60 {
        whole_minutes -= 60;
        whole_degrees += 1;
    }
    format!("{whole_degrees} deg {whole_minutes}' {seconds:.2}\" {reference}")
}

/// ExifTool `Image::ExifTool::Exif::RedBlueBalance`.
///
/// Each row gives the R, G, G, B component indices for one of ExifTool's nine
/// accepted white-balance layouts. `WB_RBLevels` uses the literal green level
/// 256 unless a component below 4 signals Nikon's unit scaling convention.
/// The source walks the layouts in order, averages the two green components,
/// and falls back to the separately stored component/green pair only if no
/// packed layout produced a value.
fn red_blue_balance(i: Inputs<'_>, blue: bool) -> Option<f64> {
    const LOOKUP: [[usize; 4]; 9] = [
        [0, 1, 2, 3],
        [0, 1, 3, 2],
        [0, 2, 3, 1],
        [1, 0, 3, 2],
        [1, 0, 2, 3],
        [2, 3, 0, 1],
        [0, 1, 1, 2],
        [1, 0, 0, 2],
        [0, 256, 256, 1],
    ];

    for (input, lookup) in i.iter().take(9).zip(LOOKUP) {
        let Some(levels) = input else { continue };
        let Ok(levels) = levels
            .split_whitespace()
            .map(str::parse)
            .collect::<Result<Vec<f64>, _>>()
        else {
            continue;
        };
        if levels.len() < 2 {
            continue;
        }

        let component_index = lookup[usize::from(blue) * 3];
        let component = *levels.get(component_index)?;
        let green_index = lookup[1];
        let green = if green_index < 4 {
            if levels.len() < 3 {
                continue;
            }
            let green = (levels[green_index] + levels[lookup[2]]) / 2.0;
            if green == 0.0 {
                continue;
            }
            green
        } else if component < 4.0 {
            1.0
        } else {
            green_index as f64
        };
        return Some(component / green);
    }

    let component = f(get(i, 9))?;
    let green = f(get(i, 10))?;
    if component == 0.0 || green == 0.0 {
        None
    } else {
        Some(component / green)
    }
}

/// ExifTool's `Image::ExifTool::IsFloat` (ExifTool.pm:5947-5953), applied to a
/// value that reaches this layer already carrying its `PrintConv` unit suffix.
///
/// `GPS:GPSAltitude`'s ExifTool `ValueConv` is a bare number and its
/// `PrintConv` is `"$val m"` (GPS.pm:124, in the `0x0006` tag at
/// GPS.pm:119-126) -- a verbatim append with no rounding -- but oxidex's GPS
/// parser stores only the printed form, so
/// `@val` arrives here as `"207 m"` where ExifTool's holds `207`. Stripping
/// that one suffix recovers the ValueConv value *exactly* rather than
/// approximating it; anything else (`"inf"`, `"undef"`, an empty string) is
/// refused, which is the same answer ExifTool's own `IsFloat` gives for those.
fn perl_is_float(value: &str) -> Option<f64> {
    let value = value.trim();
    let value = value.strip_suffix(" m").unwrap_or(value).trim();
    if value.is_empty() {
        return None;
    }
    // ExifTool's IsFloat regex admits digits, one optional decimal point, an
    // optional sign and an optional exponent -- and nothing else. Rust's own
    // parser additionally accepts "inf"/"NaN", which that regex rejects, so
    // screen those out rather than letting them through as numbers.
    if !value
        .bytes()
        .all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'-' | b'+' | b'e' | b'E'))
    {
        return None;
    }
    let parsed: f64 = value.parse().ok()?;
    parsed.is_finite().then_some(parsed)
}

/// Perl's `int($val * 10) / 10`: scale, truncate *toward zero*, scale back.
fn truncate_to_tenths(value: f64) -> f64 {
    (value * 10.0).trunc() / 10.0
}

/// GPS.pm:107-117 / XMP2.pl:354-366's shared `GPSAltitudeRef` `PrintConv`,
/// read in whichever direction the caller's input needs.
///
/// Composite:GPSAltitude needs both halves of ExifTool's pair: `$val[$_+1]`
/// for its Perl truthiness (`$val[$_+1] ? -abs($val[$_]) : $val[$_]`) and
/// `$prt[$_+1]` verbatim in the printed string. oxidex's parsers supply only
/// one of the two -- the EXIF path the raw `int8u`, another parser could
/// supply the label -- so this resolves either into the `(code, print)` pair
/// ExifTool holds. Both modules declare the identical four-entry hash. A code
/// outside it renders as ExifTool's `Unknown (N)`, which is also the form
/// `printed_integer` reads back; anything else is refused rather than assumed
/// to be zero.
fn gps_altitude_ref(label: &str) -> Option<(i64, String)> {
    const REFS: [(i64, &str); 4] = [
        (0, "Above Sea Level"),
        (1, "Below Sea Level"),
        (2, "Positive Sea Level (sea-level ref)"),
        (3, "Negative Sea Level (sea-level ref)"),
    ];
    let label = label.trim();
    if let Some(&(code, printed)) = REFS.iter().find(|(_, printed)| *printed == label) {
        return Some((code, printed.to_string()));
    }
    // The other direction: oxidex's EXIF parser hands the composite layer the
    // raw `int8u` (`super::value_string`), so the label has to be rendered
    // here -- forward through the same hash, with ExifTool's `Unknown (N)`
    // for a code the hash does not name.
    let code = printed_integer(label)?;
    let printed = REFS
        .iter()
        .find(|(known, _)| *known == code)
        .map_or_else(|| format!("Unknown ({code})"), |(_, p)| (*p).to_string());
    Some((code, printed))
}

/// GPS.pm:406-432 Composite::GPSAltitude.
///
/// ```perl
/// RawConv => '(defined $val[1] or defined $val[3]) ? $val : undef',
/// ValueConv => q{
///     foreach (0,2) {
///         next unless defined $val[$_] and IsFloat($val[$_]) and defined $val[$_+1];
///         return $val[$_+1] ? -abs($val[$_]) : $val[$_];
///     }
///     return undef;
/// },
/// PrintConv => q{
///     foreach (0,2) {
///         next unless defined $val[$_] and IsFloat($val[$_]);
///         next unless defined $prt[$_+1] and $prt[$_+1] =~ /Sea/;
///         return((int($val[$_]*10)/10) . ' m ' . $prt[$_+1]);
///     }
///     $val = int($val * 10) / 10;
///     return(($val =~ s/^-// ? "$val m Below" : "$val m Above") . " Sea Level");
/// },
/// ```
///
/// The `PrintConv` loop prints the *unsigned* `$val[$_]` next to the ref's own
/// label, so the displayed string never depends on the sign the `ValueConv`
/// applied -- which is why the `Positive Sea Level (sea-level ref)` code (2,
/// Perl-truthy, hence `-abs`) can only ever change the value form.
fn gps_altitude(i: Inputs<'_>) -> Option<Computed> {
    // `IsFloat($val[$_])`, over the forms an altitude can reach this layer in:
    // GPS.pm's `"$val m"` print form, an unconverted EXIF `n/d` rational, or a
    // plain number. All three are the ValueConv value ExifTool would hold,
    // recovered exactly; `inf` and `undef` (GPS.pm:124's other two PrintConv
    // outputs) parse as neither and are refused, which is the same answer
    // ExifTool's own `IsFloat` gives them.
    let altitude_of = |v: Option<&str>| -> Option<f64> {
        let v = v?.trim();
        let v = v.strip_suffix(" m").unwrap_or(v);
        if v.contains('/') {
            f(Some(v))
        } else {
            perl_is_float(v)
        }
    };

    // RawConv: at least one of the two refs must exist, or there is no
    // altitude to sign and ExifTool emits nothing at all.
    if get(i, 1).is_none() && get(i, 3).is_none() {
        return None;
    }

    let mut value = None;
    for base in [0usize, 2] {
        let (Some(altitude), Some(reference)) = (get(i, base), get(i, base + 1)) else {
            continue;
        };
        let Some(altitude) = altitude_of(Some(altitude)) else {
            continue;
        };
        value = Some(if gps_altitude_ref(reference)?.0 != 0 {
            -altitude.abs()
        } else {
            altitude
        });
        break;
    }
    let value = value?;

    for base in [0usize, 2] {
        let Some(altitude) = altitude_of(get(i, base)) else {
            continue;
        };
        let Some(reference) = get(i, base + 1)
            .and_then(gps_altitude_ref)
            .map(|(_, printed)| printed)
            .filter(|label| label.contains("Sea"))
        else {
            continue;
        };
        return Computed::new(
            crate::exiftool_tables::exprs::perl_num(value),
            format!(
                "{} m {reference}",
                crate::exiftool_tables::exprs::perl_num(truncate_to_tenths(altitude))
            ),
        );
    }

    let truncated = truncate_to_tenths(value);
    let printed = crate::exiftool_tables::exprs::perl_num(truncated);
    let print = match printed.strip_prefix('-') {
        Some(positive) => format!("{positive} m Below Sea Level"),
        None => format!("{printed} m Above Sea Level"),
    };
    Computed::new(crate::exiftool_tables::exprs::perl_num(value), print)
}

/// Reverse of Nikon.pm:928-932's `%aFDetectionMethod`, which is the only form
/// `Nikon:AFDetectionMethod` reaches this layer in: the generated binary-table
/// walker renders a field's transcribed `PrintConv` at extraction time
/// (`exiftool_tables::runtime::DecodedField::emit`), so no numeric `ValueConv`
/// survives for the two AF composites to compare against.
fn nikon_af_detection_method(label: &str) -> Option<i64> {
    Some(match label.trim() {
        "Phase Detect" => 0,
        "Contrast Detect" => 1,
        "Hybrid" => 2,
        other => printed_integer(other)?,
    })
}

/// Reverse of `Nikon:FocusPointSchema`'s `PrintConv`, which ExifTool declares
/// three times over -- Nikon.pm:4169-4177 (0-3), Nikon.pm:4383-4393 (0,1,2,7)
/// and Nikon.pm:4761-4771 (0,1,8,9), one per AFInfo2 layout. The three maps
/// disagree about which codes exist but never about what a given label means,
/// so the label -> code direction is single-valued across all three and does
/// not depend on knowing which layout produced it.
fn nikon_focus_point_schema(label: &str) -> Option<i64> {
    Some(match label.trim() {
        "Off" => 0,
        "51-point" => 1,
        "11-point" => 2,
        "39-point" => 3,
        "153-point" => 7,
        "81-point" => 8,
        "105-point" => 9,
        other => printed_integer(other)?,
    })
}

/// `Image::ExifTool::Olympus::ExtenderStatus` (Olympus.pm:4337-4351).
///
/// ```perl
/// sub ExtenderStatus($$$)
/// {
///     my ($extender, $lensType, $maxAperture) = @_;
///     my @info = split ' ', $extender;
///     # validate that extender identifier is reasonable
///     return 0 unless @info >= 2 and hex($info[1]);
///     # if it's not an EC-14 (id '0 04') then assume it was really attached
///     # (other extenders don't seem to affect the reported max aperture)
///     return 1 if "$info[0] $info[1]" ne '0 04';
///     # get the maximum aperture for this lens (in $1)
///     $lensType =~ / F(\d+(\.\d+)?)/ or return 1;
///     # If the maximum aperture at the maximum focal length is greater than the
///     # known max/max aperture of the lens, then the extender must be attached
///     return(($maxAperture - $1 > 0.2) ? 1 : 2);
/// }
/// ```
///
/// `$extender` is `Olympus:Extender`'s ValueConv, `sprintf("%x %.2x",
/// @bytes[0,2])` (Olympus.pm:1708-1724, Equipment 0x0301). oxidex's Olympus
/// parser stores the *PrintConv* of that key instead, so `extender_value_conv` below inverts
/// the four-entry lookup to recover it exactly; `$lensType` is already the
/// print form ExifTool passes here (`$prt[1]`, not `$val[1]`).
fn olympus_extender_status(extender: &str, lens_type: &str, max_aperture: f64) -> Option<i64> {
    let info: Vec<&str> = extender.split_whitespace().collect();
    if info.len() < 2 || i64::from_str_radix(info[1], 16).ok()? == 0 {
        return Some(0);
    }
    if format!("{} {}", info[0], info[1]) != "0 04" {
        return Some(1);
    }
    // Perl's ` F(\d+(\.\d+)?)`: the first " F" followed by a number.
    let Some(aperture) = lens_type.split(" F").nth(1).and_then(|rest| {
        let end = rest
            .find(|c: char| !(c.is_ascii_digit() || c == '.'))
            .unwrap_or(rest.len());
        // A bare "." or a trailing "." is not what the Perl regex accepts.
        rest[..end].parse::<f64>().ok()
    }) else {
        return Some(1);
    };
    Some(if max_aperture - aperture > 0.2 { 1 } else { 2 })
}

/// Recover `Olympus:Extender`'s ValueConv (`"%x %.2x"`) from the PrintConv
/// oxidex stores, using the same four-entry table the parser printed it with
/// (`parsers::tiff::makernotes::olympus::lookups::EQUIPMENT_EXTENDER`). The
/// map is closed and injective, and ExifTool's own `Unknown (%x %.2x)` fallback
/// carries the key verbatim, so this is an exact inverse rather than a guess.
fn olympus_extender_value_conv(printed: &str) -> Option<&str> {
    let printed = printed.trim();
    if let Some(key) = printed
        .strip_prefix("Unknown (")
        .and_then(|rest| rest.strip_suffix(')'))
    {
        return Some(key);
    }
    crate::parsers::tiff::makernotes::olympus::lookups::EQUIPMENT_EXTENDER
        .iter()
        .find(|(_, label)| *label == printed)
        .map(|(key, _)| *key)
}

/// `Image::ExifTool::Exif::PrintCFAPattern` (Exif.pm:5756-5773), which takes
/// Composite::CFAPattern's own `"rows cols c c c c"` ValueConv string.
fn print_cfa_pattern(value: &str) -> String {
    const COLORS: [&str; 7] = ["Red", "Green", "Blue", "Cyan", "Magenta", "Yellow", "White"];
    let a: Vec<&str> = value.split_whitespace().collect();
    if a.len() < 2 {
        return "<truncated data>".to_string();
    }
    let (Some(rows), Some(cols)) = (
        a[0].parse::<usize>().ok().filter(|n| *n != 0),
        a[1].parse::<usize>().ok().filter(|n| *n != 0),
    ) else {
        return "<zero pattern size>".to_string();
    };
    let end = 2 + rows * cols;
    if end > a.len() {
        return "<invalid pattern size>".to_string();
    }
    let mut out = String::from("[");
    let mut pos = 2;
    loop {
        let color = a[pos]
            .parse::<usize>()
            .ok()
            .and_then(|n| COLORS.get(n).copied())
            .unwrap_or("Unknown");
        out.push_str(color);
        pos += 1;
        if pos >= end {
            break;
        }
        if (pos - 2) % cols == 0 {
            out.push_str("][");
        } else {
            out.push(',');
        }
    }
    out.push(']');
    out
}

/// `Image::ExifTool::PostScript::ImageSize` (PostScript.pm:162-172): read the
/// first two integers of `ImageData`, or fall back to the width and height
/// implied by `BoundingBox`'s four.
fn postscript_image_size(i: Inputs<'_>, want_height: bool) -> Option<i64> {
    let ints = |s: &str, n: usize| -> Option<Vec<i64>> {
        let parsed: Vec<i64> = s
            .split_whitespace()
            .take(n)
            .map(str::parse)
            .collect::<Result<_, _>>()
            .ok()?;
        (parsed.len() == n).then_some(parsed)
    };
    if let Some(d) = get(i, 0)
        .filter(|v| perl_truthy(Some(v)))
        .and_then(|v| ints(v, 2))
    {
        return Some(d[usize::from(want_height)]);
    }
    let b = get(i, 1)
        .filter(|v| perl_truthy(Some(v)))
        .and_then(|v| ints(v, 4))?;
    Some(if want_height {
        b[3] - b[1]
    } else {
        b[2] - b[0]
    })
}

/// Reverse of the `Return` and `Mode` `PrintConv` maps of XMP.pm's `Flash`
/// struct (XMP.pm:2140-2147 and XMP.pm:2148-2156, inside the struct at
/// XMP.pm:2134-2160), whose numbers Composite::Flash shifts into the packed
/// EXIF `Flash` byte. Both are closed hashes -- `Return` has three entries
/// (0, 2, 3) and `Mode` four -- and oxidex's XMP parser stores only their
/// labels.
fn xmp_flash_field(label: &str, mode: bool) -> Option<i64> {
    Some(match (label.trim(), mode) {
        ("No return detection", false) => 0,
        ("Return not detected", false) => 2,
        ("Return detected", false) => 3,
        ("Unknown", true) => 0,
        ("On", true) => 1,
        ("Off", true) => 2,
        ("Auto", true) => 3,
        (other, _) => printed_integer(other)?,
    })
}

/// XMP.pm:2748-2788's four `GPS<Dest>L(at|ong)itudeRef` composites, which
/// differ only in which hemisphere letters they read and print.
///
/// ```perl
/// ValueConv => q{
///     IsFloat($val[0]) and return $val[0] < 0 ? "S" : "N";
///     $val[0] =~ /^.*([NS])/;
///     return $1;
/// },
/// PrintConv => { N => 'North', S => 'South' },   # E/W for longitude
/// ```
fn xmp_gps_ref(i: Inputs<'_>, longitude: bool) -> Option<Computed> {
    let (positive, negative) = if longitude { ('E', 'W') } else { ('N', 'S') };
    let coordinate = get(i, 0)?;
    let letter = match perl_is_float(coordinate) {
        Some(degrees) => {
            if degrees < 0.0 {
                negative
            } else {
                positive
            }
        }
        // Perl's `.*` is greedy, so this is the LAST N/S (or E/W) in the
        // string, not the first.
        None => coordinate
            .chars()
            .rev()
            .find(|c| *c == positive || *c == negative)?,
    };
    let print = match letter {
        'N' => "North",
        'S' => "South",
        'E' => "East",
        'W' => "West",
        _ => return None,
    };
    Computed::new(letter.to_string(), print)
}

/// ExifTool's shared `RawConv` for the three Composite SubSec timestamps:
/// append the leading digits of `$val[1]` after the seconds, then append a
/// normalized `[-+]HH:MM` from `$val[2]` only when the base has no sign.
fn subsec_date_time(i: Inputs<'_>) -> Option<String> {
    let date = get(i, 0)?;
    let mut value = None;

    if let Some(subsec) = get(i, 1) {
        let digits: String = subsec.chars().take_while(char::is_ascii_digit).collect();
        if !digits.is_empty() {
            // EXIF permits no fraction in the base tag. ExifTool nevertheless
            // checks before appending so a malformed pre-fractional value is
            // refused rather than doubled.
            if let Some(time_start) = date.rfind(' ')
                && date[time_start + 1..].len() >= 8
            {
                let time_end = time_start + 9;
                let time = &date[time_start + 1..time_end];
                let valid_time = time.as_bytes().get(2) == Some(&b':')
                    && time.as_bytes().get(5) == Some(&b':')
                    && time
                        .bytes()
                        .enumerate()
                        .all(|(n, b)| n == 2 || n == 5 || b.is_ascii_digit());
                let already_fractional = date.as_bytes().get(time_end) == Some(&b'.');
                if valid_time && !already_fractional {
                    let mut composed = String::with_capacity(date.len() + digits.len() + 1);
                    composed.push_str(&date[..time_end]);
                    composed.push('.');
                    composed.push_str(&digits);
                    composed.push_str(&date[time_end..]);
                    value = Some(composed);
                }
            }
        }
    }

    if !date.contains(['-', '+'])
        && let Some(offset) = get(i, 2)
    {
        let bytes = offset.as_bytes();
        if matches!(bytes.first(), Some(b'+') | Some(b'-'))
            && let Some(colon) = offset.find(':')
            && (2..=3).contains(&colon)
        {
            let hours = offset[1..colon].parse::<u8>().ok()?;
            let minutes = offset.get(colon + 1..colon + 3)?.parse::<u8>().ok()?;
            let base = value.get_or_insert_with(|| date.to_string());
            base.push(bytes[0] as char);
            base.push_str(&format!("{hours:02}:{minutes:02}"));
        }
    }

    value
}

/// ExifTool: `sprintf("%.*f", ($val >= 1 ? 1 : ($val >= 0.001 ? 3 : 6)), $val)`
fn fmt_megapixels(v: f64) -> String {
    let p = if v >= 1.0 {
        1
    } else if v >= 0.001 {
        3
    } else {
        6
    };
    format!("{v:.p$}", p = p)
}

/// ExifTool: `Image::ExifTool::Exif::PrintFNumber`
///
/// ```text
/// sprintf("%.1f", $val)  # (or %.2f below 1.0)
/// ```
fn print_fnumber(v: f64) -> String {
    if v > 0.0 && v < 1.0 {
        format!("{v:.2}")
    } else {
        format!("{v:.1}")
    }
}

/// Port of `Image::ExifTool::Canon::CalcSensorDiag`.
///
/// Most Canon cameras encode the sensor size in the *denominator* of the
/// FocalPlaneX/YResolution rationals, so this needs the unreduced `n/d` pair --
/// the divided float has thrown the information away. Every bound below is
/// ExifTool's, and they exist because the encoding is a convention rather than
/// a spec: if any check fails the assumption does not hold and we must return
/// nothing rather than a plausible number.
///
/// Skipping this was not harmless. Without it Canon files fell through to the
/// generic focal-plane path and produced ScaleFactor35efl 29.3 against
/// ExifTool's 1.6, which then corrupted CircleOfConfusion and
/// HyperfocalDistance -- three confidently wrong values under real tag names.
fn canon_sensor_diag(xres: Option<&str>, yres: Option<&str>) -> Option<f64> {
    fn parts(s: &str) -> Option<(i64, i64)> {
        let (n, d) = s.split_once('/')?;
        Some((n.trim().parse().ok()?, d.trim().parse().ok()?))
    }
    let (xn, xd) = parts(xres?)?;
    let (yn, yd) = parts(yres?)?;

    // Numerators are image width/height * 1000; denominators are sensor
    // width/height in inches * 1000.
    let ok = xn % 1000 == 0
        && yn % 1000 == 0
        && xn >= 640_000
        && yn >= 480_000
        && xn < 10_000_000
        && yn < 10_000_000
        && (61..1500).contains(&xd)
        && (61..1000).contains(&yd)
        // A square result means the rational was reduced and the assumption
        // no longer holds.
        && xd != yd;
    if !ok {
        return None;
    }
    Some(((xd * xd + yd * yd) as f64).sqrt() * 0.0254)
}

/// Recover Panasonic's numeric SceneMode from its printed form.
///
/// Composite inputs currently arrive after MakerNote PrintConv. Refuse labels
/// shared by multiple numeric values (notably HDR and Creative Control), since
/// choosing either would make AdvancedSceneMode look valid while being wrong.
fn panasonic_scene_mode(value: &str) -> Option<i32> {
    if value == "Off" {
        return Some(0);
    }
    if let Some(value) = printed_integer(value) {
        return i32::try_from(value).ok();
    }

    let mut found = None;
    for candidate in 1..=92 {
        if SHOOTING_MODE.decode(candidate) == value {
            if found.is_some() {
                return None;
            }
            found = Some(candidate);
        }
    }
    found
}

/// Panasonic.pm 13.59 `%Panasonic::Composite` AdvancedSceneMode PrintConv.
fn panasonic_advanced_scene_mode(model: &str, scene: &str, advanced: &str) -> Option<Computed> {
    let scene = panasonic_scene_mode(scene)?;
    let advanced = printed_integer(advanced)?;
    let value = format!("{model} {scene} {advanced}");

    let model_specific = match (model, scene, advanced) {
        ("DMC-TZ40", 90, 1) => Some("Expressive"),
        ("DMC-TZ40", 90, 2) => Some("Retro"),
        ("DMC-TZ40", 90, 3) => Some("High Key"),
        ("DMC-TZ40", 90, 4) => Some("Sepia"),
        ("DMC-TZ40", 90, 5) => Some("High Dynamic"),
        ("DMC-TZ40", 90, 6) => Some("Miniature"),
        ("DMC-TZ40", 90, 9) => Some("Low Key"),
        ("DMC-TZ40", 90, 10) => Some("Toy Effect"),
        ("DMC-TZ40", 90, 11) => Some("Dynamic Monochrome"),
        ("DMC-TZ40", 90, 12) => Some("Soft"),
        _ => None,
    };

    let fixed = model_specific.or(match (scene, advanced) {
        (0, 1) => Some("Off"),
        (2, 2) => Some("Outdoor Portrait"),
        (2, 3) => Some("Indoor Portrait"),
        (2, 4) => Some("Creative Portrait"),
        (3, 2) => Some("Nature"),
        (3, 3) => Some("Architecture"),
        (3, 4) => Some("Creative Scenery"),
        (4, 2) => Some("Outdoor Sports"),
        (4, 3) => Some("Indoor Sports"),
        (4, 4) => Some("Creative Sports"),
        (9, 2) => Some("Flower"),
        (9, 3) => Some("Objects"),
        (9, 4) => Some("Creative Macro"),
        (18, 1) => Some("High Sensitivity"),
        (20, 1) => Some("Fireworks"),
        (21, 2) => Some("Illuminations"),
        (21, 4) => Some("Creative Night Scenery"),
        (26, 1) => Some("High-speed Burst (shot 1)"),
        (27, 1) => Some("High-speed Burst (shot 2)"),
        (29, 1) => Some("Snow"),
        (30, 1) => Some("Starry Sky"),
        (31, 1) => Some("Beach"),
        (36, 1) => Some("High-speed Burst (shot 3)"),
        (39, 1) => Some("Aerial Photo / Underwater / Multi-aspect"),
        (45, 2) => Some("Cinema"),
        (45, 7) => Some("Expressive"),
        (45, 8) => Some("Retro"),
        (45, 9) => Some("Pure"),
        (45, 10) => Some("Elegant"),
        (45, 12) => Some("Monochrome"),
        (45, 13) => Some("Dynamic Art"),
        (45, 14) => Some("Silhouette"),
        (51, 2) => Some("HDR Art"),
        (51, 3) => Some("HDR B&W"),
        (59, 1) => Some("Expressive"),
        (59, 2) => Some("Retro"),
        (59, 3) => Some("High Key"),
        (59, 4) => Some("Sepia"),
        (59, 5) => Some("High Dynamic"),
        (59, 6) => Some("Miniature"),
        (59, 9) => Some("Low Key"),
        (59, 10) => Some("Toy Effect"),
        (59, 11) => Some("Dynamic Monochrome"),
        (59, 12) => Some("Soft"),
        (66, 1) => Some("Impressive Art"),
        (66, 2) => Some("Cross Process"),
        (66, 3) => Some("Color Select"),
        (66, 4) => Some("Star"),
        (90, 3) => Some("Old Days"),
        (90, 4) => Some("Sunshine"),
        (90, 5) => Some("Bleach Bypass"),
        (90, 6) => Some("Toy Pop"),
        (90, 7) => Some("Fantasy"),
        (90, 8) => Some("Monochrome"),
        (90, 9) => Some("Rough Monochrome"),
        (90, 10) => Some("Silky Monochrome"),
        (92, 1) => Some("Handheld Night Shot"),
        _ => None,
    });

    let print = if let Some(fixed) = fixed {
        fixed.to_string()
    } else {
        let shooting = SHOOTING_MODE.decode(scene);
        if shooting.starts_with("Unknown (") {
            return Computed::new(value.clone(), format!("Unknown ({value})"));
        }
        match advanced {
            1 => shooting,
            5 => format!("{shooting} (intelligent auto)"),
            7 => format!("{shooting} (intelligent auto plus)"),
            _ => format!("{shooting} ({advanced})"),
        }
    };
    Computed::new(value, print)
}

/// Compute one composite by name. `None` means "do not emit this tag".
///
/// `make` is the camera manufacturer, needed because ExifTool branches on it
/// for Canon sensor geometry.
///
/// The returned string is the print-formatted value, matching what ExifTool
/// prints by default, because that is what the comparison harness diffs.
#[must_use]
pub fn compute(module: &str, name: &str, i: Inputs, make: Option<&str>) -> Option<Computed> {
    match (module, name) {
        ("Panasonic", "AdvancedSceneMode") => {
            panasonic_advanced_scene_mode(get(i, 0)?, get(i, 1)?, get(i, 2)?)
        }

        // AIFF.pm:136-145 Composite::Duration:
        //   require:  0) AIFF:SampleRate, 1) AIFF:NumSampleFrames
        //   RawConv:  `($val[0] and $val[1]) ? $val[1] / $val[0] : undef`
        //   PrintConv: `ConvertDuration($val)`
        // Both inputs must be truthy, so a zero frame count is as disqualifying
        // as a zero rate -- a silent file has no duration rather than a
        // duration of nothing.
        ("AIFF", "Duration") => {
            let (rate, frames) = (f(get(i, 0))?, f(get(i, 1))?);
            if rate == 0.0 || frames == 0.0 {
                return None;
            }
            let seconds = frames / rate;
            Computed::new(seconds.to_string(), convert_duration(seconds))
        }

        // QuickTime.pm:8653-8665:
        // `int(MediaDataSize * 8 / (Duration / TimeScale) + 0.5)` followed
        // by `ConvertBitrate`. `Duration` reaches this layer in its unrounded
        // ValueConv form, so its displayed `29.05 s` form is never reused.
        ("QuickTime", "AvgBitrate") => {
            let (size, duration) = (f(get(i, 0))?, f(get(i, 1))?);
            if duration <= 0.0 {
                return None;
            }
            let bitrate = (size * 8.0 / duration + 0.5).floor();
            Computed::new(bitrate.to_string(), convert_bitrate(bitrate))
        }

        // require: ImageWidth, ImageHeight
        // desire:  ExifImageWidth, ExifImageHeight, RawImageCroppedSize
        // ValueConv picks Exif dimensions only for a few TIFF-based RAW types;
        // we do not track TIFF_TYPE, so we take the required pair, which is
        // what ExifTool does for every other format.
        // PrintConv: `$val =~ tr/ /x/`
        //
        // Exif.pm:4747-4766:
        //   ImageSize => {
        //       Require => { 0 => 'ImageWidth', 1 => 'ImageHeight' },
        //       Desire  => {
        //           2 => 'ExifImageWidth', 3 => 'ExifImageHeight',
        //           4 => 'RawImageCroppedSize', # (FujiFilm RAF images)
        //       },
        //       ValueConv => q{
        //           return $val[4] if $val[4];
        //           return "$val[2] $val[3]" if $val[2] and $val[3] and
        //                   $$self{TIFF_TYPE} =~ /^(CR2|Canon 1D RAW|IIQ|EIP)$/;
        //           return "$val[0] $val[1]" if IsFloat($val[0]) and IsFloat($val[1]);
        //           return undef;
        //       },
        //       PrintConv => '$val =~ tr/ /x/; $val',
        //   },
        //
        // `return $val[4] if $val[4]` is checked FIRST, before the required
        // ImageWidth/ImageHeight pair is even consulted: a FujiFilm RAF's
        // sensor read includes border pixels ImageWidth/Height do not
        // exclude, so RawImageCroppedSize (0x0111, FujiFilm.pm:1289) must
        // win outright when present. The RAF parser
        // (src/parsers/raw/raf_parser.rs) already applies FujiFilm.pm's own
        // `tr/ /x/` PrintConv when it emits `RAF:RawImageCroppedSize`, so
        // val[4] arrives pre-joined ("4256x1424"); this ImageSize PrintConv's
        // `tr/ /x/` is then a no-op, and the same string works for `.value`
        // because Megapixels' `/\d+/g` extraction does not care which
        // separator it crosses.
        ("Exif", "ImageSize") => {
            if let Some(v4) = get(i, 4) {
                // Perl truthiness: "" and "0" are false, everything else
                // (including "0.0") is true.
                if !v4.is_empty() && v4 != "0" {
                    return Computed::new(v4.to_string(), v4.to_string());
                }
            }
            let (w, h) = (f(get(i, 0))?, f(get(i, 1))?);
            // ValueConv yields "W H"; PrintConv is `$val =~ tr/ /x/`.
            Computed::new(
                format!("{} {}", w as i64, h as i64),
                format!("{}x{}", w as i64, h as i64),
            )
        }

        // require: ImageSize
        // ValueConv: `my @d = ($val =~ /\d+/g); $d[0] * $d[1] / 1000000`
        ("Exif", "Megapixels") => {
            let s = get(i, 0)?;
            let mut nums = s
                .split(|c: char| !c.is_ascii_digit())
                .filter(|t| !t.is_empty())
                .filter_map(|t| t.parse::<f64>().ok());
            let (w, h) = (nums.next()?, nums.next()?);
            let mp = w * h / 1_000_000.0;
            Computed::new(mp.to_string(), fmt_megapixels(mp))
        }

        // desire: ExposureTime, ShutterSpeedValue, BulbDuration
        // ValueConv: `($val[2] and $val[2]>0) ? $val[2]
        //             : (defined($val[0]) ? $val[0] : $val[1])`
        ("Exif", "ShutterSpeed") => {
            let v = match f(get(i, 2)) {
                Some(b) if b > 0.0 => b,
                _ => f(get(i, 0)).or_else(|| f(get(i, 1)))?,
            };
            Computed::new(v.to_string(), print_exposure_time(v))
        }

        // desire: FNumber, ApertureValue
        // ValueConv: `$val[0] || $val[1]`
        ("Exif", "Aperture") => {
            let v = f(get(i, 0))
                .filter(|v| *v != 0.0)
                .or_else(|| f(get(i, 1)))?;
            Computed::new(v.to_string(), print_fnumber(v))
        }

        // `("Exif", "LensID")` -- BOTH Exif rows that produce the
        // `Composite:LensID` Name (the `Require => 'LensType'` primary,
        // Exif.pm:5303-5360, and the `LensID-2` LensModel/Lens fallback,
        // Exif.pm:5362-5385) are dispatched by [`super::apply`] to
        // [`super::lens_id`] instead of arriving here.
        //
        // They are the one pair in the whole Composite table whose ExifTool
        // conversion is not a function of its positional `$val[N]` inputs: the
        // primary's PrintConv is handed `$self` and immediately reads
        // `$$self{TAG_INFO}{LensType}{PrintConv}` (Exif.pm:5326) to find out
        // *which manufacturer's* lookup produced the string it was given.
        // `compute`'s signature has no way to carry that, which is why the arm
        // lives elsewhere rather than here.

        // require: FocalLength; desire: ScaleFactor35efl
        // ValueConv: `ToFloat(@val); ($val[0] || 0) * ($val[1] || 1)`
        // PrintConv: `$val[1] ? "%.1f mm (35 mm equivalent: %.1f mm)" : "%.1f mm"`
        //
        // Left on the raw doubles for the same reason as HyperfocalDistance:
        // this ValueConv returns a product, not a `join`ed string, so its
        // PrintConv is handed a Perl NV and rounds at full precision.
        ("Exif", "FocalLength35efl") => {
            let fl = f(get(i, 0))?;
            match f(get(i, 1)) {
                Some(sf) if sf != 0.0 => Computed::new(
                    (fl * sf).to_string(),
                    format!("{fl:.1} mm (35 mm equivalent: {:.1} mm)", fl * sf),
                ),
                _ => Computed::new(fl.to_string(), format!("{fl:.1} mm")),
            }
        }

        // require: ScaleFactor35efl
        // ValueConv: `sqrt(24*24+36*36) / ($val * 1440)`
        ("Exif", "CircleOfConfusion") => {
            let sf = f(get(i, 0))?;
            if sf == 0.0 {
                return None;
            }
            let coc = (24.0f64 * 24.0 + 36.0 * 36.0).sqrt() / (sf * 1440.0);
            Computed::new(coc.to_string(), format!("{coc:.3} mm"))
        }

        // require: FocalLength, Aperture, CircleOfConfusion
        // ValueConv: `return 'inf' unless $val[1] and $val[2];
        //             $val[0]*$val[0] / ($val[1] * $val[2] * 1000)`
        // PrintConv: `sprintf("%.2f m", $val)`
        //
        // Deliberately *not* routed through `perl_join`: that ValueConv
        // returns a bare Perl NV, so `sprintf` formats the unrounded double
        // and there is no %.15g round trip to reproduce. Verified against the
        // pinned tree with a UserDefined composite pair over one value --
        // 2.8350000000000004 prints `2.84` when the ValueConv returns the
        // number and `2.83` when it returns `join(" ", $number)` -- which also
        // shows ExifTool adds no stringification of its own. Adding the round
        // trip here would manufacture the divergence, not remove it.
        ("Exif", "HyperfocalDistance") => {
            let fl = f(get(i, 0))?;
            let (ap, coc) = (f(get(i, 1))?, f(get(i, 2))?);
            if ap == 0.0 || coc == 0.0 {
                return Computed::same("inf");
            }
            let hd = fl * fl / (ap * coc * 1000.0);
            Computed::new(hd.to_string(), format!("{hd:.2} m"))
        }

        // require: FocalLength, Aperture, CircleOfConfusion
        // desire:  FocusDistance, SubjectDistance, ObjectDistance,
        //          ApproximateFocusDistance, FocusDistanceLower,
        //          FocusDistanceUpper
        //
        // Source: ExifTool lib/Image/ExifTool/Exif.pm, Composite::DOF
        // (13.30 lines 4761-4802):
        //   my ($d, $f) = ($val[3], $val[0]);
        //   if (defined $d) {
        //       $d or $d = 1e10;    # (use large number for infinity)
        //   } else {
        //       $d = $val[4] || $val[5] || $val[6];
        //       unless (defined $d) {
        //           return undef unless defined $val[7] and defined $val[8];
        //           $d = ($val[7] + $val[8]) / 2;
        //       }
        //   }
        //   return 0 unless $f and $val[2];
        //   my $t = $val[1] * $val[2] * ($d * 1000 - $f) / ($f * $f);
        //   my @v = ($d / (1 + $t), $d / (1 - $t));
        //   $v[1] < 0 and $v[1] = 0; # 0 means 'inf'
        //
        // Its PrintConv uses three decimals only for a positive DOF below
        // 0.02 m, and renders a zero far limit as infinity. Those boundaries
        // are compatibility behaviour, not presentation choices.
        ("Exif", "DOF") => {
            let (fl, ap, coc) = (f(get(i, 0))?, f(get(i, 1))?, f(get(i, 2))?);
            if fl == 0.0 || coc == 0.0 {
                return Computed::same("0");
            }

            let distance = match f(get(i, 3)) {
                // ExifTool represents an explicitly reported zero focus
                // distance as infinity for this calculation.
                Some(0.0) => 1e10,
                Some(d) => d,
                None => f(get(i, 4))
                    .filter(|d| *d != 0.0)
                    .or_else(|| f(get(i, 5)).filter(|d| *d != 0.0))
                    // The last `||` operand is returned even when it is zero.
                    .or_else(|| f(get(i, 6)))
                    .or_else(|| Some((f(get(i, 7))? + f(get(i, 8))?) / 2.0))?,
            };

            let t = ap * coc * (distance * 1000.0 - fl) / (fl * fl);
            let near = distance / (1.0 + t);
            let mut far = distance / (1.0 - t);
            if far < 0.0 {
                far = 0.0;
            }
            // `return join(' ',@v)`, then `my @v = split ' ', $val` in the
            // PrintConv -- so every printed digit below, and the subtraction
            // that picks the format, run on %.15g text read back as doubles.
            // See `perl_join`.
            let (value, v) = perl_join(&[near, far]);
            let (near, far) = (v[0], v[1]);
            if far == 0.0 {
                return Computed::new(value, format!("inf ({near:.2} m - inf)"));
            }

            // `my $dof = $v[1] - $v[0];` -- ExifTool subtracts the re-parsed
            // values, so this must too: differencing the unrounded doubles
            // instead can land the result on the other side of the 0.02 m
            // cutoff that selects three decimals over two.
            let dof = far - near;
            if dof > 0.0 && dof < 0.02 {
                Computed::new(value, format!("{dof:.3} m ({near:.3} - {far:.3} m)"))
            } else {
                Computed::new(value, format!("{dof:.2} m ({near:.2} - {far:.2} m)"))
            }
        }

        // require: Aperture, ShutterSpeed, ISO
        // Image::ExifTool::Exif::CalculateLV:
        //   `log($aperture**2 / $shutter * 100 / $iso) / log(2)`
        ("Exif", "LightValue") => {
            let (ap, ss, iso) = (f(get(i, 0))?, f(get(i, 1))?, f(get(i, 2))?);
            if ss <= 0.0 || iso <= 0.0 || ap <= 0.0 {
                return None;
            }
            let lv = ((ap * ap) / ss * 100.0 / iso).log2();
            Computed::new(lv.to_string(), format!("{lv:.1}"))
        }

        // require: FocalLength, ScaleFactor35efl; desire: FocusDistance
        //
        // ExifTool:
        //   return undef unless $val[0] and $val[1];
        //   my $corr = 1;
        //   if ($val[2]) { my $d = 1000*$val[2] - $val[0];
        //                  $corr += $val[0]/$d if $d > 0; }
        //   my $fd2 = atan2(36, 2*$val[0]*$val[1]*$corr);
        //   my @fov = ( $fd2 * 360 / 3.14159 );
        //   push @fov, 2*$val[2]*sin($fd2)/cos($fd2)
        //       if $val[2] and $val[2] > 0 and $val[2] < 10000;
        //
        // The literal 3.14159 is ExifTool's, not std::f64::consts::PI. It is
        // reproduced exactly: substituting the more accurate constant shifts
        // the result in the first decimal place, which is where the printed
        // value rounds, so "more correct" here would read as a mismatch.
        ("Exif", "FOV") => {
            let (fl, sf) = (f(get(i, 0))?, f(get(i, 1))?);
            if fl == 0.0 || sf == 0.0 {
                return None;
            }
            let focus = f(get(i, 2)).unwrap_or(0.0);
            let mut corr = 1.0f64;
            if focus != 0.0 {
                let d = 1000.0 * focus - fl;
                if d > 0.0 {
                    corr += fl / d;
                }
            }
            let fd2 = (36.0f64).atan2(2.0 * fl * sf * corr);
            let deg = fd2 * 360.0 / 3.14159;
            // `return join(' ', @fov)` -- so the PrintConv's `split(' ',$val)`
            // reads back %.15g text, not the doubles computed above. See
            // `perl_join`: on a boundary the two round opposite ways.
            let (value, print) = if focus > 0.0 && focus < 10000.0 {
                let dist = 2.0 * focus * fd2.sin() / fd2.cos();
                let (value, v) = perl_join(&[deg, dist]);
                // `$str .= sprintf(" (%.2f m)", $v[1]) if $v[1];` -- the
                // distance is appended only when it is non-zero.
                let print = if v[1] == 0.0 {
                    format!("{:.1} deg", v[0])
                } else {
                    format!("{:.1} deg ({:.2} m)", v[0], v[1])
                };
                (value, print)
            } else {
                let (value, v) = perl_join(&[deg]);
                (value, format!("{:.1} deg", v[0]))
            };
            Computed::new(value, print)
        }

        // desire, in ExifTool's declared order (indices match its `shift`s):
        //   0 FocalLength           1 FocalLengthIn35mmFormat  2 DigitalZoom
        //   3 FocalPlaneDiagonal    4 SensorSize               5 FocalPlaneXSize
        //   6 FocalPlaneYSize       7 FocalPlaneResolutionUnit 8 FocalPlaneXResolution
        //   9 FocalPlaneYResolution 10/11 ExifImage{Width,Height}
        //   12/13 CanonImage{Width,Height}  14/15 Image{Width,Height}
        //
        // Port of Image::ExifTool::Exif::CalcScaleFactor35efl. Worth the care:
        // it gates FocalLength35efl, CircleOfConfusion, HyperfocalDistance, FOV
        // and DOF, so one function moves six tags on every camera file.
        ("Exif", "ScaleFactor35efl") => {
            // Easiest case: the camera reported both focal lengths.
            if let (Some(focal), Some(foc35)) = (f(get(i, 0)), f(get(i, 1))) {
                if focal != 0.0 && foc35 != 0.0 {
                    let sf = foc35 / focal;
                    return Computed::new(sf.to_string(), format!("{sf:.1}"));
                }
            }

            let digz = f(get(i, 2)).filter(|v| *v != 0.0).unwrap_or(1.0);
            let mut diag = f(get(i, 3)).filter(|d| *d > 0.0);

            // ExifTool overrides FocalPlaneDiagonal with the Canon-specific
            // calculation when it succeeds, so this runs before the fallbacks
            // and takes precedence.
            if make.is_some_and(|m| m.eq_ignore_ascii_case("Canon")) {
                if let Some(d) = canon_sensor_diag(get(i, 8), get(i, 9)) {
                    diag = Some(d);
                }
            }

            if diag.is_none() {
                // `SensorSize` is a string like "6.16 x 4.62 mm"; ExifTool
                // pairs its trailing number with the scalar sensor height.
                let sens = f(get(i, 4));
                let sens_y = get(i, 4).and_then(|s| {
                    s.rsplit(|c: char| !(c.is_ascii_digit() || c == '.'))
                        .find(|t| !t.is_empty())
                        .and_then(|t| t.parse::<f64>().ok())
                });
                match (sens, sens_y) {
                    (Some(s), Some(y)) if s > 0.0 && y > 0.0 => {
                        diag = Some((s * s + y * y).sqrt());
                    }
                    _ => {
                        // FocalPlaneX/YSize is unreliable, so ExifTool accepts
                        // it only when the aspect ratio looks like 4:3 or 3:2.
                        if let (Some(x), Some(y)) = (f(get(i, 5)), f(get(i, 6))) {
                            if x > 0.0 && y > 0.0 {
                                let a = x / y;
                                if (a - 1.3333).abs() < 0.1 || (a - 1.5).abs() < 0.1 {
                                    diag = Some((x * x + y * y).sqrt());
                                }
                            }
                        }
                    }
                }

                if diag.is_none() {
                    // Derive the focal-plane size from resolution. Unit codes
                    // are EXIF's; anything unrecognised means inches.
                    let units = match get(i, 7).map(str::trim) {
                        Some("3") | Some("cm") => 10.0,
                        Some("4") | Some("mm") => 1.0,
                        Some("5") | Some("um") => 0.001,
                        _ => 25.4,
                    };
                    let x_res = f(get(i, 8)).filter(|v| *v != 0.0)?;
                    let y_res = f(get(i, 9)).filter(|v| *v != 0.0).unwrap_or(x_res);

                    // Try each width/height pair, taking the first with a
                    // plausible aspect ratio.
                    let mut found = None;
                    for (wi, hi) in [(10, 11), (12, 13), (14, 15)] {
                        let (Some(w), Some(h)) = (f(get(i, wi)), f(get(i, hi))) else {
                            continue;
                        };
                        if w == 0.0 || h == 0.0 {
                            continue;
                        }
                        let a = w / h;
                        if a > 0.5 && a < 2.0 {
                            found = Some((w * units / x_res, h * units / y_res));
                            break;
                        }
                    }
                    let (w, h) = found?;
                    let d = (w * w + h * h).sqrt();
                    // Reject implausible sensor diagonals rather than emit a
                    // scale factor that would poison five dependent tags.
                    if !(d > 1.0 && d < 100.0) {
                        return None;
                    }
                    diag = Some(d);
                }
            }

            let diag = diag.filter(|d| *d > 0.0)?;
            let sf = (36.0f64 * 36.0 + 24.0 * 24.0).sqrt() * digz / diag;
            Computed::new(sf.to_string(), format!("{sf:.1}"))
        }

        // Apple.pm Composite::RunTimeSincePowerUp (Apple.pm:348-357):
        //   require:   0) Apple:RunTimeValue, 1) Apple:RunTimeScale
        //   ValueConv: `$val[1] ? $val[0] / $val[1] : undef`
        //   PrintConv: `ConvertDuration($val)`
        ("Apple", "RunTimeSincePowerUp") => {
            let (value, scale) = (f(get(i, 0))?, f(get(i, 1))?);
            if scale == 0.0 {
                return None;
            }
            let seconds = value / scale;
            Computed::new(seconds.to_string(), convert_duration(seconds))
        }

        // Canon.pm Composite::DriveMode:
        //   `$val[0] ? 0 : ($val[1] ? 1 : 2)`
        ("Canon", "DriveMode") => {
            let continuous = !matches!(get(i, 0), Some("Single") | Some("0"));
            let self_timer = !matches!(get(i, 1), Some("Off") | Some("0"));
            let (value, print) = if continuous {
                (0, "Continuous Shooting")
            } else if self_timer {
                (1, "Self-timer Operation")
            } else {
                (2, "Single-frame Shooting")
            };
            Computed::new(value.to_string(), print)
        }

        // Canon.pm Composite::Lens and `PrintFocalRange(@val)`.
        ("Canon", "Lens") => {
            let (short, long) = (f(get(i, 0))?, f(get(i, 1))?);
            Computed::new(short.to_string(), focal_range(short, long, 1.0))
        }

        // Canon.pm Composite::Lens35efl. Reconstruct `$prt[3]` from the same
        // focal-range inputs because Composite dependencies otherwise carry
        // their full-precision ValueConv form.
        ("Canon", "Lens35efl") => {
            let (short, long) = (f(get(i, 0))?, f(get(i, 1))?);
            let scale = f(get(i, 2)).filter(|scale| *scale != 0.0);
            let value = short * scale.unwrap_or(1.0);
            let mut print = focal_range(short, long, 1.0);
            if let Some(scale) = scale {
                print.push_str(" (35 mm equivalent: ");
                print.push_str(&focal_range(short, long, scale));
                print.push(')');
            }
            Computed::new(value.to_string(), print)
        }

        // Canon.pm Composite::ShootingMode:
        //   `$val[0] ? (($val[0] eq "4" and $val[2]) ? 7 : $val[0])
        //            : $val[1] + 10`
        // and print Bulb specially, otherwise reuse the selected input's
        // PrintConv form.
        ("Canon", "ShootingMode") => {
            let exposure_print = get(i, 0)?;
            let exposure = canon_exposure_mode(exposure_print)?;
            if exposure != 0 {
                if exposure == 4 && perl_truthy(get(i, 2)) {
                    Computed::new("7", "Bulb")
                } else {
                    Computed::new(exposure.to_string(), exposure_print)
                }
            } else {
                let easy_print = get(i, 1)?;
                let easy = canon_easy_mode(easy_print)?;
                Computed::new((easy + 10).to_string(), easy_print)
            }
        }

        // Canon.pm Composite::ISO: use numerical CameraISO, otherwise derive
        // BaseISO * AutoISO / 100. PrintConv is `sprintf("%.0f",$val)`.
        ("Canon", "ISO") => {
            let value = match get(i, 0).map(str::trim) {
                Some(camera_iso)
                    if !camera_iso.is_empty()
                        && camera_iso != "0"
                        && camera_iso.bytes().all(|byte| byte.is_ascii_digit()) =>
                {
                    camera_iso.parse::<f64>().ok()?
                }
                _ => {
                    let base = f(get(i, 1)).filter(|value| *value != 0.0)?;
                    let auto = f(get(i, 2)).filter(|value| *value != 0.0)?;
                    base * auto / 100.0
                }
            };
            Computed::new(value.to_string(), format!("{value:.0}"))
        }

        // Canon.pm Composite::DigitalZoom. The raw mode must be `Other` (3),
        // with both widths non-zero; the ratio is target/source.
        ("Canon", "DigitalZoom") => {
            if !matches!(get(i, 2), Some("Other") | Some("3")) {
                return None;
            }
            let (source, target) = (f(get(i, 0))?, f(get(i, 1))?);
            if source == 0.0 || target == 0.0 {
                return None;
            }
            let value = target / source;
            Computed::new(value.to_string(), format!("{value:.2}x"))
        }

        // Canon FlashBits has a bitmask PrintConv. Its visible labels retain
        // exactly the facts these composites test: `(none)` is zero and an
        // `External` item represents bit 14.
        ("Canon", "FlashType") => {
            let bits = get(i, 0)?;
            if bits == "(none)" || bits == "0" {
                return None;
            }
            let external = bits.split(',').any(|item| item.trim() == "External");
            Computed::new(
                if external { "1" } else { "0" },
                if external {
                    "External"
                } else {
                    "Built-In Flash"
                },
            )
        }

        ("Canon", "RedEyeReduction") => {
            let bits = get(i, 1)?;
            if bits == "(none)" || bits == "0" {
                return None;
            }
            let enabled = matches!(canon_flash_mode(get(i, 0)?)?, 3 | 4 | 6);
            Computed::new(
                if enabled { "1" } else { "0" },
                if enabled { "On" } else { "Off" },
            )
        }

        ("Canon", "ConditionalFEC") => {
            let bits = get(i, 1)?;
            if bits == "(none)" || bits == "0" {
                None
            } else {
                Computed::same(get(i, 0)?)
            }
        }

        ("Canon", "ShutterCurtainHack") => {
            let bits = get(i, 1)?;
            if bits == "(none)" || bits == "0" {
                return None;
            }
            match get(i, 0) {
                Some("2nd-curtain sync") | Some("1") => Computed::new("1", "2nd-curtain sync"),
                Some(_) | None => Computed::new("0", "1st-curtain sync"),
            }
        }

        // Canon.pm Composite::FileNumber, including its 9999 wrap behavior.
        ("Canon", "FileNumber") => {
            let (mut directory, mut file) = (f(get(i, 0))? as i64, f(get(i, 1))? as i64);
            if file == 10_000 {
                file = 1;
                directory += 1;
            }
            let value = format!("{directory:03}{file:04}");
            Computed::new(value, format!("{directory:03}-{file:04}"))
        }

        // Canon.pm Composite::WB_RGGBLevels:
        //   `$val[1] ? $val[1] : $val[($val[0] || 0) + 2]`
        // The required WhiteBalance reaches us in PrintConv form, so reverse
        // the exact Canon enum before selecting its positional desired input.
        ("Canon", "WB_RGGBLevels") => {
            if let Some(as_shot) = get(i, 1).filter(|v| !v.is_empty() && *v != "0") {
                return Computed::same(as_shot);
            }
            let white_balance = match get(i, 0)? {
                "Auto" => 0,
                "Daylight" => 1,
                "Cloudy" => 2,
                "Tungsten" => 3,
                "Fluorescent" => 4,
                "Flash" => 5,
                "Custom" => 6,
                "Black & White" => 7,
                "Shade" => 8,
                "Manual Temperature (Kelvin)" => 9,
                _ => return None,
            };
            Computed::same(get(i, white_balance + 2)?)
        }

        // Nikon.pm:13240-13246 Composite::AutoFocus:
        //   Require   => { 0 => 'Nikon:FocusMode' }
        //   ValueConv => '($val[0] =~ /^Manual/i) ? 0 : 1'
        //   PrintConv => \%offOn            # ( 0 => 'Off', 1 => 'On' )
        //
        // The sole input is Nikon MakerNotes 0x0007, a `Writable => 'string'`
        // tag whose `RawConv` only stashes the value (Nikon.pm:1816-1820), so
        // `$val[0]` is the string as the camera wrote it -- typically all caps,
        // "MANUAL". Nikon::Main's table-wide `PRINT_CONV => \&FormatString`
        // (Nikon.pm:1784, 13526) is what lowercases it to "Manual" for display,
        // and that is the form oxidex stores under `Nikon:FocusMode`. The `/i`
        // makes the two indistinguishable here, so this reads either.
        //
        // Only "Manual" is Off; every autofocus mode the corpus carries
        // (AF-S, AF-C, AF-A, AF-F, AF-P) falls through to On, which is exactly
        // what a negated match on one prefix means.
        ("Nikon", "AutoFocus") => {
            let manual = get(i, 0)?
                .get(..6)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case("Manual"));
            Computed::new(
                if manual { "0" } else { "1" },
                if manual { "Off" } else { "On" },
            )
        }

        // Exif.pm `RedBlueBalance`, followed by
        // `int($val * 1e6 + 0.5) * 1e-6`.
        ("Exif", "RedBalance" | "BlueBalance") => {
            let value = red_blue_balance(i, name == "BlueBalance")?;
            let millionths = (value * 1e6 + 0.5) as i64;
            let absolute = millionths.unsigned_abs();
            let sign = if millionths < 0 { "-" } else { "" };
            let mut printed = format!("{sign}{}.{:06}", absolute / 1_000_000, absolute % 1_000_000);
            while printed.ends_with('0') {
                printed.pop();
            }
            if printed.ends_with('.') {
                printed.pop();
            }
            Computed::new(value.to_string(), printed)
        }

        ("Exif", "SubSecCreateDate" | "SubSecDateTimeOriginal" | "SubSecModifyDate") => {
            Computed::same(subsec_date_time(i)?)
        }

        // Exif.pm:5157-5163 Composite::PreviewImageSize:
        //   require:   0) PreviewImageWidth, 1) PreviewImageHeight
        //   ValueConv: `"$val[0]x$val[1]"`
        // No PrintConv at all, so the value and display forms are the same
        // string -- the `x` is part of the ValueConv here, unlike
        // Composite::ImageSize where it is the PrintConv's `tr/ /x/`.
        ("Exif", "PreviewImageSize") => Computed::same(format!("{}x{}", get(i, 0)?, get(i, 1)?)),

        // Exif.pm:5221-5234 Composite::CFAPattern:
        //   require:   0) CFARepeatPatternDim, 1) CFAPattern2
        //   ValueConv: q{
        //       my @a = split / /, $val[0];
        //       my @b = split / /, $val[1];
        //       return '?' unless @a==2 and @b==$a[0]*$a[1];
        //       return "$a[0] $a[1] @b";
        //   }
        //   PrintConv: `Image::ExifTool::Exif::PrintCFAPattern($val)`
        ("Exif", "CFAPattern") => {
            let dim: Vec<&str> = get(i, 0)?.split(' ').collect();
            let pattern: Vec<&str> = get(i, 1)?.split(' ').collect();
            let expected = dim
                .first()
                .and_then(|v| v.parse::<usize>().ok())
                .zip(dim.get(1).and_then(|v| v.parse::<usize>().ok()))
                .map(|(rows, cols)| rows * cols);
            if dim.len() != 2 || expected != Some(pattern.len()) {
                // ExifTool's own literal `'?'` -- an emitted tag, so it is
                // reproduced rather than dropped.
                return Computed::new("?", print_cfa_pattern("?"));
            }
            let value = format!("{} {} {}", dim[0], dim[1], pattern.join(" "));
            let print = print_cfa_pattern(&value);
            Computed::new(value, print)
        }

        // GPS.pm:406-432 Composite::GPSAltitude -- see `gps_altitude`.
        ("GPS", "GPSAltitude") => gps_altitude(i),

        // Kodak.pm:3023-3030 Composite::DateCreated:
        //   require:   0) Kodak:YearCreated, 1) Kodak:MonthDayCreated
        //   ValueConv: `"$val[0]:$val[1]"` (no PrintConv)
        ("Kodak", "DateCreated") => Computed::same(format!("{}:{}", get(i, 0)?, get(i, 1)?)),

        // ISO.pm:119-126 Composite::VolumeSize:
        //   require:   0) ISO:VolumeBlockCount, 1) ISO:VolumeBlockSize
        //   ValueConv: `$val[0] * $val[1]`
        //   PrintConv: `\&Image::ExifTool::ConvertFileSize`
        //
        // `core::value_formatter::format_file_size` is the existing port of
        // ConvertFileSize -- it is what the ISO parser itself called when it
        // emitted this under the wrong group (see `parsers::archive::iso`,
        // whose own comment already said "VolumeSize is a Composite tag ...
        // not a field on the descriptor").
        ("ISO", "VolumeSize") => {
            let bytes = f(get(i, 0))? * f(get(i, 1))?;
            Computed::new(
                crate::exiftool_tables::exprs::perl_num(bytes),
                crate::core::value_formatter::format_file_size(bytes as u64),
            )
        }

        // Nikon.pm:13215-13222 Composite::LensSpec:
        //   require:   0) Nikon:Lens, 1) Nikon:LensType
        //   ValueConv: `"$val[0] $val[1]"`
        //   PrintConv: `"$prt[0] $prt[1]"`
        //
        // oxidex's Nikon maker-note parser stores `Lens` and `LensType` in
        // their PrintConv forms only ("70mm f/2.8", "G"), so what arrives here
        // is ExifTool's `@prt`, not its `@val`. That makes the emitted tag --
        // the PrintConv branch, which is the only form the CLI and the
        // comparison harness ever see -- exact, and leaves the internal
        // `.value` carrying the same string instead of ExifTool's
        // `"70 70 2.8 2.8 14"`. No Composite requires `LensSpec`, so that
        // value form is never read back by anything; it is recorded here so
        // the limitation is stated rather than hidden.
        ("Nikon", "LensSpec") => Computed::same(format!("{} {}", get(i, 0)?, get(i, 1)?)),

        // Nikon.pm:13247-13262 Composite::PhaseDetectAF:
        //   require:   0) Nikon:FocusPointSchema, 1) Nikon:AFDetectionMethod
        //   ValueConv: `(($val[1]) == 0) ? ($val[0]) : 0`
        //   PrintConv: { 0 => 'Off', 1 => 'On (51-point)', 2 => 'On (11-point)',
        //                3 => 'On (39-point)', 7 => 'On (153-point)',
        //                9 => 'On (105-point)' }
        ("Nikon", "PhaseDetectAF") => {
            let schema = nikon_focus_point_schema(get(i, 0)?)?;
            let method = nikon_af_detection_method(get(i, 1)?)?;
            let value = if method == 0 { schema } else { 0 };
            let print = match value {
                0 => "Off".to_string(),
                1 => "On (51-point)".to_string(),
                2 => "On (11-point)".to_string(),
                3 => "On (39-point)".to_string(),
                7 => "On (153-point)".to_string(),
                9 => "On (105-point)".to_string(),
                other => format!("Unknown ({other})"),
            };
            Computed::new(value.to_string(), print)
        }

        // Nikon.pm:13263-13270 Composite::ContrastDetectAF:
        //   require:   0) Nikon:FocusMode, 1) Nikon:AFDetectionMethod
        //   ValueConv: `(($val[0] !~ /^Manual/i) and ($val[1] == 1)) ? 1 : 0`
        //   PrintConv: `%offOn` -- { 0 => 'Off', 1 => 'On' }
        ("Nikon", "ContrastDetectAF") => {
            let focus_mode = get(i, 0)?;
            let method = nikon_af_detection_method(get(i, 1)?)?;
            let value = i64::from(
                !focus_mode
                    .trim_start()
                    .to_ascii_lowercase()
                    .starts_with("manual")
                    && method == 1,
            );
            Computed::new(
                value.to_string(),
                if value == 1 { "On" } else { "Off" }.to_string(),
            )
        }

        // Olympus.pm:4283-4300 Composite::ExtenderStatus:
        //   require:   0) Olympus:Extender, 1) Olympus:LensType,
        //              2) MaxApertureValue
        //   ValueConv: `Image::ExifTool::Olympus::ExtenderStatus($val[0],$prt[1],$val[2])`
        //   PrintConv: { 0 => 'Not attached', 1 => 'Attached', 2 => 'Removed' }
        ("Olympus", "ExtenderStatus") => {
            let extender = olympus_extender_value_conv(get(i, 0)?)?;
            let status = olympus_extender_status(extender, get(i, 1)?, f(get(i, 2))?)?;
            let print = match status {
                0 => "Not attached",
                1 => "Attached",
                2 => "Removed",
                _ => return None,
            };
            Computed::new(status.to_string(), print)
        }

        // PostScript.pm:132-138 Composite::ImageWidth,
        // PostScript.pm:139-145 Composite::ImageHeight:
        //   desire:    0) Main:PostScript:ImageData, 1) PostScript:BoundingBox
        //   ValueConv: `Image::ExifTool::PostScript::ImageSize(\@val, 0|1)`
        ("PostScript", "ImageWidth" | "ImageHeight") => {
            let size = postscript_image_size(i, name == "ImageHeight")?;
            Computed::same(size.to_string())
        }

        // Sony.pm:10895-10903 Composite::FocusDistance:
        //   require:   0) Sony:FocusPosition, 1) FocalLength
        //   ValueConv: `$val >= 128 ? "inf" : $val * $val[1] / 1000`
        //   PrintConv: `$val eq "inf" ? $val : "$val m"`
        // `$val` is `$val[0]` for a Composite (ExifTool.pm:3611-3612).
        ("Sony", "FocusDistance") => {
            let position = f(get(i, 0))?;
            if position >= 128.0 {
                return Computed::same("inf");
            }
            let metres = position * f(get(i, 1))? / 1000.0;
            let value = crate::exiftool_tables::exprs::perl_num(metres);
            Computed::new(value.clone(), format!("{value} m"))
        }

        // Sony.pm:10904-10928 Composite::FocusDistance2:
        //   require:   0) Sony:FocusPosition2, 1) FocalLengthIn35mmFormat
        //   ValueConv: q{
        //       return undef unless $val;
        //       return 'inf' if $val >= 255;
        //       return (2**($val/16-5) + 1) * $val[1] / 1000;
        //   }
        //   PrintConv: `$val eq "inf" ? $val : sprintf("%.4g m", $val)`
        ("Sony", "FocusDistance2") => {
            let position = f(get(i, 0))?;
            if position == 0.0 {
                return None;
            }
            if position >= 255.0 {
                return Computed::same("inf");
            }
            let metres = (2f64.powf(position / 16.0 - 5.0) + 1.0) * f(get(i, 1))? / 1000.0;
            Computed::new(
                crate::exiftool_tables::exprs::perl_num(metres),
                format!("{} m", format_significant(metres, 4)),
            )
        }

        // XMP.pm:2808-2842 Composite::Flash:
        //   desire:    0) XMP:FlashFired, 1) XMP:FlashReturn, 2) XMP:FlashMode,
        //              3) XMP:FlashFunction, 4) XMP:FlashRedEyeMode, 5) XMP:Flash
        //   ValueConv: `((lc $val[0] eq 'true') ? 0x01 : 0) | (($val[1]||0) << 1) |
        //               (($val[2]||0) << 3) | ((lc $val[3] eq 'true') ? 0x20 : 0) |
        //               ((lc $val[4] eq 'true') ? 0x40 : 0)`
        //   PrintConv: `%Image::ExifTool::Exif::flash`, PrintHex => 1
        //
        // The `ref $val[5] eq 'HASH'` branch (a structured `XMP:Flash`) is not
        // reproduced: oxidex's XMP parser flattens a Flash struct into the same
        // five scalar fields this reads at 0-4, and there is no `Struct` value
        // for `value_string` to hand over here (`super::value_string` returns
        // `None` for `TagValue::Struct`), so index 5 never arrives as a hash.
        ("XMP", "Flash") => {
            let boolean =
                |n: usize| get(i, n).is_some_and(|v| v.trim().eq_ignore_ascii_case("true")) as i64;
            let field = |n: usize, mode: bool| match get(i, n) {
                Some(label) => xmp_flash_field(label, mode),
                None => Some(0),
            };
            let value = boolean(0)
                | (field(1, false)? << 1)
                | (field(2, true)? << 3)
                | (boolean(3) << 5)
                | (boolean(4) << 6);
            Computed::new(
                value.to_string(),
                crate::core::formatters::exif_enums::format_flash(value),
            )
        }

        // XMP.pm:2748-2788 Composite::GPSLatitudeRef / GPSLongitudeRef (and
        // their two GPSDest siblings), all four the same shape:
        //   require:   0) XMP-exif:GPS<Dest>L(at|ong)itude
        //   ValueConv: q{
        //       IsFloat($val[0]) and return $val[0] < 0 ? "S" : "N";
        //       $val[0] =~ /^.*([NS])/;
        //       return $1;
        //   }
        //   PrintConv: { N => 'North', S => 'South' } (E/W for longitude)
        //
        // Split in two rather than written as one four-way alternation
        // because `codegen_composite.py`'s `_COMPUTE_ARM_RE` reads this file
        // line by line: rustfmt breaks a pattern that wide across three lines,
        // and the triage line then reports all four as having no registered
        // computation. The check is precision-limited by design (its own doc
        // comment says so), so keeping each arm on one line is what keeps the
        // "never fire" count honest.
        ("XMP", "GPSLatitudeRef" | "GPSDestLatitudeRef") => xmp_gps_ref(i, false),
        ("XMP", "GPSLongitudeRef" | "GPSDestLongitudeRef") => xmp_gps_ref(i, true),

        // APE.pm:83-92 Composite::Duration:
        //   require:   0) APE:SampleRate, 1) APE:TotalFrames,
        //              2) APE:BlocksPerFrame, 3) APE:FinalFrameBlocks
        //   RawConv:   `($val[0] && $val[1]) ?
        //               (($val[1] - 1) * $val[2] + $val[3]) / $val[0] : undef`
        //   PrintConv: `ConvertDuration($val)`
        ("APE", "Duration") => {
            let (rate, frames) = (f(get(i, 0))?, f(get(i, 1))?);
            if rate == 0.0 || frames == 0.0 {
                return None;
            }
            let seconds = ((frames - 1.0) * f(get(i, 2))? + f(get(i, 3))?) / rate;
            Computed::new(
                crate::exiftool_tables::exprs::perl_num(seconds),
                convert_duration(seconds),
            )
        }

        // FLAC.pm:137-145 Composite::Duration:
        //   require:   0) FLAC:SampleRate, 1) FLAC:TotalSamples
        //   ValueConv: `($val[0] and $val[1]) ? $val[1] / $val[0] : undef`
        //   PrintConv: `ConvertDuration($val)`
        // The same shape as AIFF::Duration above, including the "a zero sample
        // count is as disqualifying as a zero rate" guard -- which is what the
        // one FLAC carrier in the corpus exercises (TotalSamples 0, so both
        // ExifTool and this emit nothing).
        ("FLAC", "Duration") => {
            let (rate, samples) = (f(get(i, 0))?, f(get(i, 1))?);
            if rate == 0.0 || samples == 0.0 {
                return None;
            }
            let seconds = samples / rate;
            Computed::new(
                crate::exiftool_tables::exprs::perl_num(seconds),
                convert_duration(seconds),
            )
        }

        // RIFF.pm:1548-1560 Composite::Duration:
        //   require:   0) RIFF:FrameRate, 1) RIFF:FrameCount
        //   desire:    2) VideoFrameRate, 3) VideoFrameCount
        //   RawConv:   `Image::ExifTool::RIFF::CalcDuration($self, @val)`
        //   PrintConv: `ConvertDuration($val)`
        //
        // RIFF.pm:1645-1666, the head of CalcDuration, is the whole of the
        // computation for a file with no sub-documents:
        //   my $dur1;
        //   $dur1 = $val[1] / $val[0] if $val[0];
        //   if ($val[2] and $val[3]) {
        //       my $dur2 = $val[3] / $val[2];
        //       my $rat = $dur1 / $dur2;
        //       $dur1 = $dur2 if $rat > 1.9 and $rat < 3.1;
        //   }
        //   $totalDuration += $dur1 if defined $dur1;
        //   last unless $subDoc++ < $$et{DOC_COUNT};
        // `$subDoc` starts at 0, so the trailing `last` fires immediately
        // unless DOC_COUNT is non-zero. oxidex extracts no RIFF sub-documents
        // at all, so `VideoFrameRate`/`VideoFrameCount` for a second stream
        // could not be resolved even if they existed -- the summation branch is
        // unreachable here rather than approximated.
        ("RIFF", "Duration") => {
            let rate = f(get(i, 0))?;
            if rate == 0.0 {
                return None;
            }
            let mut seconds = f(get(i, 1))? / rate;
            if let (Some(video_rate), Some(video_count)) = (f(get(i, 2)), f(get(i, 3)))
                && video_rate != 0.0
                && video_count != 0.0
            {
                let alternate = video_count / video_rate;
                let ratio = seconds / alternate;
                if ratio > 1.9 && ratio < 3.1 {
                    seconds = alternate;
                }
            }
            Computed::new(
                crate::exiftool_tables::exprs::perl_num(seconds),
                convert_duration(seconds),
            )
        }

        // GPS.pm Composite::GPSDateTime:
        //   `"$val[0] $val[1]Z"`, followed by ConvertDateTime.
        ("GPS", "GPSDateTime") => Computed::same(format!("{} {}Z", get(i, 0)?, get(i, 1)?)),

        // GPS.pm signed-coordinate ValueConv and default ToDMS PrintConv.
        //
        // GPS.pm:433-440 `GPSDestLatitude` is the same shape as `GPSLatitude`
        // -- `Require => { 0 => 'GPS:GPSDestLatitude', 1 =>
        // 'GPS:GPSDestLatitudeRef' }`, `ValueConv => '$val[1] =~ /^S/i ?
        // -$val[0] : $val[0]'`, `PrintConv => 'ToDMS($self, $val, 1, "N")'` --
        // so it joins the latitude side of this arm, exactly as
        // `GPSDestLongitude` (GPS.pm:441-448) already joins the longitude side.
        ("GPS", "GPSLatitude" | "GPSLongitude" | "GPSDestLatitude" | "GPSDestLongitude") => {
            let latitude = name == "GPSLatitude" || name == "GPSDestLatitude";
            let coordinate = get(i, 0)?;
            if coordinate.is_empty() {
                return Computed::same("");
            }
            let mut value = gps_degrees(coordinate)?;
            let reference = get(i, 1)?.trim();
            let negative = if latitude {
                reference.starts_with(['S', 's'])
            } else {
                reference.starts_with(['W', 'w'])
            };
            if negative {
                value = -value;
            }
            Computed::new(
                value.to_string(),
                gps_print(value, if latitude { 'N' } else { 'E' }),
            )
        }

        // Exif.pm Composite::GPSPosition uses the ValueConv forms separated by
        // a space and the coordinate PrintConv forms separated by `, `.
        ("Exif", "GPSPosition") => {
            let (latitude, longitude) = (f(get(i, 0))?, f(get(i, 1))?);
            if get(i, 0)?.is_empty() && get(i, 1)?.is_empty() {
                return None;
            }
            Computed::new(
                format!("{latitude} {longitude}"),
                format!(
                    "{}, {}",
                    gps_print(latitude, 'N'),
                    gps_print(longitude, 'E')
                ),
            )
        }

        // IPTC.pm's two time composites both concatenate their required date
        // and time, then pass the result through ConvertDateTime.
        ("IPTC", "DateTimeCreated" | "DigitalCreationDateTime") => {
            Computed::same(format!("{} {}", get(i, 0)?, get(i, 1)?))
        }

        // Exif.pm synthesizes DateTimeOriginal only when the independent date
        // and time inputs exist; DateTimeCreated wins when it contains a time.
        ("Exif", "DateTimeOriginal") => {
            let (date, time) = (get(i, 1)?, get(i, 2)?);
            let value = get(i, 0)
                .filter(|value| value.contains(' '))
                .map(str::to_string)
                .unwrap_or_else(|| format!("{date} {time}"));
            Computed::same(value)
        }

        // ID3.pm Composite::DateTimeOriginal.
        ("ID3", "DateTimeOriginal") => {
            if let Some(recording) = get(i, 0).filter(|value| !value.is_empty()) {
                return Computed::same(recording);
            }
            let mut value = get(i, 1)?.to_string();
            let Some(date) = get(i, 2)
                .filter(|date| date.len() == 4 && date.bytes().all(|byte| byte.is_ascii_digit()))
            else {
                return Computed::same(value);
            };
            value.push(':');
            value.push_str(&date[..2]);
            value.push(':');
            value.push_str(&date[2..]);
            if let Some(time) = get(i, 3)
                .filter(|time| time.len() == 4 && time.bytes().all(|byte| byte.is_ascii_digit()))
            {
                value.push(' ');
                value.push_str(&time[..2]);
                value.push(':');
                value.push_str(&time[2..]);
            }
            Computed::same(value)
        }

        // Every hand-written arm above is tried first; only a `(module,
        // name)` pair none of them recognises falls through to the
        // generated $val[N]-compiled arms (see `super::generated_compute`'s
        // own doc comment). A pair present in BOTH would be unreachable
        // here regardless -- codegen_composite.py already refuses to
        // auto-derive one this file already hand-implements -- so this is
        // strictly additive coverage, never a silent override of a
        // hand-verified translation.
        _ => super::generated_compute::compute_generated(module, name, i),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Print form of a composite, which is what ExifTool displays and what
    /// the comparison harness diffs.
    fn c(name: &str, v: &[Option<&str>]) -> Option<String> {
        compute("Exif", name, v, None).map(|c| c.print)
    }

    /// Print form, with a manufacturer in scope.
    fn cm(name: &str, v: &[Option<&str>], make: &str) -> Option<String> {
        compute("Exif", name, v, Some(make)).map(|c| c.print)
    }

    #[test]
    fn image_size_and_megapixels() {
        assert_eq!(
            c("ImageSize", &[Some("4000"), Some("3000")]).as_deref(),
            Some("4000x3000")
        );
        // 12 MP -> one decimal place, per ExifTool's %.*f precision rule.
        assert_eq!(
            c("Megapixels", &[Some("4000x3000")]).as_deref(),
            Some("12.0")
        );
        // A tiny image drops into the 6-decimal branch.
        assert_eq!(c("Megapixels", &[Some("2x2")]).as_deref(), Some("0.000004"));
    }

    #[test]
    fn image_size_prefers_raw_image_cropped_size_over_required_pair() {
        // Exif.pm:4747-4766: `return $val[4] if $val[4]` is checked before
        // the required ImageWidth/ImageHeight pair is even consulted.
        // FujiFilm.raf's required pair comes from the embedded preview JPEG
        // (8x8, a test-fixture artifact of that preview's own SOF0 header),
        // which must lose outright to desire index 4.
        assert_eq!(
            c(
                "ImageSize",
                &[Some("8"), Some("8"), None, None, Some("4256x1424")]
            )
            .as_deref(),
            Some("4256x1424")
        );
    }

    #[test]
    fn image_size_falls_back_to_required_pair_when_val4_absent() {
        // No desire index 4 at all (the common case: every non-FujiFilm
        // format never populates RawImageCroppedSize).
        assert_eq!(
            c("ImageSize", &[Some("4000"), Some("3000")]).as_deref(),
            Some("4000x3000")
        );
        // Desire index 4 present but empty/falsy -- Perl's `if $val[4]` is
        // false for "" and "0", so the required pair still wins.
        assert_eq!(
            c(
                "ImageSize",
                &[Some("4000"), Some("3000"), None, None, Some("")]
            )
            .as_deref(),
            Some("4000x3000")
        );
        assert_eq!(
            c(
                "ImageSize",
                &[Some("4000"), Some("3000"), None, None, Some("0")]
            )
            .as_deref(),
            Some("4000x3000")
        );
        // val[4] absent (None) also falls back, same as ExifTool's undef.
        assert_eq!(
            c("ImageSize", &[Some("4000"), Some("3000"), None, None, None]).as_deref(),
            Some("4000x3000")
        );
    }

    #[test]
    fn shutter_speed_prefers_bulb_then_exposure() {
        // Fast shutter renders as a reciprocal.
        assert_eq!(
            c("ShutterSpeed", &[Some("0.005"), None, None]).as_deref(),
            Some("1/200")
        );
        // Rational input is accepted, since inputs arrive print-formatted.
        assert_eq!(
            c("ShutterSpeed", &[Some("1/200"), None, None]).as_deref(),
            Some("1/200")
        );
        // BulbDuration wins when positive.
        assert_eq!(
            c("ShutterSpeed", &[Some("0.005"), None, Some("30")]).as_deref(),
            Some("30")
        );
        // Falls back to ShutterSpeedValue when ExposureTime is absent.
        assert_eq!(
            c("ShutterSpeed", &[None, Some("0.5"), None]).as_deref(),
            Some("0.5")
        );
    }

    #[test]
    fn aperture_falls_back_to_aperture_value() {
        assert_eq!(c("Aperture", &[Some("2.8"), None]).as_deref(), Some("2.8"));
        assert_eq!(c("Aperture", &[None, Some("4.0")]).as_deref(), Some("4.0"));
        // Sub-f/1.0 lenses take two decimals.
        assert_eq!(
            c("Aperture", &[Some("0.95"), None]).as_deref(),
            Some("0.95")
        );
        assert_eq!(c("Aperture", &[None, None]), None);
    }

    #[test]
    fn focal_length_35efl_with_and_without_scale() {
        assert_eq!(
            c("FocalLength35efl", &[Some("50.0 mm"), None]).as_deref(),
            Some("50.0 mm")
        );
        assert_eq!(
            c("FocalLength35efl", &[Some("50.0 mm"), Some("1.6")]).as_deref(),
            Some("50.0 mm (35 mm equivalent: 80.0 mm)")
        );
    }

    #[test]
    fn optical_derivations() {
        // 43.267 / (1 * 1440) = 0.030 mm on a full-frame sensor.
        assert_eq!(
            c("CircleOfConfusion", &[Some("1.0")]).as_deref(),
            Some("0.030 mm")
        );
        // 50^2 / (2.8 * 0.03 * 1000) = 29.76 m
        assert_eq!(
            c(
                "HyperfocalDistance",
                &[Some("50"), Some("2.8"), Some("0.030")]
            )
            .as_deref(),
            Some("29.76 m")
        );
        // f/2.8, 1/200 s, ISO 100 -> log2(2.8^2 * 200 * 100/100) = ~10.6
        assert_eq!(
            c("LightValue", &[Some("2.8"), Some("1/200"), Some("100")]).as_deref(),
            Some("10.6")
        );
    }

    #[test]
    fn white_balance_ratios_match_exiftool_layouts() {
        // NikonD70.jpg: WB_RGBGLevels = 597 256 361 256.
        let mut rgbg = vec![None; 11];
        rgbg[1] = Some("597 256 361 256");
        assert_eq!(c("RedBalance", &rgbg).as_deref(), Some("2.332031"));
        assert_eq!(c("BlueBalance", &rgbg).as_deref(), Some("1.410156"));

        // NikonD2Hs.jpg: WB_RGGBLevels = 562 256 256 537.
        let mut rggb = vec![None; 11];
        rggb[0] = Some("562 256 256 537");
        assert_eq!(c("RedBalance", &rggb).as_deref(), Some("2.195313"));
        assert_eq!(c("BlueBalance", &rggb).as_deref(), Some("2.097656"));

        // OlympusE1.jpg supplies only WB_RBLevels; ExifTool uses a literal
        // green level of 256 for this two-component layout.
        let mut rb = vec![None; 11];
        rb[8] = Some("412 290");
        assert_eq!(c("RedBalance", &rb).as_deref(), Some("1.609375"));
        assert_eq!(c("BlueBalance", &rb).as_deref(), Some("1.132813"));
    }

    #[test]
    fn white_balance_falls_back_to_separate_component_levels() {
        let mut inputs = vec![None; 11];
        inputs[9] = Some("512");
        inputs[10] = Some("256");
        assert_eq!(c("RedBalance", &inputs).as_deref(), Some("2"));
        assert_eq!(c("BlueBalance", &inputs).as_deref(), Some("2"));
        inputs[10] = Some("0");
        assert_eq!(c("RedBalance", &inputs), None);
    }

    #[test]
    fn canon_white_balance_prefers_as_shot_then_selected_preset() {
        let mut inputs = vec![None; 12];
        inputs[0] = Some("Auto");
        inputs[1] = Some("2275 1024 1024 1357");
        inputs[2] = Some("unused auto preset");
        assert_eq!(
            compute("Canon", "WB_RGGBLevels", &inputs, Some("Canon"))
                .map(|c| c.print)
                .as_deref(),
            Some("2275 1024 1024 1357")
        );

        inputs[1] = None;
        inputs[0] = Some("Shade");
        inputs[10] = Some("2433 1024 1024 1259");
        assert_eq!(
            compute("Canon", "WB_RGGBLevels", &inputs, Some("Canon"))
                .map(|c| c.print)
                .as_deref(),
            Some("2433 1024 1024 1259")
        );
    }

    #[test]
    fn subsecond_timestamps_match_exiftool_rawconv() {
        assert_eq!(
            c(
                "SubSecDateTimeOriginal",
                &[Some("2005:01:14 08:57:59"), Some("20garbage"), None]
            )
            .as_deref(),
            Some("2005:01:14 08:57:59.20")
        );
        assert_eq!(
            c(
                "SubSecCreateDate",
                &[Some("2026:08:01 01:02:03"), Some("4"), Some("+9:30garbage")]
            )
            .as_deref(),
            Some("2026:08:01 01:02:03.4+09:30")
        );
        // RawConv returns undef when neither optional input contributes.
        assert_eq!(
            c(
                "SubSecModifyDate",
                &[Some("2026:08:01 01:02:03"), None, None]
            ),
            None
        );
        // An existing fraction must not be doubled.
        assert_eq!(
            c(
                "SubSecModifyDate",
                &[Some("2026:08:01 01:02:03.5"), Some("7"), None]
            ),
            None
        );
    }

    #[test]
    fn module_disambiguates_same_named_composites() {
        assert_eq!(
            compute(
                "PostScript",
                "ImageSize",
                &[Some("4000"), Some("3000")],
                None
            ),
            None
        );
    }

    #[test]
    fn depth_of_field_matches_exiftool_boundaries() {
        // Canon.jpg has no single focus distance, so ExifTool averages the
        // lower/upper bounds. Its far limit crosses infinity.
        assert_eq!(
            c(
                "DOF",
                &[
                    Some("34"),
                    Some("14"),
                    Some("0.018913043114871"),
                    None,
                    None,
                    None,
                    None,
                    Some("5.46"),
                    Some("655.35"),
                ],
            )
            .as_deref(),
            Some("inf (4.31 m - inf)")
        );

        // A synthetic shallow positive range takes ExifTool's three-decimal
        // formatting branch.
        assert_eq!(
            c(
                "DOF",
                &[
                    Some("100"),
                    Some("1"),
                    Some("0.1"),
                    Some("1"),
                    None,
                    None,
                    None,
                    None,
                    None,
                ],
            )
            .as_deref(),
            Some("0.018 m (0.991 - 1.009 m)")
        );

        // An explicitly reported zero FocusDistance means infinity, not a
        // missing desired input.
        assert!(
            c(
                "DOF",
                &[
                    Some("50"),
                    Some("8"),
                    Some("0.03"),
                    Some("0"),
                    None,
                    None,
                    None,
                    None,
                    None,
                ],
            )
            .is_some()
        );

        // ExifTool's `return 0 unless $f and $val[2]` happens after the
        // required values were coerced. Aperture does not participate in this
        // guard, so a zero aperture still produces the zero-width interval.
        assert_eq!(
            c(
                "DOF",
                &[
                    Some("50"),
                    Some("0"),
                    Some("0.03"),
                    Some("2"),
                    None,
                    None,
                    None,
                    None,
                    None,
                ],
            )
            .as_deref(),
            Some("0.00 m (2.00 - 2.00 m)")
        );

        // Missing every distance source refuses to emit a plausible result.
        assert_eq!(
            c(
                "DOF",
                &[
                    Some("50"),
                    Some("8"),
                    Some("0.03"),
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                ],
            ),
            None
        );
    }

    /// Build a ScaleFactor35efl input vector by index.
    fn sf(pairs: &[(usize, &'static str)]) -> Vec<Option<&'static str>> {
        let mut v = vec![None; 16];
        for (i, s) in pairs {
            v[*i] = Some(*s);
        }
        v
    }

    #[test]
    fn scale_factor_from_both_focal_lengths() {
        // The direct case: 80 / 50 = 1.6
        let v = sf(&[(0, "50.0 mm"), (1, "80")]);
        assert_eq!(c("ScaleFactor35efl", &v).as_deref(), Some("1.6"));
    }

    #[test]
    fn scale_factor_from_focal_plane_diagonal() {
        // Full-frame: 43.267 / 43.267 = 1.0
        let v = sf(&[(3, "43.267")]);
        assert_eq!(c("ScaleFactor35efl", &v).as_deref(), Some("1.0"));
    }

    #[test]
    fn scale_factor_from_focal_plane_resolution() {
        // 3456x2304 px at 1000 px/mm -> 3.456 x 2.304 mm, diag 4.155 mm.
        let v = sf(&[
            (7, "4"), // resolution unit = mm
            (8, "1000"),
            (9, "1000"),
            (10, "3456"),
            (11, "2304"),
        ]);
        // 43.267 / 4.155 = 10.4
        assert_eq!(c("ScaleFactor35efl", &v).as_deref(), Some("10.4"));
    }

    #[test]
    fn scale_factor_uses_canon_sensor_diagonal() {
        // Real values from ExifTool's own Canon.jpg fixture. The rationals are
        // load-bearing: 3072000/892 divides to 3443.9, which sends the generic
        // path to 29.3 instead of 1.6.
        let v = sf(&[(7, "2"), (8, "3072000/892"), (9, "2048000/595")]);
        assert_eq!(
            cm("ScaleFactor35efl", &v, "Canon").as_deref(),
            Some("1.6"),
            "Canon sensor-diagonal path must match ExifTool"
        );
        // Same inputs from a non-Canon body must NOT take that branch.
        assert_ne!(
            cm("ScaleFactor35efl", &v, "NIKON CORPORATION").as_deref(),
            Some("1.6")
        );
    }

    #[test]
    fn canon_sensor_diag_rejects_reduced_rationals() {
        // Equal denominators mean the fraction was reduced and the
        // sensor-size-in-denominator assumption no longer holds.
        assert_eq!(
            canon_sensor_diag(Some("3072000/892"), Some("2048000/892")),
            None
        );
        // A denominator below the 61 floor is not a plausible sensor size.
        assert_eq!(
            canon_sensor_diag(Some("3072000/60"), Some("2048000/595")),
            None
        );
        // Non-rational input must not be coerced.
        assert_eq!(canon_sensor_diag(Some("3443.9"), Some("3442.0")), None);
    }

    #[test]
    fn scale_factor_rejects_implausible_sensor_size() {
        // A 1 px/mm resolution implies a 4-metre sensor: ExifTool bounds the
        // diagonal to 1..100 mm, and so do we, because a bad scale factor
        // would silently corrupt five dependent tags.
        let v = sf(&[(7, "4"), (8, "1"), (9, "1"), (10, "3456"), (11, "2304")]);
        assert_eq!(c("ScaleFactor35efl", &v), None);
    }

    #[test]
    fn scale_factor_ignores_implausible_aspect_ratio() {
        // FocalPlaneX/YSize is unreliable, so a 5:1 ratio must not be trusted.
        let v = sf(&[(5, "50"), (6, "10")]);
        assert_eq!(c("ScaleFactor35efl", &v), None);
    }

    #[test]
    fn field_of_view() {
        // atan2(36, 2*7*5.5) * 360/3.14159 = 50.106.
        //
        // ExifTool prints 49.7 deg for Olympus.jpg, whose ScaleFactor35efl
        // *displays* as 5.5 -- the difference is the unrounded scale factor it
        // actually divides by, which is why the engine feeds ValueConv forms
        // between composites rather than printed ones.
        assert_eq!(
            c("FOV", &[Some("7.0 mm"), Some("5.5"), None]).as_deref(),
            Some("50.1 deg")
        );
        // A focus distance both narrows the angle (corr = 1 + 7/1993) and
        // appends the subject width: 2 * 2.0 * tan(0.43597) = 1.867 m.
        assert_eq!(
            c("FOV", &[Some("7.0 mm"), Some("5.5"), Some("2.0")]).as_deref(),
            Some("50.0 deg (1.86 m)")
        );
        // Missing either required input yields nothing.
        assert_eq!(c("FOV", &[Some("7.0 mm"), None, None]), None);
        assert_eq!(c("FOV", &[Some("0"), Some("5.5"), None]), None);
    }

    /// `Composite:FOV` on a value that lands exactly on a `%.2f` rounding tie.
    ///
    /// Inputs are `combined-samples/Olympus/OlympusTG-610.jpg`: FocalLength 5,
    /// ScaleFactor35efl 5.6, FocusDistance 2.21. The subject width reduces to
    /// `36*focus/(fl*sf*corr)`, whose *exact rational* value is 2.835 -- a tie,
    /// not a near-miss. The double evaluates to 2.8350000000000004, so
    /// formatting it directly rounds up to 2.84; ExifTool's ValueConv instead
    /// returns `join(' ', @fov)`, which renders it with `%.15g` as "2.835",
    /// and the PrintConv's `split` re-parses that as 2.8349999999999999 --
    /// which rounds *down*.
    ///
    /// Pinned against ExifTool 13.59 (`exiftool-pinned.sh -s -FOV`):
    ///   `FOV : 65.4 deg (2.83 m)`, and with `-n`, `65.3525005746021 2.835`.
    #[test]
    fn fov_rounds_the_reparsed_value_not_the_raw_double() {
        let olympus = &[Some("5"), Some("5.6"), Some("2.21")];
        let computed = compute("Exif", "FOV", olympus, None).expect("FOV computes");

        // 2.83, not the 2.84 that formatting the unrounded double produces.
        assert_eq!(computed.print, "65.4 deg (2.83 m)");
        // The ValueConv form is Perl's %.15g of each element, not Rust's
        // shortest round-trip form ("65.35250057460213 2.8350000000000004").
        assert_eq!(computed.value, "65.3525005746021 2.835");
    }

    /// `Composite:DOF`'s ValueConv is `join(' ',@v)` too, so the same round
    /// trip governs both its value text and its printed limits.
    ///
    /// Its `PrintConv` subtracts *after* the round trip -- `my $dof = $v[1] -
    /// $v[0]` runs on the re-parsed strings -- so the difference, and the
    /// 0.02 m cutoff that selects three decimals over two, both read the
    /// re-parsed numbers.
    ///
    /// A DOF tie is unreachable from a real `CircleOfConfusion`, which is
    /// `sqrt(2592)/(sf*1440)` and therefore irrational -- so this pins a
    /// constructed one. fl=6, ap=2.5, coc=0.01, d=0.87 gives t=0.6 exactly and
    /// far = 0.87/0.4 = 2.175 exactly, a `%.2f` tie whose double is
    /// 2.1750000000000003. Confirmed by running Exif.pm's ValueConv and
    /// PrintConv verbatim under perl: direct formatting yields
    /// `1.63 m (0.54 - 2.18 m)`, ExifTool yields `... - 2.17 m)`.
    #[test]
    fn dof_prints_the_reparsed_limits_not_the_raw_doubles() {
        let computed = compute(
            "Exif",
            "DOF",
            &[Some("6"), Some("2.5"), Some("0.01"), Some("0.87")],
            None,
        )
        .expect("DOF computes");

        // Far limit 2.17, not the 2.18 the unrounded double rounds to.
        assert_eq!(computed.print, "1.63 m (0.54 - 2.17 m)");
        assert_eq!(computed.value, "0.54375 2.175");
    }

    /// The same Olympus file's DOF, where the round trip changes the value
    /// text on an ordinary (non-tie) result.
    ///
    /// Pinned against ExifTool 13.59: `-n` gives `0.776639919706934 0` and the
    /// printed form is `inf (0.78 m - inf)`. Rust's `{}` would have written a
    /// 16th and 17th digit here that Perl never emits. The far limit is
    /// negative and clamped to 0, which is what makes the PrintConv take its
    /// `inf` branch.
    ///
    /// The CircleOfConfusion input is spelled the way the `CircleOfConfusion`
    /// arm actually hands it over -- full double precision, not its 15-digit
    /// display form. That distinction is deliberate: ExifTool keeps an
    /// unrounded NV in `VALUE` and only truncates when printing, so a
    /// composite that consumes another's value must receive every bit.
    #[test]
    fn dof_value_text_uses_perls_fifteen_significant_digits() {
        let coc = (24.0f64 * 24.0 + 36.0 * 36.0).sqrt() / (5.6 * 1440.0);
        let computed = compute(
            "Exif",
            "DOF",
            &[Some("5"), Some("3.9"), Some(&coc.to_string()), Some("2.21")],
            None,
        )
        .expect("DOF computes");

        assert_eq!(computed.value, "0.776639919706934 0");
        assert_eq!(computed.print, "inf (0.78 m - inf)");
    }

    /// The siblings that must *not* acquire the round trip.
    ///
    /// `HyperfocalDistance` returns `$val[0]*$val[0]/($val[1]*$val[2]*1000)`
    /// and `FocalLength35efl` returns `($val[0]||0)*($val[1]||1)` -- bare Perl
    /// NVs, not `join`ed strings -- so their `sprintf` sees the unrounded
    /// double. Verified against the pinned tree with a UserDefined composite
    /// mirroring FocalLength35efl: fl=4.5, sf=4.7 (exact product 21.15, double
    /// 21.150000000000002) prints `4.5 mm (35 mm equivalent: 21.2 mm)`.
    /// Routing these through `perl_join` would print 21.1 and *introduce* a
    /// divergence, so this test fails in the opposite direction from the two
    /// above.
    #[test]
    fn siblings_returning_bare_numbers_keep_full_double_precision() {
        assert_eq!(
            c("FocalLength35efl", &[Some("4.5"), Some("4.7")]).as_deref(),
            Some("4.5 mm (35 mm equivalent: 21.2 mm)")
        );
        // HyperfocalDistance likewise formats the unrounded quotient.
        assert_eq!(
            c(
                "HyperfocalDistance",
                &[Some("5"), Some("3.9"), Some("0.00536540368372618")]
            )
            .as_deref(),
            Some("1.19 m")
        );
    }

    #[test]
    fn canon_scalar_composites_match_the_source_expressions() {
        let canon = |name, inputs: &[Option<&str>]| {
            compute("Canon", name, inputs, None).map(|computed| computed.print)
        };

        assert_eq!(
            canon("DriveMode", &[Some("Single"), Some("Off")]).as_deref(),
            Some("Single-frame Shooting")
        );
        assert_eq!(
            canon("DriveMode", &[Some("Single"), Some("10 s")]).as_deref(),
            Some("Self-timer Operation")
        );
        assert_eq!(
            canon("Lens", &[Some("50 mm"), Some("50 mm")]).as_deref(),
            Some("50.0 mm")
        );
        assert_eq!(
            canon(
                "Lens35efl",
                &[Some("18 mm"), Some("55 mm"), Some("1.589"), Some("18")]
            )
            .as_deref(),
            Some("18.0 - 55.0 mm (35 mm equivalent: 28.6 - 87.4 mm)")
        );
        assert_eq!(
            canon("ShootingMode", &[Some("Manual"), Some("Manual"), Some("4")]).as_deref(),
            Some("Bulb")
        );
        assert_eq!(
            canon("ShootingMode", &[Some("Easy"), Some("Unknown (83)"), None]).as_deref(),
            Some("Unknown (83)")
        );
        assert_eq!(
            canon("ISO", &[Some("n/a"), Some("100"), Some("125")]).as_deref(),
            Some("125")
        );
        assert_eq!(
            canon("ISO", &[Some("0"), Some("100"), Some("200")]).as_deref(),
            Some("200")
        );
        assert_eq!(canon("ISO", &[Some("0"), Some("0"), Some("200")]), None);
        assert_eq!(
            canon("DigitalZoom", &[Some("3072"), Some("4608"), Some("Other")]).as_deref(),
            Some("1.50x")
        );
        assert_eq!(
            canon("FileNumber", &[Some("118"), Some("1861")]).as_deref(),
            Some("118-1861")
        );
        assert_eq!(
            canon("FileNumber", &[Some("118"), Some("10000")]).as_deref(),
            Some("119-0001")
        );
    }

    #[test]
    fn canon_flash_composites_preserve_flash_guards_and_print_forms() {
        let canon = |name, inputs: &[Option<&str>]| {
            compute("Canon", name, inputs, None).map(|computed| computed.print)
        };

        assert_eq!(canon("FlashType", &[Some("(none)")]), None);
        assert_eq!(
            canon("FlashType", &[Some("TTL, External")]).as_deref(),
            Some("External")
        );
        assert_eq!(
            canon("FlashType", &[Some("E-TTL, Built-in")]).as_deref(),
            Some("Built-In Flash")
        );
        assert_eq!(
            canon(
                "RedEyeReduction",
                &[Some("Red-eye reduction"), Some("E-TTL")]
            )
            .as_deref(),
            Some("On")
        );
        assert_eq!(
            canon("ConditionalFEC", &[Some("-1/3"), Some("TTL")]).as_deref(),
            Some("-1/3")
        );
        assert_eq!(
            canon("ShutterCurtainHack", &[None, Some("TTL")]).as_deref(),
            Some("1st-curtain sync")
        );
        assert_eq!(
            canon(
                "ShutterCurtainHack",
                &[Some("2nd-curtain sync"), Some("TTL")]
            )
            .as_deref(),
            Some("2nd-curtain sync")
        );
    }

    #[test]
    fn nikon_auto_focus_is_off_only_for_manual_focus() {
        let nikon = |focus_mode: Option<&str>| {
            compute(
                "Nikon",
                "AutoFocus",
                &[focus_mode],
                Some("NIKON CORPORATION"),
            )
            .map(|computed| computed.print)
        };

        // `exiftool -a -G1 -s -FocusMode -AutoFocus` under the pinned 13.59,
        // one corpus file per distinct FocusMode the corpus carries:
        //
        //   ======== Nikon/NikonD810.jpg
        //   [Nikon]     FocusMode  : Manual
        //   [Composite] AutoFocus  : Off
        //   ======== Nikon.nef
        //   [Nikon]     FocusMode  : AF-S
        //   [Composite] AutoFocus  : On
        //   ======== Nikon/Nikon1V3.jpg
        //   [Nikon]     FocusMode  : AF-C
        //   [Composite] AutoFocus  : On
        //   ======== Nikon/Nikon1AW1.jpg
        //   [Nikon]     FocusMode  : AF-A
        //   [Composite] AutoFocus  : On
        //   ======== Nikon/NikonCoolpixA300.jpg
        //   [Nikon]     FocusMode  : AF-F
        //   [Composite] AutoFocus  : On
        //   ======== Nikon/NikonCoolpixA1000.jpg
        //   [Nikon]     FocusMode  : AF-P
        //   [Composite] AutoFocus  : On
        assert_eq!(nikon(Some("Manual")).as_deref(), Some("Off"));
        assert_eq!(nikon(Some("AF-S")).as_deref(), Some("On"));
        assert_eq!(nikon(Some("AF-C")).as_deref(), Some("On"));
        assert_eq!(nikon(Some("AF-A")).as_deref(), Some("On"));
        assert_eq!(nikon(Some("AF-F")).as_deref(), Some("On"));
        assert_eq!(nikon(Some("AF-P")).as_deref(), Some("On"));

        // The match is `/^Manual/i`, and the cameras write the tag in caps --
        // `exiftool -n -Nikon:FocusMode` reports MANUAL for all eleven files
        // that print "Manual". Nikon::Main's FormatString PrintConv is what
        // lowercases it, so both spellings must land on Off.
        assert_eq!(nikon(Some("MANUAL")).as_deref(), Some("Off"));
        // Anchored at the start, so this is a prefix test, not equality.
        assert_eq!(nikon(Some("Manual (Preset)")).as_deref(), Some("Off"));
        // ...and only at the start.
        assert_eq!(nikon(Some("AF-S (Manual)")).as_deref(), Some("On"));

        // The single input is a `Require`, so an absent FocusMode emits
        // nothing rather than defaulting to the more common "On".
        assert_eq!(nikon(None), None);
    }

    #[test]
    fn panasonic_advanced_scene_mode_matches_pinned_exiftool() {
        let panasonic = |model, scene_mode, advanced_scene_type| {
            compute(
                "Panasonic",
                "AdvancedSceneMode",
                &[Some(model), Some(scene_mode), Some(advanced_scene_type)],
                Some("Panasonic"),
            )
            .map(|computed| computed.print)
        };

        // ExifTool 13.59, Panasonic.rw2: Model=DMC-LX3, SceneMode=Off,
        // AdvancedSceneType=1.
        assert_eq!(panasonic("DMC-LX3", "Off", "1").as_deref(), Some("Off"));
    }

    #[test]
    fn gps_position_and_time_composites_match_exiftool_defaults() {
        let gps = |name, inputs: &[Option<&str>]| {
            compute("GPS", name, inputs, None).map(|computed| computed.print)
        };

        assert_eq!(
            gps("GPSDateTime", &[Some("2026:08:01"), Some("12:34:56")]).as_deref(),
            Some("2026:08:01 12:34:56Z")
        );
        assert_eq!(
            gps("GPSLatitude", &[Some("54 deg 59' 22.80\""), Some("North")]).as_deref(),
            Some("54 deg 59' 22.80\" N")
        );
        assert_eq!(
            gps("GPSLongitude", &[Some("1 deg 54' 51.00\""), Some("west")]).as_deref(),
            Some("1 deg 54' 51.00\" W")
        );
        assert_eq!(
            gps(
                "GPSDestLongitude",
                &[Some("1 deg 54' 51.00\""), Some("west")]
            )
            .as_deref(),
            Some("1 deg 54' 51.00\" W")
        );
        assert_eq!(
            c(
                "GPSPosition",
                &[Some("54.9896666667"), Some("-1.91416666667")]
            )
            .as_deref(),
            Some("54 deg 59' 22.80\" N, 1 deg 54' 51.00\" W")
        );
    }

    #[test]
    fn iptc_exif_and_id3_time_joins_preserve_default_date_rendering() {
        assert_eq!(
            compute(
                "IPTC",
                "DateTimeCreated",
                &[Some("2026:08:01"), Some("12:34:56+00:00")],
                None
            )
            .map(|computed| computed.print)
            .as_deref(),
            Some("2026:08:01 12:34:56+00:00")
        );
        assert_eq!(
            compute(
                "Exif",
                "DateTimeOriginal",
                &[
                    Some("2026:08:01 12:34:56"),
                    Some("2026:08:01"),
                    Some("01:02:03")
                ],
                None
            )
            .map(|computed| computed.print)
            .as_deref(),
            Some("2026:08:01 12:34:56")
        );
        assert_eq!(
            compute(
                "ID3",
                "DateTimeOriginal",
                &[Some("2026-08-01T12:34:56"), None, None, None],
                None
            )
            .map(|computed| computed.print)
            .as_deref(),
            Some("2026-08-01T12:34:56")
        );
        assert_eq!(
            compute(
                "ID3",
                "DateTimeOriginal",
                &[None, Some("2005"), Some("0801"), Some("1234")],
                None
            )
            .map(|computed| computed.print)
            .as_deref(),
            Some("2005:08:01 12:34")
        );
    }

    /// `LensID` no longer reaches `compute` at all -- `super::apply` routes
    /// both Exif rows to `super::lens_id`, which needs context this function's
    /// signature cannot carry (see the `("Exif", "LensID")` note above). The
    /// unknown-focal-range case this test used to pin now lives as
    /// `lens_id::tests`, against the real `Canon::PrintLensID` fallback.
    #[test]
    fn lens_id_is_not_dispatched_through_compute() {
        assert_eq!(
            compute(
                "Exif",
                "LensID",
                &[
                    Some("n/a"),
                    None,
                    None,
                    None,
                    Some("7.09375 mm"),
                    Some("21.3125 mm"),
                ],
                Some("Canon"),
            ),
            None
        );
    }

    #[test]
    fn unimplemented_composites_do_not_fire() {
        // The contract that keeps this honest: no implementation, no output.
        assert_eq!(c("LensID", &[Some("whatever")]), None);
    }

    #[test]
    fn missing_required_input_yields_nothing() {
        assert_eq!(c("ImageSize", &[Some("4000"), None]), None);
        assert_eq!(c("Megapixels", &[None]), None);
    }

    // ------------------------------------------------------------------
    // Step 30: composites that previously had no registered computation and
    // so never fired. Every expectation below is the pinned oracle's own
    // output (`exiftool-pinned.sh -G -s -j -a`, ExifTool 13.59) on the named
    // corpus carrier, with the inputs as `oxidex -j` reports them for that
    // same file -- the two instruments quoted in the commit message.
    // ------------------------------------------------------------------

    /// Print form for a composite in a module other than `Exif`.
    fn cg(module: &str, name: &str, v: &[Option<&str>]) -> Option<String> {
        compute(module, name, v, None).map(|c| c.print)
    }

    #[test]
    fn preview_image_size_joins_the_required_pair() {
        // combined-samples/Canon/CanonEOS_DIGITAL_REBEL.jpg:
        // Canon:PreviewImageWidth 1536, Canon:PreviewImageHeight 1024
        // -> Composite:PreviewImageSize "1536x1024"
        assert_eq!(
            c("PreviewImageSize", &[Some("1536"), Some("1024")]).as_deref(),
            Some("1536x1024")
        );
        assert_eq!(c("PreviewImageSize", &[Some("1536"), None]), None);
    }

    #[test]
    fn gps_altitude_prints_magnitude_beside_its_reference() {
        // combined-samples/Samsung/SamsungSCH-I535.jpg:
        // GPS:GPSAltitude "207 m", GPS:GPSAltitudeRef 0 -> "207 m Above Sea Level"
        assert_eq!(
            cg(
                "GPS",
                "GPSAltitude",
                &[Some("207 m"), Some("0"), None, None]
            )
            .as_deref(),
            Some("207 m Above Sea Level")
        );
        // combined-samples/Samsung/SamsungSM-G930F.jpg: ref 1 -> "0 m Below Sea Level".
        // The printed magnitude is the UNSIGNED $val[0]; only the value form
        // takes the -abs() the ValueConv applies.
        let below = compute(
            "GPS",
            "GPSAltitude",
            &[Some("0 m"), Some("1"), None, None],
            None,
        )
        .expect("below-sea-level altitude fires");
        assert_eq!(below.print, "0 m Below Sea Level");
        let negative = compute(
            "GPS",
            "GPSAltitude",
            &[Some("12.5 m"), Some("1"), None, None],
            None,
        )
        .expect("below-sea-level altitude fires");
        assert_eq!(negative.print, "12.5 m Below Sea Level");
        assert_eq!(negative.value, "-12.5");
        // A parser that hands over the label instead of the byte resolves the
        // same way (GPS.pm's PrintConv read in the other direction).
        assert_eq!(
            cg(
                "GPS",
                "GPSAltitude",
                &[Some("207 m"), Some("Above Sea Level"), None, None]
            )
            .as_deref(),
            Some("207 m Above Sea Level")
        );
        // RawConv: neither reference present -> ExifTool emits nothing.
        assert_eq!(
            cg("GPS", "GPSAltitude", &[Some("207 m"), None, None, None]),
            None
        );
        // "inf" is not a float, so no branch of the ValueConv loop returns.
        assert_eq!(
            cg("GPS", "GPSAltitude", &[Some("inf"), Some("0"), None, None]),
            None
        );
    }

    #[test]
    fn gps_dest_latitude_signs_and_prints_like_gps_latitude() {
        // combined-samples/Samsung/SamsungL73.jpg:
        // GPS:GPSDestLatitude "35 deg 48' 8.00\"", ref "North"
        assert_eq!(
            cg(
                "GPS",
                "GPSDestLatitude",
                &[Some("35 deg 48' 8.00\""), Some("North")]
            )
            .as_deref(),
            Some("35 deg 48' 8.00\" N")
        );
        // The southern branch is the whole point of the arm being split by name.
        assert_eq!(
            cg("GPS", "GPSDestLatitude", &[Some("35 deg"), Some("South")]).as_deref(),
            Some("35 deg 0' 0.00\" S")
        );
    }

    #[test]
    fn nikon_lens_spec_joins_the_two_print_forms() {
        // combined-samples/Nikon/NikonD750.jpg: Nikon:Lens "70mm f/2.8",
        // Nikon:LensType "G" -> Composite:LensSpec "70mm f/2.8 G".
        assert_eq!(
            cg("Nikon", "LensSpec", &[Some("70mm f/2.8"), Some("G")]).as_deref(),
            Some("70mm f/2.8 G")
        );
        // combined-samples/Nikon/NikonD850.jpg
        assert_eq!(
            cg("Nikon", "LensSpec", &[Some("105mm f/1.4"), Some("E G")]).as_deref(),
            Some("105mm f/1.4 E G")
        );
    }

    #[test]
    fn nikon_phase_detect_af_reports_the_schema_only_for_phase_detect() {
        // combined-samples/Nikon/NikonD850.jpg: FocusPointSchema "153-point",
        // AFDetectionMethod "Phase Detect" -> "On (153-point)".
        assert_eq!(
            cg(
                "Nikon",
                "PhaseDetectAF",
                &[Some("153-point"), Some("Phase Detect")]
            )
            .as_deref(),
            Some("On (153-point)")
        );
        // combined-samples/Nikon/NikonD750.jpg: schema Off -> "Off".
        assert_eq!(
            cg(
                "Nikon",
                "PhaseDetectAF",
                &[Some("Off"), Some("Phase Detect")]
            )
            .as_deref(),
            Some("Off")
        );
        // combined-samples/Nikon/NikonZ50.jpg: an 81-point schema under Hybrid
        // detection is forced to 0 by `($val[1] == 0) ? $val[0] : 0`, which is
        // why ExifTool prints "Off" and not "Unknown (8)".
        assert_eq!(
            cg(
                "Nikon",
                "PhaseDetectAF",
                &[Some("81-point"), Some("Hybrid")]
            )
            .as_deref(),
            Some("Off")
        );
        // combined-samples/Nikon/NikonD6.jpg
        assert_eq!(
            cg(
                "Nikon",
                "PhaseDetectAF",
                &[Some("105-point"), Some("Phase Detect")]
            )
            .as_deref(),
            Some("On (105-point)")
        );
        // A label neither map names is refused rather than guessed at.
        assert_eq!(
            cg(
                "Nikon",
                "PhaseDetectAF",
                &[Some("wat"), Some("Phase Detect")]
            ),
            None
        );
    }

    #[test]
    fn nikon_contrast_detect_af_needs_both_non_manual_and_contrast() {
        // combined-samples/Nikon/NikonD750.jpg: FocusMode "Manual" -> "Off".
        assert_eq!(
            cg(
                "Nikon",
                "ContrastDetectAF",
                &[Some("Manual"), Some("Phase Detect")]
            )
            .as_deref(),
            Some("Off")
        );
        assert_eq!(
            cg(
                "Nikon",
                "ContrastDetectAF",
                &[Some("AF-S"), Some("Contrast Detect")]
            )
            .as_deref(),
            Some("On")
        );
        // Contrast detect but manual focus: the `!~ /^Manual/i` half fails.
        assert_eq!(
            cg(
                "Nikon",
                "ContrastDetectAF",
                &[Some("Manual"), Some("Contrast Detect")]
            )
            .as_deref(),
            Some("Off")
        );
    }

    #[test]
    fn olympus_extender_status_reads_the_extender_id() {
        // combined-samples/Olympus/OlympusE-330.jpg: Olympus:Extender "None"
        // (ValueConv "0 00") -> hex("00") is 0 -> "Not attached".
        assert_eq!(
            cg(
                "Olympus",
                "ExtenderStatus",
                &[
                    Some("None"),
                    Some("Olympus Zuiko Digital 14-54mm F2.8-3.5"),
                    Some("2.8")
                ]
            )
            .as_deref(),
            Some("Not attached")
        );
        // A non-EC-14 extender is assumed attached outright (Olympus.pm:4345).
        assert_eq!(
            cg(
                "Olympus",
                "ExtenderStatus",
                &[
                    Some("Olympus EX-25 Extension Tube"),
                    Some("Olympus Zuiko Digital 14-54mm F2.8-3.5"),
                    Some("2.8")
                ]
            )
            .as_deref(),
            Some("Attached")
        );
        // The EC-14 branch compares the reported max aperture against the
        // lens's own: 5.6 - 3.5 > 0.2 -> attached; 3.5 - 3.5 -> removed.
        let ec14 = Some("Olympus Zuiko Digital EC-14 1.4x Teleconverter");
        assert_eq!(
            cg(
                "Olympus",
                "ExtenderStatus",
                &[
                    ec14,
                    Some("Olympus Zuiko Digital 14-54mm F3.5-5.6"),
                    Some("5.6")
                ]
            )
            .as_deref(),
            Some("Attached")
        );
        assert_eq!(
            cg(
                "Olympus",
                "ExtenderStatus",
                &[
                    ec14,
                    Some("Olympus Zuiko Digital 14-54mm F3.5-5.6"),
                    Some("3.5")
                ]
            )
            .as_deref(),
            Some("Removed")
        );
    }

    #[test]
    fn sony_focus_distances_scale_by_focal_length() {
        // combined-samples/Sony/SonyDSLR-A200.jpg: FocusPosition 94,
        // FocalLength "30.0 mm" -> 94 * 30 / 1000 = 2.82 -> "2.82 m".
        assert_eq!(
            cg("Sony", "FocusDistance", &[Some("94"), Some("30.0 mm")]).as_deref(),
            Some("2.82 m")
        );
        // combined-samples/Sony/SonyDSLR-A380.jpg: FocusPosition 128 -> "inf".
        assert_eq!(
            cg("Sony", "FocusDistance", &[Some("128"), Some("55.0 mm")]).as_deref(),
            Some("inf")
        );
        // combined-samples/Sony/SonyILCE-7S.jpg: FocusPosition2 133,
        // FocalLengthIn35mmFormat "35 mm" -> "0.3827 m" (sprintf "%.4g m").
        assert_eq!(
            cg("Sony", "FocusDistance2", &[Some("133"), Some("35 mm")]).as_deref(),
            Some("0.3827 m")
        );
        // `return undef unless $val` and `'inf' if $val >= 255`.
        assert_eq!(
            cg("Sony", "FocusDistance2", &[Some("0"), Some("35 mm")]),
            None
        );
        assert_eq!(
            cg("Sony", "FocusDistance2", &[Some("255"), Some("35 mm")]).as_deref(),
            Some("inf")
        );
    }

    #[test]
    fn xmp_flash_packs_the_five_scalar_fields() {
        // combined-samples/Canon/CanonPowerShotG15.jpg: FlashFired false,
        // FlashReturn "No return detection" (0), FlashMode "Off" (2),
        // FlashFunction false, FlashRedEyeMode false
        // -> 2 << 3 == 0x10 -> "Off, Did not fire".
        assert_eq!(
            cg(
                "XMP",
                "Flash",
                &[
                    Some("false"),
                    Some("No return detection"),
                    Some("Off"),
                    Some("false"),
                    Some("false"),
                    None
                ]
            )
            .as_deref(),
            Some("Off, Did not fire")
        );
        // 0x01 | (3 << 1) | (1 << 3) | 0x40 == 0x4f
        assert_eq!(
            cg(
                "XMP",
                "Flash",
                &[
                    Some("True"),
                    Some("Return detected"),
                    Some("On"),
                    Some("false"),
                    Some("true"),
                    None
                ]
            )
            .as_deref(),
            Some("On, Red-eye reduction, Return detected")
        );
    }

    #[test]
    fn xmp_gps_refs_read_the_last_hemisphere_letter() {
        // combined-samples/Samsung/SamsungGalaxyA55_5G.jpg:
        // XMP-exif:GPSLatitude "43,30.4233408N" -> "North".
        assert_eq!(
            cg("XMP", "GPSLatitudeRef", &[Some("43,30.4233408N")]).as_deref(),
            Some("North")
        );
        assert_eq!(
            cg("XMP", "GPSLongitudeRef", &[Some("16,26.3012136E")]).as_deref(),
            Some("East")
        );
        assert_eq!(
            cg("XMP", "GPSLatitudeRef", &[Some("43,30.4233408S")]).as_deref(),
            Some("South")
        );
        // The IsFloat branch: a signed decimal has no letter to read.
        assert_eq!(
            cg("XMP", "GPSLatitudeRef", &[Some("-43.5070557")]).as_deref(),
            Some("South")
        );
        assert_eq!(
            cg("XMP", "GPSLongitudeRef", &[Some("16.438353")]).as_deref(),
            Some("East")
        );
    }

    #[test]
    fn iso_volume_size_multiplies_the_block_geometry() {
        // combined-samples/ISO.iso: VolumeBlockCount 190976,
        // VolumeBlockSize 2048 -> 391118848 bytes -> "391 MB".
        let computed = compute("ISO", "VolumeSize", &[Some("190976"), Some("2048")], None)
            .expect("VolumeSize fires");
        assert_eq!(computed.value, "391118848");
        assert_eq!(computed.print, "391 MB");
    }

    #[test]
    fn kodak_date_created_joins_year_and_month_day() {
        // combined-samples/Kodak.jpg: YearCreated 2002, MonthDayCreated "05:01".
        assert_eq!(
            cg("Kodak", "DateCreated", &[Some("2002"), Some("05:01")]).as_deref(),
            Some("2002:05:01")
        );
    }

    #[test]
    fn postscript_image_size_prefers_image_data_over_bounding_box() {
        // combined-samples/PostScript.eps: ImageData "8 8 8 3 1 8 2 \"beginimage\"",
        // BoundingBox "0 0 8 8" -> ImageWidth 8, ImageHeight 8.
        let data = Some("8 8 8 3 1 8 2 \"beginimage\"");
        let bbox = Some("0 0 8 8");
        assert_eq!(
            cg("PostScript", "ImageWidth", &[data, bbox]).as_deref(),
            Some("8")
        );
        assert_eq!(
            cg("PostScript", "ImageHeight", &[data, bbox]).as_deref(),
            Some("8")
        );
        // With no ImageData the BoundingBox difference stands in
        // (PostScript.pm:168-169).
        assert_eq!(
            cg("PostScript", "ImageWidth", &[None, Some("10 20 100 220")]).as_deref(),
            Some("90")
        );
        assert_eq!(
            cg("PostScript", "ImageHeight", &[None, Some("10 20 100 220")]).as_deref(),
            Some("200")
        );
        assert_eq!(cg("PostScript", "ImageWidth", &[None, None]), None);
    }

    #[test]
    fn cfa_pattern_prefixes_the_repeat_dimensions() {
        // combined-samples/Nikon.nef: CFARepeatPatternDim "2 2",
        // CFAPattern2 "2 1 1 0" -> "[Blue,Green][Green,Red]".
        let computed = compute("Exif", "CFAPattern", &[Some("2 2"), Some("2 1 1 0")], None)
            .expect("CFAPattern fires");
        assert_eq!(computed.value, "2 2 2 1 1 0");
        assert_eq!(computed.print, "[Blue,Green][Green,Red]");
        // combined-samples/DNG.dng
        assert_eq!(
            c("CFAPattern", &[Some("2 2"), Some("0 1 1 2")]).as_deref(),
            Some("[Red,Green][Green,Blue]")
        );
        // A pattern whose length disagrees with the dimensions is ExifTool's
        // literal '?', which PrintCFAPattern renders as truncated data.
        assert_eq!(
            c("CFAPattern", &[Some("2 2"), Some("0 1")]).as_deref(),
            Some("<truncated data>")
        );
    }

    #[test]
    fn audio_and_riff_durations() {
        // combined-samples/APE.ape: SampleRate 44100, TotalFrames 2,
        // BlocksPerFrame 73728, FinalFrameBlocks 42662 -> "2.64 s".
        assert_eq!(
            cg(
                "APE",
                "Duration",
                &[Some("44100"), Some("2"), Some("73728"), Some("42662")]
            )
            .as_deref(),
            Some("2.64 s")
        );
        // combined-samples/FLAC.flac carries TotalSamples 0, and the
        // `($val[0] and $val[1])` guard is why ExifTool emits nothing for it.
        assert_eq!(cg("FLAC", "Duration", &[Some("8000"), Some("0")]), None);
        assert_eq!(
            cg("FLAC", "Duration", &[Some("44100"), Some("441000")]).as_deref(),
            Some("10.00 s")
        );
        // combined-samples/RIFF.avi: FrameRate 15, FrameCount 233 -> "15.53 s".
        assert_eq!(
            cg("RIFF", "Duration", &[Some("15"), Some("233"), None, None]).as_deref(),
            Some("15.53 s")
        );
        // combined-samples/Pentax.avi: FrameRate 24, FrameCount 600.
        assert_eq!(
            cg("RIFF", "Duration", &[Some("24"), Some("600"), None, None]).as_deref(),
            Some("25.00 s")
        );
        // RIFF.pm:1660-1664: the video-stream pair wins only when the header
        // duration is 2x-3x longer than it.
        assert_eq!(
            cg(
                "RIFF",
                "Duration",
                &[Some("15"), Some("466"), Some("15"), Some("233")]
            )
            .as_deref(),
            Some("15.53 s")
        );
        assert_eq!(
            cg(
                "RIFF",
                "Duration",
                &[Some("15"), Some("240"), Some("15"), Some("233")]
            )
            .as_deref(),
            Some("16.00 s")
        );
    }
}
