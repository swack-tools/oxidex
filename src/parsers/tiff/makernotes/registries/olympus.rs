//! Olympus tag registry with array schemas
//!
//! Supports both Four Thirds (E-series DSLRs) and Micro Four Thirds (OM-D, PEN) cameras.
//! Based on ExifTool's Olympus.pm module.

use crate::io::EndianReader;

use super::super::shared::{
    array_schemas::*, generic_decoders::ON_OFF_I32, tag_registry::TagRegistry,
};

// Re-export existing decoders from olympus.rs (need to make these public)
use super::super::olympus::{
    ART_FILTER_DECODER, COLOR_SPACE_DECODER, EXPOSURE_MODE_DECODER, FLASH_MODE_DECODER,
    FOCUS_MODE_DECODER, METERING_MODE_DECODER, NOISE_REDUCTION_DECODER, PICTURE_MODE_DECODER,
    SCENE_MODE_DECODER, WHITE_BALANCE_DECODER,
};

// ============================================================================
// ARRAY SCHEMAS
// ============================================================================

/// CameraSettings array schema (Tag 0x0003)
/// Contains 49+ camera configuration and image processing settings
static CAMERA_SETTINGS_SCHEMA: ArraySchema = ArraySchema {
    name: "CameraSettings",
    indices: &[
        ArrayIndexDef::raw(0, "PreviewImageValid"),
        ArrayIndexDef::raw(1, "PreviewImageStart"),
        ArrayIndexDef::raw(2, "PreviewImageLength"),
        ArrayIndexDef::with_i32_decoder(3, "ExposureMode", &EXPOSURE_MODE_DECODER),
        ArrayIndexDef::raw(4, "AELock"),
        ArrayIndexDef::with_i32_decoder(5, "MeteringMode", &METERING_MODE_DECODER),
        ArrayIndexDef::raw(6, "MacroMode"),
        ArrayIndexDef::with_i32_decoder(7, "FocusMode", &FOCUS_MODE_DECODER),
        ArrayIndexDef::raw(8, "FocusProcess"),
        ArrayIndexDef::raw(9, "AFSearch"),
        ArrayIndexDef::raw(10, "AFAreas"),
        ArrayIndexDef::raw(11, "AFPointSelected"),
        ArrayIndexDef::raw(12, "ExposureCompensation"),
        ArrayIndexDef::raw(13, "CenterWeightedArea"),
        ArrayIndexDef::raw(14, "AEBracketStep"),
        ArrayIndexDef::raw(15, "AEBracketXval"),
        ArrayIndexDef::with_i32_decoder(16, "FlashMode", &FLASH_MODE_DECODER),
        ArrayIndexDef::raw(17, "FlashExposureComp"),
        ArrayIndexDef::raw(18, "FlashRemoteControl"),
        ArrayIndexDef::raw(19, "FlashControlMode"),
        ArrayIndexDef::raw(20, "FlashIntensity"),
        ArrayIndexDef::with_i32_decoder(21, "WhiteBalance", &WHITE_BALANCE_DECODER),
        ArrayIndexDef::raw(22, "WhiteBalanceTemperature"),
        ArrayIndexDef::raw(23, "WhiteBalanceBracket"),
        ArrayIndexDef::raw(24, "CustomSaturation"),
        ArrayIndexDef::raw(25, "ModifiedSaturation"),
        ArrayIndexDef::raw(26, "ContrastSetting"),
        ArrayIndexDef::raw(27, "SharpnessSetting"),
        ArrayIndexDef::with_i32_decoder(28, "ColorSpace", &COLOR_SPACE_DECODER),
        ArrayIndexDef::with_i32_decoder(29, "SceneMode", &SCENE_MODE_DECODER),
        ArrayIndexDef::with_i32_decoder(30, "NoiseReduction", &NOISE_REDUCTION_DECODER),
        ArrayIndexDef::with_i32_decoder(31, "DistortionCorrection", &ON_OFF_I32),
        ArrayIndexDef::with_i32_decoder(32, "ShadingCompensation", &ON_OFF_I32),
        ArrayIndexDef::raw(33, "CompressionFactor"),
        ArrayIndexDef::raw(34, "Gradation"),
        ArrayIndexDef::with_i32_decoder(35, "PictureMode", &PICTURE_MODE_DECODER),
        ArrayIndexDef::raw(36, "PictureModeSaturation"),
        ArrayIndexDef::raw(37, "PictureModeContrast"),
        ArrayIndexDef::raw(38, "PictureModeSharpness"),
        ArrayIndexDef::raw(39, "PictureModeBWFilter"),
        ArrayIndexDef::raw(40, "PictureModeTone"),
        ArrayIndexDef::raw(41, "NoiseFilter"),
        ArrayIndexDef::with_i32_decoder(42, "ArtFilter", &ART_FILTER_DECODER),
        ArrayIndexDef::raw(43, "MagicFilter"),
        ArrayIndexDef::raw(44, "PictureModeEffect"),
        ArrayIndexDef::raw(45, "ToneCurve"),
        ArrayIndexDef::raw(46, "ToneLevel"),
        ArrayIndexDef::raw(47, "SharpnessFactor"),
        ArrayIndexDef::raw(48, "WBFRBBracket"),
    ],
};

