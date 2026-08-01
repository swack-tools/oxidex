//! `Image::ExifTool::NikonSettings::Main` -- generated, do not hand-edit.
//!
//! Every row here was walked out of ExifTool's own `%Image::ExifTool::
//! NikonSettings::Main` hash rather than retyped, so the names, PrintConv
//! tables, Conditions and Masks are the ones ExifTool itself applies.
//! Tags flagged `Unknown` are omitted because ExifTool does not report them
//! without `-u`.

use super::settings::{Cond, Conv, Dm, SettingsTag as E};

#[rustfmt::skip]
const PC_0: &[(u32, &str)] = &[(1, "ISO 200"), (2, "ISO 250"), (3, "ISO 280"), (4, "ISO 320"), (5, "ISO 400"), (6, "ISO 500"), (7, "ISO 560"), (8, "ISO 640"), (9, "ISO 800"), (10, "ISO 1000"), (11, "ISO 1100"), (12, "ISO 1250"), (13, "ISO 1600"), (14, "ISO 2000"), (15, "ISO 2200"), (16, "ISO 2500"), (17, "ISO 3200"), (18, "ISO 4000"), (19, "ISO 4500"), (20, "ISO 5000"), (21, "ISO 6400"), (22, "ISO 8000"), (23, "ISO 9000"), (24, "ISO 10000"), (25, "ISO 12800"), (26, "ISO 16000"), (27, "ISO 18000"), (28, "ISO 20000"), (29, "ISO 25600"), (30, "ISO 32000"), (31, "ISO 36000"), (32, "ISO 40000"), (33, "ISO 51200"), (34, "ISO 64000"), (35, "ISO 72000"), (36, "ISO 81200"), (37, "ISO 102400"), (38, "ISO Hi 0.3"), (39, "ISO Hi 0.5"), (40, "ISO Hi 0.7"), (41, "ISO Hi 1.0"), (42, "ISO Hi 2.0"), (43, "ISO Hi 3.0"), (44, "ISO Hi 4.0"), (45, "ISO Hi 5.0")];
#[rustfmt::skip]
const PC_1: &[(u32, &str)] = &[(1, "ISO 100"), (2, "ISO 125"), (4, "ISO 160"), (5, "ISO 200"), (6, "ISO 250"), (8, "ISO 320"), (9, "ISO 400"), (10, "ISO 500"), (12, "ISO 640"), (13, "ISO 800"), (14, "ISO 1000"), (16, "ISO 1250"), (17, "ISO 1600"), (18, "ISO 2000"), (20, "ISO 2500"), (21, "ISO 3200"), (22, "ISO 4000"), (24, "ISO 5000"), (25, "ISO 6400"), (26, "ISO 8000"), (28, "ISO 10000"), (29, "ISO 12800"), (30, "ISO 16000"), (32, "ISO 20000"), (33, "ISO 25600"), (38, "ISO Hi 0.3"), (39, "ISO Hi 0.5"), (40, "ISO Hi 0.7"), (41, "ISO Hi 1.0"), (42, "ISO Hi 2.0")];
#[rustfmt::skip]
const PC_2: &[(u32, &str)] = &[(1, "Auto"), (2, "Manual (dark on light)"), (3, "Manual (light on dark)")];
#[rustfmt::skip]
const PC_3: &[(u32, &str)] = &[(1, "Manual (dark on light)"), (2, "Manual (light on dark)")];
#[rustfmt::skip]
const PC_4: &[(u32, &str)] = &[(1, "Enable"), (2, "Disable")];
#[rustfmt::skip]
const PC_5: &[(u32, &str)] = &[(1, "Right to Left"), (2, "Left to Right")];
#[rustfmt::skip]
const PC_6: &[(u32, &str)] = &[(1, "Auto"), (2, "2160p"), (3, "1080p"), (4, "1080i"), (5, "720p"), (6, "576p"), (7, "480p")];
#[rustfmt::skip]
const PC_7: &[(u32, &str)] = &[(1, "Auto"), (2, "Limit"), (3, "Full")];
#[rustfmt::skip]
const PC_8: &[(u32, &str)] = &[(1, "AF-On"), (2, "AF Lock Only"), (3, "AE Lock (reset on release)"), (4, "AE Lock Only"), (5, "AE/AF Lock"), (6, "FV Lock"), (7, "Flash Disable/Enable"), (8, "Preview"), (9, "+NEF(RAW)"), (10, "LiveView Info Display On/Off"), (11, "Recall Shooting Functions"), (12, "None")];
#[rustfmt::skip]
const PC_9: &[(u32, &str)] = &[(1, "AF-On"), (2, "AF Lock Only"), (3, "AE Lock (reset on release)"), (4, "AE Lock Only"), (5, "AE/AF Lock"), (6, "FV Lock"), (7, "Flash Disable/Enable"), (8, "Preview"), (9, "+NEF(RAW)"), (10, "None"), (11, "LiveView Info Display On/Off")];
#[rustfmt::skip]
const PC_10: &[(u32, &str)] = &[(1, "ISO 200"), (2, "ISO 250"), (4, "ISO 320"), (5, "ISO 400"), (6, "ISO 500"), (8, "ISO 640"), (9, "ISO 800"), (10, "ISO 1000"), (12, "ISO 1250"), (13, "ISO 1600"), (14, "ISO 2000"), (16, "ISO 2500"), (17, "ISO 3200"), (18, "ISO 4000"), (20, "ISO 5000"), (21, "ISO 6400"), (22, "ISO 8000"), (24, "ISO 10000"), (25, "ISO 12800"), (26, "ISO 16000"), (28, "ISO 20000"), (29, "ISO 25600"), (34, "ISO Hi 0.3"), (35, "ISO Hi 0.5"), (36, "ISO Hi 0.7"), (37, "ISO Hi 1.0"), (38, "ISO Hi 2.0")];
#[rustfmt::skip]
const PC_11: &[(u32, &str)] = &[(1, "No"), (2, "Shutter Speed & Aperture")];
#[rustfmt::skip]
const PC_12: &[(u32, &str)] = &[(1, "Exposure Compensation"), (2, "Exposure Compensation, Shutter Speed & Aperture")];
#[rustfmt::skip]
const PC_13: &[(u32, &str)] = &[(1, "On"), (2, "Off")];
#[rustfmt::skip]
const PC_14: &[(u32, &str)] = &[(1, "Red"), (2, "Yellow"), (3, "Blue"), (4, "White")];
#[rustfmt::skip]
const PC_15: &[(u32, &str)] = &[(1, "255"), (2, "248"), (3, "235"), (4, "224"), (5, "213"), (6, "202"), (7, "191"), (8, "180")];
#[rustfmt::skip]
const PC_16: &[(u32, &str)] = &[(1, "1 (Quick)"), (2, "2"), (3, "3 (Normal)"), (4, "4"), (5, "5 (Delay)")];
#[rustfmt::skip]
const PC_17: &[(u32, &str)] = &[(1, "Erratic"), (2, "Steady")];
#[rustfmt::skip]
const PC_18: &[(u32, &str)] = &[(1, "Yes"), (2, "No")];
#[rustfmt::skip]
const PC_19: &[(u32, &str)] = &[(1, "Focus Point"), (2, "Focus Point and AF-area mode"), (3, "Off")];
#[rustfmt::skip]
const PC_20: &[(u32, &str)] = &[(1, "Focus Point"), (2, "Off")];
#[rustfmt::skip]
const PC_21: &[(u32, &str)] = &[(1, "1/3 EV"), (2, "1/2 EV"), (3, "1 EV")];
#[rustfmt::skip]
const PC_22: &[(u32, &str)] = &[(1, "Sync"), (2, "No Sync")];
#[rustfmt::skip]
const PC_23: &[(u32, &str)] = &[(1, "Flash/Speed"), (2, "Flash/Speed/Aperture"), (3, "Flash/Aperture"), (4, "Flash Only")];
#[rustfmt::skip]
const PC_24: &[(u32, &str)] = &[(1, "Preset Focus Point - Press To Recall"), (2, "Preset Focus Point - Hold To Recall"), (3, "AF-AreaMode S"), (4, "AF-AreaMode D9"), (5, "AF-AreaMode D25"), (6, "AF-AreaMode D49"), (7, "AF-AreaMode D105"), (8, "AF-AreaMode 3D"), (9, "AF-AreaMode Group"), (10, "AF-AreaMode Group C1"), (11, "AF-AreaMode Group C2"), (12, "AF-AreaMode Auto Area"), (13, "AF-AreaMode + AF-On S"), (14, "AF-AreaMode + AF-On D9"), (15, "AF-AreaMode + AF-On D25"), (16, "AF-AreaMode + AF-On D49"), (17, "AF-AreaMode + AF-On D105"), (18, "AF-AreaMode + AF-On 3D"), (19, "AF-AreaMode + AF-On Group"), (20, "AF-AreaMode + AF-On Group C1"), (21, "AF-AreaMode + AF-On Group C2"), (22, "AF-AreaMode + AF-On Auto Area"), (23, "AF-On"), (24, "AF Lock Only"), (25, "AE Lock (hold)"), (26, "AE/WB Lock (hold)"), (27, "AE Lock (reset on release)"), (28, "AE Lock Only"), (29, "AE/AF Lock"), (30, "FV Lock"), (31, "Flash Disable/Enable"), (32, "Preview"), (33, "Recall Shooting Functions"), (34, "Bracketing Burst"), (35, "Synchronized Release (Master)"), (36, "Synchronized Release (Remote)"), (39, "+NEF(RAW)"), (40, "Grid Display"), (41, "Virtual Horizon"), (42, "Voice Memo"), (43, "Wired LAN"), (44, "My Menu"), (45, "My Menu Top Item"), (46, "Playback"), (47, "Filtered Playback"), (48, "Photo Shooting Bank"), (49, "AF Mode/AF Area Mode"), (50, "Image Area"), (51, "Active-D Lighting"), (52, "Exposure Delay Mode"), (53, "Shutter/Aperture Lock"), (54, "1 Stop Speed/Aperture"), (55, "Non-CPU Lens"), (56, "None")];
#[rustfmt::skip]
const PC_25: &[(u32, &str)] = &[(1, "AF-On or Subject Tracking"), (2, "AF Lock Only"), (3, "AE Lock (hold)"), (4, "AE Lock (reset on release)"), (5, "AE Lock Only"), (6, "AE/AF Lock"), (7, "FV Lock"), (8, "Flash Disable/Enable"), (9, "Preview"), (10, "Matrix Metering"), (11, "Center-weighted Metering"), (12, "Spot Metering"), (13, "Highlight-weighted Metering"), (14, "Bracketing Burst"), (15, "Synchronized Release (Master)"), (16, "Synchronized Release (Remote)"), (19, "+NEF(RAW)"), (20, "Framing Grid Display"), (22, "Zoom On/Off"), (24, "My Menu"), (25, "My Menu Top Item"), (26, "Playback"), (27, "Protect"), (28, "Image Area"), (29, "Image Quality"), (30, "White Balance"), (31, "Picture Control"), (32, "Active D-Lighting"), (33, "Metering"), (34, "Flash Mode"), (35, "Focus Mode"), (36, "Auto Bracketing"), (37, "Multiple Exposure"), (38, "HDR"), (39, "Exposure Delay Mode"), (40, "Shutter/Aperture Lock"), (41, "Focus Peaking"), (42, "Rating"), (43, "Non-CPU Lens"), (44, "None")];
#[rustfmt::skip]
const PC_26: &[(u32, &str)] = &[(1, "AF-On"), (2, "AF Lock Only"), (3, "AE Lock (hold)"), (4, "AE Lock (reset on release)"), (5, "AE Lock Only"), (6, "AE/AF Lock"), (7, "FV Lock"), (8, "Flash Disable/Enable"), (9, "Preview"), (10, "Matrix Metering"), (11, "Center-weighted Metering"), (12, "Spot Metering"), (13, "Highlight-weighted Metering"), (14, "Bracketing Burst"), (15, "Synchronized Release (Master)"), (16, "Synchronized Release (Remote)"), (19, "+NEF(RAW)"), (20, "Subject Tracking"), (21, "Silent Photography"), (22, "LiveView Info Display On/Off"), (23, "Grid Display"), (24, "Zoom (Low)"), (25, "Zoom (1:1)"), (26, "Zoom (High)"), (27, "My Menu"), (28, "My Menu Top Item"), (29, "Playback"), (30, "Protect"), (31, "Image Area"), (32, "Image Quality"), (33, "White Balance"), (34, "Picture Control"), (35, "Active-D Lighting"), (36, "Metering"), (37, "Flash Mode"), (38, "Focus Mode"), (39, "Auto Bracketing"), (40, "Multiple Exposure"), (41, "HDR"), (42, "Exposure Delay Mode"), (43, "Shutter/Aperture Lock"), (44, "Focus Peaking"), (45, "Rating 0"), (46, "Rating 5"), (47, "Rating 4"), (48, "Rating 3"), (49, "Rating 2"), (50, "Rating 1"), (52, "None")];
#[rustfmt::skip]
const PC_27: &[(u32, &str)] = &[(1, "AF-AreaMode S"), (2, "AF-AreaMode D9"), (3, "AF-AreaMode D25"), (4, "AF-AreaMode D49"), (5, "AF-AreaMode D105"), (6, "AF-AreaMode 3D"), (7, "AF-AreaMode Group"), (8, "AF-AreaMode Group C1"), (9, "AF-AreaMode Group C2"), (10, "AF-AreaMode Auto Area"), (11, "AF-AreaMode + AF-On S"), (12, "AF-AreaMode + AF-On D9"), (13, "AF-AreaMode + AF-On D25"), (14, "AF-AreaMode + AF-On D49"), (15, "AF-AreaMode + AF-On D105"), (16, "AF-AreaMode + AF-On 3D"), (17, "AF-AreaMode + AF-On Group"), (18, "AF-AreaMode + AF-On Group C1"), (19, "AF-AreaMode + AF-On Group C2"), (20, "AF-AreaMode + AF-On Auto Area"), (21, "AF-On"), (22, "AF Lock Only"), (23, "AE Lock (hold)"), (24, "AE/WB Lock (hold)"), (25, "AE Lock (reset on release)"), (26, "AE Lock Only"), (27, "AE/AF Lock"), (28, "Recall Shooting Functions"), (29, "None")];
#[rustfmt::skip]
const PC_28: &[(u32, &str)] = &[(1, "Center Focus Point"), (2, "AF-On"), (3, "AF Lock Only"), (4, "AE Lock (hold)"), (5, "AE Lock (reset on release)"), (6, "AE Lock Only"), (7, "AE/AF Lock"), (8, "LiveView Info Display On/Off"), (9, "Zoom (Low)"), (10, "Zoom (1:1)"), (11, "Zoom (High)"), (12, "None")];
#[rustfmt::skip]
const PC_29: &[(u32, &str)] = &[(1, "Same as MultiSelector"), (2, "Focus Point Selection")];
#[rustfmt::skip]
const PC_30: &[(u32, &str)] = &[(1, "Preset Focus Point - Press To Recall"), (2, "Preset Focus Point - Hold To Recall"), (3, "Center Focus Point"), (4, "AF-AreaMode S"), (5, "AF-AreaMode D9"), (6, "AF-AreaMode D25"), (7, "AF-AreaMode D49"), (8, "AF-AreaMode D105"), (9, "AF-AreaMode 3D"), (10, "AF-AreaMode Group"), (11, "AF-AreaMode Group C1"), (12, "AF-AreaMode Group C2"), (13, "AF-AreaMode Auto Area"), (14, "AF-AreaMode + AF-On S"), (15, "AF-AreaMode + AF-On D9"), (16, "AF-AreaMode + AF-On D25"), (17, "AF-AreaMode + AF-On D49"), (18, "AF-AreaMode + AF-On D105"), (19, "AF-AreaMode + AF-On 3D"), (20, "AF-AreaMode + AF-On Group"), (21, "AF-AreaMode + AF-On Group C1"), (22, "AF-AreaMode + AF-On Group C2"), (23, "AF-AreaMode + AF-On Auto Area"), (24, "AF-On"), (25, "AF Lock Only"), (26, "AE Lock (hold)"), (27, "AE/WB Lock (hold)"), (28, "AE Lock (reset on release)"), (29, "AE Lock Only"), (30, "AE/AF Lock"), (31, "FV Lock"), (32, "Flash Disable/Enable"), (33, "Preview"), (34, "Recall Shooting Functions"), (35, "Bracketing Burst"), (36, "Synchronized Release (Master)"), (37, "Synchronized Release (Remote)"), (38, "None")];
#[rustfmt::skip]
const PC_31: &[(u32, &str)] = &[(1, "Center Focus Point"), (2, "AF-On"), (3, "AF Lock Only"), (4, "AE Lock (hold)"), (5, "AE Lock (reset on release)"), (6, "AE Lock Only"), (7, "AE/AF Lock"), (8, "FV Lock"), (9, "Flash Disable/Enable"), (10, "Preview"), (11, "Matrix Metering"), (12, "Center-weighted Metering"), (13, "Spot Metering"), (14, "Highlight-weighted Metering"), (15, "Bracketing Burst"), (16, "Synchronized Release (Master)"), (17, "Synchronized Release (Remote)"), (20, "+NEF(RAW)"), (21, "LiveView Info Display On/Off"), (22, "Grid Display"), (23, "Image Area"), (24, "Non-CPU Lens"), (25, "None")];
#[rustfmt::skip]
const PC_32: &[(u32, &str)] = &[(1, "Preset Focus Point - Press To Recall"), (2, "Preset Focus Point - Hold To Recall"), (3, "AF-AreaMode S"), (4, "AF-AreaMode D9"), (5, "AF-AreaMode D25"), (6, "AF-AreaMode D49"), (7, "AF-AreaMode D105"), (8, "AF-AreaMode 3D"), (9, "AF-AreaMode Group"), (10, "AF-AreaMode Group C1"), (11, "AF-AreaMode Group C2"), (12, "AF-AreaMode Auto Area"), (13, "AF-AreaMode + AF-On S"), (14, "AF-AreaMode + AF-On D9"), (15, "AF-AreaMode + AF-On D25"), (16, "AF-AreaMode + AF-On D49"), (17, "AF-AreaMode + AF-On D105"), (18, "AF-AreaMode + AF-On 3D"), (19, "AF-AreaMode + AF-On Group"), (20, "AF-AreaMode + AF-On Group C1"), (21, "AF-AreaMode + AF-On Group C2"), (22, "AF-AreaMode + AF-On Auto Area"), (23, "AF-On"), (24, "AF Lock Only"), (25, "AE Lock Only"), (26, "AE/AF Lock"), (27, "Flash Disable/Enable"), (28, "Recall Shooting Functions"), (29, "Synchronized Release (Master)"), (30, "Synchronized Release (Remote)")];
#[rustfmt::skip]
const PC_33: &[(u32, &str)] = &[(1, "AF-On"), (2, "AF Lock Only"), (3, "AE Lock (hold)"), (4, "AE Lock (reset on release)"), (5, "AE Lock Only"), (6, "AE/AF Lock"), (7, "FV Lock"), (8, "Flash Disable/Enable"), (9, "Preview"), (10, "Matrix Metering"), (11, "Center-weighted Metering"), (12, "Spot Metering"), (13, "Highlight-weighted Metering"), (14, "Bracketing Burst"), (15, "Synchronized Release (Master)"), (16, "Synchronized Release (Remote)"), (19, "+NEF(RAW)"), (20, "Subject Tracking"), (21, "Grid Display"), (22, "Zoom (Low)"), (23, "Zoom (1:1)"), (24, "Zoom (High)"), (25, "My Menu"), (26, "My Menu Top Item"), (27, "Playback"), (28, "None")];
#[rustfmt::skip]
const PC_34: &[(u32, &str)] = &[(1, "Sub-command Dial"), (2, "Aperture Ring")];
#[rustfmt::skip]
const PC_35: &[(u32, &str)] = &[(1, "Restart Standby Timer"), (2, "Do Nothing")];
#[rustfmt::skip]
const PC_36: &[(u32, &str)] = &[(1, "Enable"), (2, "Enable (Standby Timer Active)"), (3, "Disable")];
#[rustfmt::skip]
const PC_37: &[(u32, &str)] = &[(1, "LCD Backlight"), (2, "LCD Backlight and Shooting Information")];
#[rustfmt::skip]
const PC_38: &[(u32, &str)] = &[(1, "Power Aperture (Open)"), (2, "Exposure Compensation"), (3, "Grid Display"), (4, "Zoom (Low)"), (5, "Zoom (1:1)"), (6, "Zoom (High)"), (7, "Image Area"), (8, "Microphone Sensitivity"), (9, "None")];
#[rustfmt::skip]
const PC_39: &[(u32, &str)] = &[(1, "Power Aperture (Open)"), (2, "Exposure Compensation"), (3, "Subject Tracking"), (4, "LiveView Info Display On/Off"), (5, "Grid Display"), (6, "Zoom (Low)"), (7, "Zoom (1:1)"), (8, "Zoom (High)"), (9, "Protect"), (10, "Image Area"), (11, "White Balance"), (12, "Picture Control"), (13, "Active-D Lighting"), (14, "Metering"), (15, "Focus Mode"), (16, "Microphone Sensitivity"), (17, "Focus Peaking"), (18, "Rating (None)"), (19, "Rating (5)"), (20, "Rating (4)"), (21, "Rating (3)"), (22, "Rating (2)"), (23, "Rating (1)"), (25, "None")];
#[rustfmt::skip]
const PC_40: &[(u32, &str)] = &[(1, "Power Aperture (Close)"), (2, "Exposure Compensation"), (3, "Grid Display"), (4, "Zoom (Low)"), (5, "Zoom (1:1)"), (6, "Zoom (High)"), (7, "Image Area"), (8, "Microphone Sensitivity"), (9, "None")];
#[rustfmt::skip]
const PC_41: &[(u32, &str)] = &[(1, "Power Aperture (Close)"), (2, "Exposure Compensation"), (3, "Subject Tracking"), (4, "LiveView Info Display On/Off"), (5, "Grid Display"), (6, "Zoom (Low)"), (7, "Zoom (1:1)"), (8, "Zoom (High)"), (9, "Protect"), (10, "Image Area"), (11, "White Balance"), (12, "Picture Control"), (13, "Active-D Lighting"), (14, "Metering"), (15, "Focus Mode"), (16, "Microphone Sensitivity"), (17, "Focus Peaking"), (18, "Rating (None)"), (19, "Rating (5)"), (20, "Rating (4)"), (21, "Rating (3)"), (22, "Rating (2)"), (23, "Rating (1)"), (25, "None")];
#[rustfmt::skip]
const PC_42: &[(u32, &str)] = &[(1, "Grid Display"), (2, "Zoom (Low)"), (3, "Zoom (1:1)"), (4, "Zoom (High)"), (5, "Image Area"), (6, "Microphone Sensitivity"), (7, "None")];
#[rustfmt::skip]
const PC_43: &[(u32, &str)] = &[(1, "Center Focus Point"), (2, "AF Lock Only"), (3, "AE Lock (hold)"), (4, "AE/WB Lock (hold)"), (5, "AE Lock Only"), (6, "AE/AF Lock"), (7, "Zoom (Low)"), (8, "Zoom (1:1)"), (9, "Zoom (High)"), (10, "Record Movie"), (11, "None")];
#[rustfmt::skip]
const PC_44: &[(u32, &str)] = &[(1, "Center Focus Point"), (2, "AF Lock Only"), (3, "AE Lock (hold)"), (4, "AE Lock Only"), (5, "AE/AF Lock"), (6, "LiveView Info Display On/Off"), (7, "Grid Display"), (8, "Zoom (Low)"), (9, "Zoom (1:1)"), (10, "Zoom (High)"), (11, "Record Movie"), (12, "Image Area"), (13, "None")];
#[rustfmt::skip]
const PC_45: &[(u32, &str)] = &[(1, "Same As Without Flash"), (2, "ISO 200"), (3, "ISO 250"), (5, "ISO 320"), (6, "ISO 400"), (7, "ISO 500"), (9, "ISO 640"), (10, "ISO 800"), (11, "ISO 1000"), (13, "ISO 1250"), (14, "ISO 1600"), (15, "ISO 2000"), (17, "ISO 2500"), (18, "ISO 3200"), (19, "ISO 4000"), (21, "ISO 5000"), (22, "ISO 6400"), (23, "ISO 8000"), (25, "ISO 10000"), (26, "ISO 12800"), (27, "ISO 16000"), (29, "ISO 20000"), (30, "ISO 25600"), (31, "ISO 32000"), (33, "ISO 40000"), (34, "ISO 51200"), (35, "ISO 64000"), (36, "ISO 72000"), (37, "ISO 81200"), (38, "ISO 102400"), (39, "ISO Hi 0.3"), (40, "ISO Hi 0.5"), (41, "ISO Hi 0.7"), (42, "ISO Hi 1.0"), (43, "ISO Hi 2.0"), (44, "ISO Hi 3.0"), (45, "ISO Hi 4.0"), (46, "ISO Hi 5.0")];
#[rustfmt::skip]
const PC_46: &[(u32, &str)] = &[(1, "Same As Without Flash"), (2, "ISO 100"), (3, "ISO 125"), (5, "ISO 160"), (6, "ISO 200"), (7, "ISO 250"), (9, "ISO 320"), (10, "ISO 400"), (11, "ISO 500"), (13, "ISO 640"), (14, "ISO 800"), (15, "ISO 1000"), (17, "ISO 1250"), (18, "ISO 1600"), (19, "ISO 2000"), (21, "ISO 2500"), (22, "ISO 3200"), (23, "ISO 4000"), (25, "ISO 5000"), (26, "ISO 6400"), (27, "ISO 8000"), (29, "ISO 10000"), (30, "ISO 12800"), (31, "ISO 16000"), (33, "ISO 20000"), (34, "ISO 25600"), (39, "ISO Hi 0.3"), (40, "ISO Hi 0.5"), (41, "ISO Hi 0.7"), (42, "ISO Hi 1.0"), (43, "ISO Hi 2.0")];
#[rustfmt::skip]
const PC_47: &[(u32, &str)] = &[(1, "A"), (2, "B"), (3, "C"), (4, "D")];
#[rustfmt::skip]
const PC_48: &[(u32, &str)] = &[(1, "High Sensitivity"), (2, "Standard Sensitivity"), (3, "Low Sensitivity"), (4, "Off")];
#[rustfmt::skip]
const PC_49: &[(u32, &str)] = &[(1, "Aperture"), (2, "Exposure Compensation"), (3, "ISO Sensitivity"), (4, "None (Disabled)")];
#[rustfmt::skip]
const PC_50: &[(u32, &str)] = &[(1, "Center Focus Point"), (2, "Zoom (Low)"), (3, "Zoom (1:1)"), (4, "Zoom (High)"), (5, "Record Movie"), (6, "None")];
#[rustfmt::skip]
const PC_51: &[(u32, &str)] = &[(1, "Always"), (2, "Only During Recording")];
#[rustfmt::skip]
const PC_52: &[(u32, &str)] = &[(1, "1 (High)"), (2, "2"), (3, "3"), (4, "4 (Normal)"), (5, "5"), (6, "6"), (7, "7 (Low)")];
#[rustfmt::skip]
const PC_53: &[(u32, &str)] = &[(1, "Pattern 1"), (2, "Pattern 2"), (3, "Off")];
#[rustfmt::skip]
const PC_54: &[(u32, &str)] = &[(1, "Center Focus Point"), (2, "AF-On"), (3, "AF Lock Only"), (4, "AE Lock (hold)"), (5, "AE Lock Only"), (6, "AE/AF Lock"), (7, "LiveView Info Display On/Off"), (8, "Zoom (Low)"), (9, "Zoom (1:1)"), (10, "Zoom (High)"), (11, "Record Movie"), (12, "None")];
#[rustfmt::skip]
const PC_55: &[(u32, &str)] = &[(1, "Overflow"), (2, "Backup"), (3, "NEF Primary + JPG Secondary"), (4, "JPG Primary + JPG Secondary")];
#[rustfmt::skip]
const PC_56: &[(u32, &str)] = &[(1, "8 Bit"), (2, "10 Bit")];
#[rustfmt::skip]
const PC_57: &[(u32, &str)] = &[(2, "On"), (3, "Off")];
#[rustfmt::skip]
const PC_58: &[(u32, &str)] = &[(1, "AE/Flash"), (2, "AE"), (3, "Flash"), (4, "White Balance"), (5, "Active-D Lighting")];
#[rustfmt::skip]
const PC_59: &[(u32, &str)] = &[(15, "+3F"), (16, "-3F"), (17, "+2F"), (18, "-2F"), (19, "Disabled"), (20, "3F"), (21, "5F"), (22, "7F"), (23, "9F")];
#[rustfmt::skip]
const PC_60: &[(u32, &str)] = &[(1, "B3F"), (2, "A3F"), (3, "B2F"), (4, "A2F"), (5, "Disabled"), (6, "3F"), (7, "5F"), (8, "7F"), (9, "9F"), (19, "N/A")];
#[rustfmt::skip]
const PC_61: &[(u32, &str)] = &[(10, "Disabled"), (11, "2 Exposures"), (12, "3 Exposures"), (13, "4 Exposures"), (14, "5 Exposures")];
#[rustfmt::skip]
const PC_62: &[(u32, &str)] = &[(1, "0.3"), (3, "0.5"), (4, "1.0"), (5, "2.0"), (6, "3.0")];
#[rustfmt::skip]
const PC_63: &[(u32, &str)] = &[(0, "Off"), (1, "Off, Low"), (2, "Off, Normal"), (3, "Off, High"), (4, "Off, Extra High"), (5, "Off, Auto"), (6, "Off, Low, Normal"), (7, "Off, Low, Normal, High"), (8, "Off, Low, Normal, High, Extra High")];
#[rustfmt::skip]
const PC_64: &[(u32, &str)] = &[(1, "1x7"), (2, "1x5"), (3, "3x7"), (4, "3x5"), (5, "3x3"), (6, "5x7"), (7, "5x5"), (8, "5x3"), (9, "5x1"), (10, "7x7"), (11, "7x5"), (12, "7x3"), (13, "7x1"), (14, "11x3"), (15, "11x1"), (16, "15x3"), (17, "15x1")];
#[rustfmt::skip]
const PC_65: &[(u32, &str)] = &[(1, "Auto"), (2, "Off")];
#[rustfmt::skip]
const PC_66: &[(u32, &str)] = &[(1, "AF-S"), (2, "AF-C"), (3, "No Limit")];
#[rustfmt::skip]
const PC_67: &[(u32, &str)] = &[(1, "Extra High"), (2, "High"), (3, "Normal"), (4, "Low")];
#[rustfmt::skip]
const PC_68: &[(u32, &str)] = &[(1, "Single"), (2, "5 fps"), (3, "4 fps"), (4, "3 fps"), (5, "2 fps"), (6, "1 fps")];
#[rustfmt::skip]
const PC_69: &[(u32, &str)] = &[(1, "Release"), (2, "Release + Focus"), (3, "Focus + Release"), (4, "Focus")];
#[rustfmt::skip]
const PC_70: &[(u32, &str)] = &[(1, "Release"), (2, "Focus")];
#[rustfmt::skip]
const PC_71: &[(u32, &str)] = &[(1, "Release Mode"), (2, "Frame Count")];
#[rustfmt::skip]
const PC_72: &[(u32, &str)] = &[(1, "Frame Rate"), (2, "Exposure")];
#[rustfmt::skip]
const PC_73: &[(u32, &str)] = &[(1, "Off"), (2, "On")];
#[rustfmt::skip]
const PC_74: &[(u32, &str)] = &[(1, "Auto (Slowest)"), (2, "Auto (Slower)"), (3, "Auto"), (4, "Auto (Faster)"), (5, "Auto (Fastest)"), (6, "1/4000 s"), (7, "1/3200 s"), (8, "1/2500 s"), (9, "1/2000 s"), (10, "1/1600 s"), (11, "1/1250 s"), (12, "1/1000 s"), (13, "1/800 s"), (14, "1/640 s"), (15, "1/500 s"), (16, "1/400 s"), (17, "1/320 s"), (18, "1/250 s"), (19, "1/200 s"), (20, "1/160 s"), (21, "1/125 s"), (22, "1/100 s"), (23, "1/80 s"), (24, "1/60 s"), (25, "1/50 s"), (26, "1/40 s"), (27, "1/30 s"), (28, "1/25 s"), (29, "1/20 s"), (30, "1/15 s"), (31, "1/13 s"), (32, "1/10 s"), (33, "1/8 s"), (34, "1/6 s"), (35, "1/5 s"), (36, "1/4 s"), (37, "1/3 s"), (38, "1/2.5 s"), (39, "1/2 s"), (40, "1/1.6 s"), (41, "1/1.3 s"), (42, "1 s"), (43, "1.3 s"), (44, "1.6 s"), (45, "2 s"), (46, "2.5 s"), (47, "3 s"), (48, "4 s"), (49, "5 s"), (50, "6 s"), (51, "8 s"), (52, "10 s"), (53, "13 s"), (54, "15 s"), (55, "20 s"), (56, "25 s"), (57, "30 s")];
#[rustfmt::skip]
const PC_75: &[(u32, &str)] = &[(1, "Preset Focus Point"), (2, "AE Lock (hold)"), (3, "AE/WB Lock (hold)"), (4, "AE Lock (reset on release)"), (5, "FV Lock"), (6, "Preview"), (7, "+NEF(RAW)"), (8, "Grid Display"), (9, "Virtual Horizon"), (10, "Voice Memo"), (11, "Playback"), (12, "Filtered Playback"), (13, "Photo Shooting Bank"), (14, "Exposure Mode"), (15, "Exposure Comp"), (16, "AF Mode/AF Area Mode"), (17, "Image Area"), (18, "ISO"), (19, "Active-D Lighting"), (20, "Metering"), (21, "Exposure Delay Mode"), (22, "Shutter/Aperture Lock"), (23, "1 Stop Speed/Aperture"), (24, "Rating 0"), (25, "Rating 5"), (26, "Rating 4"), (27, "Rating 3"), (28, "Rating 2"), (29, "Rating 1"), (30, "Candidate For Deletion"), (31, "Non-CPU Lens"), (32, "None")];
#[rustfmt::skip]
const PC_76: &[(u32, &str)] = &[(1, "Voice Memo"), (2, "Select To Send"), (3, "Wired LAN"), (4, "My Menu"), (5, "My Menu Top Item"), (6, "Filtered Playback"), (7, "Rating 0"), (8, "Rating 5"), (9, "Rating 4"), (10, "Rating 3"), (11, "Rating 2"), (12, "Rating 1"), (13, "Candidate For Deletion"), (14, "None")];
#[rustfmt::skip]
const PC_77: &[(u32, &str)] = &[(1, "AF-AreaMode S"), (2, "AF-AreaMode D9"), (3, "AF-AreaMode D25"), (4, "AF-AreaMode D49"), (5, "AF-AreaMode D105"), (6, "AF-AreaMode 3D"), (7, "AF-AreaMode Group"), (8, "AF-AreaMode Group C1"), (9, "AF-AreaMode Group C2"), (10, "AF-AreaMode Auto Area"), (11, "AF-AreaMode + AF-On S"), (12, "AF-AreaMode + AF-On D9"), (13, "AF-AreaMode + AF-On D25"), (14, "AF-AreaMode + AF-On D49"), (15, "AF-AreaMode + AF-On D105"), (16, "AF-AreaMode + AF-On 3D"), (17, "AF-AreaMode + AF-On Group"), (18, "AF-AreaMode + AF-On Group C1"), (19, "AF-AreaMode + AF-On Group C2"), (20, "AF-AreaMode + AF-On Auto Area"), (21, "Same as AF-On"), (22, "AF-On"), (23, "AF Lock Only"), (24, "AE Lock (hold)"), (25, "AE/WB Lock (hold)"), (26, "AE Lock (reset on release)"), (27, "AE Lock Only"), (28, "AE/AF Lock"), (29, "Recall Shooting Functions"), (30, "None")];
#[rustfmt::skip]
const PC_78: &[(u32, &str)] = &[(1, "Photo Shooting Bank"), (2, "Image Area"), (3, "Active-D Lighting"), (4, "Metering"), (5, "Exposure Delay Mode"), (6, "Shutter/Aperture Lock"), (7, "1 Stop Speed/Aperture"), (8, "Non-CPU Lens"), (9, "None")];
#[rustfmt::skip]
const PC_79: &[(u32, &str)] = &[(1, "Rating"), (2, "Select To Send"), (3, "Protect"), (4, "Voice Memo"), (5, "None")];
#[rustfmt::skip]
const PC_80: &[(u32, &str)] = &[(1, "Rating 5"), (2, "Rating 4"), (3, "Rating 3"), (4, "Rating 2"), (5, "Rating 1"), (6, "Candidate for Deletion")];
#[rustfmt::skip]
const PC_81: &[(u32, &str)] = &[(1, "Record Movie"), (2, "My Menu"), (3, "My Menu Top Item"), (4, "None")];
#[rustfmt::skip]
const PC_82: &[(u32, &str)] = &[(1, "105 Points"), (2, "27 Points"), (3, "15 Points")];
#[rustfmt::skip]
const PC_83: &[(u32, &str)] = &[(1, "Use All"), (2, "Use Half")];
#[rustfmt::skip]
const PC_84: &[(u32, &str)] = &[(1, "Auto"), (2, "Mechanical"), (3, "Electronic")];
#[rustfmt::skip]
const PC_85: &[(u32, &str)] = &[(1, "Shutter/AF-On"), (2, "AF-On Only")];
#[rustfmt::skip]
const PC_86: &[(u32, &str)] = &[(1, "Wrap"), (2, "No Wrap")];
#[rustfmt::skip]
const PC_87: &[(u32, &str)] = &[(1, "CFexpress/XQD Card"), (2, "SD Card")];
#[rustfmt::skip]
const PC_88: &[(u32, &str)] = &[(1, "Not Reversed"), (2, "Reversed")];
#[rustfmt::skip]
const PC_89: &[(u32, &str)] = &[(1, "AE Lock (hold)"), (2, "AE Lock (reset on release)"), (3, "FV Lock"), (4, "Preview"), (5, "+NEF(RAW)"), (6, "Subject Tracking"), (7, "Silent Photography"), (8, "LiveView Info Display On/Off"), (9, "Playback"), (10, "Image Area"), (11, "Metering"), (12, "Flash Mode"), (13, "Focus Mode"), (14, "Exposure Delay Mode"), (15, "Shutter/Aperture Lock"), (16, "Exposure Compensation"), (17, "ISO Sensitivity"), (18, "None")];
#[rustfmt::skip]
const PC_90: &[(u32, &str)] = &[(1, "Same as AF-On Button"), (2, "Select Center Focus Point"), (3, "AF-On"), (4, "AF Lock Only"), (5, "AE Lock (hold)"), (6, "AE Lock (reset on release)"), (7, "AE Lock Only"), (8, "AE/AF Lock"), (9, "LiveView Info Display On/Off"), (10, "Zoom (Low)"), (11, "Zoom (1:1)"), (12, "Zoom (High)"), (13, "None")];
#[rustfmt::skip]
const PC_91: &[(u32, &str)] = &[(1, "LiveView Info Display On/Off"), (2, "Record Movie"), (3, "Exposure Compensation"), (4, "ISO"), (5, "None")];
#[rustfmt::skip]
const PC_92: &[(u32, &str)] = &[(1, "Same as AF-On"), (2, "Center Focus Point"), (3, "AF-On"), (4, "AF Lock Only"), (5, "AE Lock (hold)"), (6, "AE Lock Only"), (7, "AE/AF Lock"), (8, "LiveView Info Display On/Off"), (9, "Zoom (Low)"), (10, "Zoom (1:1)"), (11, "Zoom (High)"), (12, "Record Movie"), (13, "None")];
#[rustfmt::skip]
const PC_93: &[(u32, &str)] = &[(2, "Single-point"), (3, "Dynamic-area"), (4, "Wide (S)"), (5, "Wide (L)"), (6, "Wide (L-people)"), (7, "Wide (L-animals)"), (8, "Auto"), (9, "Auto (People)"), (10, "Auto (Animals)")];
#[rustfmt::skip]
const PC_94: &[(u32, &str)] = &[(1, "Single-point"), (2, "Wide (S)"), (3, "Wide (L)"), (4, "Wide (L-people)"), (5, "Wide (L-animals)"), (6, "Auto"), (7, "Auto (People)"), (8, "Auto (Animals)")];
#[rustfmt::skip]
const PC_95: &[(u32, &str)] = &[(1, "Off"), (2, "Shutter Speed"), (3, "ISO")];
#[rustfmt::skip]
const PC_96: &[(u32, &str)] = &[(1, "On"), (2, "On During Focus Point Selection Only")];
#[rustfmt::skip]
const PC_97: &[(u32, &str)] = &[(1, "Normal"), (2, "High"), (3, "Very High")];
#[rustfmt::skip]
const PC_98: &[(u32, &str)] = &[(1, "On (auto reset)"), (2, "On"), (3, "Off")];
#[rustfmt::skip]
const PC_99: &[(u32, &str)] = &[(1, "Face Detection On"), (2, "Face Detection Off")];
#[rustfmt::skip]
const PC_100: &[(u32, &str)] = &[(1, "8 mm"), (2, "12 mm"), (3, "15 mm"), (4, "20 mm"), (5, "Average")];
#[rustfmt::skip]
const PC_101: &[(u32, &str)] = &[(1, "12 mm"), (2, "Average")];
#[rustfmt::skip]
const PC_102: &[(u32, &str)] = &[(1, "On (Half Press)"), (2, "On (Burst Mode)"), (3, "Off")];
#[rustfmt::skip]
const PC_103: &[(u32, &str)] = &[(1, "4 s"), (2, "6 s"), (3, "10 s"), (4, "30 s"), (5, "1 min"), (6, "5 min"), (7, "10 min"), (8, "30 min"), (9, "No Limit")];
#[rustfmt::skip]
const PC_104: &[(u32, &str)] = &[(1, "10 s"), (2, "20 s"), (3, "30 s"), (4, "1 min"), (5, "5 min"), (6, "10 min"), (7, "30 min"), (8, "No Limit")];
#[rustfmt::skip]
const PC_105: &[(u32, &str)] = &[(1, "2 s"), (2, "5 s"), (3, "10 s"), (4, "20 s")];
#[rustfmt::skip]
const PC_106: &[(u32, &str)] = &[(1, "0.5 s"), (2, "1 s"), (3, "2 s"), (4, "3 s")];
#[rustfmt::skip]
const PC_107: &[(u32, &str)] = &[(1, "4 s"), (2, "10 s"), (3, "20 s"), (4, "1 min"), (5, "5 min"), (6, "10 min")];
#[rustfmt::skip]
const PC_108: &[(u32, &str)] = &[(1, "2 s"), (2, "4 s"), (3, "10 s"), (4, "20 s"), (5, "1 min"), (6, "5 min"), (7, "10 min")];
#[rustfmt::skip]
const PC_109: &[(u32, &str)] = &[(1, "5 min"), (2, "10 min"), (3, "15 min"), (4, "20 min"), (5, "30 min"), (6, "No Limit")];
#[rustfmt::skip]
const PC_110: &[(u32, &str)] = &[(1, "3 s"), (2, "2 s"), (3, "1 s"), (4, "0.5 s"), (5, "0.2 s"), (6, "Off")];
#[rustfmt::skip]
const PC_111: &[(u32, &str)] = &[(1, "1/250 s (auto FP)"), (2, "1/250 s"), (3, "1/200 s"), (4, "1/160 s"), (5, "1/125 s"), (6, "1/100 s"), (7, "1/80 s"), (8, "1/60 s")];
#[rustfmt::skip]
const PC_112: &[(u32, &str)] = &[(1, "1/200 s (auto FP)"), (2, "1/200 s"), (3, "1/160 s"), (4, "1/125 s"), (5, "1/100 s"), (6, "1/80 s"), (7, "1/60 s")];
#[rustfmt::skip]
const PC_113: &[(u32, &str)] = &[(1, "1/60 s"), (2, "1/30 s"), (3, "1/15 s"), (4, "1/8 s"), (5, "1/4 s"), (6, "1/2 s"), (7, "1 s"), (8, "2 s")];
#[rustfmt::skip]
const PC_114: &[(u32, &str)] = &[(1, "Entire Frame"), (2, "Background Only")];
#[rustfmt::skip]
const PC_115: &[(u32, &str)] = &[(1, "Subject and Background"), (2, "Subject Only")];
#[rustfmt::skip]
const PC_116: &[(u32, &str)] = &[(1, "Auto Bracketing"), (2, "Multiple Exposure"), (3, "HDR (high dynamic range)"), (4, "None")];
#[rustfmt::skip]
const PC_117: &[(u32, &str)] = &[(1, "Voice Memo"), (2, "Photo Shooting Bank"), (3, "Exposure Mode"), (4, "AF Mode/AF Area Mode"), (5, "Image Area"), (6, "Shutter/Aperture Lock"), (7, "None")];
#[rustfmt::skip]
const PC_118: &[(u32, &str)] = &[(1, "AE Lock (hold)"), (2, "AE Lock (reset on release)"), (3, "Preview"), (4, "+NEF(RAW)"), (5, "LiveView Info Display On/Off"), (6, "Grid Display"), (7, "Zoom (Low)"), (8, "Zoom (1:1)"), (9, "Zoom (High)"), (10, "My Menu"), (11, "My Menu Top Item"), (12, "Image Area"), (13, "Image Quality"), (14, "White Balance"), (15, "Picture Control"), (16, "Active-D Lighting"), (17, "Metering"), (18, "Flash Mode"), (19, "Focus Mode"), (20, "Auto Bracketing"), (21, "Multiple Exposure"), (22, "HDR"), (23, "Exposure Delay Mode"), (24, "Shutter/Aperture Lock"), (25, "Non-CPU Lens"), (26, "None")];
#[rustfmt::skip]
const PC_119: &[(u32, &str)] = &[(1, "Select Center Focus Point"), (2, "Preset Focus Point - Press To Recall"), (3, "Preset Focus Point - Hold To Recall"), (4, "None")];
#[rustfmt::skip]
const PC_120: &[(u32, &str)] = &[(1, "Select Center Focus Point"), (2, "Zoom (Low)"), (3, "Zoom (1:1)"), (4, "Zoom (High)"), (5, "None")];
#[rustfmt::skip]
const PC_121: &[(u32, &str)] = &[(1, "Filtered Playback"), (2, "View Histograms"), (3, "Zoom (Low)"), (4, "Zoom (1:1)"), (5, "Zoom (High)"), (6, "Choose Folder")];
#[rustfmt::skip]
const PC_122: &[(u32, &str)] = &[(1, "Thumbnail On/Off"), (2, "View Histograms"), (3, "Zoom (Low)"), (4, "Zoom (1:1)"), (5, "Zoom (High)"), (6, "Choose Folder")];
#[rustfmt::skip]
const PC_123: &[(u32, &str)] = &[(1, "Autofocus On, Exposure On"), (2, "Autofocus Off, Exposure On")];
#[rustfmt::skip]
const PC_124: &[(u32, &str)] = &[(1, "Autofocus On, Exposure On (Mode A)"), (2, "Autofocus Off, Exposure On (Mode A)")];
#[rustfmt::skip]
const PC_125: &[(u32, &str)] = &[(1, "Autofocus On, Exposure Off"), (2, "Autofocus Off, Exposure Off")];
#[rustfmt::skip]
const PC_126: &[(u32, &str)] = &[(1, "On"), (2, "On (Image Review Excluded)"), (3, "Off")];
#[rustfmt::skip]
const PC_127: &[(u32, &str)] = &[(1, "10 Frames"), (2, "50 Frames"), (3, "Rating"), (4, "Protect"), (5, "Stills Only"), (6, "Movies Only"), (7, "Folder")];
#[rustfmt::skip]
const PC_128: &[(u32, &str)] = &[(1, "+ 0 -"), (2, "- 0 +")];
#[rustfmt::skip]
const PC_129: &[(u32, &str)] = &[(1, "Take Photo"), (2, "Record Movie")];
#[rustfmt::skip]
const PC_130: &[(u32, &str)] = &[(5, "English"), (6, "Spanish"), (8, "French"), (15, "Portuguese (Br)")];

