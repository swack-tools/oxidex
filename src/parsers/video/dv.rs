//! Raw DV (Digital Video) metadata parser.
//!
//! ExifTool routes `.dv` files through `Image::ExifTool::DV::ProcessDV`
//! (DV.pm:148-268), which locates the first DIF block, reads the DSF and
//! video-signal-type bits to select one of ten hard-coded profiles
//! (DV.pm:20-111), then scans the VAUX DIF blocks for a date/time, aspect
//! ratio and scan type, and one audio DIF block for the audio parameters.
//!
//! # Why there is no transcribed table to read
//!
//! `DV::Main` (DV.pm:123-145) declares `VARS => { ID_FMT => 'none' }` and its
//! keys are tag *names*, not offsets or IDs -- it exists only to hang the
//! four PrintConvs on. Everything that is actually decoded comes from
//! `@dvProfiles`, a Perl list of constants, and from `ProcessDV`'s own bit
//! arithmetic. There is no `ProcessBinaryData` layout here for the generator
//! to transcribe, so the profile table and the DIF walk are ported directly
//! from the cited Perl.
//!
//! # What is deliberately absent
//!
//! ExifTool itself skips two fields it can see, and so does this parser,
//! for the same stated reasons:
//!
//! - The date record's timezone byte (DV.pm:214, "(ignore timezone in byte 0
//!   until we can test this properly - see ref 2)").
//! - The time record's frame count (DV.pm:227, "(ignore frames past second in
//!   byte 0 for now - see ref 2)").
//!
//! A file whose DSF/signal-type pair matches no profile yields no DV tags at
//! all, matching DV.pm:187's `$profile or $et->Warn("Unrecognized DV
//! profile"), return 1` -- the file is still recognised, it just has nothing
//! to report.
//!
//! # References
//!
//! - ExifTool source: `lib/Image/ExifTool/DV.pm`

use crate::core::formatters::numeric_precision::perl_number;
use crate::core::formatters::{convert_bitrate, convert_duration};
use crate::core::{FileReader, MetadataMap, TagValue};

/// DV.pm:157, `$raf->Read($buff, 12000)`.
const SCAN_LEN: usize = 12000;

/// One entry of `@dvProfiles` (DV.pm:20-111).
struct Profile {
    dsf: u8,
    video_stype: u8,
    frame_size: f64,
    video_format: &'static str,
    colorimetry: &'static str,
    frame_rate: f64,
    image_height: u32,
    image_width: u32,
}

/// `@dvProfiles` (DV.pm:20-111), in declaration order -- DV.pm:182-186 takes
/// the *first* entry matching the DSF/signal-type pair, so order matters.
const PROFILES: &[Profile] = &[
    Profile {
        dsf: 0,
        video_stype: 0x0,
        frame_size: 120000.0,
        video_format: "IEC 61834, SMPTE-314M - 525/60 (NTSC)",
        colorimetry: "4:1:1",
        frame_rate: 30000.0 / 1001.0,
        image_height: 480,
        image_width: 720,
    },
    Profile {
        dsf: 1,
        video_stype: 0x0,
        frame_size: 144000.0,
        video_format: "IEC 61834 - 625/50 (PAL)",
        colorimetry: "4:2:0",
        frame_rate: 25.0,
        image_height: 576,
        image_width: 720,
    },
    Profile {
        dsf: 1,
        video_stype: 0x0,
        frame_size: 144000.0,
        video_format: "SMPTE-314M - 625/50 (PAL)",
        colorimetry: "4:1:1",
        frame_rate: 25.0,
        image_height: 576,
        image_width: 720,
    },
    Profile {
        dsf: 0,
        video_stype: 0x4,
        frame_size: 240000.0,
        video_format: "DVCPRO50: SMPTE-314M - 525/60 (NTSC) 50 Mbps",
        colorimetry: "4:2:2",
        frame_rate: 30000.0 / 1001.0,
        image_height: 480,
        image_width: 720,
    },
    Profile {
        dsf: 1,
        video_stype: 0x4,
        frame_size: 288000.0,
        video_format: "DVCPRO50: SMPTE-314M - 625/50 (PAL) 50 Mbps",
        colorimetry: "4:2:2",
        frame_rate: 25.0,
        image_height: 576,
        image_width: 720,
    },
    Profile {
        dsf: 0,
        video_stype: 0x14,
        frame_size: 480000.0,
        video_format: "DVCPRO HD: SMPTE-370M - 1080i60 100 Mbps",
        colorimetry: "4:2:2",
        frame_rate: 30000.0 / 1001.0,
        image_height: 1080,
        image_width: 1280,
    },
    Profile {
        dsf: 1,
        video_stype: 0x14,
        frame_size: 576000.0,
        video_format: "DVCPRO HD: SMPTE-370M - 1080i50 100 Mbps",
        colorimetry: "4:2:2",
        frame_rate: 25.0,
        image_height: 1080,
        image_width: 1440,
    },
    Profile {
        dsf: 0,
        video_stype: 0x18,
        frame_size: 240000.0,
        video_format: "DVCPRO HD: SMPTE-370M - 720p60 100 Mbps",
        colorimetry: "4:2:2",
        frame_rate: 60000.0 / 1001.0,
        image_height: 720,
        image_width: 960,
    },
    Profile {
        dsf: 1,
        video_stype: 0x18,
        frame_size: 288000.0,
        video_format: "DVCPRO HD: SMPTE-370M - 720p50 100 Mbps",
        colorimetry: "4:2:2",
        frame_rate: 50.0,
        image_height: 720,
        image_width: 960,
    },
    Profile {
        dsf: 1,
        video_stype: 0x1,
        frame_size: 144000.0,
        video_format: "IEC 61883-5 - 625/50 (PAL)",
        colorimetry: "4:2:0",
        frame_rate: 25.0,
        image_height: 576,
        image_width: 720,
    },
];