/// Equipment array schema (Tag 0x0201)
/// Contains equipment information including body, lens, and flash details
/// Note: This is a byte array with complex internal structure
static EQUIPMENT_SCHEMA: ArraySchema = ArraySchema {
    name: "Equipment",
    indices: &[
        ArrayIndexDef::raw(0, "EquipmentVersion"),
        // Indices 1-19 are handled specially in parse_equipment function
        // due to complex byte-level parsing requirements
    ],
};

// ============================================================================
// TAG REGISTRY
// ============================================================================

/// Create Olympus tag registry with all tag definitions and array schemas
///
/// This registry includes 200+ tags covering:
/// - Main IFD basic tags (0x0000-0x00FF)
/// - Extended main IFD tags (0x0200-0x0FFF)
/// - Sensor/capture tags (0x1000-0x103F)
/// - Sub-IFD pointers (0x2010-0x5000)
pub fn olympus_registry() -> TagRegistry {
    TagRegistry::new()
        // ====================================================================
        // MAIN IFD BASIC TAGS (0x0000 - 0x00FF)
        // ====================================================================
        .register_raw(0x0000, "MakerNoteVersion")
        .register_raw(0x0001, "MinoltaCameraSettingsOld")
        .register_raw(0x0003, "MinoltaCameraSettings")
        .register_raw(0x0040, "CompressedImageSize")
        .register_raw(0x0081, "PreviewImageData")
        .register_raw(0x0088, "PreviewImageStart")
        .register_raw(0x0089, "PreviewImageLength")
        .register_raw(0x0100, "ThumbnailImage")
        .register_raw(0x0104, "BodyFirmwareVersion")
        // ====================================================================
        // EXTENDED MAIN IFD TAGS (0x0200 - 0x0FFF)
        // ====================================================================
        .register_raw(0x0200, "SpecialMode")
        .register_raw(0x0201, "Quality")
        .register_raw(0x0202, "Macro")
        .register_raw(0x0203, "BWMode")
        .register_raw(0x0204, "DigitalZoom")
        .register_raw(0x0205, "FocalPlaneDiagonal")
        .register_raw(0x0206, "LensDistortionParams")
        .register_raw(0x0207, "SoftwareRelease")
        .register_raw(0x0208, "PictureInfo")
        .register_raw(0x0209, "CameraID")
        .register_raw(0x020B, "EpsonImageWidth")
        .register_raw(0x020C, "EpsonImageHeight")
        .register_raw(0x020D, "EpsonSoftware")
        .register_raw(0x0280, "PreviewImage")
        .register_raw(0x0300, "PreCaptureFrames")
        .register_raw(0x0301, "WhiteBoard")
        .register_raw(0x0302, "OneTouchWB")
        .register_raw(0x0303, "WhiteBalanceBracket")
        .register_raw(0x0304, "WhiteBalanceBias")
        .register_simple_i32(0x0403, "SceneMode", &SCENE_MODE_DECODER)
        .register_raw(0x0404, "SerialNumber")
        .register_raw(0x0405, "Firmware")
        .register_raw(0x0E00, "PrintIM")
        .register_raw(0x0F00, "DataDump")
        .register_raw(0x0F01, "DataDump2")
        .register_raw(0x0F04, "ZoomedPreviewStart")
        .register_raw(0x0F05, "ZoomedPreviewLength")
        .register_raw(0x0F06, "ZoomedPreviewSize")
        // ====================================================================
        // SENSOR/CAPTURE TAGS (0x1000 - 0x103F)
        // ====================================================================
        .register_raw(0x1000, "ShutterSpeedValue")
        .register_raw(0x1001, "ISOValue")
        .register_raw(0x1002, "ApertureValue")
        .register_raw(0x1003, "BrightnessValue")
        .register_simple_i32(0x1004, "FlashMode", &FLASH_MODE_DECODER)
        .register_raw(0x1005, "FlashDevice")
        .register_raw(0x1006, "ExposureCompensation")
        .register_raw(0x1007, "SensorTemperature")
        .register_raw(0x1008, "LensTemperature")
        .register_raw(0x1009, "LightCondition")
        .register_raw(0x100A, "FocusRange")
        .register_simple_i32(0x100B, "FocusMode", &FOCUS_MODE_DECODER)
        .register_raw(0x100C, "ManualFocusDistance")
        .register_raw(0x100D, "ZoomStepCount")
        .register_raw(0x100E, "FocusStepCount")
        .register_raw(0x100F, "Sharpness")
        .register_raw(0x1010, "FlashChargeLevel")
        .register_raw(0x1011, "ColorMatrix")
        .register_raw(0x1012, "BlackLevel")
        .register_raw(0x1013, "ColorTemperatureBG")
        .register_raw(0x1014, "ColorTemperatureRG")
        .register_raw(0x1015, "WBMode")
        .register_raw(0x1017, "RedBalance")
        .register_raw(0x1018, "BlueBalance")
        .register_raw(0x1019, "ColorMatrixNumber")
        .register_raw(0x101A, "SerialNumber2")
        .register_raw(0x101B, "ExternalFlashAE1")
        .register_raw(0x101C, "ExternalFlashAE2")
        .register_raw(0x101D, "InternalFlashAE1")
        .register_raw(0x101E, "InternalFlashAE2")
        .register_raw(0x101F, "ExternalFlashAE1_0")
        .register_raw(0x1020, "ExternalFlashAE2_0")
        .register_raw(0x1021, "InternalFlashAE1_0")
        .register_raw(0x1022, "InternalFlashAE2_0")
        .register_raw(0x1023, "FlashExposureComp")
        .register_raw(0x1024, "InternalFlashTable")
        .register_raw(0x1025, "ExternalFlashGValue")
        .register_raw(0x1026, "ExternalFlashBounce")
        .register_raw(0x1027, "ExternalFlashZoom")
        .register_raw(0x1028, "ExternalFlashMode")
        .register_raw(0x1029, "Contrast")
        .register_raw(0x102A, "SharpnessFactor")
        .register_raw(0x102B, "ColorControl")
        .register_raw(0x102C, "ValidBits")
        .register_raw(0x102D, "CoringFilter")
        .register_raw(0x102E, "OlympusImageWidth")
        .register_raw(0x102F, "OlympusImageHeight")
        .register_raw(0x1030, "SceneDetect")
        .register_raw(0x1031, "SceneArea")
        .register_raw(0x1033, "SceneDetectData")
        .register_raw(0x1034, "CompressionRatio")
        .register_simple_i32(0x1035, "PreviewImageValid", &ON_OFF_I32)
        .register_raw(0x1036, "PreviewImageStart")
        .register_raw(0x1037, "PreviewImageLength")
        .register_raw(0x1038, "AFResult")
        .register_raw(0x1039, "CCDScanMode")
        .register_simple_i32(0x103A, "NoiseReduction", &NOISE_REDUCTION_DECODER)
        .register_raw(0x103B, "FocusStepInfinity")
        .register_raw(0x103C, "FocusStepNear")
        .register_raw(0x103D, "LightValueCenter")
        .register_raw(0x103E, "LightValuePeriphery")
        .register_raw(0x103F, "FieldCount")
        // ====================================================================
        // SUB-IFD POINTERS (0x2010 - 0x5000)
        // These tags point to sub-IFDs that contain additional structured data
        // ====================================================================
        .register_raw(0x2010, "Equipment")
        .register_raw(0x2020, "CameraSettings")
        .register_raw(0x2030, "RawDevelopment")
        .register_raw(0x2031, "RawDev2")
        .register_raw(0x2040, "ImageProcessing")
        .register_raw(0x2050, "FocusInfo")
        .register_raw(0x2100, "Olympus2100")
        .register_raw(0x2200, "Olympus2200")
        .register_raw(0x2300, "Olympus2300")
        .register_raw(0x2400, "Olympus2400")
        .register_raw(0x2500, "Olympus2500")
        .register_raw(0x2600, "Olympus2600")
        .register_raw(0x2700, "Olympus2700")
        .register_raw(0x2800, "Olympus2800")
        .register_raw(0x2900, "Olympus2900")
        .register_raw(0x3000, "RawInfo")
        .register_raw(0x4000, "MainInfo")
        .register_raw(0x5000, "UnknownInfo")
        // Array-based tags with schemas
        .register_array_schema(0x0003, &CAMERA_SETTINGS_SCHEMA)
        .register_array_schema(0x0201, &EQUIPMENT_SCHEMA)
}

