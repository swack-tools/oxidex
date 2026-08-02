//! RIFF/ASF "TwoCC" audio encoding codes -- the single implementation.
//!
//! WAV's `fmt ` chunk and AVI's audio `strf` both carry the same 16-bit
//! `wFormatTag`, and ExifTool prints both through one table:
//! `%Image::ExifTool::RIFF::AudioFormat{0}{PrintConv}`, whose own Notes say
//! *these codes are used in RIFF and ASF files*.
//!
//! The table below is that PrintConv, dumped verbatim from the installed
//! ExifTool 13.55 -- all 243 entries, in code order. It is transcribed by
//! script rather than by hand precisely because hand-copies are what produced
//! the two disagreeing versions this replaces.

/// ExifTool's `%RIFF::audioEncoding`, as `(code, name)` pairs sorted by code.
const AUDIO_ENCODING: &[(u16, &str)] = &[
    (0x0001, "Microsoft PCM"),
    (0x0002, "Microsoft ADPCM"),
    (0x0003, "Microsoft IEEE float"),
    (0x0004, "Compaq VSELP"),
    (0x0005, "IBM CVSD"),
    (0x0006, "Microsoft a-Law"),
    (0x0007, "Microsoft u-Law"),
    (0x0008, "Microsoft DTS"),
    (0x0009, "DRM"),
    (0x000a, "WMA 9 Speech"),
    (0x000b, "Microsoft Windows Media RT Voice"),
    (0x0010, "OKI-ADPCM"),
    (0x0011, "Intel IMA/DVI-ADPCM"),
    (0x0012, "Videologic Mediaspace ADPCM"),
    (0x0013, "Sierra ADPCM"),
    (0x0014, "Antex G.723 ADPCM"),
    (0x0015, "DSP Solutions DIGISTD"),
    (0x0016, "DSP Solutions DIGIFIX"),
    (0x0017, "Dialoic OKI ADPCM"),
    (0x0018, "Media Vision ADPCM"),
    (0x0019, "HP CU"),
    (0x001a, "HP Dynamic Voice"),
    (0x0020, "Yamaha ADPCM"),
    (0x0021, "SONARC Speech Compression"),
    (0x0022, "DSP Group True Speech"),
    (0x0023, "Echo Speech Corp."),
    (0x0024, "Virtual Music Audiofile AF36"),
    (0x0025, "Audio Processing Tech."),
    (0x0026, "Virtual Music Audiofile AF10"),
    (0x0027, "Aculab Prosody 1612"),
    (0x0028, "Merging Tech. LRC"),
    (0x0030, "Dolby AC2"),
    (0x0031, "Microsoft GSM610"),
    (0x0032, "MSN Audio"),
    (0x0033, "Antex ADPCME"),
    (0x0034, "Control Resources VQLPC"),
    (0x0035, "DSP Solutions DIGIREAL"),
    (0x0036, "DSP Solutions DIGIADPCM"),
    (0x0037, "Control Resources CR10"),
    (0x0038, "Natural MicroSystems VBX ADPCM"),
    (0x0039, "Crystal Semiconductor IMA ADPCM"),
    (0x003a, "Echo Speech ECHOSC3"),
    (0x003b, "Rockwell ADPCM"),
    (0x003c, "Rockwell DIGITALK"),
    (0x003d, "Xebec Multimedia"),
    (0x0040, "Antex G.721 ADPCM"),
    (0x0041, "Antex G.728 CELP"),
    (0x0042, "Microsoft MSG723"),
    (0x0043, "IBM AVC ADPCM"),
    (0x0045, "ITU-T G.726"),
    (0x0050, "Microsoft MPEG"),
    (0x0051, "RT23 or PAC"),
    (0x0052, "InSoft RT24"),
    (0x0053, "InSoft PAC"),
    (0x0055, "MP3"),
    (0x0059, "Cirrus"),
    (0x0060, "Cirrus Logic"),
    (0x0061, "ESS Tech. PCM"),
    (0x0062, "Voxware Inc."),
    (0x0063, "Canopus ATRAC"),
    (0x0064, "APICOM G.726 ADPCM"),
    (0x0065, "APICOM G.722 ADPCM"),
    (0x0066, "Microsoft DSAT"),
    (0x0067, "Microsoft DSAT DISPLAY"),
    (0x0069, "Voxware Byte Aligned"),
    (0x0070, "Voxware AC8"),
    (0x0071, "Voxware AC10"),
    (0x0072, "Voxware AC16"),
    (0x0073, "Voxware AC20"),
    (0x0074, "Voxware MetaVoice"),
    (0x0075, "Voxware MetaSound"),
    (0x0076, "Voxware RT29HW"),
    (0x0077, "Voxware VR12"),
    (0x0078, "Voxware VR18"),
    (0x0079, "Voxware TQ40"),
    (0x007a, "Voxware SC3"),
    (0x007b, "Voxware SC3"),
    (0x0080, "Soundsoft"),
    (0x0081, "Voxware TQ60"),
    (0x0082, "Microsoft MSRT24"),
    (0x0083, "AT&T G.729A"),
    (0x0084, "Motion Pixels MVI MV12"),
    (0x0085, "DataFusion G.726"),
    (0x0086, "DataFusion GSM610"),
    (0x0088, "Iterated Systems Audio"),
    (0x0089, "Onlive"),
    (0x008a, "Multitude, Inc. FT SX20"),
    (0x008b, "Infocom ITS A/S G.721 ADPCM"),
    (0x008c, "Convedia G729"),
    (0x008d, "Not specified congruency, Inc."),
    (0x0091, "Siemens SBC24"),
    (0x0092, "Sonic Foundry Dolby AC3 APDIF"),
    (0x0093, "MediaSonic G.723"),
    (0x0094, "Aculab Prosody 8kbps"),
    (0x0097, "ZyXEL ADPCM"),
    (0x0098, "Philips LPCBB"),
    (0x0099, "Studer Professional Audio Packed"),
    (0x00a0, "Malden PhonyTalk"),
    (0x00a1, "Racal Recorder GSM"),
    (0x00a2, "Racal Recorder G720.a"),
    (0x00a3, "Racal G723.1"),
    (0x00a4, "Racal Tetra ACELP"),
    (0x00b0, "NEC AAC NEC Corporation"),
    (0x00ff, "AAC"),
    (0x0100, "Rhetorex ADPCM"),
    (0x0101, "IBM u-Law"),
    (0x0102, "IBM a-Law"),
    (0x0103, "IBM ADPCM"),
    (0x0111, "Vivo G.723"),
    (0x0112, "Vivo Siren"),
    (0x0120, "Philips Speech Processing CELP"),
    (0x0121, "Philips Speech Processing GRUNDIG"),
    (0x0123, "Digital G.723"),
    (0x0125, "Sanyo LD ADPCM"),
    (0x0130, "Sipro Lab ACEPLNET"),
    (0x0131, "Sipro Lab ACELP4800"),
    (0x0132, "Sipro Lab ACELP8V3"),
    (0x0133, "Sipro Lab G.729"),
    (0x0134, "Sipro Lab G.729A"),
    (0x0135, "Sipro Lab Kelvin"),
    (0x0136, "VoiceAge AMR"),
    (0x0140, "Dictaphone G.726 ADPCM"),
    (0x0150, "Qualcomm PureVoice"),
    (0x0151, "Qualcomm HalfRate"),
    (0x0155, "Ring Zero Systems TUBGSM"),
    (0x0160, "Microsoft Audio1"),
    (
        0x0161,
        "Windows Media Audio V2 V7 V8 V9 / DivX audio (WMA) / Alex AC3 Audio",
    ),
    (0x0162, "Windows Media Audio Professional V9"),
    (0x0163, "Windows Media Audio Lossless V9"),
    (0x0164, "WMA Pro over S/PDIF"),
    (0x0170, "UNISYS NAP ADPCM"),
    (0x0171, "UNISYS NAP ULAW"),
    (0x0172, "UNISYS NAP ALAW"),
    (0x0173, "UNISYS NAP 16K"),
    (0x0174, "MM SYCOM ACM SYC008 SyCom Technologies"),
    (0x0175, "MM SYCOM ACM SYC701 G726L SyCom Technologies"),
    (0x0176, "MM SYCOM ACM SYC701 CELP54 SyCom Technologies"),
    (0x0177, "MM SYCOM ACM SYC701 CELP68 SyCom Technologies"),
    (0x0178, "Knowledge Adventure ADPCM"),
    (0x0180, "Fraunhofer IIS MPEG2AAC"),
    (0x0190, "Digital Theater Systems DTS DS"),
    (0x0200, "Creative Labs ADPCM"),
    (0x0202, "Creative Labs FASTSPEECH8"),
    (0x0203, "Creative Labs FASTSPEECH10"),
    (0x0210, "UHER ADPCM"),
    (0x0215, "Ulead DV ACM"),
    (0x0216, "Ulead DV ACM"),
    (0x0220, "Quarterdeck Corp."),
    (0x0230, "I-Link VC"),
    (0x0240, "Aureal Semiconductor Raw Sport"),
    (0x0241, "ESST AC3"),
    (0x0250, "Interactive Products HSX"),
    (0x0251, "Interactive Products RPELP"),
    (0x0260, "Consistent CS2"),
    (0x0270, "Sony SCX"),
    (0x0271, "Sony SCY"),
    (0x0272, "Sony ATRAC3"),
    (0x0273, "Sony SPC"),
    (0x0280, "TELUM Telum Inc."),
    (0x0281, "TELUMIA Telum Inc."),
    (0x0285, "Norcom Voice Systems ADPCM"),
    (0x0300, "Fujitsu FM TOWNS SND"),
    (0x0301, "Fujitsu (not specified)"),
    (0x0302, "Fujitsu (not specified)"),
    (0x0303, "Fujitsu (not specified)"),
    (0x0304, "Fujitsu (not specified)"),
    (0x0305, "Fujitsu (not specified)"),
    (0x0306, "Fujitsu (not specified)"),
    (0x0307, "Fujitsu (not specified)"),
    (0x0308, "Fujitsu (not specified)"),
    (0x0350, "Micronas Semiconductors, Inc. Development"),
    (0x0351, "Micronas Semiconductors, Inc. CELP833"),
    (0x0400, "Brooktree Digital"),
    (0x0401, "Intel Music Coder (IMC)"),
    (0x0402, "Ligos Indeo Audio"),
    (0x0450, "QDesign Music"),
    (0x0500, "On2 VP7 On2 Technologies"),
    (0x0501, "On2 VP6 On2 Technologies"),
    (0x0680, "AT&T VME VMPCM"),
    (0x0681, "AT&T TCP"),
    (0x0700, "YMPEG Alpha (dummy for MPEG-2 compressor)"),
    (0x08ae, "ClearJump LiteWave (lossless)"),
    (0x1000, "Olivetti GSM"),
    (0x1001, "Olivetti ADPCM"),
    (0x1002, "Olivetti CELP"),
    (0x1003, "Olivetti SBC"),
    (0x1004, "Olivetti OPR"),
    (0x1100, "Lernout & Hauspie"),
    (0x1101, "Lernout & Hauspie CELP codec"),
    (0x1102, "Lernout & Hauspie SBC codec"),
    (0x1103, "Lernout & Hauspie SBC codec"),
    (0x1104, "Lernout & Hauspie SBC codec"),
    (0x1400, "Norris Comm. Inc."),
    (0x1401, "ISIAudio"),
    (0x1500, "AT&T Soundspace Music Compression"),
    (0x181c, "VoxWare RT24 speech codec"),
    (0x181e, "Lucent elemedia AX24000P Music codec"),
    (0x1971, "Sonic Foundry LOSSLESS"),
    (0x1979, "Innings Telecom Inc. ADPCM"),
    (0x1c07, "Lucent SX8300P speech codec"),
    (0x1c0c, "Lucent SX5363S G.723 compliant codec"),
    (0x1f03, "CUseeMe DigiTalk (ex-Rocwell)"),
    (0x1fc4, "NCT Soft ALF2CD ACM"),
    (0x2000, "FAST Multimedia DVM"),
    (0x2001, "Dolby DTS (Digital Theater System)"),
    (0x2002, "RealAudio 1 / 2 14.4"),
    (0x2003, "RealAudio 1 / 2 28.8"),
    (0x2004, "RealAudio G2 / 8 Cook (low bitrate)"),
    (0x2005, "RealAudio 3 / 4 / 5 Music (DNET)"),
    (0x2006, "RealAudio 10 AAC (RAAC)"),
    (0x2007, "RealAudio 10 AAC+ (RACP)"),
    (0x2500, "Reserved range to 0x2600 Microsoft"),
    (
        0x3313,
        "makeAVIS (ffvfw fake AVI sound from AviSynth scripts)",
    ),
    (0x4143, "Divio MPEG-4 AAC audio"),
    (0x4201, "Nokia adaptive multirate"),
    (0x4243, "Divio G726 Divio, Inc."),
    (0x434c, "LEAD Speech"),
    (0x564c, "LEAD Vorbis"),
    (0x5756, "WavPack Audio"),
    (0x674f, "Ogg Vorbis (mode 1)"),
    (0x6750, "Ogg Vorbis (mode 2)"),
    (0x6751, "Ogg Vorbis (mode 3)"),
    (0x676f, "Ogg Vorbis (mode 1+)"),
    (0x6770, "Ogg Vorbis (mode 2+)"),
    (0x6771, "Ogg Vorbis (mode 3+)"),
    (0x7000, "3COM NBX 3Com Corporation"),
    (0x706d, "FAAD AAC"),
    (0x7a21, "GSM-AMR (CBR, no SID)"),
    (0x7a22, "GSM-AMR (VBR, including SID)"),
    (0xa100, "Comverse Infosys Ltd. G723 1"),
    (0xa101, "Comverse Infosys Ltd. AVQSBC"),
    (0xa102, "Comverse Infosys Ltd. OLDSBC"),
    (0xa103, "Symbol Technologies G729A"),
    (0xa104, "VoiceAge AMR WB VoiceAge Corporation"),
    (0xa105, "Ingenient Technologies Inc. G726"),
    (0xa106, "ISO/MPEG-4 advanced audio Coding"),
    (0xa107, "Encore Software Ltd G726"),
    (0xa109, "Speex ACM Codec xiph.org"),
    (0xdfac, "DebugMode SonicFoundry Vegas FrameServer ACM Codec"),
    (0xe708, "Unknown -"),
    (0xf1ac, "Free Lossless Audio Codec FLAC"),
    (0xfffe, "Extensible"),
    (0xffff, "Development"),
];