/// Extract DV metadata (`Image::ExifTool::DV::ProcessDV`).
pub fn parse_dv_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let file_size = reader.size();
    let want = SCAN_LEN.min(file_size as usize);
    if want == 0 {
        return Err("empty DV file".to_string());
    }
    let buff = reader.read(0, want).map_err(|e| e.to_string())?;

    let start = find_start(&buff).ok_or("no DV DIF header found")?;

    // DV.pm:170-171: must have a full DIF header.
    if start + 80 * 6 > buff.len() {
        return Err("DV file is truncated before a full DIF header".to_string());
    }

    // DV.pm:176-177.
    let dsf = (buff[start + 3] & 0x80) >> 7;
    let stype = buff[start + 80 * 5 + 48 + 3] & 0x1f;

    // DV.pm:180-187. Note the special case reads absolute offset 4, not
    // `$start + 4` -- that is what the Perl says, and it is what decides
    // between the two otherwise-identical 625/50 profiles.
    let profile = if dsf == 1 && stype == 0 && buff.len() > 4 && (buff[4] & 0x07) != 0 {
        &PROFILES[2]
    } else {
        match PROFILES
            .iter()
            .find(|p| p.dsf == dsf && p.video_stype == stype)
        {
            Some(profile) => profile,
            // DV.pm:187: "Unrecognized DV profile" -- the file is valid, it
            // just yields no DV tags.
            None => return Ok(MetadataMap::new()),
        }
    };

    let mut metadata = MetadataMap::new();

    // DV.pm:190-194: total bit rate and duration.
    let byte_rate = profile.frame_size * profile.frame_rate;

    // DV.pm:196-235: scan the VAUX DIF blocks for date/time, aspect ratio
    // and scan type.
    let vaux = scan_vaux(&buff, start);

    // DV.pm:262-265 emits `@dvTags` in a fixed order (DV.pm:114-121). The
    // insertion order here matches so `-a -G1 -s` lists them the same way.

    // DV.pm:236-243: DateTimeOriginal only exists when *both* a valid date
    // and a consecutive time record were found.
    if let (Some(date), Some(time)) = (&vaux.date, &vaux.time) {
        metadata.insert(
            "DV:DateTimeOriginal".to_string(),
            TagValue::new_string(format!("{date} {time}")),
        );
    }

    metadata.insert(
        "DV:ImageWidth".to_string(),
        TagValue::new_integer(i64::from(profile.image_width)),
    );
    metadata.insert(
        "DV:ImageHeight".to_string(),
        TagValue::new_integer(i64::from(profile.image_height)),
    );

    // DV.pm:193, `$$profile{Duration} = $fileSize / $byteRate` -- only when
    // the file size is known, which it always is here.
    if byte_rate > 0.0 {
        metadata.insert(
            "DV:Duration".to_string(),
            TagValue::new_string(convert_duration(file_size as f64 / byte_rate)),
        );
    }
    // DV.pm:192, `$$profile{TotalBitrate} = 8 * $byteRate`.
    metadata.insert(
        "DV:TotalBitrate".to_string(),
        TagValue::new_string(convert_bitrate(8.0 * byte_rate)),
    );

    metadata.insert(
        "DV:VideoFormat".to_string(),
        TagValue::new_string(profile.video_format),
    );

    // DV.pm:238-241: AspectRatio and VideoScanType are only set inside the
    // `$date and $time` branch, and only when a video-control record was
    // seen.
    let aspect = if vaux.date.is_some() && vaux.time.is_some() {
        vaux.is_16_9
    } else {
        None
    };
    if aspect.is_some() {
        metadata.insert(
            "DV:VideoScanType".to_string(),
            TagValue::new_string(if vaux.interlaced {
                "Interlaced"
            } else {
                "Progressive"
            }),
        );
    }

    // DV.pm:138, `PrintConv => 'int($val * 1000 + 0.5) / 1000'`.
    metadata.insert(
        "DV:FrameRate".to_string(),
        TagValue::new_string(perl_number(
            (profile.frame_rate * 1000.0 + 0.5).trunc() / 1000.0,
        )),
    );

    if let Some(is_16_9) = aspect {
        metadata.insert(
            "DV:AspectRatio".to_string(),
            TagValue::new_string(if is_16_9 { "16:9" } else { "4:3" }),
        );
    }

    metadata.insert(
        "DV:Colorimetry".to_string(),
        TagValue::new_string(profile.colorimetry),
    );

    // DV.pm:245-259: audio parameters from the first audio DIF block.
    if let Some(audio) = read_audio(&buff, start) {
        if let Some(channels) = audio.channels {
            metadata.insert(
                "DV:AudioChannels".to_string(),
                TagValue::new_integer(i64::from(channels)),
            );
        }
        if let Some(rate) = audio.sample_rate {
            metadata.insert(
                "DV:AudioSampleRate".to_string(),
                TagValue::new_integer(i64::from(rate)),
            );
        }
        metadata.insert(
            "DV:AudioBitsPerSample".to_string(),
            TagValue::new_integer(i64::from(audio.bits_per_sample)),
        );
    }

    Ok(metadata)
}