// ============================================================================
// SUB-IFD TAG REGISTRIES
// ============================================================================

/// Create Equipment sub-IFD registry (0x2010)
/// Contains lens, body, and accessory information
pub fn olympus_equipment_registry() -> TagRegistry {
    TagRegistry::new()
        .register_raw(0x0000, "EquipmentVersion")
        .register_raw(0x0100, "CameraType")
        .register_raw(0x0101, "SerialNumber")
        .register_raw(0x0102, "InternalSerialNumber")
        .register_raw(0x0103, "FocalPlaneDiagonal")
        .register_raw(0x0104, "BodyFirmwareVersion")
        .register_raw(0x0201, "LensType")
        .register_raw(0x0202, "LensSerialNumber")
        .register_raw(0x0203, "LensModel")
        .register_raw(0x0204, "LensFirmwareVersion")
        .register_raw(0x0205, "MaxApertureAtMinFocal")
        .register_raw(0x0206, "MaxApertureAtMaxFocal")
        .register_raw(0x0207, "MinFocalLength")
        .register_raw(0x0208, "MaxFocalLength")
        .register_raw(0x020A, "MaxApertureAtCurrentFocal")
        .register_raw(0x020B, "LensProperties")
        .register_raw(0x0301, "Extender")
        .register_raw(0x0302, "ExtenderSerialNumber")
        .register_raw(0x0303, "ExtenderModel")
        .register_raw(0x0304, "ExtenderFirmwareVersion")
        .register_raw(0x0403, "ConversionLens")
        .register_raw(0x1000, "FlashType")
        .register_raw(0x1001, "FlashModel")
        .register_raw(0x1002, "FlashFirmwareVersion")
        .register_raw(0x1003, "FlashSerialNumber")
}

