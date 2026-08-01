//! Sony plain (unenciphered) binary-data tables -- generated, do not hand-edit.
//!
//! `ShotInfo` (0x3000, with its `FaceInfo1`/`FaceInfo2` sub-directories) and
//! the three `CameraSettings` layouts the A-mount DSLRs write into 0x0114.
//! None of these is enciphered; they go through the same
//! [`super::binary_data`] interpreter as the enciphered blocks because they
//! are the same `ProcessBinaryData` shape. Every row was read out of
//! ExifTool's own `%Image::ExifTool::Sony::*` hashes in-process (13.59) rather
//! than retyped.

use super::binary_data::{BinTable, BinTag, Cond, Dm, Fmt, Hook, NumCmp, Other, Pc, Raw, Vc};

#[rustfmt::skip]
static M0: &[(&str, &str)] = &[("0", "Off"), ("1", "On")];
#[rustfmt::skip]
static M1: &[(&str, &str)] = &[("1", "Single Frame"), ("10", "Remote Commander"), ("11", "Mirror Lock-up"), ("18", "Continuous Low"), ("2", "Continuous High"), ("24", "White Balance Bracketing Low"), ("25", "D-Range Optimizer Bracketing Low"), ("4", "Self-timer 10 sec"), ("40", "White Balance Bracketing High"), ("41", "D-Range Optimizer Bracketing High"), ("5", "Self-timer 2 sec, Mirror Lock-up"), ("6", "Single-frame Bracketing"), ("7", "Continuous Bracketing")];
#[rustfmt::skip]
static M2: &[(&str, &str)] = &[("16", "Cloudy"), ("17", "Shade"), ("18", "Color Temperature/Color Filter"), ("2", "Auto"), ("32", "Custom 1"), ("33", "Custom 2"), ("34", "Custom 3"), ("4", "Daylight"), ("5", "Fluorescent"), ("6", "Tungsten"), ("7", "Flash")];
#[rustfmt::skip]
static M3: &[(&str, &str)] = &[("12", "Color Temperature"), ("13", "Color Filter"), ("14", "Custom"), ("16", "Cloudy"), ("17", "Shade"), ("2", "Auto"), ("4", "Daylight"), ("5", "Fluorescent"), ("6", "Tungsten"), ("7", "Flash")];
#[rustfmt::skip]
static M4: &[(&str, &str)] = &[("0", "Manual"), ("1", "AF-S"), ("2", "AF-C"), ("3", "AF-A"), ("4", "DMF")];
#[rustfmt::skip]
static M5: &[(&str, &str)] = &[("0", "Wide"), ("1", "Local"), ("2", "Spot")];
#[rustfmt::skip]
static M6: &[(&str, &str)] = &[("1", "Center"), ("10", "Far Right"), ("11", "Far Left"), ("2", "Top"), ("3", "Upper-right"), ("4", "Right"), ("5", "Lower-right"), ("6", "Bottom"), ("7", "Lower-left"), ("8", "Left"), ("9", "Upper-left")];
#[rustfmt::skip]
static M7: &[(&str, &str)] = &[("0", "Autoflash"), ("2", "Rear Sync"), ("3", "Wireless"), ("4", "Fill-flash"), ("5", "Flash Off"), ("6", "Slow Sync")];
#[rustfmt::skip]
static M8: &[(&str, &str)] = &[("1", "Multi-segment"), ("2", "Center-weighted average"), ("4", "Spot")];
#[rustfmt::skip]
static M9: &[(&str, &str)] = &[("0", "Off"), ("1", "Standard"), ("2", "Advanced Auto"), ("3", "Advanced Level")];
#[rustfmt::skip]
static M10: &[(&str, &str)] = &[("1", "Standard"), ("11", "Neutral"), ("12", "Clear"), ("13", "Deep"), ("14", "Light"), ("15", "Autumn Leaves"), ("16", "Sepia"), ("2", "Vivid"), ("3", "Portrait"), ("4", "Landscape"), ("5", "Sunset"), ("6", "Night View/Portrait"), ("8", "B&W"), ("9", "Adobe RGB")];
#[rustfmt::skip]
static M11: &[(&str, &str)] = &[("0", "sRGB"), ("1", "Adobe RGB"), ("5", "Adobe RGB (A700)")];
#[rustfmt::skip]
static M12: &[(&str, &str)] = &[("0", "ADI"), ("1", "Pre-flash TTL"), ("2", "Manual")];
#[rustfmt::skip]
static M13: &[(&str, &str)] = &[("0", "AF"), ("1", "Release")];
#[rustfmt::skip]
static M14: &[(&str, &str)] = &[("0", "Auto"), ("1", "Off")];
#[rustfmt::skip]
static M15: &[(&str, &str)] = &[("0", "On"), ("1", "Off")];
#[rustfmt::skip]
static M16: &[(&str, &str)] = &[("0", "Normal"), ("1", "Low"), ("2", "High"), ("3", "Off")];
#[rustfmt::skip]
static M17: &[(&str, &str)] = &[("1", "Standard"), ("11", "Neutral"), ("129", "StyleBox1"), ("130", "StyleBox2"), ("131", "StyleBox3"), ("132", "StyleBox4"), ("133", "StyleBox5"), ("134", "StyleBox6"), ("2", "Vivid"), ("3", "Portrait"), ("4", "Landscape"), ("5", "Sunset"), ("7", "Night View/Portrait"), ("8", "B&W"), ("9", "Adobe RGB")];
#[rustfmt::skip]
static M18: &[(&str, &str)] = &[("0", "AF"), ("1", "Manual")];
#[rustfmt::skip]
static M19: &[(&str, &str)] = &[("0", "Auto"), ("1", "Manual"), ("16", "Portrait"), ("17", "Sports"), ("18", "Sunset"), ("19", "Night Portrait"), ("2", "Program AE"), ("20", "Landscape"), ("21", "Macro"), ("3", "Aperture-priority AE"), ("35", "Auto No Flash"), ("4", "Shutter speed priority AE"), ("8", "Program Shift A"), ("9", "Program Shift S")];
#[rustfmt::skip]
static M20: &[(&str, &str)] = &[("0", "Did not fire"), ("1", "Fired"), ("2", "External Flash, Did not fire"), ("3", "External Flash, Fired")];
#[rustfmt::skip]
static M21: &[(&str, &str)] = &[("0", "Horizontal (normal)"), ("1", "Rotate 90 CW"), ("2", "Rotate 270 CW")];
#[rustfmt::skip]
static M22: &[(&str, &str)] = &[("1", "Off"), ("2", "On")];
#[rustfmt::skip]
static M23: &[(&str, &str)] = &[("1", "Fired, Autoflash"), ("17", "Fired, Autoflash, Red-eye reduction"), ("18", "Fired, Fill-flash, Red-eye reduction"), ("2", "Fired, Fill-flash"), ("3", "Fired, Rear Sync"), ("34", "Fired, Fill-flash, HSS"), ("4", "Fired, Wireless"), ("5", "Did not fire"), ("6", "Fired, Slow Sync")];
#[rustfmt::skip]
static M24: &[(&str, &str)] = &[("2", "Empty"), ("3", "Very Low"), ("4", "Low"), ("5", "Sufficient"), ("6", "Full")];
#[rustfmt::skip]
static M25: &[(&str, &str)] = &[("0", "Not confirmed"), ("4", "Not confirmed, Tracking")];
#[rustfmt::skip]
static M26: &[(&str, &str)] = &[("1", "Large"), ("2", "Medium"), ("3", "Small")];
#[rustfmt::skip]
static M27: &[(&str, &str)] = &[("1", "3:2"), ("2", "16:9")];
#[rustfmt::skip]
static M28: &[(&str, &str)] = &[("0", "RAW"), ("16", "Extra Fine"), ("2", "CRAW"), ("32", "Fine"), ("34", "RAW + JPEG"), ("35", "CRAW + JPEG"), ("48", "Standard")];
#[rustfmt::skip]
static M29: &[(&str, &str)] = &[("33", "1/3 EV"), ("50", "1/2 EV")];
#[rustfmt::skip]
static M30: &[(&str, &str)] = &[("0", "Manual"), ("1", "AF-S"), ("2", "AF-C"), ("3", "AF-A")];
#[rustfmt::skip]
static M31: &[(&str, &str)] = &[("1", "Center"), ("2", "Top"), ("3", "Upper-right"), ("4", "Right"), ("5", "Lower-right"), ("6", "Bottom"), ("7", "Lower-left"), ("8", "Left"), ("9", "Upper-left")];
#[rustfmt::skip]
static M32: &[(&str, &str)] = &[("1", "Standard"), ("2", "Vivid"), ("3", "Portrait"), ("4", "Landscape"), ("5", "Sunset"), ("6", "Night View/Portrait"), ("8", "B&W")];
#[rustfmt::skip]
static M33: &[(&str, &str)] = &[("0", "Off"), ("1", "Low"), ("2", "Normal"), ("3", "High")];
#[rustfmt::skip]
static M34: &[(&str, &str)] = &[("1", "Standard"), ("2", "Vivid"), ("3", "Portrait"), ("4", "Landscape"), ("5", "Sunset"), ("7", "Night View/Portrait"), ("8", "B&W")];
#[rustfmt::skip]
static M35: &[(&str, &str)] = &[("1", "Single Frame"), ("10", "Remote Commander"), ("11", "Continuous Self-timer"), ("2", "Continuous High"), ("4", "Self-timer 10 sec"), ("5", "Self-timer 2 sec, Mirror Lock-up"), ("7", "Continuous Bracketing")];
#[rustfmt::skip]
static M36: &[(&str, &str)] = &[("5", "Adobe RGB"), ("6", "sRGB")];
#[rustfmt::skip]
static M37: &[(&str, &str)] = &[("0", "Auto"), ("254", "n/a")];
#[rustfmt::skip]
static M38: &[(&str, &str)] = &[("113", "Continuous Bracketing 0.3 EV"), ("117", "Continuous Bracketing 0.7 EV"), ("145", "White Balance Bracketing Low"), ("146", "White Balance Bracketing High"), ("16", "Single Frame"), ("192", "Remote Commander"), ("33", "Continuous High"), ("34", "Continuous Low"), ("48", "Speed Priority Continuous"), ("81", "Self-timer 10 sec"), ("82", "Self-timer 2 sec, Mirror Lock-up")];
#[rustfmt::skip]
static M39: &[(&str, &str)] = &[("1", "Program AE"), ("128", "Toy Camera"), ("129", "Pop Color"), ("130", "Posterization"), ("131", "Posterization B/W"), ("132", "Retro Photo"), ("133", "High-key"), ("134", "Partial Color Red"), ("135", "Partial Color Green"), ("136", "Partial Color Blue"), ("137", "Partial Color Yellow"), ("138", "High Contrast Monochrome"), ("16", "Auto"), ("17", "Auto (no flash)"), ("18", "Auto+"), ("2", "Aperture-priority AE"), ("3", "Shutter speed priority AE"), ("4", "Manual"), ("49", "Portrait"), ("5", "Cont. Priority AE"), ("50", "Landscape"), ("51", "Macro"), ("52", "Sports"), ("53", "Sunset"), ("54", "Night view"), ("55", "Night view/portrait"), ("56", "Handheld Night Shot"), ("57", "3D Sweep Panorama"), ("64", "Auto 2"), ("65", "Auto 2 (no flash)"), ("80", "Sweep Panorama"), ("96", "Anti Motion Blur")];
#[rustfmt::skip]
static M40: &[(&str, &str)] = &[("17", "AF-S"), ("18", "AF-C"), ("19", "AF-A"), ("32", "Manual"), ("48", "DMF")];
#[rustfmt::skip]
static M41: &[(&str, &str)] = &[("1", "Multi-segment"), ("2", "Center-weighted average"), ("3", "Spot")];
#[rustfmt::skip]
static M42: &[(&str, &str)] = &[("21", "Large (3:2)"), ("22", "Medium (3:2)"), ("23", "Small (3:2)"), ("25", "Large (16:9)"), ("26", "Medium (16:9)"), ("27", "Small (16:9)")];
#[rustfmt::skip]
static M43: &[(&str, &str)] = &[("4", "3:2"), ("8", "16:9")];
#[rustfmt::skip]
static M44: &[(&str, &str)] = &[("2", "RAW"), ("4", "RAW + JPEG"), ("6", "Fine"), ("7", "Standard")];
#[rustfmt::skip]
static M45: &[(&str, &str)] = &[("1", "Off"), ("16", "On (Auto)"), ("17", "On (Manual)")];
#[rustfmt::skip]
static M46: &[(&str, &str)] = &[("1", "sRGB"), ("2", "Adobe RGB")];
#[rustfmt::skip]
static M47: &[(&str, &str)] = &[("16", "Standard"), ("160", "Sunset"), ("32", "Vivid"), ("64", "Portrait"), ("80", "Landscape"), ("96", "B&W")];
#[rustfmt::skip]
static M48: &[(&str, &str)] = &[("100", "Fluorescent (+1)"), ("101", "Fluorescent (+2)"), ("102", "Fluorescent (+3)"), ("112", "Flash (-3)"), ("113", "Flash (-2)"), ("114", "Flash (-1)"), ("115", "Flash (0)"), ("116", "Flash (+1)"), ("117", "Flash (+2)"), ("118", "Flash (+3)"), ("16", "Auto (-3)"), ("163", "Custom"), ("17", "Auto (-2)"), ("18", "Auto (-1)"), ("19", "Auto (0)"), ("20", "Auto (+1)"), ("21", "Auto (+2)"), ("22", "Auto (+3)"), ("243", "Color Temperature/Color Filter"), ("32", "Daylight (-3)"), ("33", "Daylight (-2)"), ("34", "Daylight (-1)"), ("35", "Daylight (0)"), ("36", "Daylight (+1)"), ("37", "Daylight (+2)"), ("38", "Daylight (+3)"), ("48", "Shade (-3)"), ("49", "Shade (-2)"), ("50", "Shade (-1)"), ("51", "Shade (0)"), ("52", "Shade (+1)"), ("53", "Shade (+2)"), ("54", "Shade (+3)"), ("64", "Cloudy (-3)"), ("65", "Cloudy (-2)"), ("66", "Cloudy (-1)"), ("67", "Cloudy (0)"), ("68", "Cloudy (+1)"), ("69", "Cloudy (+2)"), ("70", "Cloudy (+3)"), ("80", "Tungsten (-3)"), ("81", "Tungsten (-2)"), ("82", "Tungsten (-1)"), ("83", "Tungsten (0)"), ("84", "Tungsten (+1)"), ("85", "Tungsten (+2)"), ("86", "Tungsten (+3)"), ("96", "Fluorescent (-3)"), ("97", "Fluorescent (-2)"), ("98", "Fluorescent (-1)"), ("99", "Fluorescent (0)")];
#[rustfmt::skip]
static M49: &[(&str, &str)] = &[("1", "Flash Off"), ("16", "Autoflash"), ("17", "Fill-flash"), ("18", "Slow Sync"), ("19", "Rear Sync"), ("20", "Wireless")];
#[rustfmt::skip]
static M50: &[(&str, &str)] = &[("1", "ADI Flash"), ("2", "Pre-flash TTL")];
#[rustfmt::skip]
static M51: &[(&str, &str)] = &[("1", "Wide"), ("2", "Spot"), ("3", "Local"), ("4", "Flexible")];
#[rustfmt::skip]
static M52: &[(&str, &str)] = &[("1", "Off"), ("16", "On")];
#[rustfmt::skip]
static M53: &[(&str, &str)] = &[("16", "Low"), ("17", "High"), ("19", "Auto")];
#[rustfmt::skip]
static M54: &[(&str, &str)] = &[("17", "Slight Smile"), ("18", "Normal Smile"), ("19", "Big Smile")];
#[rustfmt::skip]
static M55: &[(&str, &str)] = &[("33", "1 EV"), ("34", "1.5 EV"), ("35", "2 EV"), ("36", "2.5 EV"), ("37", "3 EV"), ("38", "3.5 EV"), ("39", "4 EV"), ("40", "5 EV"), ("41", "6 EV")];
#[rustfmt::skip]
static M56: &[(&str, &str)] = &[("16", "ViewFinder"), ("33", "Focus Check Live View"), ("34", "Quick AF Live View")];
#[rustfmt::skip]
static M57: &[(&str, &str)] = &[("1", "Standard"), ("2", "Wide")];
#[rustfmt::skip]
static M58: &[(&str, &str)] = &[("1", "Right"), ("2", "Left"), ("3", "Up"), ("4", "Down")];
#[rustfmt::skip]
static M59: &[(&str, &str)] = &[("113", "Continuous Bracketing 0.3 EV"), ("117", "Continuous Bracketing 0.7 EV"), ("145", "White Balance Bracketing Low"), ("146", "White Balance Bracketing High"), ("16", "Single Frame"), ("192", "Remote Commander"), ("209", "Continuous - HDR"), ("210", "Continuous - Multi Frame NR"), ("211", "Continuous - Handheld Night Shot"), ("212", "Continuous - Anti Motion Blur"), ("213", "Continuous - Sweep Panorama"), ("214", "Continuous - 3D Sweep Panorama"), ("33", "Continuous High"), ("34", "Continuous Low"), ("48", "Speed Priority Continuous"), ("81", "Self-timer 10 sec"), ("82", "Self-timer 2 sec, Mirror Lock-up")];
#[rustfmt::skip]
static M60: &[(&str, &str)] = &[("0", "n/a"), ("1", "Off"), ("16", "On"), ("255", "None")];
#[rustfmt::skip]
static M61: &[(&str, &str)] = &[("0", "n/a"), ("1", "Phase-detect AF"), ("2", "Contrast AF")];
#[rustfmt::skip]
static M62: &[(&str, &str)] = &[("0", "n/a"), ("1", "Standard"), ("2", "Wide"), ("3", "16:9")];
#[rustfmt::skip]
static M63: &[(&str, &str)] = &[("1", "No"), ("16", "Yes")];
#[rustfmt::skip]
static M64: &[(&str, &str)] = &[("0", "n/a"), ("16", "40 Segment"), ("32", "1200-zone Evaluative")];
#[rustfmt::skip]
static M65: &[(&str, &str)] = &[("0", "n/a"), ("16", "Viewfinder"), ("33", "Focus Check Live View"), ("34", "Quick AF Live View")];
#[rustfmt::skip]
static M66: &[(&str, &str)] = &[("1", "On"), ("2", "Off")];
#[rustfmt::skip]
static M67: &[(&str, &str)] = &[("1", "None"), ("2", "Off"), ("3", "On")];
#[rustfmt::skip]
static M68: &[(&str, &str)] = &[("0", "n/a"), ("1", "AF"), ("16", "Manual")];
#[rustfmt::skip]
static M69: &[(&str, &str)] = &[("1", "Unknown"), ("16", "A-mount"), ("17", "E-mount")];
#[rustfmt::skip]
static M70: &[(&str, &str)] = &[("0", "Single"), ("255", "n/a")];
#[rustfmt::skip]
static M71: &[(&str, &str)] = &[("0", "Unknown E-mount lens or other lens"), ("0.1", "Sigma 19mm F2.8 [EX] DN"), ("0.10", "Zeiss Touit 50mm F2.8 Macro"), ("0.11", "Zeiss Loxia 50mm F2"), ("0.12", "Zeiss Loxia 35mm F2"), ("0.13", "Viltrox 85mm F1.8"), ("0.2", "Sigma 30mm F2.8 [EX] DN"), ("0.3", "Sigma 60mm F2.8 DN"), ("0.4", "Sony E 18-200mm F3.5-6.3 OSS LE"), ("0.5", "Tamron 18-200mm F3.5-6.3 Di III VC"), ("0.6", "Tokina FiRIN 20mm F2 FE AF"), ("0.7", "Tokina FiRIN 20mm F2 FE MF"), ("0.8", "Zeiss Touit 12mm F2.8"), ("0.9", "Zeiss Touit 32mm F1.8"), ("1", "Sony LA-EA1 or Sigma MC-11 Adapter"), ("13", "Samyang AF 35-150mm F2-2.8"), ("17", "Samyang RS 21mm F3.5"), ("18", "Samyang RS 28mm F3.5"), ("184", "Metabones Canon EF Speed Booster Ultra"), ("19", "Samyang RS 32mm F2.8"), ("2", "Sony LA-EA2 Adapter"), ("20", "Samyang AF 35mm F1.4 P FE"), ("21", "Samyang AF 14-24mm F2.8"), ("22", "Samyang AF 24-60mm F2.8"), ("234", "Metabones Canon EF Smart Adapter Mark IV"), ("239", "Metabones Canon EF Speed Booster"), ("24", "Samyang AF 85mm F1.8 P FE"), ("24593", "LA-EA4r MonsterAdapter"), ("3", "Sony LA-EA3 Adapter"), ("32784", "Sony E 16mm F2.8"), ("32785", "Sony E 18-55mm F3.5-5.6 OSS"), ("32786", "Sony E 55-210mm F4.5-6.3 OSS"), ("32787", "Sony E 18-200mm F3.5-6.3 OSS"), ("32788", "Sony E 30mm F3.5 Macro"), ("32789", "Sony E 24mm F1.8 ZA or Samyang AF 50mm F1.4"), ("32789.1", "Samyang AF 50mm F1.4"), ("32790", "Sony E 50mm F1.8 OSS or Samyang AF 14mm F2.8"), ("32790.1", "Samyang AF 14mm F2.8"), ("32791", "Sony E 16-70mm F4 ZA OSS"), ("32792", "Sony E 10-18mm F4 OSS"), ("32793", "Sony E PZ 16-50mm F3.5-5.6 OSS"), ("32794", "Sony FE 35mm F2.8 ZA or Samyang Lens"), ("32794.1", "Samyang AF 24mm F2.8"), ("32794.2", "Samyang AF 35mm F2.8"), ("32795", "Sony FE 24-70mm F4 ZA OSS"), ("32796", "Sony FE 85mm F1.8 or Viltrox PFU RBMH 85mm F1.8"), ("32796.1", "Viltrox PFU RBMH 85mm F1.8"), ("32797", "Sony E 18-200mm F3.5-6.3 OSS LE"), ("32798", "Sony E 20mm F2.8"), ("32799", "Sony E 35mm F1.8 OSS"), ("32800", "Sony E PZ 18-105mm F4 G OSS"), ("32801", "Sony FE 12-24mm F4 G"), ("32802", "Sony FE 90mm F2.8 Macro G OSS"), ("32803", "Sony E 18-50mm F4-5.6"), ("32804", "Sony FE 24mm F1.4 GM"), ("32805", "Sony FE 24-105mm F4 G OSS"), ("32807", "Sony E PZ 18-200mm F3.5-6.3 OSS"), ("32808", "Sony FE 55mm F1.8 ZA"), ("32810", "Sony FE 70-200mm F4 G OSS"), ("32811", "Sony FE 16-35mm F4 ZA OSS"), ("32812", "Sony FE 50mm F2.8 Macro"), ("32813", "Sony FE 28-70mm F3.5-5.6 OSS"), ("32814", "Sony FE 35mm F1.4 ZA"), ("32815", "Sony FE 24-240mm F3.5-6.3 OSS"), ("32816", "Sony FE 28mm F2"), ("32817", "Sony FE PZ 28-135mm F4 G OSS"), ("32819", "Sony FE 100mm F2.8 STF GM OSS"), ("32820", "Sony E PZ 18-110mm F4 G OSS"), ("32821", "Sony FE 24-70mm F2.8 GM"), ("32822", "Sony FE 50mm F1.4 ZA"), ("32823", "Sony FE 85mm F1.4 GM or Samyang AF 85mm F1.4"), ("32823.1", "Samyang AF 85mm F1.4"), ("32824", "Sony FE 50mm F1.8"), ("32826", "Sony FE 21mm F2.8 (SEL28F20 + SEL075UWC)"), ("32827", "Sony FE 16mm F3.5 Fisheye (SEL28F20 + SEL057FEC)"), ("32828", "Sony FE 70-300mm F4.5-5.6 G OSS"), ("32829", "Sony FE 100-400mm F4.5-5.6 GM OSS"), ("32830", "Sony FE 70-200mm F2.8 GM OSS"), ("32831", "Sony FE 16-35mm F2.8 GM"), ("32848", "Sony FE 400mm F2.8 GM OSS"), ("32849", "Sony E 18-135mm F3.5-5.6 OSS"), ("32850", "Sony FE 135mm F1.8 GM"), ("32851", "Sony FE 200-600mm F5.6-6.3 G OSS"), ("32852", "Sony FE 600mm F4 GM OSS"), ("32853", "Sony E 16-55mm F2.8 G"), ("32854", "Sony E 70-350mm F4.5-6.3 G OSS"), ("32855", "Sony FE C 16-35mm T3.1 G"), ("32858", "Sony FE 35mm F1.8"), ("32859", "Sony FE 20mm F1.8 G"), ("32860", "Sony FE 12-24mm F2.8 GM"), ("32862", "Sony FE 50mm F1.2 GM"), ("32863", "Sony FE 14mm F1.8 GM"), ("32864", "Sony FE 28-60mm F4-5.6"), ("32865", "Sony FE 35mm F1.4 GM"), ("32866", "Sony FE 24mm F2.8 G"), ("32867", "Sony FE 40mm F2.5 G"), ("32868", "Sony FE 50mm F2.5 G"), ("32871", "Sony FE PZ 16-35mm F4 G"), ("32873", "Sony E PZ 10-20mm F4 G"), ("32874", "Sony FE 70-200mm F2.8 GM OSS II"), ("32875", "Sony FE 24-70mm F2.8 GM II"), ("32876", "Sony E 11mm F1.8"), ("32877", "Sony E 15mm F1.4 G"), ("32878", "Sony FE 20-70mm F4 G"), ("32879", "Sony FE 50mm F1.4 GM"), ("32880", "Sony FE 16mm F1.8 G"), ("32881", "Sony FE 24-50mm F2.8 G"), ("32882", "Sony FE 16-25mm F2.8 G"), ("32884", "Sony FE 70-200mm F4 Macro G OSS II"), ("32885", "Sony FE 16-35mm F2.8 GM II"), ("32886", "Sony FE 300mm F2.8 GM OSS"), ("32887", "Sony E PZ 16-50mm F3.5-5.6 OSS II"), ("32888", "Sony FE 85mm F1.4 GM II"), ("32889", "Sony FE 28-70mm F2 GM"), ("32890", "Sony FE 400-800mm F6.3-8 G OSS"), ("32891", "Sony FE 50-150mm F2 GM"), ("32893", "Sony FE 100mm F2.8 Macro GM OSS"), ("32895", "Sony FE 100-400mm F4.5 GM OSS"), ("33072", "Sony FE 70-200mm F2.8 GM OSS + 1.4X Teleconverter"), ("33073", "Sony FE 70-200mm F2.8 GM OSS + 2X Teleconverter"), ("33076", "Sony FE 100mm F2.8 STF GM OSS (macro mode)"), ("33077", "Sony FE 100-400mm F4.5-5.6 GM OSS + 1.4X Teleconverter"), ("33078", "Sony FE 100-400mm F4.5-5.6 GM OSS + 2X Teleconverter"), ("33079", "Sony FE 400mm F2.8 GM OSS + 1.4X Teleconverter"), ("33080", "Sony FE 400mm F2.8 GM OSS + 2X Teleconverter"), ("33081", "Sony FE 200-600mm F5.6-6.3 G OSS + 1.4X Teleconverter"), ("33082", "Sony FE 200-600mm F5.6-6.3 G OSS + 2X Teleconverter"), ("33083", "Sony FE 600mm F4 GM OSS + 1.4X Teleconverter"), ("33084", "Sony FE 600mm F4 GM OSS + 2X Teleconverter"), ("33085", "Sony FE 70-200mm F2.8 GM OSS II + 1.4X Teleconverter"), ("33086", "Sony FE 70-200mm F2.8 GM OSS II + 2X Teleconverter"), ("33087", "Sony FE 70-200mm F4 Macro G OSS II + 1.4X Teleconverter"), ("33088", "Sony FE 70-200mm F4 Macro G OSS II + 2X Teleconverter"), ("33089", "Sony FE 300mm F2.8 GM OSS + 1.4X Teleconverter"), ("33090", "Sony FE 300mm F2.8 GM OSS + 2X Teleconverter"), ("33091", "Sony FE 400-800mm F6.3-8 G OSS + 1.4X Teleconverter"), ("33092", "Sony FE 400-800mm F6.3-8 G OSS + 2X Teleconverter"), ("33093", "Sony FE 100mm F2.8 Macro GM OSS + 1.4X Teleconverter"), ("33094", "Sony FE 100mm F2.8 Macro GM OSS + 2X Teleconverter"), ("33095", "Sony FE 100-400mm F4.5 GM OSS + 1.4X Teleconverter"), ("33096", "Sony FE 100-400mm F4.5 GM OSS + 2X Teleconverter"), ("44", "Metabones Canon EF Smart Adapter"), ("49201", "Zeiss Touit 12mm F2.8 or other Touit lens"), ("49201.1", "Zeiss Touit 32mm F1.8"), ("49201.2", "Zeiss Touit 50mm F2.8"), ("49202", "Zeiss Touit 32mm F1.8"), ("49203", "Zeiss Touit 50mm F2.8 Macro"), ("49216", "Zeiss Batis 25mm F2"), ("49217", "Zeiss Batis 85mm F1.8"), ("49218", "Zeiss Batis 18mm F2.8"), ("49219", "Zeiss Batis 135mm F2.8"), ("49220", "Zeiss Batis 40mm F2 CF"), ("49232", "Zeiss Loxia 50mm F2"), ("49233", "Zeiss Loxia 35mm F2"), ("49234", "Zeiss Loxia 21mm F2.8"), ("49235", "Zeiss Loxia 85mm F2.4"), ("49236", "Zeiss Loxia 25mm F2.4"), ("49456", "Tamron E 18-200mm F3.5-6.3 Di III VC"), ("49457", "Tamron 28-75mm F2.8 Di III RXD"), ("49458", "Tamron 17-28mm F2.8 Di III RXD"), ("49459", "Tamron 35mm F2.8 Di III OSD M1:2"), ("49460", "Tamron 24mm F2.8 Di III OSD M1:2"), ("49461", "Tamron 20mm F2.8 Di III OSD M1:2"), ("49462", "Tamron 70-180mm F2.8 Di III VXD"), ("49463", "Tamron 28-200mm F2.8-5.6 Di III RXD"), ("49464", "Tamron 70-300mm F4.5-6.3 Di III RXD"), ("49465", "Tamron 17-70mm F2.8 Di III-A VC RXD"), ("49466", "Tamron 150-500mm F5-6.7 Di III VC VXD"), ("49467", "Tamron 11-20mm F2.8 Di III-A RXD"), ("49468", "Tamron 18-300mm F3.5-6.3 Di III-A VC VXD"), ("49469", "Tamron 35-150mm F2-F2.8 Di III VXD"), ("49470", "Tamron 28-75mm F2.8 Di III VXD G2"), ("49471", "Tamron 50-400mm F4.5-6.3 Di III VC VXD"), ("49472", "Tamron 20-40mm F2.8 Di III VXD"), ("49473", "Tamron 17-50mm F4 Di III VXD or Tokina or Viltrox lens"), ("49473.1", "Tokina atx-m 85mm F1.8 FE"), ("49473.2", "Viltrox 23mm F1.4 E"), ("49473.3", "Viltrox 56mm F1.4 E"), ("49473.4", "Viltrox 85mm F1.8 II FE"), ("49474", "Tamron 70-180mm F2.8 Di III VXD G2 or Viltrox lens"), ("49474.1", "Viltrox 13mm F1.4 E"), ("49474.10", "Viltrox 20mm F2.8 FE Air"), ("49474.11", "Viltrox 135mm F1.8 FE LAB"), ("49474.12", "Viltrox 27mm F1.2 E Pro"), ("49474.13", "Viltrox 56mm F1.4 E"), ("49474.2", "Viltrox 16mm F1.8 FE"), ("49474.3", "Viltrox 23mm F1.4 E"), ("49474.4", "Viltrox 24mm F1.8 FE"), ("49474.5", "Viltrox 28mm F1.8 FE"), ("49474.6", "Viltrox 33mm F1.4 E"), ("49474.7", "Viltrox 35mm F1.8 FE"), ("49474.8", "Viltrox 50mm F1.8 FE"), ("49474.9", "Viltrox 75mm F1.2 E Pro"), ("49475", "Tamron 50-300mm F4.5-6.3 Di III VC VXD"), ("49476", "Tamron 28-300mm F4-7.1 Di III VC VXD"), ("49477", "Tamron 90mm F2.8 Di III Macro VXD"), ("49478", "Tamron 16-30mm F2.8 Di III VXD G2"), ("49479", "Tamron 25-200mm F2.8-5.6 Di III VXD G2"), ("49480", "Tamron 35-100mm F2.8 Di III VXD"), ("49712", "Tokina FiRIN 20mm F2 FE AF"), ("49713", "Tokina FiRIN 100mm F2.8 FE MACRO"), ("49714", "Tokina atx-m 11-18mm F2.8 E"), ("50480", "Sigma 30mm F1.4 DC DN | C"), ("50481", "Sigma 50mm F1.4 DG HSM | A"), ("50482", "Sigma 18-300mm F3.5-6.3 DC MACRO OS HSM | C + MC-11"), ("50483", "Sigma 18-35mm F1.8 DC HSM | A + MC-11"), ("50484", "Sigma 24-35mm F2 DG HSM | A + MC-11"), ("50485", "Sigma 24mm F1.4 DG HSM | A + MC-11"), ("50486", "Sigma 150-600mm F5-6.3 DG OS HSM | C + MC-11"), ("50487", "Sigma 20mm F1.4 DG HSM | A + MC-11"), ("50488", "Sigma 35mm F1.4 DG HSM | A"), ("50489", "Sigma 150-600mm F5-6.3 DG OS HSM | S + MC-11"), ("50490", "Sigma 120-300mm F2.8 DG OS HSM | S + MC-11"), ("50492", "Sigma 24-105mm F4 DG OS HSM | A + MC-11"), ("50493", "Sigma 17-70mm F2.8-4 DC MACRO OS HSM | C + MC-11"), ("50495", "Sigma 50-100mm F1.8 DC HSM | A + MC-11"), ("50499", "Sigma 85mm F1.4 DG HSM | A"), ("50501", "Sigma 100-400mm F5-6.3 DG OS HSM | C + MC-11"), ("50503", "Sigma 16mm F1.4 DC DN | C"), ("50507", "Sigma 105mm F1.4 DG HSM | A"), ("50508", "Sigma 56mm F1.4 DC DN | C"), ("50512", "Sigma 70-200mm F2.8 DG OS HSM | S + MC-11"), ("50513", "Sigma 70mm F2.8 DG MACRO | A"), ("50514", "Sigma 45mm F2.8 DG DN | C"), ("50515", "Sigma 35mm F1.2 DG DN | A"), ("50516", "Sigma 14-24mm F2.8 DG DN | A"), ("50517", "Sigma 24-70mm F2.8 DG DN | A"), ("50518", "Sigma 100-400mm F5-6.3 DG DN OS | C"), ("50521", "Sigma 85mm F1.4 DG DN | A"), ("50522", "Sigma 105mm F2.8 DG DN MACRO | A"), ("50523", "Sigma 65mm F2 DG DN | C"), ("50524", "Sigma 35mm F2 DG DN | C"), ("50525", "Sigma 24mm F3.5 DG DN | C"), ("50526", "Sigma 28-70mm F2.8 DG DN | C"), ("50527", "Sigma 150-600mm F5-6.3 DG DN OS | S"), ("50528", "Sigma 35mm F1.4 DG DN | A"), ("50529", "Sigma 90mm F2.8 DG DN | C"), ("50530", "Sigma 24mm F2 DG DN | C"), ("50531", "Sigma 18-50mm F2.8 DC DN | C"), ("50532", "Sigma 20mm F2 DG DN | C"), ("50533", "Sigma 16-28mm F2.8 DG DN | C"), ("50534", "Sigma 20mm F1.4 DG DN | A"), ("50535", "Sigma 24mm F1.4 DG DN | A"), ("50536", "Sigma 60-600mm F4.5-6.3 DG DN OS | S"), ("50537", "Sigma 50mm F2 DG DN | C"), ("50538", "Sigma 17mm F4 DG DN | C"), ("50539", "Sigma 50mm F1.4 DG DN | A"), ("50540", "Sigma 14mm F1.4 DG DN | A"), ("50543", "Sigma 70-200mm F2.8 DG DN OS | S"), ("50544", "Sigma 23mm F1.4 DC DN | C"), ("50545", "Sigma 24-70mm F2.8 DG DN II | A"), ("50546", "Sigma 500mm F5.6 DG DN OS | S"), ("50547", "Sigma 10-18mm F2.8 DC DN | C"), ("50548", "Sigma 15mm F1.4 DG DN DIAGONAL FISHEYE | A"), ("50549", "Sigma 50mm F1.2 DG DN | A"), ("50550", "Sigma 28-105mm F2.8 DG DN | A"), ("50551", "Sigma 28-45mm F1.8 DG DN | A"), ("50552", "Sigma 35mm F1.2 DG II | A"), ("50553", "Sigma 300-600mm F4 DG OS | S"), ("50554", "Sigma 16-300mm F3.5-6.7 DC OS | C"), ("50555", "Sigma 12mm F1.4 DC | C"), ("50556", "Sigma 17-40mm F1.8 DC | A"), ("50557", "Sigma 200mm F2 DG OS | S"), ("50558", "Sigma 20-200mm F3.5-6.3 DG | C"), ("50559", "Sigma 135mm F1.4 DG | A"), ("50563", "Sigma 35mm F1.4 DG II | A"), ("50564", "Sigma 15mm F1.4 DC | C"), ("50992", "Voigtlander SUPER WIDE-HELIAR 15mm F4.5 III"), ("50993", "Voigtlander HELIAR-HYPER WIDE 10mm F5.6"), ("50994", "Voigtlander ULTRA WIDE-HELIAR 12mm F5.6 III"), ("50995", "Voigtlander MACRO APO-LANTHAR 65mm F2 Aspherical"), ("50996", "Voigtlander NOKTON 40mm F1.2 Aspherical"), ("50997", "Voigtlander NOKTON classic 35mm F1.4"), ("50998", "Voigtlander MACRO APO-LANTHAR 110mm F2.5"), ("50999", "Voigtlander COLOR-SKOPAR 21mm F3.5 Aspherical"), ("51000", "Voigtlander NOKTON 50mm F1.2 Aspherical"), ("51001", "Voigtlander NOKTON 21mm F1.4 Aspherical"), ("51002", "Voigtlander APO-LANTHAR 50mm F2 Aspherical"), ("51003", "Voigtlander NOKTON 35mm F1.2 Aspherical SE"), ("51006", "Voigtlander APO-LANTHAR 35mm F2 Aspherical"), ("51007", "Voigtlander NOKTON 50mm F1 Aspherical"), ("51008", "Voigtlander NOKTON 75mm F1.5 Aspherical"), ("51009", "Voigtlander NOKTON 28mm F1.5 Aspherical"), ("51011", "Voigtlander APO-LANTHAR 28mm F2 Aspherical"), ("51072", "ZEISS Otus ML 50mm F1.4"), ("51073", "ZEISS Otus ML 85mm F1.4"), ("51504", "Samyang AF 50mm F1.4"), ("51505", "Samyang AF 14mm F2.8 or Samyang AF 35mm F2.8"), ("51505.1", "Samyang AF 35mm F2.8"), ("51507", "Samyang AF 35mm F1.4"), ("51508", "Samyang AF 45mm F1.8"), ("51510", "Samyang AF 18mm F2.8 or Samyang AF 35mm F1.8"), ("51510.1", "Samyang AF 35mm F1.8"), ("51512", "Samyang AF 75mm F1.8"), ("51513", "Samyang AF 35mm F1.8"), ("51514", "Samyang AF 24mm F1.8"), ("51515", "Samyang AF 12mm F2.0"), ("51516", "Samyang AF 24-70mm F2.8"), ("51517", "Samyang AF 50mm F1.4 II"), ("51518", "Samyang AF 135mm F1.8"), ("6", "Sony LA-EA4 Adapter"), ("61569", "LAOWA FFII 10mm F2.8 C&D Dreamer"), ("61572", "LAOWA FFII 12mm F2.8 C&D Dreamer"), ("61600", "Thypoch AF 24-50mm F2.8 FE"), ("61760", "Viltrox 135mm F1.8 FE LAB"), ("61761", "Viltrox 28mm F4.5 FE"), ("61762", "Viltrox 35mm F1.2 FE LAB"), ("61763", "Viltrox 85mm F1.4 FE Pro"), ("61766", "Viltrox 40mm F2.5 FE Air"), ("61767", "Viltrox 50mm F2.0 FE Air"), ("61768", "Viltrox 25mm F1.7 E Air"), ("61776", "Viltrox 50mm F1.4 FE Pro"), ("61777", "Viltrox 9mm F2.8 E Air"), ("61778", "Viltrox 14mm F4.0 FE Air"), ("61779", "Viltrox 56mm F1.2 E Pro"), ("61780", "Viltrox 85mm F2.0 FE EVO"), ("61781", "Viltrox 55mm F1.8 FE EVO"), ("61783", "Viltrox 15mm F1.7 E Air"), ("61789", "Viltrox 35mm F1.8 II FE EVO"), ("7", "Sony LA-EA5 Adapter"), ("78", "Metabones Canon EF Smart Adapter Mark III or Other Adapter")];
#[rustfmt::skip]
static B0: &[(u32, &str)] = &[(0u32, "Confirmed"), (1u32, "Failed"), (2u32, "Tracking")];

