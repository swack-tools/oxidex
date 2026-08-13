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
use crate::core::formatters::exif_enums::flash_label;
use crate::core::formatters::exif_print_conv::print_exposure_time;
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
    fn same(v: impl Into<String>) -> Option<Self> {
        let v = v.into();
        Some(Computed {
            print: v.clone(),
            value: v,
        })
    }

    /// Distinct value and display forms.
    fn new(value: impl Into<String>, print: impl Into<String>) -> Option<Self> {
        Some(Computed {
            value: value.into(),
            print: print.into(),
        })
    }
}

/// Parse a value ExifTool would have fed to `ToFloat`.
///
/// Handles the rational forms that reach composites unconverted (`1/200`) and
/// trailing units (`50.0 mm`), because the inputs are print-formatted values
/// rather than raw ones.
fn f(v: Option<&str>) -> Option<f64> {
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

fn get<'a>(i: Inputs<'a>, n: usize) -> Option<&'a str> {
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

/// Reverse the XMP `exif:Flash` member PrintConvs before XMP.pm packs them
/// into the ordinary EXIF Flash bitfield.  The parser has already applied
/// these PrintConvs, so Composite must not feed labels such as `Off` into
/// Perl's numeric operators as though they were the raw 2.
fn xmp_flash_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn xmp_flash_return(value: &str) -> Option<i64> {
    match value.trim() {
        "No return detection" => Some(0),
        "Return not detected" => Some(2),
        "Return detected" => Some(3),
        other => printed_integer(other),
    }
}

fn xmp_flash_mode(value: &str) -> Option<i64> {
    match value.trim() {
        "Unknown" => Some(0),
        "On" => Some(1),
        "Off" => Some(2),
        "Auto" => Some(3),
        other => printed_integer(other),
    }
}

/// XMP.pm:2808-2840 `Composite::Flash` ValueConv followed by Exif.pm's
/// `%flash` PrintConv.  Return `None` for values outside the documented XMP
/// field domains rather than manufacture a plausible EXIF bitfield.
fn xmp_flash(i: Inputs) -> Option<Computed> {
    let fired = xmp_flash_bool(get(i, 0)?)?;
    let returned = xmp_flash_return(get(i, 1)?)?;
    let mode = xmp_flash_mode(get(i, 2)?)?;
    let function = xmp_flash_bool(get(i, 3)?)?;
    let red_eye = xmp_flash_bool(get(i, 4)?)?;
    if !(0..=3).contains(&returned) || !(0..=3).contains(&mode) {
        return None;
    }
    let value = (i64::from(fired))
        | (returned << 1)
        | (mode << 3)
        | (i64::from(function) << 5)
        | (i64::from(red_eye) << 6);
    let print = flash_label(value)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("Unknown (0x{value:x})"));
    Computed::new(value.to_string(), print)
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
    if value == 0.0 {
        return "0".to_string();
    }
    let magnitude = value.abs().log10().floor() as i32;
    let decimals = (2 - magnitude).max(0) as usize;
    let rendered = format!("{value:.decimals$}");
    rendered
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}

/// Render a fixed-precision decimal without a non-significant trailing zero.
/// This is the textual form Perl emits after GPS.pm's `int($val * 10) / 10`.
fn format_decimal(value: f64, decimals: usize) -> String {
    format!("{value:.decimals$}")
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}