/// Create CameraSettings sub-IFD registry (0x2020)
/// Contains exposure, focus, and shooting mode settings
pub fn olympus_camera_settings_registry() -> TagRegistry {
    TagRegistry::new()
        .register_raw(0x0000, "CameraSettingsVersion")
        .register_simple_i32(0x0100, "PreviewImageValid", &ON_OFF_I32)
        .register_raw(0x0101, "PreviewImageStart")
        .register_raw(0x0102, "PreviewImageLength")
        .register_simple_i32(0x0200, "ExposureMode", &EXPOSURE_MODE_DECODER)
        .register_simple_i32(0x0201, "AELock", &ON_OFF_I32)
        .register_simple_i32(0x0202, "MeteringMode", &METERING_MODE_DECODER)
        .register_raw(0x0203, "ExposureShift")
        .register_raw(0x0204, "NDFilter")
        .register_simple_i32(0x0300, "MacroMode", &ON_OFF_I32)
        .register_simple_i32(0x0301, "FocusMode", &FOCUS_MODE_DECODER)
        .register_raw(0x0302, "FocusProcess")
        .register_raw(0x0303, "AFSearch")
        .register_raw(0x0304, "AFAreas")
        .register_raw(0x0305, "AFPointSelected")
        .register_raw(0x0306, "AFFineTune")
        .register_raw(0x0307, "AFFineTuneAdj")
        .register_simple_i32(0x0400, "FlashMode", &FLASH_MODE_DECODER)
        .register_raw(0x0401, "FlashExposureComp")
        .register_raw(0x0402, "FlashRemoteControl")
        .register_raw(0x0403, "FlashControlMode")
        .register_raw(0x0404, "FlashIntensity")
        .register_raw(0x0405, "ManualFlashStrength")
        .register_simple_i32(0x0500, "WhiteBalance", &WHITE_BALANCE_DECODER)
        .register_raw(0x0501, "WhiteBalanceTemperature")
        .register_raw(0x0502, "WhiteBalanceBracket")
        .register_raw(0x0503, "CustomSaturation")
        .register_raw(0x0504, "ModifiedSaturation")
        .register_raw(0x0505, "ContrastSetting")
        .register_raw(0x0506, "SharpnessSetting")
        .register_simple_i32(0x0507, "ColorSpace", &COLOR_SPACE_DECODER)
        .register_simple_i32(0x0509, "SceneMode", &SCENE_MODE_DECODER)
        .register_simple_i32(0x050A, "NoiseReduction", &NOISE_REDUCTION_DECODER)
        .register_simple_i32(0x050B, "DistortionCorrection", &ON_OFF_I32)
        .register_simple_i32(0x050C, "ShadingCompensation", &ON_OFF_I32)
        .register_raw(0x050D, "CompressionFactor")
        .register_raw(0x050F, "Gradation")
        .register_simple_i32(0x0520, "PictureMode", &PICTURE_MODE_DECODER)
        .register_raw(0x0521, "PictureModeSaturation")
        .register_raw(0x0522, "PictureModeContrast")
        .register_raw(0x0523, "PictureModeSharpness")
        .register_raw(0x0524, "PictureModeBWFilter")
        .register_raw(0x0525, "PictureModeTone")
        .register_raw(0x0526, "NoiseFilter")
        .register_simple_i32(0x0527, "ArtFilter", &ART_FILTER_DECODER)
        .register_raw(0x0529, "MagicFilter")
        .register_raw(0x052A, "PictureModeEffect")
        .register_raw(0x052B, "ToneCurve")
        .register_raw(0x052C, "ToneLevel")
        .register_raw(0x052D, "SharpnessFactor")
        .register_raw(0x0600, "DriveMode")
        .register_raw(0x0601, "PanoramaMode")
        .register_raw(0x0603, "ImageStabilization")
        .register_raw(0x0604, "SequenceLength")
}

