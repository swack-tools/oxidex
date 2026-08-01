//! Sony DSLR-A100 binary-data tables -- generated, do not hand-edit.
//!
//! The A100 writes a Minolta MakerNote, and ExifTool decodes four of its blocks
//! with tables that exist only for this body: `CameraInfoA100` (0x0010),
//! `ISInfoA100` (0x0018), `WBInfoA100` (0x0020) and `CameraSettingsA100`
//! (0x0114). None of them is enciphered. Every row below was read out of
//! ExifTool's own `%Image::ExifTool::Minolta::*` hashes in-process (13.59)
//! rather than retyped, and is interpreted by
//! [`super::sony::binary_data`](crate::parsers::tiff::makernotes::sony::binary_data).

use crate::parsers::tiff::makernotes::sony::binary_data::{
    BinTable, BinTag, Cond, Fmt, Hook, Other, Pc, Raw, Vc,
};

#[rustfmt::skip]
static M0: &[(&str, &str)] = &[("0", "Top-right"), ("1", "Bottom-right"), ("2", "Bottom"), ("3", "Middle Horizontal"), ("4", "Center Vertical"), ("5", "Top"), ("6", "Top-left"), ("7", "Bottom-left")];
#[rustfmt::skip]
static M1: &[(&str, &str)] = &[("-32768", "Out of Focus"), ("0", "In Focus")];
#[rustfmt::skip]
static M2: &[(&str, &str)] = &[("0", "Manual Focus"), ("16", "Continuous Focus"), ("4", "No"), ("64", "Yes")];
#[rustfmt::skip]
static M3: &[(&str, &str)] = &[("0", "Auto"), ("1", "Center"), ("2", "Top"), ("3", "Top-right"), ("4", "Right"), ("5", "Bottom-right"), ("6", "Bottom"), ("7", "Bottom-left"), ("8", "Left"), ("9", "Top-left")];
#[rustfmt::skip]
static M4: &[(&str, &str)] = &[("0", "DMF"), ("1", "AF-S"), ("2", "AF-C"), ("3", "AF-A")];
#[rustfmt::skip]
static M5: &[(&str, &str)] = &[("0", "Wide"), ("1", "Local"), ("2", "Spot")];
#[rustfmt::skip]
static M6: &[(&str, &str)] = &[("0", "Program"), ("1", "Aperture Priority"), ("2", "Shutter Priority"), ("3", "Manual"), ("4", "Auto"), ("4115", "Portrait"), ("4131", "Sports"), ("4147", "Sunset"), ("4163", "Night View/Portrait"), ("4179", "Landscape"), ("4227", "Macro"), ("5", "Program Shift A"), ("6", "Program Shift S")];
#[rustfmt::skip]
static M7: &[(&str, &str)] = &[("0", "Off"), ("1", "On")];
#[rustfmt::skip]
static M8: &[(&str, &str)] = &[("0", "Self-timer 10 sec"), ("1", "Continuous"), ("1794", "Single-frame Bracketing High"), ("1795", "Continuous Bracketing High"), ("4", "Self-timer 2 sec"), ("5", "Single Frame"), ("770", "Single-frame Bracketing Low"), ("771", "Continous Bracketing Low"), ("8", "White Balance Bracketing Low"), ("9", "White Balance Bracketing High")];
#[rustfmt::skip]
static M9: &[(&str, &str)] = &[("0", "Auto"), ("1", "Daylight"), ("2", "Cloudy"), ("256", "Kelvin"), ("3", "Shade"), ("4", "Tungsten"), ("5", "Fluorescent"), ("512", "Manual"), ("6", "Flash")];
#[rustfmt::skip]
static M10: &[(&str, &str)] = &[("0", "AF-S"), ("1", "AF-C"), ("4", "AF-A"), ("5", "Manual"), ("6", "DMF")];
#[rustfmt::skip]
static M11: &[(&str, &str)] = &[("1", "Center"), ("2", "Top"), ("3", "Top-right"), ("4", "Right"), ("5", "Bottom-right"), ("6", "Bottom"), ("7", "Bottom-left"), ("8", "Left"), ("9", "Top-left")];
#[rustfmt::skip]
static M12: &[(&str, &str)] = &[("0", "Auto"), ("2", "Rear Sync"), ("3", "Wireless"), ("4", "Fill Flash")];
#[rustfmt::skip]
static M13: &[(&str, &str)] = &[("0", "Multi-segment"), ("1", "Center-weighted average"), ("2", "Spot")];
#[rustfmt::skip]
static M14: &[(&str, &str)] = &[("0", "Auto"), ("174", "80 (Zone Matching Low)"), ("184", "200 (Zone Matching High)"), ("48", "100"), ("56", "200"), ("64", "400"), ("72", "800"), ("80", "1600")];
#[rustfmt::skip]
static M15: &[(&str, &str)] = &[("0", "Off"), ("1", "Standard"), ("2", "Advanced")];
#[rustfmt::skip]
static M16: &[(&str, &str)] = &[("0", "Standard"), ("1", "Vivid"), ("2", "Portrait"), ("3", "Landscape"), ("4", "Sunset"), ("5", "Night Scene"), ("7", "B&W"), ("8", "Adobe RGB")];
#[rustfmt::skip]
static M17: &[(&str, &str)] = &[("0", "sRGB"), ("2", "B&W"), ("5", "Adobe RGB")];
#[rustfmt::skip]
static M18: &[(&str, &str)] = &[("0", "Normal")];
#[rustfmt::skip]
static M19: &[(&str, &str)] = &[("0", "ADI (Advanced Distance Integration)"), ("1", "Pre-flash TTL")];
#[rustfmt::skip]
static M20: &[(&str, &str)] = &[("0", "AF"), ("1", "Release")];
#[rustfmt::skip]
static M21: &[(&str, &str)] = &[("0", "Single Frame"), ("1", "Continuous"), ("2", "Self-timer"), ("3", "Continuous Bracketing"), ("4", "Single-Frame Bracketing"), ("5", "White Balance Bracketing")];
#[rustfmt::skip]
static M22: &[(&str, &str)] = &[("0", "10 s"), ("4", "2 s")];
#[rustfmt::skip]
static M23: &[(&str, &str)] = &[("1795", "High"), ("771", "Low")];
#[rustfmt::skip]
static M24: &[(&str, &str)] = &[("1794", "High"), ("770", "Low")];
#[rustfmt::skip]
static M25: &[(&str, &str)] = &[("8", "Low"), ("9", "High")];
#[rustfmt::skip]
static M26: &[(&str, &str)] = &[("0", "Auto"), ("1", "Preset"), ("2", "Custom"), ("3", "Color Temperature/Color Filter"), ("32769", "Preset"), ("32770", "Custom"), ("32771", "Color Temperature/Color Filter")];
#[rustfmt::skip]
static M27: &[(&str, &str)] = &[("1", "Daylight"), ("2", "Cloudy"), ("3", "Shade"), ("4", "Tungsten"), ("5", "Fluorescent"), ("6", "Flash")];
#[rustfmt::skip]
static M28: &[(&str, &str)] = &[("0", "Temperature"), ("2", "Color Filter")];
#[rustfmt::skip]
static M29: &[(&str, &str)] = &[("0", "Setup"), ("1", "Recall")];
#[rustfmt::skip]
static M30: &[(&str, &str)] = &[("0", "OK"), ("1", "Error")];
#[rustfmt::skip]
static M31: &[(&str, &str)] = &[("0", "Standard"), ("1", "Medium"), ("2", "Small")];
#[rustfmt::skip]
static M32: &[(&str, &str)] = &[("0", "RAW"), ("32", "Fine"), ("34", "RAW + JPEG"), ("48", "Standard")];
#[rustfmt::skip]
static M33: &[(&str, &str)] = &[("0", "Image and Information"), ("1", "Image Only"), ("3", "Image and Histogram")];
#[rustfmt::skip]
static M34: &[(&str, &str)] = &[("0", "On"), ("1", "Off")];
#[rustfmt::skip]
static M35: &[(&str, &str)] = &[("0", "Auto"), ("1", "Fill Flash")];
#[rustfmt::skip]
static M36: &[(&str, &str)] = &[("0", "0 - +"), ("1", "- 0 +")];
#[rustfmt::skip]
static M37: &[(&str, &str)] = &[("0", "Focus Hold"), ("1", "DOF Preview")];
#[rustfmt::skip]
static M38: &[(&str, &str)] = &[("0", "Hold"), ("1", "Toggle"), ("2", "Spot Hold"), ("3", "Spot Toggle")];
#[rustfmt::skip]
static M39: &[(&str, &str)] = &[("0", "Shutter Speed"), ("1", "Aperture")];
#[rustfmt::skip]
static M40: &[(&str, &str)] = &[("0", "Ambient and Flash"), ("1", "Ambient Only")];
#[rustfmt::skip]
static M41: &[(&str, &str)] = &[("0", "0.3 s"), ("1", "0.6 s"), ("2", "Off")];
#[rustfmt::skip]
static M42: &[(&str, &str)] = &[("0", "Automatic"), ("1", "Manual")];
#[rustfmt::skip]
static M43: &[(&str, &str)] = &[("0", "Auto Rotate"), ("1", "Horizontal")];
#[rustfmt::skip]
static M44: &[(&str, &str)] = &[("0", "Auto Rotate"), ("1", "Manual Rotate")];
#[rustfmt::skip]
static M45: &[(&str, &str)] = &[("0", "Not Indicated"), ("1", "Under Scale"), ("119", "Bottom of Scale"), ("120", "-2.0"), ("121", "-1.7"), ("122", "-1.5"), ("123", "-1.3"), ("124", "-1.0"), ("125", "-0.7"), ("126", "-0.5"), ("127", "-0.3"), ("128", "0"), ("129", "+0.3"), ("130", "+0.5"), ("131", "+0.7"), ("132", "+1.0"), ("133", "+1.3"), ("134", "+1.5"), ("135", "+1.7"), ("136", "+2.0"), ("253", "Top of Scale"), ("254", "Over Scale")];
#[rustfmt::skip]
static M46: &[(&str, &str)] = &[("0", "Within Range"), ("1", "Under/Over Range"), ("255", "Out of Range")];
#[rustfmt::skip]
static M47: &[(&str, &str)] = &[("0", "AF"), ("1", "MF")];
#[rustfmt::skip]
static M48: &[(&str, &str)] = &[("0", "Off"), ("1", "Built-in"), ("2", "External")];
#[rustfmt::skip]
static M49: &[(&str, &str)] = &[("0", "Horizontal (normal)"), ("1", "Rotate 270 CW"), ("2", "Rotate 90 CW")];
#[rustfmt::skip]
static M50: &[(&str, &str)] = &[("3", "Very Low"), ("4", "Low"), ("5", "Half Full"), ("6", "Sufficient Power Remaining")];
#[rustfmt::skip]
static M51: &[(&str, &str)] = &[("0", "Off"), ("10116", "On")];
#[rustfmt::skip]
static M52: &[(&str, &str)] = &[("0", "Self-timer 10 sec"), ("1", "Continuous"), ("2", "Single-frame Exposure Bracketing"), ("3", "Continuous Exposure Bracketing"), ("4", "Self-Timer 2 sec"), ("5", "Single Frame"), ("8", "White Balance Bracketing Low"), ("9", "White Balance Bracketing High")];
#[rustfmt::skip]
static M53: &[(&str, &str)] = &[("0", "Off"), ("1", "Low"), ("2", "High")];
#[rustfmt::skip]
static M54: &[(&str, &str)] = &[("0", "No flash"), ("4613", "Manual"), ("4622", "Strobe"), ("4750", "Fill flash, Pre-flash TTL"), ("4782", "Bounce flash"), ("5134", "Rear sync, ADI"), ("5262", "Fill flash, ADI"), ("5504", "Wireless"), ("6030", "HSS"), ("768", "Built-in flash")];
#[rustfmt::skip]
static M55: &[(&str, &str)] = &[("0", "Standard"), ("1", "Vivid"), ("2", "Portrait"), ("3", "Landscape"), ("4", "Sunset"), ("5", "Night View"), ("7", "B&W"), ("8", "Adobe RGB")];
#[rustfmt::skip]
static M56: &[(&str, &str)] = &[("0", "Minolta AF 28-85mm F3.5-4.5 New"), ("1", "Minolta AF 80-200mm F2.8 HS-APO G"), ("10", "Minolta AF 28-105mm F3.5-4.5 [New]"), ("11", "Minolta AF 300mm F4 HS-APO G"), ("12", "Minolta AF 100mm F2.8 Soft Focus"), ("128", "Tamron or Sigma Lens (128)"), ("128.1", "Tamron AF 18-200mm F3.5-6.3 XR Di II LD Aspherical [IF] Macro"), ("128.10", "Sigma 85mm F1.4 EX DG HSM"), ("128.11", "Sigma 24-70mm F2.8 IF EX DG HSM"), ("128.12", "Sigma 18-250mm F3.5-6.3 DC OS HSM"), ("128.13", "Sigma 17-50mm F2.8 EX DC HSM"), ("128.14", "Sigma 17-70mm F2.8-4 DC Macro HSM"), ("128.15", "Sigma 150mm F2.8 EX DG OS HSM APO Macro"), ("128.16", "Sigma 150-500mm F5-6.3 APO DG OS HSM"), ("128.17", "Tamron AF 28-105mm F4-5.6 [IF]"), ("128.18", "Sigma 35mm F1.4 DG HSM"), ("128.19", "Sigma 18-35mm F1.8 DC HSM"), ("128.2", "Tamron AF 28-300mm F3.5-6.3 XR Di LD Aspherical [IF] Macro"), ("128.20", "Sigma 50-500mm F4.5-6.3 APO DG OS HSM"), ("128.21", "Sigma 24-105mm F4 DG HSM | A"), ("128.22", "Sigma 30mm F1.4"), ("128.23", "Sigma 35mm F1.4 DG HSM | A"), ("128.24", "Sigma 105mm F2.8 EX DG OS HSM Macro"), ("128.25", "Sigma 180mm F2.8 EX DG OS HSM APO Macro"), ("128.26", "Sigma 18-300mm F3.5-6.3 DC Macro HSM | C"), ("128.27", "Sigma 18-50mm F2.8-4.5 DC HSM"), ("128.3", "Tamron AF 28-200mm F3.8-5.6 XR Di Aspherical [IF] Macro"), ("128.4", "Tamron SP AF 17-35mm F2.8-4 Di LD Aspherical IF"), ("128.5", "Sigma AF 50-150mm F2.8 EX DC APO HSM II"), ("128.6", "Sigma 10-20mm F3.5 EX DC HSM"), ("128.7", "Sigma 70-200mm F2.8 II EX DG APO MACRO HSM"), ("128.8", "Sigma 10mm F2.8 EX DC HSM Fisheye"), ("128.9", "Sigma 50mm F1.4 EX DG HSM"), ("129", "Tamron Lens (129)"), ("129.1", "Tamron 200-400mm F5.6 LD"), ("129.2", "Tamron 70-300mm F4-5.6 LD"), ("13", "Minolta AF 75-300mm F4.5-5.6 (New or II)"), ("131", "Tamron 20-40mm F2.7-3.5 SP Aspherical IF"), ("135", "Vivitar 28-210mm F3.5-5.6"), ("136", "Tokina EMZ M100 AF 100mm F3.5"), ("137", "Cosina 70-210mm F2.8-4 AF"), ("138", "Soligor 19-35mm F3.5-4.5"), ("139", "Tokina AF 28-300mm F4-6.3"), ("14", "Minolta AF 100-400mm F4.5-6.7 APO"), ("142", "Cosina AF 70-300mm F4.5-5.6 MC"), ("146", "Voigtlander Macro APO-Lanthar 125mm F2.5 SL"), ("15", "Minolta AF 400mm F4.5 HS-APO G"), ("16", "Minolta AF 17-35mm F3.5 G"), ("17", "Minolta AF 20-35mm F3.5-4.5"), ("18", "Minolta AF 28-80mm F3.5-5.6 II"), ("18688", "Sigma MC-11 SA-E Mount Converter with not-supported Sigma lens"), ("19", "Minolta AF 35mm F1.4 G"), ("194", "Tamron SP AF 17-50mm F2.8 XR Di II LD Aspherical [IF]"), ("2", "Minolta AF 28-70mm F2.8 G"), ("20", "Minolta/Sony 135mm F2.8 [T4.5] STF"), ("202", "Tamron SP AF 70-200mm F2.8 Di LD [IF] Macro"), ("203", "Tamron SP 70-200mm F2.8 Di USD"), ("204", "Tamron SP 24-70mm F2.8 Di USD"), ("212", "Tamron 28-300mm F3.5-6.3 Di PZD"), ("213", "Tamron 16-300mm F3.5-6.3 Di II PZD Macro"), ("214", "Tamron SP 150-600mm F5-6.3 Di USD"), ("215", "Tamron SP 15-30mm F2.8 Di USD"), ("216", "Tamron SP 45mm F1.8 Di USD"), ("217", "Tamron SP 35mm F1.8 Di USD"), ("218", "Tamron SP 90mm F2.8 Di Macro 1:1 USD (F017)"), ("22", "Minolta AF 35-80mm F4-5.6 II"), ("220", "Tamron SP 150-600mm F5-6.3 Di USD G2"), ("224", "Tamron SP 90mm F2.8 Di Macro 1:1 USD (F004)"), ("23", "Minolta AF 200mm F4 Macro APO G"), ("24", "Minolta/Sony AF 24-105mm F3.5-4.5 (D) or Sigma or Tamron Lens"), ("24.1", "Sigma 18-50mm F2.8"), ("24.2", "Sigma 17-70mm F2.8-4.5 DC Macro"), ("24.3", "Sigma 20-40mm F2.8 EX DG Aspherical IF"), ("24.4", "Sigma 18-200mm F3.5-6.3 DC"), ("24.5", "Sigma DC 18-125mm F4-5,6 D"), ("24.6", "Tamron SP AF 28-75mm F2.8 XR Di LD Aspherical [IF] Macro"), ("24.7", "Sigma 15-30mm F3.5-4.5 EX DG Aspherical"), ("25", "Minolta AF 100-300mm F4.5-5.6 APO (D) or Sigma Lens"), ("25.1", "Sigma 100-300mm F4 EX (APO (D) or D IF)"), ("25.2", "Sigma 70mm F2.8 EX DG Macro"), ("25.3", "Sigma 20mm F1.8 EX DG Aspherical RF"), ("25.4", "Sigma 30mm F1.4 EX DC"), ("25.5", "Sigma 24mm F1.8 EX DG ASP Macro"), ("255", "Tamron Lens (255)"), ("255.1", "Tamron SP AF 17-50mm F2.8 XR Di II LD Aspherical"), ("255.2", "Tamron AF 18-250mm F3.5-6.3 XR Di II LD"), ("255.3", "Tamron AF 55-200mm F4-5.6 Di II LD Macro"), ("255.4", "Tamron AF 70-300mm F4-5.6 Di LD Macro 1:2"), ("255.5", "Tamron SP AF 200-500mm F5.0-6.3 Di LD IF"), ("255.6", "Tamron SP AF 10-24mm F3.5-4.5 Di II LD Aspherical IF"), ("255.7", "Tamron SP AF 70-200mm F2.8 Di LD IF Macro"), ("255.8", "Tamron SP AF 28-75mm F2.8 XR Di LD Aspherical IF"), ("255.9", "Tamron AF 90-300mm F4.5-5.6 Telemacro"), ("25501", "Minolta AF 50mm F1.7"), ("25511", "Minolta AF 35-70mm F4 or Other Lens"), ("25511.1", "Sigma UC AF 28-70mm F3.5-4.5"), ("25511.2", "Sigma AF 28-70mm F2.8"), ("25511.3", "Sigma M-AF 70-200mm F2.8 EX Aspherical"), ("25511.4", "Quantaray M-AF 35-80mm F4-5.6"), ("25511.5", "Tokina 28-70mm F2.8-4.5 AF"), ("25521", "Minolta AF 28-85mm F3.5-4.5 or Other Lens"), ("25521.1", "Tokina 19-35mm F3.5-4.5"), ("25521.2", "Tokina 28-70mm F2.8 AT-X"), ("25521.3", "Tokina 80-400mm F4.5-5.6 AT-X AF II 840"), ("25521.4", "Tokina AF PRO 28-80mm F2.8 AT-X 280"), ("25521.5", "Tokina AT-X PRO [II] AF 28-70mm F2.6-2.8 270"), ("25521.6", "Tamron AF 19-35mm F3.5-4.5"), ("25521.7", "Angenieux AF 28-70mm F2.6"), ("25521.8", "Tokina AT-X 17 AF 17mm F3.5"), ("25521.9", "Tokina 20-35mm F3.5-4.5 II AF"), ("25531", "Minolta AF 28-135mm F4-4.5 or Other Lens"), ("25531.1", "Sigma ZOOM-alpha 35-135mm F3.5-4.5"), ("25531.2", "Sigma 28-105mm F2.8-4 Aspherical"), ("25531.3", "Sigma 28-105mm F4-5.6 UC"), ("25531.4", "Tokina AT-X 242 AF 24-200mm F3.5-5.6"), ("25541", "Minolta AF 35-105mm F3.5-4.5"), ("25551", "Minolta AF 70-210mm F4 Macro or Sigma Lens"), ("25551.1", "Sigma 70-210mm F4-5.6 APO"), ("25551.2", "Sigma M-AF 70-200mm F2.8 EX APO"), ("25551.3", "Sigma 75-200mm F2.8-3.5"), ("25561", "Minolta AF 135mm F2.8"), ("25571", "Minolta/Sony AF 28mm F2.8"), ("25581", "Minolta AF 24-50mm F4"), ("25601", "Minolta AF 100-200mm F4.5"), ("25611", "Minolta AF 75-300mm F4.5-5.6 or Sigma Lens"), ("25611.1", "Sigma 70-300mm F4-5.6 DL Macro"), ("25611.10", "Sigma 1000mm F8 APO"), ("25611.2", "Sigma 300mm F4 APO Macro"), ("25611.3", "Sigma AF 500mm F4.5 APO"), ("25611.4", "Sigma AF 170-500mm F5-6.3 APO Aspherical"), ("25611.5", "Tokina AT-X AF 300mm F4"), ("25611.6", "Tokina AT-X AF 400mm F5.6 SD"), ("25611.7", "Tokina AF 730 II 75-300mm F4.5-5.6"), ("25611.8", "Sigma 800mm F5.6 APO"), ("25611.9", "Sigma AF 400mm F5.6 APO Macro"), ("25621", "Minolta AF 50mm F1.4 [New]"), ("25631", "Minolta AF 300mm F2.8 APO or Sigma Lens"), ("25631.1", "Sigma AF 50-500mm F4-6.3 EX DG APO"), ("25631.2", "Sigma AF 170-500mm F5-6.3 APO Aspherical"), ("25631.3", "Sigma AF 500mm F4.5 EX DG APO"), ("25631.4", "Sigma 400mm F5.6 APO"), ("25641", "Minolta AF 50mm F2.8 Macro or Sigma Lens"), ("25641.1", "Sigma 50mm F2.8 EX Macro"), ("25651", "Minolta AF 600mm F4 APO"), ("25661", "Minolta AF 24mm F2.8 or Sigma Lens"), ("25661.1", "Sigma 17-35mm F2.8-4 EX Aspherical"), ("25721", "Minolta/Sony AF 500mm F8 Reflex"), ("25781", "Minolta/Sony AF 16mm F2.8 Fisheye or Sigma Lens"), ("25781.1", "Sigma 8mm F4 EX [DG] Fisheye"), ("25781.2", "Sigma 14mm F3.5"), ("25781.3", "Sigma 15mm F2.8 Fisheye"), ("25791", "Minolta/Sony AF 20mm F2.8 or Tokina Lens"), ("25791.1", "Tokina AT-X Pro DX 11-16mm F2.8"), ("25811", "Minolta AF 100mm F2.8 Macro [New] or Sigma or Tamron Lens"), ("25811.1", "Sigma AF 90mm F2.8 Macro"), ("25811.2", "Sigma AF 105mm F2.8 EX [DG] Macro"), ("25811.3", "Sigma 180mm F5.6 Macro"), ("25811.4", "Sigma 180mm F3.5 EX DG Macro"), ("25811.5", "Tamron 90mm F2.8 Macro"), ("25851", "Beroflex 35-135mm F3.5-4.5"), ("25858", "Minolta AF 35-105mm F3.5-4.5 New or Tamron Lens"), ("25858.1", "Tamron 24-135mm F3.5-5.6"), ("25881", "Minolta AF 70-210mm F3.5-4.5"), ("25891", "Minolta AF 80-200mm F2.8 APO or Tokina Lens"), ("25891.1", "Tokina 80-200mm F2.8"), ("25901", "Minolta AF 200mm F2.8 G APO + Minolta AF 1.4x APO or Other Lens + 1.4x"), ("25901.1", "Minolta AF 600mm F4 HS-APO G + Minolta AF 1.4x APO"), ("25911", "Minolta AF 35mm F1.4"), ("25921", "Minolta AF 85mm F1.4 G (D)"), ("25931", "Minolta AF 200mm F2.8 APO"), ("25941", "Minolta AF 3x-1x F1.7-2.8 Macro"), ("25961", "Minolta AF 28mm F2"), ("25971", "Minolta AF 35mm F2 [New]"), ("25981", "Minolta AF 100mm F2"), ("26011", "Minolta AF 200mm F2.8 G APO + Minolta AF 2x APO or Other Lens + 2x"), ("26011.1", "Minolta AF 600mm F4 HS-APO G + Minolta AF 2x APO"), ("26041", "Minolta AF 80-200mm F4.5-5.6"), ("26051", "Minolta AF 35-80mm F4-5.6"), ("26061", "Minolta AF 100-300mm F4.5-5.6"), ("26071", "Minolta AF 35-80mm F4-5.6"), ("26081", "Minolta AF 300mm F2.8 HS-APO G"), ("26091", "Minolta AF 600mm F4 HS-APO G"), ("26121", "Minolta AF 200mm F2.8 HS-APO G"), ("26131", "Minolta AF 50mm F1.7 New"), ("26151", "Minolta AF 28-105mm F3.5-4.5 xi"), ("26161", "Minolta AF 35-200mm F4.5-5.6 xi"), ("26181", "Minolta AF 28-80mm F4-5.6 xi"), ("26191", "Minolta AF 80-200mm F4.5-5.6 xi"), ("26201", "Minolta AF 28-70mm F2.8 G"), ("26211", "Minolta AF 100-300mm F4.5-5.6 xi"), ("26241", "Minolta AF 35-80mm F4-5.6 Power Zoom"), ("26281", "Minolta AF 80-200mm F2.8 HS-APO G"), ("26291", "Minolta AF 85mm F1.4 New"), ("26311", "Minolta AF 100-300mm F4.5-5.6 APO"), ("26321", "Minolta AF 24-50mm F4 New"), ("26381", "Minolta AF 50mm F2.8 Macro New"), ("26391", "Minolta AF 100mm F2.8 Macro"), ("26411", "Minolta/Sony AF 20mm F2.8 New"), ("26421", "Minolta AF 24mm F2.8 New"), ("26441", "Minolta AF 100-400mm F4.5-6.7 APO"), ("26621", "Minolta AF 50mm F1.4 New"), ("26671", "Minolta AF 35mm F2 New"), ("26681", "Minolta AF 28mm F2 New"), ("26721", "Minolta AF 24-105mm F3.5-4.5 (D)"), ("27", "Minolta AF 85mm F1.4 G (D)"), ("28", "Minolta/Sony AF 100mm F2.8 Macro (D) or Tamron Lens"), ("28.1", "Tamron SP AF 90mm F2.8 Di Macro"), ("28.2", "Tamron SP AF 180mm F3.5 Di LD [IF] Macro"), ("29", "Minolta/Sony AF 75-300mm F4.5-5.6 (D)"), ("3", "Minolta AF 28-80mm F4-5.6"), ("30", "Minolta AF 28-80mm F3.5-5.6 (D) or Sigma Lens"), ("30.1", "Sigma AF 10-20mm F4-5.6 EX DC"), ("30.2", "Sigma AF 12-24mm F4.5-5.6 EX DG"), ("30.3", "Sigma 28-70mm EX DG F2.8"), ("30.4", "Sigma 55-200mm F4-5.6 DC"), ("30464", "Metabones Canon EF Speed Booster"), ("31", "Minolta/Sony AF 50mm F2.8 Macro (D) or F3.5"), ("31.1", "Minolta/Sony AF 50mm F3.5 Macro"), ("32", "Minolta/Sony AF 300mm F2.8 G or 1.5x Teleconverter"), ("33", "Minolta/Sony AF 70-200mm F2.8 G"), ("35", "Minolta AF 85mm F1.4 G (D) Limited"), ("36", "Minolta AF 28-100mm F3.5-5.6 (D)"), ("38", "Minolta AF 17-35mm F2.8-4 (D)"), ("39", "Minolta AF 28-75mm F2.8 (D)"), ("4", "Minolta AF 85mm F1.4G"), ("40", "Minolta/Sony AF DT 18-70mm F3.5-5.6 (D)"), ("41", "Minolta/Sony AF DT 11-18mm F4.5-5.6 (D) or Tamron Lens"), ("41.1", "Tamron SP AF 11-18mm F4.5-5.6 Di II LD Aspherical IF"), ("42", "Minolta/Sony AF DT 18-200mm F3.5-6.3 (D)"), ("43", "Sony 35mm F1.4 G (SAL35F14G)"), ("44", "Sony 50mm F1.4 (SAL50F14)"), ("45", "Carl Zeiss Planar T* 85mm F1.4 ZA (SAL85F14Z)"), ("45671", "Tokina 70-210mm F4-5.6"), ("45681", "Tokina AF 35-200mm F4-5.6 Zoom SD"), ("45701", "Tamron AF 35-135mm F3.5-4.5"), ("45711", "Vivitar 70-210mm F4.5-5.6"), ("45741", "2x Teleconverter or Tamron or Tokina Lens"), ("45741.1", "Tamron SP AF 90mm F2.5"), ("45741.2", "Tokina RF 500mm F8.0 x2"), ("45741.3", "Tokina 300mm F2.8 x2"), ("45751", "1.4x Teleconverter"), ("45851", "Tamron SP AF 300mm F2.8 LD IF"), ("45861", "Tamron SP AF 35-105mm F2.8 LD Aspherical IF"), ("45871", "Tamron AF 70-210mm F2.8 SP LD"), ("46", "Carl Zeiss Vario-Sonnar T* DT 16-80mm F3.5-4.5 ZA (SAL1680Z)"), ("47", "Carl Zeiss Sonnar T* 135mm F1.8 ZA (SAL135F18Z)"), ("48", "Carl Zeiss Vario-Sonnar T* 24-70mm F2.8 ZA SSM (SAL2470Z) or Other Lens"), ("48.1", "Carl Zeiss Vario-Sonnar T* 24-70mm F2.8 ZA SSM II (SAL2470Z2)"), ("48.2", "Tamron SP 24-70mm F2.8 Di USD"), ("48128", "Metabones Canon EF Speed Booster Ultra"), ("49", "Sony DT 55-200mm F4-5.6 (SAL55200)"), ("5", "Minolta AF 35-70mm F3.5-4.5 [II]"), ("50", "Sony DT 18-250mm F3.5-6.3 (SAL18250)"), ("51", "Sony DT 16-105mm F3.5-5.6 (SAL16105)"), ("52", "Sony 70-300mm F4.5-5.6 G SSM (SAL70300G) or G SSM II or Tamron Lens"), ("52.1", "Sony 70-300mm F4.5-5.6 G SSM II (SAL70300G2)"), ("52.2", "Tamron SP 70-300mm F4-5.6 Di USD"), ("53", "Sony 70-400mm F4-5.6 G SSM (SAL70400G)"), ("54", "Carl Zeiss Vario-Sonnar T* 16-35mm F2.8 ZA SSM (SAL1635Z) or ZA SSM II"), ("54.1", "Carl Zeiss Vario-Sonnar T* 16-35mm F2.8 ZA SSM II (SAL1635Z2)"), ("55", "Sony DT 18-55mm F3.5-5.6 SAM (SAL1855) or SAM II"), ("55.1", "Sony DT 18-55mm F3.5-5.6 SAM II (SAL18552)"), ("56", "Sony DT 55-200mm F4-5.6 SAM (SAL55200-2)"), ("57", "Sony DT 50mm F1.8 SAM (SAL50F18) or Tamron Lens or Commlite CM-EF-NEX adapter"), ("57.1", "Tamron SP AF 60mm F2 Di II LD [IF] Macro 1:1"), ("57.2", "Tamron 18-270mm F3.5-6.3 Di II PZD"), ("58", "Sony DT 30mm F2.8 Macro SAM (SAL30M28)"), ("59", "Sony 28-75mm F2.8 SAM (SAL2875)"), ("6", "Minolta AF 24-85mm F3.5-4.5 [New]"), ("60", "Carl Zeiss Distagon T* 24mm F2 ZA SSM (SAL24F20Z)"), ("61", "Sony 85mm F2.8 SAM (SAL85F28)"), ("61184", "Canon EF Adapter"), ("62", "Sony DT 35mm F1.8 SAM (SAL35F18)"), ("63", "Sony DT 16-50mm F2.8 SSM (SAL1650)"), ("64", "Sony 500mm F4 G SSM (SAL500F40G)"), ("65", "Sony DT 18-135mm F3.5-5.6 SAM (SAL18135)"), ("65280", "Sigma 16mm F2.8 Filtermatic Fisheye"), ("65535", "E-Mount, T-Mount, Other Lens or no lens"), ("65535.1", "Arax MC 35mm F2.8 Tilt+Shift"), ("65535.2", "Arax MC 80mm F2.8 Tilt+Shift"), ("65535.3", "Zenitar MF 16mm F2.8 Fisheye M42"), ("65535.4", "Samyang 500mm Mirror F8.0"), ("65535.5", "Pentacon Auto 135mm F2.8"), ("65535.6", "Pentacon Auto 29mm F2.8"), ("65535.7", "Helios 44-2 58mm F2.0"), ("66", "Sony 300mm F2.8 G SSM II (SAL300F28G2)"), ("67", "Sony 70-200mm F2.8 G SSM II (SAL70200G2)"), ("68", "Sony DT 55-300mm F4.5-5.6 SAM (SAL55300)"), ("69", "Sony 70-400mm F4-5.6 G SSM II (SAL70400G2)"), ("7", "Minolta AF 100-300mm F4.5-5.6 APO [New] or 100-400mm or Sigma Lens"), ("7.1", "Minolta AF 100-400mm F4.5-6.7 APO"), ("7.2", "Sigma AF 100-300mm F4 EX DG IF"), ("70", "Carl Zeiss Planar T* 50mm F1.4 ZA SSM (SAL50F14Z)"), ("8", "Minolta AF 70-210mm F4.5-5.6 [II]"), ("9", "Minolta AF 50mm F3.5 Macro")];