#[rustfmt::skip]
static T0: &[BinTag] = &[
    BinTag { index: 0, name: "ExposureTime", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::ExpTime(6.0_f64, 8.0_f64), pc: Pc::ExposureTimeOrBulb, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 1, name: "FNumber", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::Pow2DivSubHalf(8.0_f64, 1.0_f64), pc: Pc::FNumber, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 2, name: "HighSpeedSync", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M0, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 3, name: "ExposureCompensationSet", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::SubDiv(128.0_f64, 24.0_f64), pc: Pc::Signed1OrZero, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 4, name: "DriveMode", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 255, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M1, Other::None), hook: Hook::None, print_hex: true, low_priority: true, subdir: None },
    BinTag { index: 5, name: "WhiteBalanceSetting", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M2, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 6, name: "WhiteBalanceFineTune", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::Signed8Above128, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 7, name: "ColorTemperatureSet", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::Mul(100.0_f64), pc: Pc::Suffix(" K"), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 8, name: "ColorCompensationFilterSet", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::Signed8Above128, pc: Pc::PlusOrVal, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 12, name: "ColorTemperatureCustom", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::Mul(100.0_f64), pc: Pc::Suffix(" K"), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 13, name: "ColorCompensationFilterCustom", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::Signed8Above128, pc: Pc::PlusOrVal, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 15, name: "WhiteBalance", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M3, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 16, name: "FocusModeSetting", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M4, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 17, name: "AFAreaMode", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M5, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 18, name: "AFPointSetting", cond: Cond::Always, fmt: Fmt::U16, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M6, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 19, name: "FlashMode", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M7, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 20, name: "FlashExposureCompSet", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::SubDiv(128.0_f64, 24.0_f64), pc: Pc::Signed1OrZero, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 21, name: "MeteringMode", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M8, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 22, name: "ISOSetting", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::IsoExp, pc: Pc::Fixed0OrAuto, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 24, name: "DynamicRangeOptimizerMode", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M9, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 25, name: "DynamicRangeOptimizerLevel", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 26, name: "CreativeStyle", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M10, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 27, name: "ColorSpace", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M11, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 28, name: "Sharpness", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::Add(-10.0_f64), pc: Pc::PlusOrVal, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 29, name: "Contrast", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::Add(-10.0_f64), pc: Pc::PlusOrVal, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 30, name: "Saturation", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::Add(-10.0_f64), pc: Pc::PlusOrVal, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 31, name: "ZoneMatchingValue", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::Add(-10.0_f64), pc: Pc::PlusOrVal, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 34, name: "Brightness", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::Add(-10.0_f64), pc: Pc::PlusOrVal, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 35, name: "FlashControl", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M12, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 40, name: "PrioritySetupShutterRelease", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M13, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 41, name: "AFIlluminator", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M14, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 42, name: "AFWithShutter", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M15, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 43, name: "LongExposureNoiseReduction", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M0, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 44, name: "HighISONoiseReduction", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M16, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 45, name: "ImageStyle", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M17, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 46, name: "FocusModeSwitch", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M18, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 47, name: "ShutterSpeedSetting", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::ExpTime(6.0_f64, 8.0_f64), pc: Pc::ExposureTimeOrBulb, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 48, name: "ApertureSetting", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::Pow2DivSubHalf(8.0_f64, 1.0_f64), pc: Pc::FNumber, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 60, name: "ExposureProgram", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M19, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 61, name: "ImageStabilizationSetting", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M0, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 62, name: "FlashAction", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M20, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 63, name: "Rotation", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M21, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 64, name: "AELock", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M22, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 76, name: "FlashAction2", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M23, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 77, name: "FocusMode", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M4, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 80, name: "BatteryState", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M24, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 81, name: "BatteryLevel", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Suffix("%"), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 83, name: "FocusStatus", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Bitmask(M25, B0, 32, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 84, name: "SonyImageSize", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M26, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 85, name: "AspectRatio", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M27, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 86, name: "Quality", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M28, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 88, name: "ExposureLevelIncrements", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M29, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 106, name: "RedEyeReduction", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M0, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 154, name: "FolderNumber", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 1023, raw: Raw::None, vc: Vc::None, pc: Pc::ZeroPad(3), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 155, name: "ImageNumber", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 16383, raw: Raw::None, vc: Vc::None, pc: Pc::ZeroPad(4), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
];
#[rustfmt::skip]
static T1: &[BinTag] = &[
    BinTag { index: 0, name: "ExposureTime", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::ExpTime(6.0_f64, 8.0_f64), pc: Pc::ExposureTimeOrBulb, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 1, name: "FNumber", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::Pow2DivSubHalf(8.0_f64, 1.0_f64), pc: Pc::FNumber, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 2, name: "HighSpeedSync", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M0, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 3, name: "ExposureCompensationSet", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::SubDiv(128.0_f64, 24.0_f64), pc: Pc::Signed1OrZero, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 4, name: "WhiteBalanceSetting", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M2, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 5, name: "WhiteBalanceFineTune", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::Signed8Above128, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 6, name: "ColorTemperatureSet", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::Mul(100.0_f64), pc: Pc::Suffix(" K"), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 7, name: "ColorCompensationFilterSet", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::Signed8Above128, pc: Pc::PlusOrVal, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 8, name: "CustomWB_RGBLevels", cond: Cond::Always, fmt: Fmt::U16, count: 3, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 11, name: "ColorTemperatureCustom", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::Mul(100.0_f64), pc: Pc::Suffix(" K"), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 12, name: "ColorCompensationFilterCustom", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::Signed8Above128, pc: Pc::PlusOrVal, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 14, name: "WhiteBalance", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M3, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 15, name: "FocusModeSetting", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M30, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 16, name: "AFAreaMode", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M5, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 17, name: "AFPointSetting", cond: Cond::Always, fmt: Fmt::U16, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M31, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 18, name: "FlashExposureCompSet", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::SubDiv(128.0_f64, 24.0_f64), pc: Pc::Signed1OrZero, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 19, name: "MeteringMode", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M8, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 20, name: "ISOSetting", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::IsoExp, pc: Pc::Fixed0OrAuto, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 22, name: "DynamicRangeOptimizerMode", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M9, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 23, name: "DynamicRangeOptimizerLevel", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 24, name: "CreativeStyle", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M32, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 25, name: "Sharpness", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::Add(-10.0_f64), pc: Pc::PlusOrVal, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 26, name: "Contrast", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::Add(-10.0_f64), pc: Pc::PlusOrVal, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 27, name: "Saturation", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::Add(-10.0_f64), pc: Pc::PlusOrVal, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 31, name: "FlashControl", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M12, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 37, name: "LongExposureNoiseReduction", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M0, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 38, name: "HighISONoiseReduction", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M33, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 39, name: "ImageStyle", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M34, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 40, name: "ShutterSpeedSetting", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::ExpTime(6.0_f64, 8.0_f64), pc: Pc::ExposureTimeOrBulb, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 41, name: "ApertureSetting", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::Pow2DivSubHalf(8.0_f64, 1.0_f64), pc: Pc::FNumber, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 60, name: "ExposureProgram", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M19, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 61, name: "ImageStabilizationSetting", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M0, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 62, name: "FlashAction", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M20, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 63, name: "Rotation", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M21, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 64, name: "AELock", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M22, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 76, name: "FlashAction2", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M23, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 77, name: "FocusMode", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M30, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 83, name: "FocusStatus", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Bitmask(M25, B0, 32, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 84, name: "SonyImageSize", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M26, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 85, name: "AspectRatio", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M27, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 86, name: "Quality", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M28, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 88, name: "ExposureLevelIncrements", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M29, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 126, name: "DriveMode", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 255, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M35, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 127, name: "FlashMode", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M7, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 131, name: "ColorSpace", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M36, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
];
#[rustfmt::skip]
static T2: &[BinTag] = &[
    BinTag { index: 0, name: "ShutterSpeedSetting", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::ExpTime(6.0_f64, 8.0_f64), pc: Pc::ExposureTimeOrBulb, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 1, name: "ApertureSetting", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::Pow2DivSubHalf(8.0_f64, 1.0_f64), pc: Pc::FNumber, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 2, name: "ISOSetting", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::IsoExpBelow254, pc: Pc::Map(M37, Other::RoundHalfUp), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 3, name: "ExposureCompensationSet", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::SubDiv(128.0_f64, 24.0_f64), pc: Pc::Signed1OrZero, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 4, name: "DriveModeSetting", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M38, Other::None), hook: Hook::None, print_hex: true, low_priority: true, subdir: None },
    BinTag { index: 5, name: "ExposureProgram", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M39, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 6, name: "FocusModeSetting", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M40, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 7, name: "MeteringMode", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M41, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 9, name: "SonyImageSize", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M42, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 10, name: "AspectRatio", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M43, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 11, name: "Quality", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M44, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 12, name: "DynamicRangeOptimizerSetting", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M45, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 13, name: "DynamicRangeOptimizerLevel", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 14, name: "ColorSpace", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M46, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 15, name: "CreativeStyleSetting", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M47, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 16, name: "ContrastSetting", cond: Cond::Always, fmt: Fmt::I8, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::PlusOrVal, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 17, name: "SaturationSetting", cond: Cond::Always, fmt: Fmt::I8, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::PlusOrVal, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 18, name: "SharpnessSetting", cond: Cond::Always, fmt: Fmt::I8, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::PlusOrVal, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 22, name: "WhiteBalanceSetting", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M48, Other::None), hook: Hook::None, print_hex: true, low_priority: true, subdir: None },
    BinTag { index: 23, name: "ColorTemperatureSetting", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::Mul(100.0_f64), pc: Pc::Suffix(" K"), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 24, name: "ColorCompensationFilterSet", cond: Cond::Always, fmt: Fmt::I8, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::PlusOrVal, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 25, name: "CustomWB_RGBLevels", cond: Cond::Always, fmt: Fmt::U16Rev, count: 3, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 32, name: "FlashMode", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M49, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 33, name: "FlashControl", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M50, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 35, name: "FlashExposureCompSet", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::SubDiv(128.0_f64, 24.0_f64), pc: Pc::Signed1OrZero, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 36, name: "AFAreaMode", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M51, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 37, name: "LongExposureNoiseReduction", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M52, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 38, name: "HighISONoiseReduction", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M53, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 39, name: "SmileShutterMode", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M54, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 40, name: "RedEyeReduction", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M52, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 45, name: "HDRSetting", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M45, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 46, name: "HDRLevel", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M55, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 47, name: "ViewingMode", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M56, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 48, name: "FaceDetection", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M52, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 49, name: "SmileShutter", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M52, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 50, name: "SweepPanoramaSize", cond: Cond::ModelRe(true, r"^DSLR-(A450|A500|A550)$"), fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M57, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 51, name: "SweepPanoramaDirection", cond: Cond::ModelRe(true, r"^DSLR-(A450|A500|A550)$"), fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M58, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 52, name: "DriveMode", cond: Cond::ModelRe(true, r"^DSLR-(A450|A500|A550)$"), fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M59, Other::None), hook: Hook::None, print_hex: true, low_priority: true, subdir: None },
    BinTag { index: 53, name: "MultiFrameNoiseReduction", cond: Cond::ModelRe(true, r"^DSLR-(A450|A500|A550)$"), fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M60, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 54, name: "LiveViewAFSetting", cond: Cond::ModelRe(true, r"^(NEX-|DSLR-(A450|A500|A550)$)"), fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M61, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 56, name: "PanoramaSize3D", cond: Cond::ModelRe(true, r"^DSLR-(A450|A500|A550)$"), fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M62, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 131, name: "AFButtonPressed", cond: Cond::ModelRe(true, r"^(NEX-|DSLR-(A450|A500|A550)$)"), fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M63, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 132, name: "LiveViewMetering", cond: Cond::ModelRe(true, r"^(NEX-|DSLR-(A450|A500|A550)$)"), fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M64, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 133, name: "ViewingMode2", cond: Cond::ModelRe(true, r"^(NEX-|DSLR-(A450|A500|A550)$)"), fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M65, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 134, name: "AELock", cond: Cond::ModelRe(true, r"^(NEX-|DSLR-(A450|A500|A550)$)"), fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M66, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 135, name: "FlashStatusBuilt-in", cond: Cond::ModelRe(true, r"^DSLR-(A450|A500|A550)"), fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M22, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 136, name: "FlashStatusExternal", cond: Cond::ModelRe(true, r"^DSLR-(A450|A500|A550)"), fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M67, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 139, name: "LiveViewFocusMode", cond: Cond::ModelRe(true, r"^(NEX-|DSLR-(A450|A500|A550)$)"), fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M68, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 153, name: "LensMount", cond: Cond::ModelRe(true, r"^DSLR-(A450|A500|A550)$"), fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::Store(Dm::LensMount), vc: Vc::None, pc: Pc::Map(M69, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 268, name: "SequenceNumber", cond: Cond::ModelRe(true, r"^DSLR-(A450|A500|A550)$"), fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M70, Other::Identity), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 276, name: "FolderNumber", cond: Cond::ModelRe(true, r"^DSLR-(A450|A500|A550)$"), fmt: Fmt::U32, count: 1, mask: 16760832, raw: Raw::None, vc: Vc::None, pc: Pc::ZeroPad(3), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 276, name: "ImageNumber", cond: Cond::ModelRe(true, r"^DSLR-(A450|A500|A550)$"), fmt: Fmt::U32, count: 1, mask: 16383, raw: Raw::None, vc: Vc::None, pc: Pc::ZeroPad(4), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 512, name: "ShotNumberSincePowerUp2", cond: Cond::ModelRe(true, r"^DSLR-(A450|A500|A550)$"), fmt: Fmt::U32, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 643, name: "AFButtonPressed", cond: Cond::ModelRe(false, r"^DSLR-(A450|A500|A550)$"), fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M63, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 644, name: "LiveViewMetering", cond: Cond::ModelRe(false, r"^DSLR-(A450|A500|A550)$"), fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M64, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 645, name: "ViewingMode2", cond: Cond::ModelRe(false, r"^DSLR-(A450|A500|A550)$"), fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M65, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 646, name: "AELock", cond: Cond::ModelRe(false, r"^DSLR-(A450|A500|A550)$"), fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M66, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 647, name: "FlashStatusBuilt-in", cond: Cond::ModelRe(false, r"^DSLR-(A450|A500|A550)$"), fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M22, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 648, name: "FlashStatusExternal", cond: Cond::ModelRe(false, r"^DSLR-(A450|A500|A550)$"), fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M67, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 651, name: "LiveViewFocusMode", cond: Cond::ModelRe(false, r"^DSLR-(A450|A500|A550)$"), fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M68, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 780, name: "SequenceNumber", cond: Cond::ModelRe(false, r"^DSLR-(A450|A500|A550)$"), fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M70, Other::Identity), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 788, name: "ImageNumber", cond: Cond::ModelRe(false, r"^DSLR-(A450|A500|A550)$"), fmt: Fmt::U16, count: 1, mask: 16383, raw: Raw::None, vc: Vc::None, pc: Pc::ZeroPad(4), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 790, name: "FolderNumber", cond: Cond::ModelRe(false, r"^DSLR-(A450|A500|A550)$"), fmt: Fmt::U16, count: 1, mask: 1023, raw: Raw::None, vc: Vc::None, pc: Pc::ZeroPad(3), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 1008, name: "LensE-mountVersion", cond: Cond::ModelRe(false, r"^NEX-"), fmt: Fmt::U16, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::HexDotHex, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 1011, name: "LensFirmwareVersion", cond: Cond::ModelRe(false, r"^NEX-"), fmt: Fmt::U16, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::VerHex, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 1015, name: "LensType2", cond: Cond::All(&[Cond::ModelRe(false, r"^NEX-"), Cond::DmCmp(Dm::LensMount, NumCmp::Ne, 1.0_f64)]), fmt: Fmt::U16, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M71, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 1024, name: "ImageNumber", cond: Cond::ModelRe(false, r"^DSLR-(A450|A500|A550)$"), fmt: Fmt::U16, count: 1, mask: 16383, raw: Raw::None, vc: Vc::None, pc: Pc::ZeroPad(4), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
    BinTag { index: 1026, name: "FolderNumber", cond: Cond::ModelRe(false, r"^DSLR-(A450|A500|A550)$"), fmt: Fmt::U16, count: 1, mask: 1023, raw: Raw::None, vc: Vc::None, pc: Pc::ZeroPad(3), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
];
#[rustfmt::skip]
static T3: &[BinTag] = &[
    BinTag { index: 0, name: "Face1Position", cond: Cond::Always, fmt: Fmt::U16, count: 4, mask: 0, raw: Raw::DropIfDmLess(Dm::FacesDetected, 1.0_f64), vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: false, subdir: None },
    BinTag { index: 32, name: "Face2Position", cond: Cond::Always, fmt: Fmt::U16, count: 4, mask: 0, raw: Raw::DropIfDmLess(Dm::FacesDetected, 2.0_f64), vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: false, subdir: None },
    BinTag { index: 64, name: "Face3Position", cond: Cond::Always, fmt: Fmt::U16, count: 4, mask: 0, raw: Raw::DropIfDmLess(Dm::FacesDetected, 3.0_f64), vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: false, subdir: None },
    BinTag { index: 96, name: "Face4Position", cond: Cond::Always, fmt: Fmt::U16, count: 4, mask: 0, raw: Raw::DropIfDmLess(Dm::FacesDetected, 4.0_f64), vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: false, subdir: None },
    BinTag { index: 128, name: "Face5Position", cond: Cond::Always, fmt: Fmt::U16, count: 4, mask: 0, raw: Raw::DropIfDmLess(Dm::FacesDetected, 5.0_f64), vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: false, subdir: None },
    BinTag { index: 160, name: "Face6Position", cond: Cond::Always, fmt: Fmt::U16, count: 4, mask: 0, raw: Raw::DropIfDmLess(Dm::FacesDetected, 6.0_f64), vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: false, subdir: None },
    BinTag { index: 192, name: "Face7Position", cond: Cond::Always, fmt: Fmt::U16, count: 4, mask: 0, raw: Raw::DropIfDmLess(Dm::FacesDetected, 7.0_f64), vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: false, subdir: None },
    BinTag { index: 224, name: "Face8Position", cond: Cond::Always, fmt: Fmt::U16, count: 4, mask: 0, raw: Raw::DropIfDmLess(Dm::FacesDetected, 8.0_f64), vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: false, subdir: None },
];
#[rustfmt::skip]
static T4: &[BinTag] = &[
    BinTag { index: 0, name: "Face1Position", cond: Cond::Always, fmt: Fmt::U16, count: 4, mask: 0, raw: Raw::DropIfDmLess(Dm::FacesDetected, 1.0_f64), vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: false, subdir: None },
    BinTag { index: 37, name: "Face2Position", cond: Cond::Always, fmt: Fmt::U16, count: 4, mask: 0, raw: Raw::DropIfDmLess(Dm::FacesDetected, 2.0_f64), vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: false, subdir: None },
    BinTag { index: 74, name: "Face3Position", cond: Cond::Always, fmt: Fmt::U16, count: 4, mask: 0, raw: Raw::DropIfDmLess(Dm::FacesDetected, 3.0_f64), vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: false, subdir: None },
    BinTag { index: 111, name: "Face4Position", cond: Cond::Always, fmt: Fmt::U16, count: 4, mask: 0, raw: Raw::DropIfDmLess(Dm::FacesDetected, 4.0_f64), vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: false, subdir: None },
    BinTag { index: 148, name: "Face5Position", cond: Cond::Always, fmt: Fmt::U16, count: 4, mask: 0, raw: Raw::DropIfDmLess(Dm::FacesDetected, 5.0_f64), vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: false, subdir: None },
    BinTag { index: 185, name: "Face6Position", cond: Cond::Always, fmt: Fmt::U16, count: 4, mask: 0, raw: Raw::DropIfDmLess(Dm::FacesDetected, 6.0_f64), vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: false, subdir: None },
    BinTag { index: 222, name: "Face7Position", cond: Cond::Always, fmt: Fmt::U16, count: 4, mask: 0, raw: Raw::DropIfDmLess(Dm::FacesDetected, 7.0_f64), vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: false, subdir: None },
    BinTag { index: 259, name: "Face8Position", cond: Cond::Always, fmt: Fmt::U16, count: 4, mask: 0, raw: Raw::DropIfDmLess(Dm::FacesDetected, 8.0_f64), vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: false, subdir: None },
];
#[rustfmt::skip]
static T5: &[BinTag] = &[
    BinTag { index: 2, name: "FaceInfoOffset", cond: Cond::Always, fmt: Fmt::U16, count: 1, mask: 0, raw: Raw::Store(Dm::FaceInfoOffset), vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: false, subdir: None },
    BinTag { index: 6, name: "SonyDateTime", cond: Cond::Always, fmt: Fmt::Str, count: 20, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::DateTime, hook: Hook::None, print_hex: false, low_priority: false, subdir: None },
    BinTag { index: 26, name: "SonyImageHeight", cond: Cond::Always, fmt: Fmt::U16, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: false, subdir: None },
    BinTag { index: 28, name: "SonyImageWidth", cond: Cond::Always, fmt: Fmt::U16, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: false, subdir: None },
    BinTag { index: 48, name: "FacesDetected", cond: Cond::Always, fmt: Fmt::U16, count: 1, mask: 0, raw: Raw::Store(Dm::FacesDetected), vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: false, subdir: None },
    BinTag { index: 50, name: "FaceInfoLength", cond: Cond::Always, fmt: Fmt::U16, count: 1, mask: 0, raw: Raw::Store(Dm::FaceInfoLength), vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: false, subdir: None },
    BinTag { index: 52, name: "MetaVersion", cond: Cond::Always, fmt: Fmt::Str, count: 16, mask: 0, raw: Raw::Store(Dm::MetaVersion), vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: false, subdir: None },
    BinTag { index: 72, name: "FaceInfo1", cond: Cond::All(&[Cond::DmTruthy(Dm::FacesDetected), Cond::DmCmp(Dm::FaceInfoOffset, NumCmp::Eq, 72.0_f64), Cond::DmCmp(Dm::FaceInfoLength, NumCmp::Eq, 32.0_f64)]), fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: false, subdir: Some(3) },
    BinTag { index: 94, name: "FaceInfo2", cond: Cond::All(&[Cond::DmTruthy(Dm::FacesDetected), Cond::DmCmp(Dm::FaceInfoOffset, NumCmp::Eq, 94.0_f64), Cond::DmCmp(Dm::FaceInfoLength, NumCmp::Eq, 37.0_f64)]), fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: false, subdir: Some(4) },
];

/// Every table, indexed by the `SubDir`/`Root` table numbers above.
pub static TABLES: &[BinTable] = &[
    BinTable {
        name: "CameraSettings",
        fmt: Fmt::U16,
        tags: T0,
    },
    BinTable {
        name: "CameraSettings2",
        fmt: Fmt::U16,
        tags: T1,
    },
    BinTable {
        name: "CameraSettings3",
        fmt: Fmt::U8,
        tags: T2,
    },
    BinTable {
        name: "FaceInfo1",
        fmt: Fmt::Default,
        tags: T3,
    },
    BinTable {
        name: "FaceInfo2",
        fmt: Fmt::Default,
        tags: T4,
    },
    BinTable {
        name: "ShotInfo",
        fmt: Fmt::Default,
        tags: T5,
    },
];

/// Table numbers, by ExifTool table name.
#[allow(dead_code)]
pub mod idx {
    pub const CAMERASETTINGS: usize = 0;
    pub const CAMERASETTINGS2: usize = 1;
    pub const CAMERASETTINGS3: usize = 2;
    pub const FACEINFO1: usize = 3;
    pub const FACEINFO2: usize = 4;
    pub const SHOTINFO: usize = 5;
}