/// Create RawDevelopment sub-IFD registry (0x2030)
/// Contains RAW processing settings
pub fn olympus_raw_development_registry() -> TagRegistry {
    TagRegistry::new()
        .register_raw(0x0000, "RawDevVersion")
        .register_raw(0x0100, "RawDevExposureBiasValue")
        .register_raw(0x0101, "RawDevWhiteBalanceValue")
        .register_raw(0x0102, "RawDevWBFineAdjustment")
        .register_raw(0x0103, "RawDevGrayPoint")
        .register_raw(0x0104, "RawDevSaturationEmphasis")
        .register_raw(0x0105, "RawDevMemoryColorEmphasis")
        .register_raw(0x0106, "RawDevContrastValue")
        .register_raw(0x0107, "RawDevSharpnessValue")
        .register_raw(0x0108, "RawDevColorSpace")
        .register_raw(0x0109, "RawDevEngine")
        .register_raw(0x010A, "RawDevNoiseReduction")
        .register_raw(0x010B, "RawDevEditStatus")
        .register_raw(0x010C, "RawDevSettings")
}

/// Create ImageProcessing sub-IFD registry (0x2040)
/// Contains image processing and enhancement settings
pub fn olympus_image_processing_registry() -> TagRegistry {
    TagRegistry::new()
        .register_raw(0x0000, "ImageProcessingVersion")
        .register_raw(0x0100, "WB_RBLevels")
        .register_raw(0x0102, "WB_RBLevels3000K")
        .register_raw(0x0103, "WB_RBLevels3300K")
        .register_raw(0x0104, "WB_RBLevels3600K")
        .register_raw(0x0105, "WB_RBLevels3900K")
        .register_raw(0x0106, "WB_RBLevels4000K")
        .register_raw(0x0107, "WB_RBLevels4300K")
        .register_raw(0x0108, "WB_RBLevels4500K")
        .register_raw(0x0109, "WB_RBLevels4800K")
        .register_raw(0x010A, "WB_RBLevels5300K")
        .register_raw(0x010B, "WB_RBLevels6000K")
        .register_raw(0x010C, "WB_RBLevels6600K")
        .register_raw(0x010D, "WB_RBLevels7500K")
        .register_raw(0x010E, "WB_RBLevelsCWB1")
        .register_raw(0x010F, "WB_RBLevelsCWB2")
        .register_raw(0x0110, "WB_RBLevelsCWB3")
        .register_raw(0x0111, "WB_RBLevelsCWB4")
        .register_raw(0x0113, "WB_GLevel3000K")
        .register_raw(0x0114, "WB_GLevel3300K")
        .register_raw(0x0115, "WB_GLevel3600K")
        .register_raw(0x0116, "WB_GLevel3900K")
        .register_raw(0x0117, "WB_GLevel4000K")
        .register_raw(0x0118, "WB_GLevel4300K")
        .register_raw(0x0119, "WB_GLevel4500K")
        .register_raw(0x011A, "WB_GLevel4800K")
        .register_raw(0x011B, "WB_GLevel5300K")
        .register_raw(0x011C, "WB_GLevel6000K")
        .register_raw(0x011D, "WB_GLevel6600K")
        .register_raw(0x011E, "WB_GLevel7500K")
        .register_raw(0x011F, "WB_GLevel")
        .register_raw(0x0200, "ColorMatrix")
        .register_raw(0x0300, "Enhancer")
        .register_raw(0x0301, "EnhancerValues")
        .register_raw(0x0310, "CoringFilterAdjust")
        .register_raw(0x0311, "CoringValues")
        .register_raw(0x0600, "BlackLevel2")
        .register_raw(0x0610, "GainBase")
        .register_raw(0x0611, "ValidBits")
        .register_raw(0x0612, "CropLeft")
        .register_raw(0x0613, "CropTop")
        .register_raw(0x0614, "CropWidth")
        .register_raw(0x0615, "CropHeight")
        .register_raw(0x0635, "UnknownBlock1")
        .register_raw(0x0636, "UnknownBlock2")
        .register_raw(0x0805, "SensorCalibration")
        .register_raw(0x1010, "NoiseReduction2")
        .register_raw(0x1011, "DistortionCorrection2")
        .register_raw(0x1012, "ShadingCompensation2")
        .register_raw(0x101C, "MultipleExposureMode")
        .register_raw(0x1103, "UnknownBlock3")
        .register_raw(0x1104, "UnknownBlock4")
        .register_raw(0x1112, "AspectRatio")
        .register_raw(0x1113, "AspectFrame")
        .register_raw(0x1200, "FacesDetected")
        .register_raw(0x1201, "FaceDetectArea")
        .register_raw(0x1202, "MaxFaces")
        .register_raw(0x1203, "FaceDetectFrameSize")
        .register_raw(0x1207, "FaceDetectFrameCrop")
        .register_raw(0x1306, "CameraTemperature")
        .register_raw(0x1900, "KeystoneCompensation")
        .register_raw(0x1901, "KeystoneDirection")
        .register_raw(0x1906, "KeystoneValue")
}

