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

/// ExifTool: `Image::ExifTool::Exif::PrintExposureTime`
///
/// ```text
/// return $val unless Image::ExifTool::IsFloat($val);
/// return sprintf("1/%d", int(1/$val + 0.5)) if $val < 0.25001 and $val > 0;
/// $val = int($val * 10 + 0.5) / 10;   # (0.3 not 1/3)
/// ```
fn print_exposure_time(v: f64) -> String {
    if v > 0.0 && v < 0.25001 {
        return format!("1/{}", (1.0 / v + 0.5) as i64);
    }
    let r = (v * 10.0 + 0.5).floor() / 10.0;
    if (r - r.round()).abs() < f64::EPSILON {
        format!("{}", r as i64)
    } else {
        format!("{r}")
    }
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
    fn unimplemented_composites_do_not_fire() {
        // The contract that keeps this honest: no implementation, no output.
        assert_eq!(c("LensID", &[Some("whatever")]), None);
        assert_eq!(c("GPSPosition", &[Some("1"), Some("2")]), None);
    }

    #[test]
    fn missing_required_input_yields_nothing() {
        assert_eq!(c("ImageSize", &[Some("4000"), None]), None);
        assert_eq!(c("Megapixels", &[None]), None);
    }
}