/// DV.pm:158-167: find the first DIF block.
///
/// The primary pattern is `\x1f\x07\0[\x3f\xbf]`, whose match start *is* the
/// DIF start (`pos($buff) - 4`). The fallback
/// `[\0\xff]\x3f\x07\0.{76}\xff\x3f\x07\x01` is 84 bytes long and the start
/// is 163 bytes before its end; DV.pm:163 skips a match whose computed start
/// would be negative.
fn find_start(buff: &[u8]) -> Option<usize> {
    if buff.len() >= 4 {
        for i in 0..=buff.len() - 4 {
            if buff[i] == 0x1f
                && buff[i + 1] == 0x07
                && buff[i + 2] == 0x00
                && (buff[i + 3] == 0x3f || buff[i + 3] == 0xbf)
            {
                return Some(i);
            }
        }
    }
    const FALLBACK_LEN: usize = 84;
    if buff.len() >= FALLBACK_LEN {
        for i in 0..=buff.len() - FALLBACK_LEN {
            let head_ok = (buff[i] == 0x00 || buff[i] == 0xff)
                && buff[i + 1] == 0x3f
                && buff[i + 2] == 0x07
                && buff[i + 3] == 0x00;
            if !head_ok {
                continue;
            }
            let t = i + 4 + 76;
            if buff[t] != 0xff || buff[t + 1] != 0x3f || buff[t + 2] != 0x07 || buff[t + 3] != 0x01
            {
                continue;
            }
            // DV.pm:162-165: `pos($buff)` is the end of the match.
            let end = i + FALLBACK_LEN;
            if end < 163 {
                continue;
            }
            return Some(end - 163);
        }
    }
    None
}