/// Create FocusInfo sub-IFD registry (0x2050)
/// Contains autofocus and focus-related data
pub fn olympus_focus_info_registry() -> TagRegistry {
    TagRegistry::new()
        .register_raw(0x0000, "FocusInfoVersion")
        .register_raw(0x0209, "AutoFocus")
        .register_raw(0x0210, "SceneDetect")
        .register_raw(0x0211, "SceneArea")
        .register_raw(0x0212, "SceneDetectData")
        .register_raw(0x0300, "ZoomStepCount")
        .register_raw(0x0301, "FocusStepCount")
        .register_raw(0x0303, "FocusStepInfinity")
        .register_raw(0x0304, "FocusStepNear")
        .register_raw(0x0305, "FocusDistance")
        .register_raw(0x0308, "AFPoint")
        .register_raw(0x030C, "AFInfo")
        .register_raw(0x1201, "ExternalFlash")
        .register_raw(0x1203, "ExternalFlashGuideNumber")
        .register_raw(0x1204, "ExternalFlashBounce")
        .register_raw(0x1205, "ExternalFlashZoom")
        .register_raw(0x1208, "InternalFlashStrength")
        .register_raw(0x1209, "ManualFlash")
        .register_raw(0x1500, "SensorTemperature")
        .register_raw(0x1600, "ImageStabilization")
}