#[rustfmt::skip]
static T0: &[BinTag] = &[
    BinTag { index: 1, name: "AFSensorActive", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M0, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 2, name: "AFStatusActiveSensor", cond: Cond::Always, fmt: Fmt::I16, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M1, Other::MinoltaFocus), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 4, name: "AFStatusTop-right", cond: Cond::Always, fmt: Fmt::I16, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M1, Other::MinoltaFocus), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 6, name: "AFStatusBottom-right", cond: Cond::Always, fmt: Fmt::I16, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M1, Other::MinoltaFocus), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 8, name: "AFStatusBottom", cond: Cond::Always, fmt: Fmt::I16, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M1, Other::MinoltaFocus), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 10, name: "AFStatusMiddleHorizontal", cond: Cond::Always, fmt: Fmt::I16, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M1, Other::MinoltaFocus), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 12, name: "AFStatusCenterVertical", cond: Cond::Always, fmt: Fmt::I16, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M1, Other::MinoltaFocus), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 14, name: "AFStatusTop", cond: Cond::Always, fmt: Fmt::I16, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M1, Other::MinoltaFocus), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 16, name: "AFStatusTop-left", cond: Cond::Always, fmt: Fmt::I16, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M1, Other::MinoltaFocus), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 18, name: "AFStatusBottom-left", cond: Cond::Always, fmt: Fmt::I16, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M1, Other::MinoltaFocus), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 20, name: "FocusLocked", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M2, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 21, name: "AFPoint", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M3, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 22, name: "AFMode", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M4, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 45, name: "AFStatusLeft", cond: Cond::Always, fmt: Fmt::I16, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M1, Other::MinoltaFocus), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 47, name: "AFStatusCenterHorizontal", cond: Cond::Always, fmt: Fmt::I16, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M1, Other::MinoltaFocus), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 49, name: "AFStatusRight", cond: Cond::Always, fmt: Fmt::I16, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M1, Other::MinoltaFocus), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 51, name: "AFAreaMode", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M5, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
];
#[rustfmt::skip]
static T1: &[BinTag] = &[
    BinTag { index: 0, name: "ExposureMode", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M6, Other::None), hook: Hook::None, print_hex: true, low_priority: true, subdir: None },
    BinTag { index: 1, name: "ExposureCompensationSetting", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::DivSub(100.0_f64, 3.0_f64), pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 5, name: "HighSpeedSync", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M7, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 6, name: "ShutterSpeedSetting", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::ExpTime(6.0_f64, 8.0_f64), pc: Pc::ExposureTimeOrBulb, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 7, name: "ApertureSetting", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::Pow2DivSubHalf(8.0_f64, 1.0_f64), pc: Pc::FNumber, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 8, name: "ExposureTime", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::ExpTime(6.0_f64, 8.0_f64), pc: Pc::ExposureTimeOrBulb, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 9, name: "FNumber", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::Pow2DivSubHalf(8.0_f64, 1.0_f64), pc: Pc::FNumber, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 10, name: "DriveMode2", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M8, Other::None), hook: Hook::None, print_hex: true, low_priority: true, subdir: None },
    BinTag { index: 11, name: "WhiteBalance", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M9, Other::None), hook: Hook::None, print_hex: true, low_priority: true, subdir: None },
    BinTag { index: 12, name: "FocusMode", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M10, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 13, name: "AFPointSelected", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M11, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 14, name: "AFAreaMode", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M5, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 15, name: "FlashMode", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M12, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 16, name: "FlashExposureCompSet", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::DivSub(100.0_f64, 3.0_f64), pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 18, name: "MeteringMode", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M13, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 19, name: "ISOSetting", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M14, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 20, name: "ZoneMatchingMode", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M15, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 21, name: "DynamicRangeOptimizer", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M15, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 22, name: "ColorMode", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M16, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 23, name: "ColorSpace", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M17, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 24, name: "Sharpness", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::Add(-10.0_f64), pc: Pc::Map(M18, Other::ExifParameter), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 25, name: "Contrast", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::Add(-10.0_f64), pc: Pc::Map(M18, Other::ExifParameter), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 26, name: "Saturation", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::Add(-10.0_f64), pc: Pc::Map(M18, Other::ExifParameter), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 28, name: "FlashMetering", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M19, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 29, name: "PrioritySetupShutterRelease", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M20, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 30, name: "DriveMode", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M21, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 31, name: "SelfTimerTime", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M22, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 32, name: "ContinuousBracketing", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M23, Other::None), hook: Hook::None, print_hex: true, low_priority: true, subdir: None },
    BinTag { index: 33, name: "SingleFrameBracketing", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M24, Other::None), hook: Hook::None, print_hex: true, low_priority: true, subdir: None },
    BinTag { index: 34, name: "WhiteBalanceBracketing", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M25, Other::None), hook: Hook::None, print_hex: true, low_priority: true, subdir: None },
    BinTag { index: 35, name: "WhiteBalanceSetting", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M26, Other::None), hook: Hook::None, print_hex: true, low_priority: true, subdir: None },
    BinTag { index: 36, name: "PresetWhiteBalance", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M27, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 37, name: "ColorTemperatureSetting", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M28, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 38, name: "CustomWBSetting", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M29, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 39, name: "DynamicRangeOptimizerSetting", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M15, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 50, name: "FreeMemoryCardImages", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 52, name: "CustomWBRedLevel", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 53, name: "CustomWBGreenLevel", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 54, name: "CustomWBBlueLevel", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 55, name: "CustomWBError", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M30, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 56, name: "WhiteBalanceFineTune", cond: Cond::Always, fmt: Fmt::I16, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 57, name: "ColorTemperature", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::Mul(100.0_f64), pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 58, name: "ColorCompensationFilter", cond: Cond::Always, fmt: Fmt::I16, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 59, name: "SonyImageSize", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M31, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 60, name: "SonyQuality", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M32, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 61, name: "InstantPlaybackTime", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Suffix(" s"), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 62, name: "InstantPlaybackSetup", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M33, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 63, name: "NoiseReduction", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M7, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 64, name: "EyeStartAF", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M34, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 65, name: "RedEyeReduction", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M7, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 66, name: "FlashDefault", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M35, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 67, name: "AutoBracketOrder", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M36, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 68, name: "FocusHoldButton", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M37, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 69, name: "AELButton", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M38, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 70, name: "ControlDialSet", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M39, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 71, name: "ExposureCompensationMode", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M40, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 72, name: "AFAssist", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M34, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 73, name: "CardShutterLock", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M34, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 74, name: "LensShutterLock", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M34, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 75, name: "AFAreaIllumination", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M41, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 76, name: "MonitorDisplayOff", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M42, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 77, name: "RecordDisplay", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M43, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 78, name: "PlayDisplay", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M44, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 80, name: "ExposureIndicator", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M45, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 81, name: "AELExposureIndicator", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M45, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 82, name: "ExposureBracketingIndicatorLast", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M45, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 83, name: "MeteringOffScaleIndicator", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M46, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 84, name: "FlashExposureIndicator", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M45, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 85, name: "FlashExposureIndicatorNext", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M45, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 86, name: "FlashExposureIndicatorLast", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M45, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 87, name: "ImageStabilization", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M7, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 88, name: "FocusModeSwitch", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M47, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 89, name: "FlashType", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M48, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 90, name: "Rotation", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M49, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 91, name: "AELock", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M7, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 94, name: "ColorTemperature", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::Mul(100.0_f64), pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 95, name: "ColorCompensationFilter", cond: Cond::Always, fmt: Fmt::I16, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 96, name: "BatteryState", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M50, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
];
#[rustfmt::skip]
static T2: &[BinTag] = &[
    BinTag { index: 0, name: "ImageStabilization", cond: Cond::Always, fmt: Fmt::U16, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M51, Other::None), hook: Hook::None, print_hex: true, low_priority: true, subdir: None },
];
#[rustfmt::skip]
static T3: &[BinTag] = &[
    BinTag { index: 14, name: "DriveMode", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M52, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 16, name: "Rotation", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M49, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 20, name: "ImageStabilizationSetting", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M7, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 21, name: "DynamicRangeOptimizerMode", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M15, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 42, name: "ExposureCompensationMode", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M40, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 43, name: "WBBracketShotNumber", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 44, name: "WhiteBalanceBracketing", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M53, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 45, name: "ExposureBracketShotNumber", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 49, name: "FlashFunction", cond: Cond::Always, fmt: Fmt::U16, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M54, Other::None), hook: Hook::None, print_hex: true, low_priority: true, subdir: None },
    BinTag { index: 52, name: "ExposureMode", cond: Cond::Always, fmt: Fmt::U16, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M6, Other::None), hook: Hook::None, print_hex: true, low_priority: true, subdir: None },
    BinTag { index: 54, name: "ColorMode", cond: Cond::Always, fmt: Fmt::U16, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M55, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 56, name: "AverageLV", cond: Cond::Always, fmt: Fmt::U16, count: 1, mask: 0, raw: Raw::None, vc: Vc::SubDiv(106.0_f64, 8.0_f64), pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 60, name: "FrameNumber", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 150, name: "WB_RGBLevels", cond: Cond::Always, fmt: Fmt::U16, count: 3, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 174, name: "WB_GBRGLevels", cond: Cond::Always, fmt: Fmt::U16, count: 4, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 192, name: "WB_RedLevelsTungsten", cond: Cond::Always, fmt: Fmt::U16, count: 7, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 206, name: "WB_BlueLevelsTungsten", cond: Cond::Always, fmt: Fmt::U16, count: 7, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 220, name: "WB_RedLevelsDaylight", cond: Cond::Always, fmt: Fmt::U16, count: 7, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 234, name: "WB_BlueLevelsDaylight", cond: Cond::Always, fmt: Fmt::U16, count: 7, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 248, name: "WB_RedLevelsCloudy", cond: Cond::Always, fmt: Fmt::U16, count: 7, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 262, name: "WB_BlueLevelsCloudy", cond: Cond::Always, fmt: Fmt::U16, count: 7, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 276, name: "WB_RedLevelsFlash", cond: Cond::Always, fmt: Fmt::U16, count: 7, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 290, name: "WB_BlueLevelsFlash", cond: Cond::Always, fmt: Fmt::U16, count: 7, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 332, name: "WB_RedLevelsFluorescent", cond: Cond::Always, fmt: Fmt::U16, count: 7, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 346, name: "WB_BlueLevelsFluorescent", cond: Cond::Always, fmt: Fmt::U16, count: 7, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 360, name: "WB_RedLevelsShade", cond: Cond::Always, fmt: Fmt::U16, count: 7, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 374, name: "WB_BlueLevelsShade", cond: Cond::Always, fmt: Fmt::U16, count: 7, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 392, name: "WB_RedLevel6500K", cond: Cond::Always, fmt: Fmt::U16, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 394, name: "WB_BlueLevel6500K", cond: Cond::Always, fmt: Fmt::U16, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 396, name: "WB_RedLevelCustom", cond: Cond::Always, fmt: Fmt::U16, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 398, name: "WB_BlueLevelCustom", cond: Cond::Always, fmt: Fmt::U16, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 408, name: "WB_RedLevel3500K", cond: Cond::Always, fmt: Fmt::U16, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 410, name: "WB_BlueLevel3500K", cond: Cond::Always, fmt: Fmt::U16, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 446, name: "WB_RedLevelsKelvin", cond: Cond::Always, fmt: Fmt::U16, count: 75, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 596, name: "WB_BlueLevelsKelvin", cond: Cond::Always, fmt: Fmt::U16, count: 75, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 772, name: "WB_RBLevelsFlash", cond: Cond::Always, fmt: Fmt::U16, count: 2, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 776, name: "WB_RBLevelsCoolWhiteF", cond: Cond::Always, fmt: Fmt::U16, count: 2, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 1000, name: "WB_RBLevelsTungsten", cond: Cond::Always, fmt: Fmt::U16, count: 2, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 1004, name: "WB_RBLevelsDaylight", cond: Cond::Always, fmt: Fmt::U16, count: 2, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 1008, name: "WB_RBLevelsCloudy", cond: Cond::Always, fmt: Fmt::U16, count: 2, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 1012, name: "WB_RBLevelsFlash", cond: Cond::Always, fmt: Fmt::U16, count: 2, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 1020, name: "WB_RedLevelsFluorescent", cond: Cond::Always, fmt: Fmt::U16, count: 7, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 1034, name: "WB_BlueLevelsFluorescent", cond: Cond::Always, fmt: Fmt::U16, count: 7, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 1048, name: "WB_RBLevelsShade", cond: Cond::Always, fmt: Fmt::U16, count: 2, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 1056, name: "WB_RBLevels6500K", cond: Cond::Always, fmt: Fmt::U16, count: 2, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 1060, name: "WB_RBLevelsCustom", cond: Cond::Always, fmt: Fmt::U16, count: 2, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 1072, name: "WB_RBLevels3500K", cond: Cond::Always, fmt: Fmt::U16, count: 2, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 1320, name: "WB_RBLevelsDaylight", cond: Cond::Always, fmt: Fmt::U16, count: 2, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 1350, name: "WB_RGBLevels", cond: Cond::Always, fmt: Fmt::U16, count: 3, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 1576, name: "AEMeteringSegments", cond: Cond::Always, fmt: Fmt::U8, count: 40, mask: 0, raw: Raw::None, vc: Vc::EachSubDiv(106.0_f64, 8.0_f64), pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 1680, name: "MeasuredLV", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::SubDiv(106.0_f64, 8.0_f64), pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 1681, name: "BrightnessValue", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::SubDiv(106.0_f64, 8.0_f64), pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 18872, name: "ExposureTime", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::ExpTime(6.0_f64, 8.0_f64), pc: Pc::ExposureTimeOrBulb, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 18874, name: "ISO", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::Pow2SubDivMul(48.0_f64, 8.0_f64, 100.0_f64), pc: Pc::RoundHalfUp, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 18875, name: "FocusDistance", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::Pow2SubDiv(126.0_f64, 16.0_f64), pc: Pc::InfAboveOrMeters(266.0), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 18877, name: "LensType", cond: Cond::Always, fmt: Fmt::U16Rev, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M56, Other::MinoltaLens), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 18880, name: "ExposureCompensation", cond: Cond::Always, fmt: Fmt::I8, count: 1, mask: 0, raw: Raw::None, vc: Vc::Div(8.0_f64), pc: Pc::Signed1OrZero, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 18881, name: "FlashExposureComp", cond: Cond::Always, fmt: Fmt::I8, count: 1, mask: 0, raw: Raw::None, vc: Vc::Div(8.0_f64), pc: Pc::Signed1OrZero, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 18882, name: "ImageStabilization", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M7, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 18883, name: "BrightnessValue", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::SubDiv(106.0_f64, 8.0_f64), pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 18885, name: "MaxAperture", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::Pow2SubDiv(8.0_f64, 16.0_f64), pc: Pc::Fixed(1), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 18887, name: "FNumber", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::Pow2SubDiv(8.0_f64, 16.0_f64), pc: Pc::Fixed(1), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 18908, name: "InternalSerialNumber", cond: Cond::Always, fmt: Fmt::Str, count: 12, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
];

/// Every table, indexed by the `SubDir`/`Root` table numbers above.
pub static TABLES: &[BinTable] = &[
    BinTable {
        name: "CameraInfoA100",
        fmt: Fmt::Default,
        tags: T0,
    },
    BinTable {
        name: "CameraSettingsA100",
        fmt: Fmt::U16,
        tags: T1,
    },
    BinTable {
        name: "ISInfoA100",
        fmt: Fmt::Default,
        tags: T2,
    },
    BinTable {
        name: "WBInfoA100",
        fmt: Fmt::Default,
        tags: T3,
    },
];

/// Table numbers, by ExifTool table name.
#[allow(dead_code)]
pub mod idx {
    pub const CAMERAINFOA100: usize = 0;
    pub const CAMERASETTINGSA100: usize = 1;
    pub const ISINFOA100: usize = 2;
    pub const WBINFOA100: usize = 3;
}