/// What DV.pm:196-235's VAUX scan produces.
#[derive(Default)]
struct Vaux {
    date: Option<String>,
    time: Option<String>,
    is_16_9: Option<bool>,
    interlaced: bool,
}

/// DV.pm:200-235.
fn scan_vaux(buff: &[u8], start: usize) -> Vaux {
    let mut out = Vaux::default();
    let mut pos = start;
    for _ in 1..6 {
        pos += 80;
        if pos >= buff.len() {
            break;
        }
        // DV.pm:203, `next unless ($type & 0xf0) == 0x50` -- VAUX blocks only.
        if buff[pos] & 0xf0 != 0x50 {
            continue;
        }
        for j in 0..15usize {
            let p = pos + j * 5 + 3;
            if p + 4 > buff.len() {
                break;
            }
            match buff[p] {
                // DV.pm:206-211: video control.
                0x61 => {
                    if start + 4 >= buff.len() {
                        continue;
                    }
                    let apt = buff[start + 4] & 0x07;
                    let t = buff[p + 2];
                    out.is_16_9 = Some((t & 0x07) == 0x02 || (apt == 0 && (t & 0x07) == 0x07));
                    // DV.pm:210 (ref 2).
                    out.interlaced = buff[p + 3] & 0x10 != 0;
                }
                // DV.pm:212-224: date.
                0x62 => {
                    let d = &buff[p + 1..p + 5];
                    // DV.pm:216: the bytes are BCD; the mask drops the flag
                    // bits that share them.
                    let text = format!("{:02x}:{:02x}:{:02x}", d[3], d[2] & 0x1f, d[1] & 0x3f);
                    if text.bytes().any(|b| b.is_ascii_lowercase()) {
                        // DV.pm:218: a hex digit a-f means the BCD is not a
                        // real date.
                        out.date = None;
                    } else {
                        // DV.pm:221-222: "add century (this will work until
                        // 2089)". The Perl compares the whole string against
                        // '9', which is a comparison on its first character.
                        let century = if text.as_str() < "9" { "20" } else { "19" };
                        out.date = Some(format!("{century}{text}"));
                    }
                    out.time = None;
                }
                // DV.pm:225-231: time, and only immediately after a date.
                0x63 if out.date.is_some() => {
                    let t = &buff[p + 1..p + 5];
                    out.time = Some(format!(
                        "{:02x}:{:02x}:{:02x}",
                        t[3] & 0x3f,
                        t[2] & 0x7f,
                        t[1] & 0x7f
                    ));
                    break;
                }
                // DV.pm:232-234: any other record breaks the date/time
                // adjacency.
                _ => out.time = None,
            }
        }
    }
    out
}

/// What DV.pm:245-259's audio block produces.
struct Audio {
    sample_rate: Option<u32>,
    channels: Option<u32>,
    bits_per_sample: u32,
}

/// DV.pm:248-259.
fn read_audio(buff: &[u8], start: usize) -> Option<Audio> {
    let pos = start + 80 * 6 + 80 * 16 * 3 + 3;
    // DV.pm:249, `if ($pos + 4 < $len and Get8u(\$buff, $pos) == 0x50)`.
    if pos + 4 >= buff.len() || buff[pos] != 0x50 {
        return None;
    }
    let freq = (buff[pos + 4] >> 3) & 0x07;
    let mut stype = buff[pos + 3] & 0x1f;
    let quant = buff[pos + 4] & 0x07;

    // DV.pm:253-255.
    let sample_rate = match freq {
        0 => Some(48000),
        1 => Some(44100),
        2 => Some(32000),
        _ => None,
    };
    // DV.pm:256-259.
    let channels = if stype < 3 {
        // DV.pm:257: `$stype = 2 if $stype == 0 and $quant and $freq == 2`.
        if stype == 0 && quant != 0 && freq == 2 {
            stype = 2;
        }
        match stype {
            0 => Some(2),
            1 => Some(0),
            2 => Some(4),
            _ => None,
        }
    } else {
        None
    };
    Some(Audio {
        sample_rate,
        channels,
        // DV.pm:260, `$$profile{AudioBitsPerSample} = $quant ? 12 : 16`.
        bits_per_sample: if quant != 0 { 12 } else { 16 },
    })
}