/// Create RawInfo sub-IFD registry (0x3000)
/// Contains RAW file specific information
pub fn olympus_raw_info_registry() -> TagRegistry {
    TagRegistry::new()
        .register_raw(0x0000, "RawInfoVersion")
        .register_raw(0x0100, "WB_RBLevelsUsed")
        .register_raw(0x0110, "WB_RBLevelsAuto")
        .register_raw(0x0120, "WB_RBLevelsShade")
        .register_raw(0x0121, "WB_RBLevelsCloudy")
        .register_raw(0x0122, "WB_RBLevelsFineWeather")
        .register_raw(0x0123, "WB_RBLevelsTungsten")
        .register_raw(0x0124, "WB_RBLevelsEveningSunlight")
        .register_raw(0x0130, "WB_RBLevelsDaylightFluor")
        .register_raw(0x0131, "WB_RBLevelsNeutralWhiteFluor")
        .register_raw(0x0132, "WB_RBLevelsCoolWhiteFluor")
        .register_raw(0x0133, "WB_RBLevelsWhiteFluorescent")
        .register_raw(0x0200, "ColorMatrix2")
        .register_raw(0x0310, "CoringFilter")
        .register_raw(0x0311, "CoringValues")
        .register_raw(0x0600, "BlackLevel2")
        .register_raw(0x0601, "YCbCrCoefficients")
        .register_raw(0x0611, "ValidBits")
        .register_raw(0x0612, "CropLeft")
        .register_raw(0x0613, "CropTop")
        .register_raw(0x0614, "CropWidth")
        .register_raw(0x0615, "CropHeight")
        .register_raw(0x1000, "LightSource")
        .register_raw(0x1001, "WhiteBalanceComp")
        .register_raw(0x1010, "SaturationSetting")
        .register_raw(0x1011, "HueSetting")
        .register_raw(0x1012, "ContrastSetting")
        .register_raw(0x1013, "SharpnessSetting")
        .register_raw(0x2000, "CMExposureCompensation")
        .register_raw(0x2001, "CMWhiteBalance")
        .register_raw(0x2002, "CMWhiteBalanceComp")
        .register_raw(0x2010, "CMWhiteBalanceGrayPoint")
        .register_raw(0x2020, "CMSaturationSetting")
        .register_raw(0x2021, "CMHueSetting")
        .register_raw(0x2022, "CMContrastSetting")
        .register_raw(0x2023, "CMSharpnessSetting")
}

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