/// Sony.pm's `sprintf("%.4g", $val)` for `FocusDistance2`.
fn sony_four_sig(value: f64) -> String {
    if value == 0.0 {
        return "0".to_string();
    }
    let exponent = value.abs().log10().floor() as i32;
    if exponent < -4 || exponent >= 4 {
        let rendered = format!("{:.3e}", value);
        let (mantissa, exponent) = rendered.split_once('e').unwrap_or((&rendered, "0"));
        let mantissa = mantissa.trim_end_matches('0').trim_end_matches('.');
        return format!("{mantissa}e{exponent}");
    }
    let decimals = (3 - exponent).max(0) as usize;
    let rendered = format!("{value:.decimals$}");
    rendered
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
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
        // XMP.pm:2808-2840 packs the separately extracted `exif:Flash`
        // fields into the ordinary EXIF Flash value, then uses Exif.pm's
        // `%flash` PrintConv.  NikonCoolpixP520.jpg exercises the all-false,
        // FlashMode=Off branch.
        ("XMP", "Flash") => xmp_flash(i),

        // Olympus.pm:4337-4345 first validates the Extender byte-string.  A
        // printed `None` is its exact `0 00` case and immediately returns 0;
        // handle only that branch until the remaining byte-string and
        // aperture comparisons are independently represented here.
        ("Olympus", "ExtenderStatus") if get(i, 0) == Some("None") => {
            Computed::same("Not attached")
        }

        ("Panasonic", "AdvancedSceneMode") => {
            panasonic_advanced_scene_mode(get(i, 0)?, get(i, 1)?, get(i, 2)?)
        }

        // Sony.pm:10895-10928.  FocusPosition's 128-and-up range is Sony's
        // infinity sentinel; FocusPosition2 has a distinct 255 sentinel and
        // is displayed with four significant digits.
        ("Sony", "FocusDistance") => {
            let (position, focal_length) = (f(get(i, 0))?, f(get(i, 1))?);
            if position >= 128.0 {
                Computed::same("inf")
            } else {
                let distance = position * focal_length / 1000.0;
                Computed::new(distance.to_string(), format!("{distance} m"))
            }
        }

        ("Sony", "FocusDistance2") => {
            let (position, focal_length_35mm) = (f(get(i, 0))?, f(get(i, 1))?);
            if position == 0.0 {
                return None;
            }
            if position >= 255.0 {
                return Computed::same("inf");
            }
            let distance = (2f64.powf(position / 16.0 - 5.0) + 1.0) * focal_length_35mm / 1000.0;
            Computed::new(
                distance.to_string(),
                format!("{} m", sony_four_sig(distance)),
            )
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

        // FLIR.pm:1311-1314 Composite::PeakSpectralSensitivity:
        //   ValueConv => '14387.6515/$val'
        //   PrintConv => 'sprintf("%.1f um", $val)'
        // `PlanckB` is a camera calibration constant, so a zero value is not
        // meaningful and must not turn into an infinite derived tag.
        ("FLIR", "PeakSpectralSensitivity") => {
            let planck_b = f(get(i, 0))?;
            if planck_b == 0.0 {
                return None;
            }
            let micrometres = 14_387.651_5 / planck_b;
            Computed::new(micrometres.to_string(), format!("{micrometres:.1} um"))
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
        ("Exif", "ImageSize") => {
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

        // Exif.pm's PrintLensID falls back to an unknown focal range when a
        // Canon body records LensType as 65535 (displayed as `n/a`) and no
        // lens lookup entry exists. Keep this narrow: known lens labels need
        // the full model-matching routine and are intentionally left alone.
        ("Exif", "LensID") => {
            if !matches!(get(i, 0).map(str::trim), Some("n/a" | "N/A" | "65535")) {
                return None;
            }
            let short = f(get(i, 4))?;
            let long = f(get(i, 5))?;
            if short <= 0.0 || long <= 0.0 {
                return None;
            }
            Computed::same(format!("Unknown {:.0}-{:.0}mm", short, long))
        }

        // require: FocalLength; desire: ScaleFactor35efl
        // ValueConv: `($val[0] || 0) * ($val[1] || 1)`
        // PrintConv: `$val[1] ? "%.1f mm (35 mm equivalent: %.1f mm)" : "%.1f mm"`
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
            let value = format!("{near} {far}");
            if far == 0.0 {
                return Computed::new(value, format!("inf ({near:.2} m - inf)"));
            }

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
            if focus > 0.0 && focus < 10000.0 {
                let dist = 2.0 * focus * fd2.sin() / fd2.cos();
                Computed::new(
                    format!("{deg} {dist}"),
                    format!("{deg:.1} deg ({dist:.2} m)"),
                )
            } else {
                Computed::new(deg.to_string(), format!("{deg:.1} deg"))
            }
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

        // Nikon.pm:13215-13222 Composite::LensSpec. Both dependencies have
        // already passed their MakerNote PrintConv, and Nikon's ValueConv and
        // PrintConv are the same literal space-separated join at this layer.
        ("Nikon", "LensSpec") => Computed::same(format!("{} {}", get(i, 0)?, get(i, 1)?)),

        // Nikon.pm:13247-13261.  AFInfo2 has already PrintConv'd these two
        // inputs by the time Composite runs, so translate only the documented
        // display forms back to the small integer domains used by Nikon.pm's
        // ValueConv.  Unknown schema/detection values must stay absent rather
        // than being guessed as an AF mode.
        ("Nikon", "PhaseDetectAF") => {
            let detection = match get(i, 1)? {
                "Phase Detect" => true,
                "Contrast Detect" | "Hybrid" => false,
                _ => return None,
            };
            let raw_schema = match get(i, 0)? {
                "Off" => 0,
                "51-point" => 1,
                "11-point" => 2,
                "39-point" => 3,
                "153-point" => 7,
                "81-point" => 8,
                "105-point" => 9,
                _ => return None,
            };
            let printed = if !detection {
                "Off"
            } else {
                match raw_schema {
                    0 => "Off",
                    1 => "On (51-point)",
                    2 => "On (11-point)",
                    3 => "On (39-point)",
                    7 => "On (153-point)",
                    9 => "On (105-point)",
                    // Nikon.pm deliberately has no PrintConv for 81-point.
                    _ => return None,
                }
            };
            Computed::same(printed)
        }

        // Nikon.pm:13263-13270.  As above, AFDetectionMethod is already in
        // its Nikon.pm PrintConv form here.
        ("Nikon", "ContrastDetectAF") => {
            let autofocus = !get(i, 0)?
                .get(..6)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case("Manual"));
            let contrast = match get(i, 1)? {
                "Phase Detect" | "Hybrid" => false,
                "Contrast Detect" => true,
                _ => return None,
            };
            Computed::same(if autofocus && contrast { "On" } else { "Off" })
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

        // GPS.pm Composite::GPSDateTime:
        //   `"$val[0] $val[1]Z"`, followed by ConvertDateTime.
        ("GPS", "GPSDateTime") => Computed::same(format!("{} {}Z", get(i, 0)?, get(i, 1)?)),

        // GPS.pm signed-coordinate ValueConv and default ToDMS PrintConv.
        // Destination latitude has the same latitude (S/N) polarity as the
        // ordinary latitude field; it is deliberately kept in this shared
        // branch so the DMS formatting cannot drift between the two tags.
        ("GPS", "GPSLatitude" | "GPSDestLatitude" | "GPSLongitude" | "GPSDestLongitude") => {
            let coordinate = get(i, 0)?;
            if coordinate.is_empty() {
                return Computed::same("");
            }
            let mut value = gps_degrees(coordinate)?;
            let reference = get(i, 1)?.trim();
            let negative = if matches!(name, "GPSLatitude" | "GPSDestLatitude") {
                reference.starts_with(['S', 's'])
            } else {
                reference.starts_with(['W', 'w'])
            };
            if negative {
                value = -value;
            }
            Computed::new(
                value.to_string(),
                gps_print(
                    value,
                    if matches!(name, "GPSLatitude" | "GPSDestLatitude") {
                        'N'
                    } else {
                        'E'
                    },
                ),
            )
        }

        // GPS.pm Composite::GPSAltitude (GPS.pm:406-431).  The parser has
        // already applied GPSAltitudeRef's PrintConv, so recognize both its
        // displayed Above/Below-Sea-Level form and a raw numeric fallback.
        ("GPS", "GPSAltitude") => {
            let pairs = [(get(i, 0), get(i, 1)), (get(i, 2), get(i, 3))];
            for (altitude, reference) in pairs {
                let (Some(altitude), Some(reference)) = (altitude, reference) else {
                    continue;
                };
                let Some(mut value) = f(Some(altitude)) else {
                    continue;
                };
                let below = reference.to_ascii_lowercase().contains("below")
                    || f(Some(reference)).is_some_and(|raw| raw != 0.0);
                if below {
                    value = -value.abs();
                }
                // Perl's `int($val * 10) / 10` truncates toward zero.
                let truncated = (value * 10.0).trunc() / 10.0;
                let rendered = format_decimal(truncated, 1);
                let print = if reference.contains("Sea") {
                    // The parsed altitude can already carry its unit in the
                    // intermediate value form.  GPSAltitudeRef itself is
                    // unitless; strip that inherited suffix before composing
                    // ExifTool's single `m` in the final display value.
                    let reference = reference.trim_end().trim_end_matches(" m");
                    format!("{rendered} m {reference}")
                } else if truncated.is_sign_negative() {
                    format!("{} m Below Sea Level", rendered.trim_start_matches('-'))
                } else {
                    format!("{rendered} m Above Sea Level")
                };
                return Computed::new(value.to_string(), print);
            }
            None
        }

        // Exif.pm Composite::PreviewImageSize: concatenate the two dimensions
        // verbatim, not numerically, matching ExifTool's `"$val[0]x$val[1]"`.
        ("Exif", "PreviewImageSize") => Computed::same(format!("{}x{}", get(i, 0)?, get(i, 1)?)),

        // XMP.pm:2748-2785. XMP coordinates may be numeric decimal degrees or
        // Adobe's DMS-like `43,30.4233408N` string. `IsFloat` only accepts the
        // former; otherwise Perl takes the final direction character.
        ("XMP", "GPSLatitudeRef" | "GPSDestLatitudeRef") => {
            let coordinate = get(i, 0)?.trim();
            let raw = coordinate.parse::<f64>().ok();
            let value = match raw {
                Some(value) if value < 0.0 => "S",
                Some(_) => "N",
                None => match coordinate.chars().last()?.to_ascii_uppercase() {
                    'N' => "N",
                    'S' => "S",
                    _ => return None,
                },
            };
            Computed::new(value, if value == "N" { "North" } else { "South" })
        }

        ("XMP", "GPSLongitudeRef" | "GPSDestLongitudeRef") => {
            let coordinate = get(i, 0)?.trim();
            let raw = coordinate.parse::<f64>().ok();
            let value = match raw {
                Some(value) if value < 0.0 => "W",
                Some(_) => "E",
                None => match coordinate.chars().last()?.to_ascii_uppercase() {
                    'E' => "E",
                    'W' => "W",
                    _ => return None,
                },
            };
            Computed::new(value, if value == "E" { "East" } else { "West" })
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

        // Kodak.pm:3023-3030 Composite::DateCreated. `MonthDayCreated` is
        // already ValueConv'd from its two bytes to `MM:DD`, so the Composite
        // merely joins it to the four-digit year. Keeping this as a string
        // also retains ExifTool's zero-padded date representation exactly.
        ("Kodak", "DateCreated") => Computed::same(format!("{}:{}", get(i, 0)?, get(i, 1)?)),

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

        _ => None,
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

    #[test]
    fn canon_unknown_lens_type_uses_focal_range_fallback() {
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
            )
            .map(|computed| computed.print),
            Some("Unknown 7-21mm".to_string())
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
}