/// Names a RIFF/ASF audio encoding code the way ExifTool does.
///
/// A code the table does not name prints as `Unknown (N)` in decimal, which is
/// ExifTool's default for an unmatched PrintConv key -- the tag is never
/// dropped and never given a bare `"Unknown"` that loses the code.
///
/// # Examples
///
/// ```
/// use oxidex::core::formatters::audio_encoding::audio_encoding_name;
///
/// assert_eq!(audio_encoding_name(0x0001), "Microsoft PCM");
/// // 0x55 is MP3 -- not "MPEG" and not "MPEG Layer 3"
/// assert_eq!(audio_encoding_name(0x0055), "MP3");
/// assert_eq!(audio_encoding_name(0x0050), "Microsoft MPEG");
/// assert_eq!(audio_encoding_name(0x1234), "Unknown (4660)");
/// ```
pub fn audio_encoding_name(code: u16) -> String {
    match AUDIO_ENCODING.binary_search_by_key(&code, |(c, _)| *c) {
        Ok(i) => AUDIO_ENCODING[i].1.to_string(),
        Err(_) => format!("Unknown ({})", code),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The table must stay sorted, or the binary search silently misses.
    #[test]
    fn test_table_is_sorted_and_complete() {
        assert_eq!(AUDIO_ENCODING.len(), 243);
        assert!(AUDIO_ENCODING.windows(2).all(|w| w[0].0 < w[1].0));
    }

    /// The codes the two copies this replaces disagreed on. Every expected
    /// string here came out of ExifTool 13.55's own PrintConv hash.
    #[test]
    fn test_codes_the_duplicates_got_wrong() {
        // `wav.rs` said "MPEG"; `avi.rs` said "MPEG Layer 3". Both wrong.
        assert_eq!(audio_encoding_name(0x0055), "MP3");
        // `avi.rs` said "MPEG"; `wav.rs` had no entry at all.
        assert_eq!(audio_encoding_name(0x0050), "Microsoft MPEG");
        // `wav.rs` invented this one; ExifTool names it something else.
        assert_eq!(audio_encoding_name(0x0069), "Voxware Byte Aligned");
        // Labels both copies paraphrased.
        assert_eq!(audio_encoding_name(0x0006), "Microsoft a-Law");
        assert_eq!(audio_encoding_name(0x0007), "Microsoft u-Law");
        assert_eq!(audio_encoding_name(0x0011), "Intel IMA/DVI-ADPCM");
        assert_eq!(audio_encoding_name(0x0016), "DSP Solutions DIGIFIX");
        assert_eq!(audio_encoding_name(0x0031), "Microsoft GSM610");
        assert_eq!(audio_encoding_name(0x0040), "Antex G.721 ADPCM");
        assert_eq!(
            audio_encoding_name(0x0161),
            "Windows Media Audio V2 V7 V8 V9 / DivX audio (WMA) / Alex AC3 Audio"
        );
        assert_eq!(audio_encoding_name(0xfffe), "Extensible");
    }

    #[test]
    fn test_common_codes() {
        assert_eq!(audio_encoding_name(0x0001), "Microsoft PCM");
        assert_eq!(audio_encoding_name(0x0002), "Microsoft ADPCM");
        assert_eq!(audio_encoding_name(0x0003), "Microsoft IEEE float");
    }

    /// An unnamed code reports itself. `wav.rs` returned a bare "Unknown" and
    /// `avi.rs` returned an empty string, which reaches the output as an
    /// `Encoding` tag with no value at all.
    #[test]
    fn test_unknown_codes_report_themselves() {
        assert_eq!(audio_encoding_name(0x1234), "Unknown (4660)");
        assert_eq!(audio_encoding_name(0x0000), "Unknown (0)");
        assert_ne!(audio_encoding_name(0x1234), "");
    }
}