/// Process Equipment array with special lens lookup handling
///
/// Equipment array has complex byte-level structure that requires custom parsing
/// beyond what ArraySchema can provide. This handles serial numbers, firmware
/// versions, lens data, and aperture/focal length information.
pub fn process_equipment_with_lens(
    array: &[u8],
    prefix: &str,
    lens_db: &impl super::super::shared::LensDatabase,
    byte_order: super::super::super::ifd_parser::ByteOrder,
    tags: &mut std::collections::HashMap<String, String>,
) {
    // Create EndianReader for the array data
    let reader = EndianReader::new(array, byte_order.to_io_byte_order());

    // Serial number (8 bytes starting at offset 2)
    if array.len() >= 10 {
        let serial_bytes = &array[2..10];
        if let Ok(serial) = std::str::from_utf8(serial_bytes) {
            let serial_str = serial.trim_end_matches('\0').trim();
            if !serial_str.is_empty() {
                tags.insert(format!("{}:SerialNumber", prefix), serial_str.to_string());
            }
        }
    }

    // Body firmware version (5 bytes starting at offset 10)
    if array.len() >= 15 {
        let fw_bytes = &array[10..15];
        if let Ok(fw) = std::str::from_utf8(fw_bytes) {
            let fw_str = fw.trim_end_matches('\0').trim();
            if !fw_str.is_empty() {
                tags.insert(
                    format!("{}:BodyFirmwareVersion", prefix),
                    fw_str.to_string(),
                );
            }
        }
    }

    // Lens type (2 bytes at offset 16) - uses lens database
    if array.len() >= 18 {
        let lens_id = reader.u16_at(16).unwrap_or(0);

        if lens_id != 0 {
            if let Some(lens_name) = lens_db.lookup(lens_id) {
                tags.insert(format!("{}:LensType", prefix), lens_name.to_string());
            } else {
                tags.insert(format!("{}:LensID", prefix), lens_id.to_string());
            }
        }
    }

    // Lens serial number (8 bytes at offset 18)
    if array.len() >= 26 {
        let lens_serial_bytes = &array[18..26];
        if let Ok(lens_serial) = std::str::from_utf8(lens_serial_bytes) {
            let lens_serial_str = lens_serial.trim_end_matches('\0').trim();
            if !lens_serial_str.is_empty() {
                tags.insert(
                    format!("{}:LensSerialNumber", prefix),
                    lens_serial_str.to_string(),
                );
            }
        }
    }

    // Max aperture at min focal (2 bytes at offset 52)
    if array.len() >= 54 {
        let max_ap_min = reader.u16_at(52).unwrap_or(0);
        if max_ap_min > 0 {
            let f_stop = (max_ap_min as f32) / 10.0;
            tags.insert(
                format!("{}:MaxApertureAtMinFocal", prefix),
                format!("f/{:.1}", f_stop),
            );
        }
    }

    // Max aperture at max focal (2 bytes at offset 54)
    if array.len() >= 56 {
        let max_ap_max = reader.u16_at(54).unwrap_or(0);
        if max_ap_max > 0 {
            let f_stop = (max_ap_max as f32) / 10.0;
            tags.insert(
                format!("{}:MaxApertureAtMaxFocal", prefix),
                format!("f/{:.1}", f_stop),
            );
        }
    }

    // Min focal length (2 bytes at offset 56)
    if array.len() >= 58 {
        let min_focal = reader.u16_at(56).unwrap_or(0);
        if min_focal > 0 {
            tags.insert(
                format!("{}:MinFocalLength", prefix),
                format!("{} mm", min_focal),
            );
        }
    }

    // Max focal length (2 bytes at offset 58)
    if array.len() >= 60 {
        let max_focal = reader.u16_at(58).unwrap_or(0);
        if max_focal > 0 {
            tags.insert(
                format!("{}:MaxFocalLength", prefix),
                format!("{} mm", max_focal),
            );
        }
    }
}