/// Directory order is irrelevant here; lookup is by tag id, and the first
/// row whose `Cond` holds wins, exactly as ExifTool picks a Condition variant.
#[rustfmt::skip]
pub(super) const SETTINGS_TAGS: &[E] = &[
    E { id: 0x0001, name: "ISOAutoHiLimit", cond: Cond::ModelD6, mask: 0x0, conv: Conv::Map(PC_0), dm: Dm::None },
    E { id: 0x0001, name: "ISOAutoHiLimit", cond: Cond::ModelZ7, mask: 0x0, conv: Conv::Map(PC_1), dm: Dm::None },
    E { id: 0x006c, name: "ShootingInfoDisplay", cond: Cond::ModelD6, mask: 0x0, conv: Conv::Map(PC_2), dm: Dm::None },
    E { id: 0x006c, name: "ShootingInfoDisplay", cond: Cond::ModelZSeries, mask: 0x0, conv: Conv::Map(PC_3), dm: Dm::None },
    E { id: 0x000b, name: "FlickerReductionShooting", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_4), dm: Dm::None },
    E { id: 0x0074, name: "FlickAdvanceDirection", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_5), dm: Dm::None },
    E { id: 0x0075, name: "HDMIOutputResolution", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_6), dm: Dm::None },
    E { id: 0x0077, name: "HDMIOutputRange", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_7), dm: Dm::None },
    E { id: 0x000c, name: "FlickerReductionIndicator", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_4), dm: Dm::None },
    E { id: 0x0080, name: "RemoteFuncButton", cond: Cond::ModelD6, mask: 0x0, conv: Conv::Map(PC_8), dm: Dm::None },
    E { id: 0x0080, name: "RemoteFuncButton", cond: Cond::ModelZSeries, mask: 0x0, conv: Conv::Map(PC_9), dm: Dm::None },
    E { id: 0x000d, name: "MovieISOAutoHiLimit", cond: Cond::ModelD6, mask: 0x0, conv: Conv::Map(PC_0), dm: Dm::None },
    E { id: 0x000d, name: "MovieISOAutoHiLimit", cond: Cond::ModelZ7, mask: 0x0, conv: Conv::Map(PC_10), dm: Dm::None },
    E { id: 0x008b, name: "CmdDialsReverseRotation", cond: Cond::CmdDialsReverseRotExposureCompIs1, mask: 0x0, conv: Conv::Map(PC_11), dm: Dm::None },
    E { id: 0x008b, name: "CmdDialsReverseRotation", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_12), dm: Dm::None },
    E { id: 0x000e, name: "MovieISOAutoControlManualMode", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_13), dm: Dm::None },
    E { id: 0x008d, name: "FocusPeakingHighlightColor", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_14), dm: Dm::None },
    E { id: 0x008e, name: "ContinuousModeDisplay", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_13), dm: Dm::None },
    E { id: 0x008f, name: "ShutterSpeedLock", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_13), dm: Dm::None },
    E { id: 0x0090, name: "ApertureLock", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_13), dm: Dm::None },
    E { id: 0x0091, name: "MovieHighlightDisplayThreshold", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_15), dm: Dm::None },
    E { id: 0x0092, name: "HDMIExternalRecorder", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_13), dm: Dm::None },
    E { id: 0x0093, name: "BlockShotAFResponse", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_16), dm: Dm::None },
    E { id: 0x0094, name: "SubjectMotion", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_17), dm: Dm::None },
    E { id: 0x0095, name: "Three-DTrackingFaceDetection", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_13), dm: Dm::None },
    E { id: 0x000f, name: "MovieWhiteBalanceSameAsPhoto", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_18), dm: Dm::None },
    E { id: 0x0097, name: "StoreByOrientation", cond: Cond::ModelD6, mask: 0x0, conv: Conv::Map(PC_19), dm: Dm::None },
    E { id: 0x0097, name: "StoreByOrientation", cond: Cond::ModelZSeries, mask: 0x0, conv: Conv::Map(PC_20), dm: Dm::None },
    E { id: 0x0099, name: "DynamicAreaAFAssist", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_13), dm: Dm::None },
    E { id: 0x009a, name: "ExposureCompStepSize", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_21), dm: Dm::None },
    E { id: 0x009b, name: "SyncReleaseMode", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_22), dm: Dm::None },
    E { id: 0x009c, name: "ModelingFlash", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_13), dm: Dm::None },
    E { id: 0x009d, name: "AutoBracketModeM", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_23), dm: Dm::None },
    E { id: 0x009e, name: "PreviewButton", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_24), dm: Dm::None },
    E { id: 0x00a0, name: "Func1Button", cond: Cond::ModelD6, mask: 0x0, conv: Conv::Map(PC_24), dm: Dm::None },
    E { id: 0x00a0, name: "Func1Button", cond: Cond::ModelZ6Or7, mask: 0x0, conv: Conv::Map(PC_25), dm: Dm::None },
    E { id: 0x00a0, name: "Func1Button", cond: Cond::ModelZSeries, mask: 0x0, conv: Conv::Map(PC_26), dm: Dm::None },
    E { id: 0x00a2, name: "Func2Button", cond: Cond::ModelD6, mask: 0x0, conv: Conv::Map(PC_24), dm: Dm::None },
    E { id: 0x00a2, name: "Func2Button", cond: Cond::ModelZ6Or7, mask: 0x0, conv: Conv::Map(PC_25), dm: Dm::None },
    E { id: 0x00a2, name: "Func2Button", cond: Cond::ModelZSeries, mask: 0x0, conv: Conv::Map(PC_26), dm: Dm::None },
    E { id: 0x00a3, name: "AF-OnButton", cond: Cond::ModelD6, mask: 0x0, conv: Conv::Map(PC_27), dm: Dm::None },
    E { id: 0x00a3, name: "AF-OnButton", cond: Cond::ModelZSeries, mask: 0x0, conv: Conv::Map(PC_28), dm: Dm::None },
    E { id: 0x00a4, name: "SubSelector", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_29), dm: Dm::None },
    E { id: 0x00a5, name: "SubSelectorCenter", cond: Cond::ModelD6, mask: 0x0, conv: Conv::Map(PC_30), dm: Dm::None },
    E { id: 0x00a5, name: "SubSelectorCenter", cond: Cond::ModelZSeries, mask: 0x0, conv: Conv::Map(PC_31), dm: Dm::None },
    E { id: 0x00a7, name: "LensFunc1Button", cond: Cond::ModelD6, mask: 0x0, conv: Conv::Map(PC_32), dm: Dm::None },
    E { id: 0x00a7, name: "LensFunc1Button", cond: Cond::ModelZSeries, mask: 0x0, conv: Conv::Map(PC_33), dm: Dm::None },
    E { id: 0x00a8, name: "CmdDialsApertureSetting", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_34), dm: Dm::None },
    E { id: 0x00a9, name: "MultiSelector", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_35), dm: Dm::None },
    E { id: 0x00aa, name: "LiveViewButtonOptions", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_36), dm: Dm::None },
    E { id: 0x00ab, name: "LightSwitch", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_37), dm: Dm::None },
    E { id: 0x00b1, name: "MoviePreviewButton", cond: Cond::ModelD6, mask: 0x0, conv: Conv::Map(PC_38), dm: Dm::None },
    E { id: 0x00b1, name: "MovieFunc1Button", cond: Cond::ModelZSeries, mask: 0x0, conv: Conv::Map(PC_39), dm: Dm::None },
    E { id: 0x00b3, name: "MovieFunc1Button", cond: Cond::ModelD6, mask: 0x0, conv: Conv::Map(PC_40), dm: Dm::None },
    E { id: 0x00b3, name: "MovieFunc2Button", cond: Cond::ModelZSeries, mask: 0x0, conv: Conv::Map(PC_41), dm: Dm::None },
    E { id: 0x00b5, name: "MovieFunc2Button", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_42), dm: Dm::None },
    E { id: 0x00b6, name: "AssignMovieSubselector", cond: Cond::ModelD6, mask: 0x0, conv: Conv::Map(PC_43), dm: Dm::None },
    E { id: 0x00b6, name: "AssignMovieSubselector", cond: Cond::ModelZSeries, mask: 0x0, conv: Conv::Map(PC_44), dm: Dm::None },
    E { id: 0x0002, name: "ISOAutoFlashLimit", cond: Cond::ModelD6, mask: 0x0, conv: Conv::Map(PC_45), dm: Dm::None },
    E { id: 0x0002, name: "ISOAutoFlashLimit", cond: Cond::ModelZ7, mask: 0x0, conv: Conv::Map(PC_46), dm: Dm::None },
    E { id: 0x00d4, name: "PhotoShootingMenuBank", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_47), dm: Dm::None },
    E { id: 0x00d5, name: "CustomSettingsBank", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_47), dm: Dm::None },
    E { id: 0x00da, name: "LowLightAF", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_13), dm: Dm::None },
    E { id: 0x00df, name: "ApplySettingsToLiveView", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_13), dm: Dm::None },
    E { id: 0x00e0, name: "FocusPeakingLevel", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_48), dm: Dm::None },
    E { id: 0x00ea, name: "LensControlRing", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_49), dm: Dm::None },
    E { id: 0x00ed, name: "MovieMultiSelector", cond: Cond::ModelD6, mask: 0x0, conv: Conv::Map(PC_50), dm: Dm::None },
    E { id: 0x00ed, name: "MovieMultiSelector", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_50), dm: Dm::None },
    E { id: 0x00ee, name: "MovieAFSpeed", cond: Cond::Always, mask: 0x0, conv: Conv::Offset(-6), dm: Dm::None },
    E { id: 0x00ef, name: "MovieAFSpeedApply", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_51), dm: Dm::None },
    E { id: 0x00f0, name: "MovieAFTrackingSensitivity", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_52), dm: Dm::None },
    E { id: 0x00f1, name: "MovieHighlightDisplayPattern", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_53), dm: Dm::None },
    E { id: 0x00f9, name: "MovieAF-OnButton", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_54), dm: Dm::None },
    E { id: 0x00fb, name: "SecondarySlotFunction", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_55), dm: Dm::None },
    E { id: 0x00fc, name: "SilentPhotography", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_13), dm: Dm::None },
    E { id: 0x00fd, name: "ExtendedShutterSpeeds", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_13), dm: Dm::None },
    E { id: 0x0102, name: "HDMIBitDepth", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_56), dm: Dm::HdmiBitDepth },
    E { id: 0x0103, name: "HDMIOutputHDR", cond: Cond::HdmiBitDepthIs2, mask: 0x0, conv: Conv::Map(PC_57), dm: Dm::HdmiOutputHdr },
    E { id: 0x0104, name: "HDMIViewAssist", cond: Cond::HdmiBitDepthIs2, mask: 0x0, conv: Conv::Map(PC_13), dm: Dm::None },
    E { id: 0x0109, name: "BracketSet", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_58), dm: Dm::BracketSet },
    E { id: 0x010a, name: "BracketProgram", cond: Cond::BracketSetLt4, mask: 0x0, conv: Conv::Map(PC_59), dm: Dm::BracketProgram },
    E { id: 0x010a, name: "BracketProgram", cond: Cond::BracketSetIs(4), mask: 0x0, conv: Conv::Map(PC_60), dm: Dm::BracketProgram },
    E { id: 0x010a, name: "BracketProgram", cond: Cond::BracketSetIs(5), mask: 0xf, conv: Conv::Map(PC_61), dm: Dm::BracketProgram },
    E { id: 0x010b, name: "BracketIncrement", cond: Cond::BracketSetLt4AndProgramNe(19), mask: 0x0, conv: Conv::Map(PC_62), dm: Dm::None },
    E { id: 0x010b, name: "BracketIncrement", cond: Cond::BracketSetEqAndProgramNe(4, 5), mask: 0x0, conv: Conv::Offset(-6), dm: Dm::None },
    E { id: 0x010c, name: "BracketIncrement", cond: Cond::BracketSetEqAndProgramNe(5, 10), mask: 0x0, conv: Conv::Map(PC_63), dm: Dm::None },
    E { id: 0x010e, name: "MonitorBrightness", cond: Cond::Always, mask: 0x0, conv: Conv::Offset(-6), dm: Dm::None },
    E { id: 0x0116, name: "GroupAreaC1", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_64), dm: Dm::None },
    E { id: 0x0117, name: "AutoAreaAFStartingPoint", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_4), dm: Dm::None },
    E { id: 0x0118, name: "FocusPointPersistence", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_65), dm: Dm::None },
    E { id: 0x011d, name: "AutoFocusModeRestrictions", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_66), dm: Dm::None },
    E { id: 0x011e, name: "FocusPointBrightness", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_67), dm: Dm::None },
    E { id: 0x011f, name: "CHModeShootingSpeed", cond: Cond::Always, mask: 0x0, conv: Conv::Fps(15), dm: Dm::None },
    E { id: 0x0120, name: "CLModeShootingSpeed", cond: Cond::Always, mask: 0x0, conv: Conv::Fps(11), dm: Dm::None },
    E { id: 0x0121, name: "QuietShutterShootingSpeed", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_68), dm: Dm::None },
    E { id: 0x001d, name: "AF-CPrioritySel", cond: Cond::ModelD6, mask: 0x0, conv: Conv::Map(PC_69), dm: Dm::None },
    E { id: 0x001d, name: "AF-CPrioritySel", cond: Cond::ModelZSeries, mask: 0x0, conv: Conv::Map(PC_70), dm: Dm::None },
    E { id: 0x0128, name: "RearControPanelDisplay", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_71), dm: Dm::None },
    E { id: 0x0129, name: "FlashBurstPriority", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_72), dm: Dm::None },
    E { id: 0x012a, name: "RecallShootFuncExposureMode", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_73), dm: Dm::None },
    E { id: 0x012b, name: "RecallShootFuncShutterSpeed", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_73), dm: Dm::None },
    E { id: 0x0003, name: "ISOAutoShutterTime", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_74), dm: Dm::None },
    E { id: 0x001e, name: "AF-SPrioritySel", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_70), dm: Dm::None },
    E { id: 0x012c, name: "RecallShootFuncAperture", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_73), dm: Dm::None },
    E { id: 0x012d, name: "RecallShootFuncExposureComp", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_73), dm: Dm::None },
    E { id: 0x012e, name: "RecallShootFuncISO", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_73), dm: Dm::None },
    E { id: 0x012f, name: "RecallShootFuncMeteringMode", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_73), dm: Dm::None },
    E { id: 0x0130, name: "RecallShootFuncWhiteBalance", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_73), dm: Dm::None },
    E { id: 0x0131, name: "RecallShootFuncAFAreaMode", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_73), dm: Dm::None },
    E { id: 0x0132, name: "RecallShootFuncFocusTracking", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_73), dm: Dm::None },
    E { id: 0x0133, name: "RecallShootFuncAF-On", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_73), dm: Dm::None },
    E { id: 0x0134, name: "VerticalFuncButton", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_75), dm: Dm::None },
    E { id: 0x0135, name: "Func3Button", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_76), dm: Dm::None },
    E { id: 0x0136, name: "VerticalAF-OnButton", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_77), dm: Dm::None },
    E { id: 0x0137, name: "VerticalMultiSelector", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_29), dm: Dm::None },
    E { id: 0x0138, name: "MeteringButton", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_78), dm: Dm::None },
    E { id: 0x0139, name: "PlaybackFlickUp", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_79), dm: Dm::PlaybackFlickUp },
    E { id: 0x013a, name: "PlaybackFlickUpRating", cond: Cond::PlaybackFlickUpIs1, mask: 0x0, conv: Conv::Map(PC_80), dm: Dm::None },
    E { id: 0x013b, name: "PlaybackFlickDown", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_79), dm: Dm::PlaybackFlickDown },
    E { id: 0x013c, name: "PlaybackFlickDownRating", cond: Cond::PlaybackFlickDownIs1, mask: 0x0, conv: Conv::Map(PC_80), dm: Dm::None },
    E { id: 0x013d, name: "MovieFunc3Button", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_81), dm: Dm::None },
    E { id: 0x0020, name: "AFPointSel", cond: Cond::ModelD6, mask: 0x0, conv: Conv::Map(PC_82), dm: Dm::None },
    E { id: 0x0020, name: "AFPointSel", cond: Cond::ModelZSeries, mask: 0x0, conv: Conv::Map(PC_83), dm: Dm::None },
    E { id: 0x0150, name: "ShutterType", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_84), dm: Dm::None },
    E { id: 0x0151, name: "LensFunc2Button", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_33), dm: Dm::None },
    E { id: 0x0022, name: "AFActivation", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_85), dm: Dm::None },
    E { id: 0x0158, name: "USBPowerDelivery", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_4), dm: Dm::None },
    E { id: 0x0159, name: "EnergySavingMode", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_13), dm: Dm::None },
    E { id: 0x015c, name: "BracketingBurstOptions", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_4), dm: Dm::None },
    E { id: 0x0023, name: "FocusPointWrap", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_86), dm: Dm::None },
    E { id: 0x015e, name: "PrimarySlot", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_87), dm: Dm::None },
    E { id: 0x015f, name: "ReverseFocusRing", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_88), dm: Dm::None },
    E { id: 0x0160, name: "VerticalFuncButton", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_89), dm: Dm::None },
    E { id: 0x0161, name: "VerticalAFOnButton", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_90), dm: Dm::None },
    E { id: 0x0162, name: "VerticalMultiSelector", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_29), dm: Dm::None },
    E { id: 0x0164, name: "VerticalMovieFuncButton", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_91), dm: Dm::None },
    E { id: 0x0165, name: "VerticalMovieAFOnButton", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_92), dm: Dm::None },
    E { id: 0x016d, name: "SaveFocus", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_13), dm: Dm::None },
    E { id: 0x016e, name: "AFAreaMode", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_93), dm: Dm::None },
    E { id: 0x016f, name: "MovieAFAreaMode", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_94), dm: Dm::None },
    E { id: 0x0170, name: "PreferSubSelectorCenter", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_73), dm: Dm::None },
    E { id: 0x0171, name: "KeepExposureWithTeleconverter", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_95), dm: Dm::None },
    E { id: 0x0025, name: "ManualFocusPointIllumination", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_96), dm: Dm::None },
    E { id: 0x0174, name: "FocusPointSelectionSpeed", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_97), dm: Dm::None },
    E { id: 0x0026, name: "AF-AssistIlluminator", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_13), dm: Dm::None },
    E { id: 0x0027, name: "ManualFocusRingInAFMode", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_13), dm: Dm::None },
    E { id: 0x0029, name: "ISOStepSize", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_21), dm: Dm::None },
    E { id: 0x002a, name: "ExposureControlStepSize", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_21), dm: Dm::None },
    E { id: 0x002b, name: "EasyExposureCompensation", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_98), dm: Dm::None },
    E { id: 0x002c, name: "MatrixMetering", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_99), dm: Dm::None },
    E { id: 0x002d, name: "CenterWeightedAreaSize", cond: Cond::ModelD6, mask: 0x0, conv: Conv::Map(PC_100), dm: Dm::None },
    E { id: 0x002d, name: "CenterWeightedAreaSize", cond: Cond::ModelZSeries, mask: 0x0, conv: Conv::Map(PC_101), dm: Dm::None },
    E { id: 0x002f, name: "FineTuneOptMatrixMetering", cond: Cond::Always, mask: 0x0, conv: Conv::FineTune, dm: Dm::None },
    E { id: 0x0030, name: "FineTuneOptCenterWeighted", cond: Cond::Always, mask: 0x0, conv: Conv::FineTune, dm: Dm::None },
    E { id: 0x0031, name: "FineTuneOptSpotMetering", cond: Cond::Always, mask: 0x0, conv: Conv::FineTune, dm: Dm::None },
    E { id: 0x0032, name: "FineTuneOptHighlightWeighted", cond: Cond::Always, mask: 0x0, conv: Conv::FineTune, dm: Dm::None },
    E { id: 0x0033, name: "ShutterReleaseButtonAE-L", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_102), dm: Dm::None },
    E { id: 0x0034, name: "StandbyMonitorOffTime", cond: Cond::ModelD6, mask: 0x0, conv: Conv::Map(PC_103), dm: Dm::None },
    E { id: 0x0034, name: "StandbyMonitorOffTime", cond: Cond::ModelZSeries, mask: 0x0, conv: Conv::Map(PC_104), dm: Dm::None },
    E { id: 0x0035, name: "SelfTimerTime", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_105), dm: Dm::None },
    E { id: 0x0036, name: "SelfTimerShotCount", cond: Cond::Always, mask: 0x0, conv: Conv::Negate(10), dm: Dm::None },
    E { id: 0x0037, name: "SelfTimerShotInterval", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_106), dm: Dm::None },
    E { id: 0x0038, name: "PlaybackMonitorOffTime", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_107), dm: Dm::None },
    E { id: 0x0039, name: "MenuMonitorOffTime", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_107), dm: Dm::None },
    E { id: 0x003a, name: "ShootingInfoMonitorOffTime", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_107), dm: Dm::None },
    E { id: 0x003b, name: "ImageReviewMonitorOffTime", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_108), dm: Dm::None },
    E { id: 0x003c, name: "LiveViewMonitorOffTime", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_109), dm: Dm::None },
    E { id: 0x003e, name: "CLModeShootingSpeed", cond: Cond::Always, mask: 0x0, conv: Conv::Fps(6), dm: Dm::None },
    E { id: 0x003f, name: "MaxContinuousRelease", cond: Cond::Always, mask: 0x0, conv: Conv::Raw, dm: Dm::None },
    E { id: 0x0040, name: "ExposureDelayMode", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_110), dm: Dm::None },
    E { id: 0x0041, name: "ElectronicFront-CurtainShutter", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_13), dm: Dm::None },
    E { id: 0x0042, name: "FileNumberSequence", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_13), dm: Dm::None },
    E { id: 0x0043, name: "FramingGridDisplay", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_13), dm: Dm::None },
    E { id: 0x0045, name: "LCDIllumination", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_13), dm: Dm::None },
    E { id: 0x0046, name: "OpticalVR", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_13), dm: Dm::None },
    E { id: 0x0047, name: "FlashSyncSpeed", cond: Cond::ModelD6, mask: 0x0, conv: Conv::Map(PC_111), dm: Dm::None },
    E { id: 0x0047, name: "FlashSyncSpeed", cond: Cond::ModelZSeries, mask: 0x0, conv: Conv::Map(PC_112), dm: Dm::None },
    E { id: 0x0048, name: "FlashShutterSpeed", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_113), dm: Dm::None },
    E { id: 0x0049, name: "FlashExposureCompArea", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_114), dm: Dm::None },
    E { id: 0x004a, name: "AutoFlashISOSensitivity", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_115), dm: Dm::None },
    E { id: 0x0051, name: "AssignBktButton", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_116), dm: Dm::None },
    E { id: 0x0052, name: "AssignMovieRecordButton", cond: Cond::ModelD6, mask: 0x0, conv: Conv::Map(PC_117), dm: Dm::None },
    E { id: 0x0052, name: "AssignMovieRecordButton", cond: Cond::ModelZSeries, mask: 0x0, conv: Conv::Map(PC_118), dm: Dm::None },
    E { id: 0x0053, name: "MultiSelectorShootMode", cond: Cond::ModelD6, mask: 0x0, conv: Conv::Map(PC_119), dm: Dm::None },
    E { id: 0x0053, name: "MultiSelectorShootMode", cond: Cond::ModelZSeries, mask: 0x0, conv: Conv::Map(PC_120), dm: Dm::None },
    E { id: 0x0054, name: "MultiSelectorPlaybackMode", cond: Cond::ModelD6, mask: 0x0, conv: Conv::Map(PC_121), dm: Dm::None },
    E { id: 0x0054, name: "MultiSelectorPlaybackMode", cond: Cond::ModelZSeries, mask: 0x0, conv: Conv::Map(PC_122), dm: Dm::None },
    E { id: 0x0056, name: "MultiSelectorLiveView", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_120), dm: Dm::None },
    E { id: 0x005a, name: "CmdDialsChangeMainSub", cond: Cond::CmdDialsChangeMainSubExposureIs(1), mask: 0x0, conv: Conv::Map(PC_123), dm: Dm::None },
    E { id: 0x005a, name: "CmdDialsChangeMainSub", cond: Cond::CmdDialsChangeMainSubExposureIs(2), mask: 0x0, conv: Conv::Map(PC_124), dm: Dm::None },
    E { id: 0x005a, name: "CmdDialsChangeMainSub", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_125), dm: Dm::None },
    E { id: 0x005b, name: "CmdDialsMenuAndPlayback", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_126), dm: Dm::None },
    E { id: 0x005c, name: "SubDialFrameAdvance", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_127), dm: Dm::None },
    E { id: 0x005d, name: "ReleaseButtonToUseDial", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_18), dm: Dm::None },
    E { id: 0x005e, name: "ReverseIndicators", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_128), dm: Dm::None },
    E { id: 0x0062, name: "MovieShutterButton", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_129), dm: Dm::None },
    E { id: 0x0063, name: "Language", cond: Cond::Always, mask: 0x0, conv: Conv::Map(PC_130), dm: Dm::None },
];
