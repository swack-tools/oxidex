//! Tag Registry - 500+ Metadata Tags
//!
//! This module provides a static registry of 500+ metadata tags covering EXIF (300+),
//! GPS (30+), XMP (100+), IPTC (50+), PDF (10+), and QuickTime (10+) formats.
//! This is a manual implementation that will later be replaced by automated tag
//! generation in build.rs (task I5.T5).

use super::generated_tags::GENERATED_TAG_REGISTRY;
use crate::core::tag_descriptor::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

/// Static registry containing all 500+ registered metadata tags.
/// Uses lazy initialization for zero-cost abstraction until first access.
static TAG_REGISTRY: Lazy<HashMap<&'static str, TagDescriptor>> = Lazy::new(|| {
    let mut registry = HashMap::with_capacity(512);

    // ===========================
    // EXIF TAGS (310+ total)
    // ===========================

    // --- Camera Information (10 tags) ---
    registry.insert(
        "EXIF:Make",
        TagDescriptor::new(
            TagId::new_numeric(0x010F),
            "EXIF:Make".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::String,
            "Manufacturer of the recording equipment".to_string(),
            vec!["Canon".to_string(), "Nikon".to_string(), "Sony".to_string()],
        ),
    );

    registry.insert(
        "EXIF:Model",
        TagDescriptor::new(
            TagId::new_numeric(0x0110),
            "EXIF:Model".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::String,
            "Model name or number of the recording equipment".to_string(),
            vec![
                "EOS 5D Mark IV".to_string(),
                "D850".to_string(),
                "Alpha 7R IV".to_string(),
            ],
        ),
    );

    registry.insert(
        "EXIF:Software",
        TagDescriptor::new(
            TagId::new_numeric(0x0131),
            "EXIF:Software".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::String,
            "Software used to create or process the image".to_string(),
            vec![
                "Adobe Photoshop 2024".to_string(),
                "GIMP 2.10".to_string(),
                "Lightroom Classic".to_string(),
            ],
        ),
    );

    registry.insert(
        "EXIF:LensModel",
        TagDescriptor::new(
            TagId::new_numeric(0xA434),
            "EXIF:LensModel".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::String,
            "Lens model name".to_string(),
            vec![
                "EF 24-70mm f/2.8L II USM".to_string(),
                "AF-S 50mm f/1.8G".to_string(),
            ],
        ),
    );

    registry.insert(
        "EXIF:LensMake",
        TagDescriptor::new(
            TagId::new_numeric(0xA433),
            "EXIF:LensMake".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::String,
            "Lens manufacturer".to_string(),
            vec![
                "Canon".to_string(),
                "Nikon".to_string(),
                "Sigma".to_string(),
            ],
        ),
    );

    registry.insert(
        "EXIF:SerialNumber",
        TagDescriptor::new(
            TagId::new_numeric(0xA431),
            "EXIF:SerialNumber".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::String,
            "Camera serial number".to_string(),
            vec!["1234567890".to_string(), "ABC123XYZ".to_string()],
        ),
    );

    registry.insert(
        "EXIF:LensSerialNumber",
        TagDescriptor::new(
            TagId::new_numeric(0xA435),
            "EXIF:LensSerialNumber".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::String,
            "Lens serial number".to_string(),
            vec!["9876543210".to_string()],
        ),
    );

    registry.insert(
        "EXIF:FirmwareVersion",
        TagDescriptor::new(
            TagId::new_numeric(0xA432),
            "EXIF:FirmwareVersion".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::String,
            "Camera firmware version".to_string(),
            vec!["1.2.5".to_string(), "Firmware 3.0.1".to_string()],
        ),
    );

    registry.insert(
        "EXIF:Artist",
        TagDescriptor::new(
            TagId::new_numeric(0x013B),
            "EXIF:Artist".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::String,
            "Name of photographer or creator".to_string(),
            vec!["John Doe".to_string(), "Jane Smith".to_string()],
        ),
    );

    registry.insert(
        "EXIF:Copyright",
        TagDescriptor::new(
            TagId::new_numeric(0x8298),
            "EXIF:Copyright".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::String,
            "Copyright notice".to_string(),
            vec![
                "Copyright 2024 John Doe".to_string(),
                "© 2024 All Rights Reserved".to_string(),
            ],
        ),
    );

    // --- Exposure Settings (15 tags) ---
    registry.insert(
        "EXIF:ExposureTime",
        TagDescriptor::new(
            TagId::new_numeric(0x829A),
            "EXIF:ExposureTime".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Rational,
            "Exposure time in seconds".to_string(),
            vec!["1/125".to_string(), "1/250".to_string(), "2.5".to_string()],
        ),
    );

    registry.insert(
        "EXIF:FNumber",
        TagDescriptor::new(
            TagId::new_numeric(0x829D),
            "EXIF:FNumber".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Rational,
            "F-number or aperture".to_string(),
            vec!["f/2.8".to_string(), "f/5.6".to_string(), "f/11".to_string()],
        ),
    );

    registry.insert(
        "EXIF:ISO",
        TagDescriptor::new(
            TagId::new_numeric(0x8827),
            "EXIF:ISO".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "ISO speed rating".to_string(),
            vec!["100".to_string(), "400".to_string(), "1600".to_string()],
        ),
    );

    registry.insert(
        "EXIF:ShutterSpeedValue",
        TagDescriptor::new(
            TagId::new_numeric(0x9201),
            "EXIF:ShutterSpeedValue".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Rational,
            "Shutter speed value in APEX units".to_string(),
            vec!["6.96".to_string(), "7.64".to_string()],
        ),
    );

    registry.insert(
        "EXIF:ApertureValue",
        TagDescriptor::new(
            TagId::new_numeric(0x9202),
            "EXIF:ApertureValue".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Rational,
            "Aperture value in APEX units".to_string(),
            vec!["2.97".to_string(), "5.66".to_string()],
        ),
    );

    registry.insert(
        "EXIF:ExposureProgram",
        TagDescriptor::new(
            TagId::new_numeric(0x8822),
            "EXIF:ExposureProgram".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Exposure program mode (1=Manual, 2=Normal, 3=Aperture Priority, 4=Shutter Priority)"
                .to_string(),
            vec!["1".to_string(), "2".to_string(), "3".to_string()],
        ),
    );

    registry.insert(
        "EXIF:ExposureBiasValue",
        TagDescriptor::new(
            TagId::new_numeric(0x9204),
            "EXIF:ExposureBiasValue".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Rational,
            "Exposure bias/compensation value in EV".to_string(),
            vec!["+0.3".to_string(), "-1.0".to_string(), "0".to_string()],
        ),
    );

    registry.insert(
        "EXIF:MeteringMode",
        TagDescriptor::new(
            TagId::new_numeric(0x9207),
            "EXIF:MeteringMode".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Metering mode (1=Average, 2=Center-weighted, 3=Spot, 5=Multi-segment)".to_string(),
            vec!["5".to_string(), "3".to_string()],
        ),
    );

    registry.insert(
        "EXIF:Flash",
        TagDescriptor::new(
            TagId::new_numeric(0x9209),
            "EXIF:Flash".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Flash status and mode".to_string(),
            vec!["0".to_string(), "1".to_string(), "16".to_string()],
        ),
    );

    registry.insert(
        "EXIF:FocalLength",
        TagDescriptor::new(
            TagId::new_numeric(0x920A),
            "EXIF:FocalLength".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Rational,
            "Focal length in millimeters".to_string(),
            vec!["50".to_string(), "24".to_string(), "200".to_string()],
        ),
    );

    registry.insert(
        "EXIF:FocalLengthIn35mmFormat",
        TagDescriptor::new(
            TagId::new_numeric(0xA405),
            "EXIF:FocalLengthIn35mmFormat".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Focal length in 35mm film equivalent".to_string(),
            vec!["50".to_string(), "75".to_string(), "300".to_string()],
        ),
    );

    registry.insert(
        "EXIF:MaxApertureValue",
        TagDescriptor::new(
            TagId::new_numeric(0x9205),
            "EXIF:MaxApertureValue".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Rational,
            "Maximum aperture of lens in APEX units".to_string(),
            vec!["1.4".to_string(), "2.8".to_string()],
        ),
    );

    registry.insert(
        "EXIF:BrightnessValue",
        TagDescriptor::new(
            TagId::new_numeric(0x9203),
            "EXIF:BrightnessValue".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Rational,
            "Scene brightness value in APEX units".to_string(),
            vec!["5.2".to_string(), "7.8".to_string()],
        ),
    );

    registry.insert(
        "EXIF:SubjectDistance",
        TagDescriptor::new(
            TagId::new_numeric(0x9206),
            "EXIF:SubjectDistance".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Rational,
            "Distance to subject in meters".to_string(),
            vec!["1.5".to_string(), "5.0".to_string(), "inf".to_string()],
        ),
    );

    registry.insert(
        "EXIF:LightSource",
        TagDescriptor::new(
            TagId::new_numeric(0x9208),
            "EXIF:LightSource".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Light source type (0=Unknown, 1=Daylight, 2=Fluorescent, 3=Tungsten, etc.)"
                .to_string(),
            vec!["0".to_string(), "1".to_string(), "3".to_string()],
        ),
    );

    // --- Image Properties (15 tags) ---
    registry.insert(
        "EXIF:ImageWidth",
        TagDescriptor::new(
            TagId::new_numeric(0x0100),
            "EXIF:ImageWidth".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Image width in pixels".to_string(),
            vec!["1920".to_string(), "3840".to_string(), "6000".to_string()],
        ),
    );

    registry.insert(
        "EXIF:ImageHeight",
        TagDescriptor::new(
            TagId::new_numeric(0x0101),
            "EXIF:ImageHeight".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Image height in pixels".to_string(),
            vec!["1080".to_string(), "2160".to_string(), "4000".to_string()],
        ),
    );

    registry.insert(
        "EXIF:Orientation",
        TagDescriptor::new(
            TagId::new_numeric(0x0112),
            "EXIF:Orientation".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Image orientation (1=Horizontal, 3=Rotate 180, 6=Rotate 90 CW, 8=Rotate 270 CW)"
                .to_string(),
            vec!["1".to_string(), "6".to_string(), "8".to_string()],
        ),
    );

    registry.insert(
        "EXIF:XResolution",
        TagDescriptor::new(
            TagId::new_numeric(0x011A),
            "EXIF:XResolution".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Rational,
            "Horizontal resolution in pixels per resolution unit".to_string(),
            vec!["72".to_string(), "300".to_string()],
        ),
    );

    registry.insert(
        "EXIF:YResolution",
        TagDescriptor::new(
            TagId::new_numeric(0x011B),
            "EXIF:YResolution".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Rational,
            "Vertical resolution in pixels per resolution unit".to_string(),
            vec!["72".to_string(), "300".to_string()],
        ),
    );

    registry.insert(
        "EXIF:ResolutionUnit",
        TagDescriptor::new(
            TagId::new_numeric(0x0128),
            "EXIF:ResolutionUnit".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Unit for measuring XResolution and YResolution (2=inches, 3=centimeters)".to_string(),
            vec!["2".to_string(), "3".to_string()],
        ),
    );

    registry.insert(
        "EXIF:Compression",
        TagDescriptor::new(
            TagId::new_numeric(0x0103),
            "EXIF:Compression".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Compression scheme (1=Uncompressed, 6=JPEG)".to_string(),
            vec!["1".to_string(), "6".to_string()],
        ),
    );

    registry.insert(
        "EXIF:BitsPerSample",
        TagDescriptor::new(
            TagId::new_numeric(0x0102),
            "EXIF:BitsPerSample".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Number of bits per color component".to_string(),
            vec!["8".to_string(), "16".to_string()],
        ),
    );

    registry.insert(
        "EXIF:PhotometricInterpretation",
        TagDescriptor::new(
            TagId::new_numeric(0x0106),
            "EXIF:PhotometricInterpretation".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Pixel composition (2=RGB, 6=YCbCr)".to_string(),
            vec!["2".to_string(), "6".to_string()],
        ),
    );

    registry.insert(
        "EXIF:SamplesPerPixel",
        TagDescriptor::new(
            TagId::new_numeric(0x0115),
            "EXIF:SamplesPerPixel".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Number of color components (3=RGB, 4=CMYK)".to_string(),
            vec!["3".to_string(), "4".to_string()],
        ),
    );

    registry.insert(
        "EXIF:PlanarConfiguration",
        TagDescriptor::new(
            TagId::new_numeric(0x011C),
            "EXIF:PlanarConfiguration".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Storage format for color components (1=Chunky, 2=Planar)".to_string(),
            vec!["1".to_string(), "2".to_string()],
        ),
    );

    registry.insert(
        "EXIF:YCbCrPositioning",
        TagDescriptor::new(
            TagId::new_numeric(0x0213),
            "EXIF:YCbCrPositioning".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Positioning of subsampled chrominance components (1=Centered, 2=Co-sited)".to_string(),
            vec!["1".to_string(), "2".to_string()],
        ),
    );

    registry.insert(
        "EXIF:YCbCrSubSampling",
        TagDescriptor::new(
            TagId::new_numeric(0x0212),
            "EXIF:YCbCrSubSampling".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Subsampling ratio of Y to Cr and Cb components".to_string(),
            vec!["2 2".to_string(), "4 2 2".to_string()],
        ),
    );

    registry.insert(
        "EXIF:ThumbnailOffset",
        TagDescriptor::new(
            TagId::new_numeric(0x0201),
            "EXIF:ThumbnailOffset".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Byte offset to thumbnail image data".to_string(),
            vec!["5120".to_string(), "10240".to_string()],
        ),
    );

    registry.insert(
        "EXIF:ThumbnailLength",
        TagDescriptor::new(
            TagId::new_numeric(0x0202),
            "EXIF:ThumbnailLength".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Size of thumbnail image data in bytes".to_string(),
            vec!["12288".to_string(), "20480".to_string()],
        ),
    );

    // --- Date/Time (5 tags) ---
    registry.insert(
        "EXIF:DateTime",
        TagDescriptor::new(
            TagId::new_numeric(0x0132),
            "EXIF:DateTime".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::DateTime,
            "File change date and time".to_string(),
            vec![
                "2024:03:15 14:30:45".to_string(),
                "2024:12:01 09:15:22".to_string(),
            ],
        ),
    );

    registry.insert(
        "EXIF:DateTimeOriginal",
        TagDescriptor::new(
            TagId::new_numeric(0x9003),
            "EXIF:DateTimeOriginal".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::DateTime,
            "Date and time when original image was captured".to_string(),
            vec![
                "2024:03:15 14:30:45".to_string(),
                "2024:11:25 16:42:10".to_string(),
            ],
        ),
    );

    registry.insert(
        "EXIF:DateTimeDigitized",
        TagDescriptor::new(
            TagId::new_numeric(0x9004),
            "EXIF:DateTimeDigitized".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::DateTime,
            "Date and time when image was digitized".to_string(),
            vec![
                "2024:03:15 14:30:45".to_string(),
                "2024:11:26 10:20:30".to_string(),
            ],
        ),
    );

    registry.insert(
        "EXIF:SubSecTime",
        TagDescriptor::new(
            TagId::new_numeric(0x9290),
            "EXIF:SubSecTime".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::String,
            "Fractional seconds for DateTime".to_string(),
            vec!["123".to_string(), "456".to_string()],
        ),
    );

    registry.insert(
        "EXIF:SubSecTimeOriginal",
        TagDescriptor::new(
            TagId::new_numeric(0x9291),
            "EXIF:SubSecTimeOriginal".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::String,
            "Fractional seconds for DateTimeOriginal".to_string(),
            vec!["789".to_string(), "012".to_string()],
        ),
    );

    // --- Color and Scene Information (10 tags) ---
    registry.insert(
        "EXIF:ColorSpace",
        TagDescriptor::new(
            TagId::new_numeric(0xA001),
            "EXIF:ColorSpace".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Color space information (1=sRGB, 65535=Uncalibrated)".to_string(),
            vec!["1".to_string(), "65535".to_string()],
        ),
    );

    registry.insert(
        "EXIF:WhiteBalance",
        TagDescriptor::new(
            TagId::new_numeric(0xA403),
            "EXIF:WhiteBalance".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "White balance mode (0=Auto, 1=Manual)".to_string(),
            vec!["0".to_string(), "1".to_string()],
        ),
    );

    registry.insert(
        "EXIF:ExifVersion",
        TagDescriptor::new(
            TagId::new_numeric(0x9000),
            "EXIF:ExifVersion".to_string(),
            FormatFamily::EXIF,
            false,
            ValueType::String,
            "EXIF version number".to_string(),
            vec!["0232".to_string(), "0230".to_string()],
        ),
    );

    registry.insert(
        "EXIF:FlashpixVersion",
        TagDescriptor::new(
            TagId::new_numeric(0xA000),
            "EXIF:FlashpixVersion".to_string(),
            FormatFamily::EXIF,
            false,
            ValueType::String,
            "FlashPix version number".to_string(),
            vec!["0100".to_string()],
        ),
    );

    registry.insert(
        "EXIF:ComponentsConfiguration",
        TagDescriptor::new(
            TagId::new_numeric(0x9101),
            "EXIF:ComponentsConfiguration".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Binary,
            "Color component configuration".to_string(),
            vec!["Y Cb Cr -".to_string()],
        ),
    );

    registry.insert(
        "EXIF:SceneCaptureType",
        TagDescriptor::new(
            TagId::new_numeric(0xA406),
            "EXIF:SceneCaptureType".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Type of scene (0=Standard, 1=Landscape, 2=Portrait, 3=Night)".to_string(),
            vec!["0".to_string(), "1".to_string(), "2".to_string()],
        ),
    );

    registry.insert(
        "EXIF:Contrast",
        TagDescriptor::new(
            TagId::new_numeric(0xA408),
            "EXIF:Contrast".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Contrast processing (0=Normal, 1=Low, 2=High)".to_string(),
            vec!["0".to_string(), "1".to_string()],
        ),
    );

    registry.insert(
        "EXIF:Saturation",
        TagDescriptor::new(
            TagId::new_numeric(0xA409),
            "EXIF:Saturation".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Saturation processing (0=Normal, 1=Low, 2=High)".to_string(),
            vec!["0".to_string(), "2".to_string()],
        ),
    );

    registry.insert(
        "EXIF:Sharpness",
        TagDescriptor::new(
            TagId::new_numeric(0xA40A),
            "EXIF:Sharpness".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Sharpness processing (0=Normal, 1=Soft, 2=Hard)".to_string(),
            vec!["0".to_string(), "1".to_string()],
        ),
    );

    registry.insert(
        "EXIF:UserComment",
        TagDescriptor::new(
            TagId::new_numeric(0x9286),
            "EXIF:UserComment".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::String,
            "User comment or description".to_string(),
            vec![
                "Beautiful sunset photo".to_string(),
                "Family vacation 2024".to_string(),
            ],
        ),
    );

    // --- Additional EXIF (5 tags to reach 60) ---
    registry.insert(
        "EXIF:ImageDescription",
        TagDescriptor::new(
            TagId::new_numeric(0x010E),
            "EXIF:ImageDescription".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::String,
            "Image title or description".to_string(),
            vec![
                "Mountain landscape".to_string(),
                "Portrait session".to_string(),
            ],
        ),
    );

    registry.insert(
        "EXIF:PixelXDimension",
        TagDescriptor::new(
            TagId::new_numeric(0xA002),
            "EXIF:PixelXDimension".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Valid image width in pixels".to_string(),
            vec!["3840".to_string(), "6000".to_string()],
        ),
    );

    registry.insert(
        "EXIF:PixelYDimension",
        TagDescriptor::new(
            TagId::new_numeric(0xA003),
            "EXIF:PixelYDimension".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Valid image height in pixels".to_string(),
            vec!["2160".to_string(), "4000".to_string()],
        ),
    );

    registry.insert(
        "EXIF:SensingMethod",
        TagDescriptor::new(
            TagId::new_numeric(0xA217),
            "EXIF:SensingMethod".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Image sensor type (2=One-chip color area)".to_string(),
            vec!["2".to_string()],
        ),
    );

    registry.insert(
        "EXIF:FileSource",
        TagDescriptor::new(
            TagId::new_numeric(0xA300),
            "EXIF:FileSource".to_string(),
            FormatFamily::EXIF,
            false,
            ValueType::Integer,
            "Source of image file (3=Digital camera)".to_string(),
            vec!["3".to_string()],
        ),
    );

    // ===========================
    // EXIF TAGS (300+ total)
    // ===========================

    // --- Additional EXIF/TIFF Standard Tags (150 tags) ---
    registry.insert(
        "EXIF:SubfileType",
        TagDescriptor::new(
            TagId::new_numeric(0x00fe),
            "EXIF:SubfileType".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Type of data in subfile".to_string(),
            vec!["0".to_string(), "1".to_string()],
        ),
    );

    registry.insert(
        "EXIF:NewSubfileType",
        TagDescriptor::new(
            TagId::new_numeric(0x00ff),
            "EXIF:NewSubfileType".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "General indication of data contained in subfile".to_string(),
            vec!["0".to_string(), "1".to_string()],
        ),
    );

    registry.insert(
        "EXIF:RowsPerStrip",
        TagDescriptor::new(
            TagId::new_numeric(0x0116),
            "EXIF:RowsPerStrip".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Number of rows per strip".to_string(),
            vec!["8".to_string(), "16".to_string()],
        ),
    );

    registry.insert(
        "EXIF:StripOffsets",
        TagDescriptor::new(
            TagId::new_numeric(0x0111),
            "EXIF:StripOffsets".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Byte offset of each strip".to_string(),
            vec!["5000".to_string(), "10000".to_string()],
        ),
    );

    registry.insert(
        "EXIF:StripByteCounts",
        TagDescriptor::new(
            TagId::new_numeric(0x0117),
            "EXIF:StripByteCounts".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Number of bytes in each strip".to_string(),
            vec!["2048".to_string(), "4096".to_string()],
        ),
    );

    registry.insert(
        "EXIF:TileWidth",
        TagDescriptor::new(
            TagId::new_numeric(0x0142),
            "EXIF:TileWidth".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Tile width in pixels".to_string(),
            vec!["256".to_string(), "512".to_string()],
        ),
    );

    registry.insert(
        "EXIF:TileLength",
        TagDescriptor::new(
            TagId::new_numeric(0x0143),
            "EXIF:TileLength".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Tile height in pixels".to_string(),
            vec!["256".to_string(), "512".to_string()],
        ),
    );

    registry.insert(
        "EXIF:TileOffsets",
        TagDescriptor::new(
            TagId::new_numeric(0x0144),
            "EXIF:TileOffsets".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Byte offset of each tile".to_string(),
            vec!["1024".to_string(), "2048".to_string()],
        ),
    );

    registry.insert(
        "EXIF:TileByteCounts",
        TagDescriptor::new(
            TagId::new_numeric(0x0145),
            "EXIF:TileByteCounts".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Number of bytes in each tile".to_string(),
            vec!["512".to_string(), "1024".to_string()],
        ),
    );

    registry.insert(
        "EXIF:CellWidth",
        TagDescriptor::new(
            TagId::new_numeric(0x0108),
            "EXIF:CellWidth".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Width of dithering or halftoning matrix".to_string(),
            vec!["8".to_string()],
        ),
    );

    registry.insert(
        "EXIF:CellLength",
        TagDescriptor::new(
            TagId::new_numeric(0x0109),
            "EXIF:CellLength".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Height of dithering or halftoning matrix".to_string(),
            vec!["8".to_string()],
        ),
    );

    registry.insert(
        "EXIF:FillOrder",
        TagDescriptor::new(
            TagId::new_numeric(0x010a),
            "EXIF:FillOrder".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Logical order of bits within a byte".to_string(),
            vec!["1".to_string(), "2".to_string()],
        ),
    );

    registry.insert(
        "EXIF:DocumentName",
        TagDescriptor::new(
            TagId::new_numeric(0x010d),
            "EXIF:DocumentName".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::String,
            "Name of the scanned document".to_string(),
            vec!["Document1".to_string(), "Page1".to_string()],
        ),
    );

    registry.insert(
        "EXIF:PageName",
        TagDescriptor::new(
            TagId::new_numeric(0x011d),
            "EXIF:PageName".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::String,
            "Name of the page".to_string(),
            vec!["Page 1".to_string(), "Cover".to_string()],
        ),
    );

    registry.insert(
        "EXIF:XPosition",
        TagDescriptor::new(
            TagId::new_numeric(0x011e),
            "EXIF:XPosition".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Rational,
            "X position of image on page".to_string(),
            vec!["0".to_string(), "72.5".to_string()],
        ),
    );

    registry.insert(
        "EXIF:YPosition",
        TagDescriptor::new(
            TagId::new_numeric(0x011f),
            "EXIF:YPosition".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Rational,
            "Y position of image on page".to_string(),
            vec!["0".to_string(), "72.5".to_string()],
        ),
    );

    registry.insert(
        "EXIF:FreeOffsets",
        TagDescriptor::new(
            TagId::new_numeric(0x0120),
            "EXIF:FreeOffsets".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Byte offsets to free blocks".to_string(),
            vec!["1000".to_string()],
        ),
    );

    registry.insert(
        "EXIF:FreeByteCounts",
        TagDescriptor::new(
            TagId::new_numeric(0x0121),
            "EXIF:FreeByteCounts".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Sizes of free blocks in bytes".to_string(),
            vec!["512".to_string()],
        ),
    );

    registry.insert(
        "EXIF:GrayResponseUnit",
        TagDescriptor::new(
            TagId::new_numeric(0x0122),
            "EXIF:GrayResponseUnit".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Precision of gray response curve".to_string(),
            vec!["1".to_string(), "2".to_string()],
        ),
    );

    registry.insert(
        "EXIF:GrayResponseCurve",
        TagDescriptor::new(
            TagId::new_numeric(0x0123),
            "EXIF:GrayResponseCurve".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Gray scale response curve".to_string(),
            vec!["0".to_string(), "255".to_string()],
        ),
    );

    registry.insert(
        "EXIF:T4Options",
        TagDescriptor::new(
            TagId::new_numeric(0x0124),
            "EXIF:T4Options".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Options for Group 3 Fax compression".to_string(),
            vec!["0".to_string(), "1".to_string()],
        ),
    );

    registry.insert(
        "EXIF:T6Options",
        TagDescriptor::new(
            TagId::new_numeric(0x0125),
            "EXIF:T6Options".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Options for Group 4 Fax compression".to_string(),
            vec!["0".to_string()],
        ),
    );

    registry.insert(
        "EXIF:PageNumber",
        TagDescriptor::new(
            TagId::new_numeric(0x0129),
            "EXIF:PageNumber".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Page number of multi-page document".to_string(),
            vec!["1".to_string(), "2".to_string()],
        ),
    );

    registry.insert(
        "EXIF:TransferFunction",
        TagDescriptor::new(
            TagId::new_numeric(0x012d),
            "EXIF:TransferFunction".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Transfer function for image".to_string(),
            vec!["0".to_string()],
        ),
    );

    registry.insert(
        "EXIF:Predictor",
        TagDescriptor::new(
            TagId::new_numeric(0x013d),
            "EXIF:Predictor".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Prediction scheme for LZW compression".to_string(),
            vec!["1".to_string(), "2".to_string()],
        ),
    );

    registry.insert(
        "EXIF:WhitePoint",
        TagDescriptor::new(
            TagId::new_numeric(0x013e),
            "EXIF:WhitePoint".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Rational,
            "Chromaticity of white point".to_string(),
            vec!["0.3127".to_string(), "0.329".to_string()],
        ),
    );

    registry.insert(
        "EXIF:PrimaryChromaticities",
        TagDescriptor::new(
            TagId::new_numeric(0x013f),
            "EXIF:PrimaryChromaticities".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Rational,
            "Chromaticities of primaries".to_string(),
            vec!["0.64".to_string(), "0.33".to_string()],
        ),
    );

    registry.insert(
        "EXIF:ColorMap",
        TagDescriptor::new(
            TagId::new_numeric(0x0140),
            "EXIF:ColorMap".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Color map for palette images".to_string(),
            vec!["0".to_string(), "255".to_string()],
        ),
    );

    registry.insert(
        "EXIF:HalftoneHints",
        TagDescriptor::new(
            TagId::new_numeric(0x0141),
            "EXIF:HalftoneHints".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Halftone function parameters".to_string(),
            vec!["128".to_string(), "255".to_string()],
        ),
    );

    registry.insert(
        "EXIF:InkSet",
        TagDescriptor::new(
            TagId::new_numeric(0x014c),
            "EXIF:InkSet".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Set of inks used (CMYK or not CMYK)".to_string(),
            vec!["1".to_string(), "2".to_string()],
        ),
    );

    registry.insert(
        "EXIF:InkNames",
        TagDescriptor::new(
            TagId::new_numeric(0x014d),
            "EXIF:InkNames".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::String,
            "Names of each ink".to_string(),
            vec!["Cyan".to_string(), "Magenta".to_string()],
        ),
    );

    registry.insert(
        "EXIF:NumberOfInks",
        TagDescriptor::new(
            TagId::new_numeric(0x014e),
            "EXIF:NumberOfInks".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Number of inks".to_string(),
            vec!["4".to_string(), "6".to_string()],
        ),
    );

    registry.insert(
        "EXIF:DotRange",
        TagDescriptor::new(
            TagId::new_numeric(0x0150),
            "EXIF:DotRange".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Dot gain percentage range".to_string(),
            vec!["0".to_string(), "100".to_string()],
        ),
    );

    registry.insert(
        "EXIF:TargetPrinter",
        TagDescriptor::new(
            TagId::new_numeric(0x0151),
            "EXIF:TargetPrinter".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::String,
            "Target printer description".to_string(),
            vec!["Epson".to_string(), "Canon".to_string()],
        ),
    );

    registry.insert(
        "EXIF:ExtraSamples",
        TagDescriptor::new(
            TagId::new_numeric(0x0152),
            "EXIF:ExtraSamples".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Description of extra components".to_string(),
            vec!["0".to_string(), "1".to_string()],
        ),
    );

    registry.insert(
        "EXIF:SampleFormat",
        TagDescriptor::new(
            TagId::new_numeric(0x0153),
            "EXIF:SampleFormat".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Data format of samples".to_string(),
            vec!["1".to_string(), "2".to_string()],
        ),
    );

    registry.insert(
        "EXIF:SMinSampleValue",
        TagDescriptor::new(
            TagId::new_numeric(0x0154),
            "EXIF:SMinSampleValue".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Minimum sample value".to_string(),
            vec!["0".to_string()],
        ),
    );

    registry.insert(
        "EXIF:SMaxSampleValue",
        TagDescriptor::new(
            TagId::new_numeric(0x0155),
            "EXIF:SMaxSampleValue".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Maximum sample value".to_string(),
            vec!["255".to_string(), "65535".to_string()],
        ),
    );

    registry.insert(
        "EXIF:TransferRange",
        TagDescriptor::new(
            TagId::new_numeric(0x0156),
            "EXIF:TransferRange".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Transfer function range".to_string(),
            vec!["0".to_string(), "255".to_string()],
        ),
    );

    registry.insert(
        "EXIF:ClipPath",
        TagDescriptor::new(
            TagId::new_numeric(0x0157),
            "EXIF:ClipPath".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Binary,
            "Clipping path".to_string(),
            vec!["binary data".to_string()],
        ),
    );

    registry.insert(
        "EXIF:JPEGProc",
        TagDescriptor::new(
            TagId::new_numeric(0x0200),
            "EXIF:JPEGProc".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "JPEG compression process".to_string(),
            vec!["1".to_string(), "14".to_string()],
        ),
    );

    registry.insert(
        "EXIF:JPEGInterchangeFormat",
        TagDescriptor::new(
            TagId::new_numeric(0x0201),
            "EXIF:JPEGInterchangeFormat".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Offset to JPEG SOI marker".to_string(),
            vec!["1024".to_string()],
        ),
    );

    registry.insert(
        "EXIF:JPEGInterchangeFormatLength",
        TagDescriptor::new(
            TagId::new_numeric(0x0202),
            "EXIF:JPEGInterchangeFormatLength".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Bytes of JPEG data".to_string(),
            vec!["65536".to_string()],
        ),
    );

    registry.insert(
        "EXIF:JPEGRestartInterval",
        TagDescriptor::new(
            TagId::new_numeric(0x0203),
            "EXIF:JPEGRestartInterval".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Restart interval length".to_string(),
            vec!["0".to_string(), "8".to_string()],
        ),
    );

    registry.insert(
        "EXIF:JPEGLosslessPredictors",
        TagDescriptor::new(
            TagId::new_numeric(0x0205),
            "EXIF:JPEGLosslessPredictors".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Lossless predictor selection".to_string(),
            vec!["1".to_string()],
        ),
    );

    registry.insert(
        "EXIF:JPEGPointTransforms",
        TagDescriptor::new(
            TagId::new_numeric(0x0206),
            "EXIF:JPEGPointTransforms".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Point transform parameter".to_string(),
            vec!["0".to_string()],
        ),
    );

    registry.insert(
        "EXIF:JPEGQTables",
        TagDescriptor::new(
            TagId::new_numeric(0x0207),
            "EXIF:JPEGQTables".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Offsets to Q tables".to_string(),
            vec!["0".to_string()],
        ),
    );

    registry.insert(
        "EXIF:JPEGDCTables",
        TagDescriptor::new(
            TagId::new_numeric(0x0208),
            "EXIF:JPEGDCTables".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Offsets to DC Huffman tables".to_string(),
            vec!["0".to_string()],
        ),
    );

    registry.insert(
        "EXIF:JPEGACTables",
        TagDescriptor::new(
            TagId::new_numeric(0x0209),
            "EXIF:JPEGACTables".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Offsets to AC Huffman tables".to_string(),
            vec!["0".to_string()],
        ),
    );

    registry.insert(
        "EXIF:YCbCrCoefficients",
        TagDescriptor::new(
            TagId::new_numeric(0x0211),
            "EXIF:YCbCrCoefficients".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Rational,
            "Matrix for RGB to YCbCr".to_string(),
            vec!["0.299".to_string(), "0.587".to_string()],
        ),
    );

    registry.insert(
        "EXIF:ReferenceBlackWhite",
        TagDescriptor::new(
            TagId::new_numeric(0x0214),
            "EXIF:ReferenceBlackWhite".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Rational,
            "Black and white reference values".to_string(),
            vec!["0".to_string(), "255".to_string()],
        ),
    );

    registry.insert(
        "EXIF:RelatedImageFileFormat",
        TagDescriptor::new(
            TagId::new_numeric(0x1000),
            "EXIF:RelatedImageFileFormat".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::String,
            "Related image file format".to_string(),
            vec!["JPEG".to_string(), "TIFF".to_string()],
        ),
    );

    registry.insert(
        "EXIF:RelatedImageWidth",
        TagDescriptor::new(
            TagId::new_numeric(0x1001),
            "EXIF:RelatedImageWidth".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Related image width".to_string(),
            vec!["1024".to_string(), "2048".to_string()],
        ),
    );

    registry.insert(
        "EXIF:RelatedImageHeight",
        TagDescriptor::new(
            TagId::new_numeric(0x1002),
            "EXIF:RelatedImageHeight".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Related image height".to_string(),
            vec!["768".to_string(), "1536".to_string()],
        ),
    );

    registry.insert(
        "EXIF:CFARepeatPatternDim",
        TagDescriptor::new(
            TagId::new_numeric(0x828d),
            "EXIF:CFARepeatPatternDim".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "CFA pattern dimension".to_string(),
            vec!["2".to_string()],
        ),
    );

    registry.insert(
        "EXIF:CFAPattern",
        TagDescriptor::new(
            TagId::new_numeric(0x828e),
            "EXIF:CFAPattern".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Binary,
            "Color filter array pattern".to_string(),
            vec!["RGGB".to_string(), "BGGR".to_string()],
        ),
    );

    registry.insert(
        "EXIF:BatteryLevel",
        TagDescriptor::new(
            TagId::new_numeric(0x828f),
            "EXIF:BatteryLevel".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Rational,
            "Battery level".to_string(),
            vec!["75".to_string(), "100".to_string()],
        ),
    );

    registry.insert(
        "EXIF:IPTC-NAA",
        TagDescriptor::new(
            TagId::new_numeric(0x83bb),
            "EXIF:IPTC-NAA".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Binary,
            "IPTC/NAA metadata".to_string(),
            vec!["binary".to_string()],
        ),
    );

    registry.insert(
        "EXIF:InterColorProfile",
        TagDescriptor::new(
            TagId::new_numeric(0x8773),
            "EXIF:InterColorProfile".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Binary,
            "ICC color profile".to_string(),
            vec!["binary".to_string()],
        ),
    );

    registry.insert(
        "EXIF:SpectralSensitivity",
        TagDescriptor::new(
            TagId::new_numeric(0x8824),
            "EXIF:SpectralSensitivity".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::String,
            "Spectral sensitivity of sensor".to_string(),
            vec!["ISO 25328".to_string()],
        ),
    );

    registry.insert(
        "EXIF:OECF",
        TagDescriptor::new(
            TagId::new_numeric(0x8828),
            "EXIF:OECF".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Binary,
            "Opto-Electric Conversion Function".to_string(),
            vec!["binary".to_string()],
        ),
    );

    registry.insert(
        "EXIF:Interlace",
        TagDescriptor::new(
            TagId::new_numeric(0x8829),
            "EXIF:Interlace".to_string(),
            FormatFamily::EXIF,
            false,
            ValueType::Integer,
            "Interlace indicator".to_string(),
            vec!["0".to_string(), "1".to_string()],
        ),
    );

    registry.insert(
        "EXIF:TimeZoneOffset",
        TagDescriptor::new(
            TagId::new_numeric(0x882a),
            "EXIF:TimeZoneOffset".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Time zone offset from UTC".to_string(),
            vec!["-8".to_string(), "+1".to_string()],
        ),
    );

    registry.insert(
        "EXIF:SelfTimerMode",
        TagDescriptor::new(
            TagId::new_numeric(0x882b),
            "EXIF:SelfTimerMode".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Self timer mode".to_string(),
            vec!["0".to_string(), "10".to_string()],
        ),
    );

    registry.insert(
        "EXIF:SensitivityType",
        TagDescriptor::new(
            TagId::new_numeric(0x8830),
            "EXIF:SensitivityType".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Sensitivity type (ISO speed)".to_string(),
            vec!["1".to_string(), "2".to_string()],
        ),
    );

    registry.insert(
        "EXIF:StandardOutputSensitivity",
        TagDescriptor::new(
            TagId::new_numeric(0x8831),
            "EXIF:StandardOutputSensitivity".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Standard output sensitivity".to_string(),
            vec!["100".to_string(), "400".to_string()],
        ),
    );

    registry.insert(
        "EXIF:RecommendedExposureIndex",
        TagDescriptor::new(
            TagId::new_numeric(0x8832),
            "EXIF:RecommendedExposureIndex".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Recommended exposure index".to_string(),
            vec!["100".to_string(), "400".to_string()],
        ),
    );

    registry.insert(
        "EXIF:ISOSpeed",
        TagDescriptor::new(
            TagId::new_numeric(0x8833),
            "EXIF:ISOSpeed".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "ISO speed value".to_string(),
            vec!["100".to_string(), "1600".to_string()],
        ),
    );

    registry.insert(
        "EXIF:ISOSpeedLatitudeyyy",
        TagDescriptor::new(
            TagId::new_numeric(0x8834),
            "EXIF:ISOSpeedLatitudeyyy".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "ISO speed latitude yyy".to_string(),
            vec!["100".to_string()],
        ),
    );

    registry.insert(
        "EXIF:ISOSpeedLatitudezzz",
        TagDescriptor::new(
            TagId::new_numeric(0x8835),
            "EXIF:ISOSpeedLatitudezzz".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "ISO speed latitude zzz".to_string(),
            vec!["100".to_string()],
        ),
    );

    registry.insert(
        "EXIF:HumidityValue",
        TagDescriptor::new(
            TagId::new_numeric(0x9401),
            "EXIF:HumidityValue".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Rational,
            "Ambient humidity percentage".to_string(),
            vec!["50".to_string(), "75".to_string()],
        ),
    );

    registry.insert(
        "EXIF:Pressure",
        TagDescriptor::new(
            TagId::new_numeric(0x9402),
            "EXIF:Pressure".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Rational,
            "Ambient pressure in kPa".to_string(),
            vec!["101.3".to_string(), "98.5".to_string()],
        ),
    );

    registry.insert(
        "EXIF:WaterDepth",
        TagDescriptor::new(
            TagId::new_numeric(0x9403),
            "EXIF:WaterDepth".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Rational,
            "Water depth in meters".to_string(),
            vec!["5.5".to_string(), "10.2".to_string()],
        ),
    );

    registry.insert(
        "EXIF:Acceleration",
        TagDescriptor::new(
            TagId::new_numeric(0x9404),
            "EXIF:Acceleration".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Rational,
            "Camera acceleration in mGal".to_string(),
            vec!["980".to_string()],
        ),
    );

    registry.insert(
        "EXIF:CameraElevationAngle",
        TagDescriptor::new(
            TagId::new_numeric(0x9405),
            "EXIF:CameraElevationAngle".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Rational,
            "Camera elevation angle".to_string(),
            vec!["0".to_string(), "45".to_string()],
        ),
    );

    registry.insert(
        "EXIF:ImageUniqueID",
        TagDescriptor::new(
            TagId::new_numeric(0xa420),
            "EXIF:ImageUniqueID".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::String,
            "Unique image identifier".to_string(),
            vec!["1234567890ABCDEF".to_string()],
        ),
    );

    registry.insert(
        "EXIF:OwnerName",
        TagDescriptor::new(
            TagId::new_numeric(0xa430),
            "EXIF:OwnerName".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::String,
            "Camera owner name".to_string(),
            vec!["John Doe".to_string()],
        ),
    );

    registry.insert(
        "EXIF:BodySerialNumber",
        TagDescriptor::new(
            TagId::new_numeric(0xa431),
            "EXIF:BodySerialNumber".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::String,
            "Camera body serial number".to_string(),
            vec!["1234567890".to_string()],
        ),
    );

    registry.insert(
        "EXIF:LensSpecification",
        TagDescriptor::new(
            TagId::new_numeric(0xa432),
            "EXIF:LensSpecification".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Rational,
            "Lens specification".to_string(),
            vec!["24".to_string(), "70".to_string()],
        ),
    );

    registry.insert(
        "EXIF:GainControl",
        TagDescriptor::new(
            TagId::new_numeric(0xa407),
            "EXIF:GainControl".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Gain control (0=None, 1=Low, 2=High)".to_string(),
            vec!["0".to_string(), "1".to_string()],
        ),
    );

    registry.insert(
        "EXIF:SubjectDistanceRange",
        TagDescriptor::new(
            TagId::new_numeric(0xa40c),
            "EXIF:SubjectDistanceRange".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Subject distance range (1=Macro, 2=Close, 3=Distant)".to_string(),
            vec!["2".to_string()],
        ),
    );

    registry.insert(
        "EXIF:ImageNumber",
        TagDescriptor::new(
            TagId::new_numeric(0xa500),
            "EXIF:ImageNumber".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Image number in sequence".to_string(),
            vec!["1".to_string(), "100".to_string()],
        ),
    );

    registry.insert(
        "EXIF:SecurityClassification",
        TagDescriptor::new(
            TagId::new_numeric(0x9212),
            "EXIF:SecurityClassification".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::String,
            "Security classification".to_string(),
            vec!["Unclassified".to_string()],
        ),
    );

    registry.insert(
        "EXIF:ImageHistory",
        TagDescriptor::new(
            TagId::new_numeric(0x9213),
            "EXIF:ImageHistory".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::String,
            "Image history".to_string(),
            vec!["Captured, Edited".to_string()],
        ),
    );

    registry.insert(
        "EXIF:SubjectLocation",
        TagDescriptor::new(
            TagId::new_numeric(0xa214),
            "EXIF:SubjectLocation".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Subject location in image".to_string(),
            vec!["1024".to_string(), "768".to_string()],
        ),
    );

    registry.insert(
        "EXIF:ExposureIndex",
        TagDescriptor::new(
            TagId::new_numeric(0xa215),
            "EXIF:ExposureIndex".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Rational,
            "Exposure index".to_string(),
            vec!["100".to_string()],
        ),
    );

    registry.insert(
        "EXIF:TIFFEPStandardID",
        TagDescriptor::new(
            TagId::new_numeric(0x9216),
            "EXIF:TIFFEPStandardID".to_string(),
            FormatFamily::EXIF,
            false,
            ValueType::Binary,
            "TIFF/EP standard version".to_string(),
            vec!["1 0 0 0".to_string()],
        ),
    );

    registry.insert(
        "EXIF:CFAPattern2",
        TagDescriptor::new(
            TagId::new_numeric(0xa302),
            "EXIF:CFAPattern2".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Binary,
            "CFA pattern".to_string(),
            vec!["RGGB".to_string()],
        ),
    );

    registry.insert(
        "EXIF:GammaValue",
        TagDescriptor::new(
            TagId::new_numeric(0xa500),
            "EXIF:GammaValue".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Rational,
            "Gamma value".to_string(),
            vec!["2.2".to_string(), "1.8".to_string()],
        ),
    );

    registry.insert(
        "EXIF:OffsetTime",
        TagDescriptor::new(
            TagId::new_numeric(0x9010),
            "EXIF:OffsetTime".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::String,
            "Offset time for DateTime".to_string(),
            vec!["-08:00".to_string(), "+01:00".to_string()],
        ),
    );

    registry.insert(
        "EXIF:OffsetTimeOriginal",
        TagDescriptor::new(
            TagId::new_numeric(0x9011),
            "EXIF:OffsetTimeOriginal".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::String,
            "Offset time for DateTimeOriginal".to_string(),
            vec!["-08:00".to_string()],
        ),
    );

    registry.insert(
        "EXIF:OffsetTimeDigitized",
        TagDescriptor::new(
            TagId::new_numeric(0x9012),
            "EXIF:OffsetTimeDigitized".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::String,
            "Offset time for DateTimeDigitized".to_string(),
            vec!["-08:00".to_string()],
        ),
    );

    registry.insert(
        "EXIF:Temperature",
        TagDescriptor::new(
            TagId::new_numeric(0x9400),
            "EXIF:Temperature".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Rational,
            "Ambient temperature in Celsius".to_string(),
            vec!["20".to_string(), "25".to_string()],
        ),
    );

    registry.insert(
        "EXIF:Humidity",
        TagDescriptor::new(
            TagId::new_numeric(0x9401),
            "EXIF:Humidity".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Rational,
            "Ambient humidity".to_string(),
            vec!["50".to_string(), "75".to_string()],
        ),
    );

    registry.insert(
        "EXIF:CompositeImage",
        TagDescriptor::new(
            TagId::new_numeric(0xa460),
            "EXIF:CompositeImage".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Composite image indicator".to_string(),
            vec!["0".to_string(), "1".to_string()],
        ),
    );

    registry.insert(
        "EXIF:SourceImageNumberOfCompositeImage",
        TagDescriptor::new(
            TagId::new_numeric(0xa461),
            "EXIF:SourceImageNumberOfCompositeImage".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Source images in composite".to_string(),
            vec!["2".to_string(), "5".to_string()],
        ),
    );

    registry.insert(
        "EXIF:SourceExposureTimesOfCompositeImage",
        TagDescriptor::new(
            TagId::new_numeric(0xa462),
            "EXIF:SourceExposureTimesOfCompositeImage".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Binary,
            "Exposure times of source images".to_string(),
            vec!["binary".to_string()],
        ),
    );

    registry.insert(
        "EXIF:SubSecTimeDigitized",
        TagDescriptor::new(
            TagId::new_numeric(0x9292),
            "EXIF:SubSecTimeDigitized".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::String,
            "Fractional seconds for DateTimeDigitized".to_string(),
            vec!["123".to_string(), "456".to_string()],
        ),
    );

    registry.insert(
        "EXIF:FlashEnergy",
        TagDescriptor::new(
            TagId::new_numeric(0xa20b),
            "EXIF:FlashEnergy".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Rational,
            "Flash energy in BCPS".to_string(),
            vec!["50".to_string(), "100".to_string()],
        ),
    );

    registry.insert(
        "EXIF:SpatialFrequencyResponse",
        TagDescriptor::new(
            TagId::new_numeric(0xa20c),
            "EXIF:SpatialFrequencyResponse".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Binary,
            "Spatial frequency response".to_string(),
            vec!["binary".to_string()],
        ),
    );

    registry.insert(
        "EXIF:FocalPlaneXResolution",
        TagDescriptor::new(
            TagId::new_numeric(0xa20e),
            "EXIF:FocalPlaneXResolution".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Rational,
            "Focal plane X resolution".to_string(),
            vec!["72".to_string(), "300".to_string()],
        ),
    );

    registry.insert(
        "EXIF:FocalPlaneYResolution",
        TagDescriptor::new(
            TagId::new_numeric(0xa20f),
            "EXIF:FocalPlaneYResolution".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Rational,
            "Focal plane Y resolution".to_string(),
            vec!["72".to_string(), "300".to_string()],
        ),
    );

    registry.insert(
        "EXIF:FocalPlaneResolutionUnit",
        TagDescriptor::new(
            TagId::new_numeric(0xa210),
            "EXIF:FocalPlaneResolutionUnit".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Focal plane resolution unit".to_string(),
            vec!["2".to_string(), "3".to_string()],
        ),
    );

    registry.insert(
        "EXIF:SubjectArea",
        TagDescriptor::new(
            TagId::new_numeric(0xa214),
            "EXIF:SubjectArea".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Subject area in image".to_string(),
            vec!["1024".to_string(), "768".to_string()],
        ),
    );

    registry.insert(
        "EXIF:MakerNote",
        TagDescriptor::new(
            TagId::new_numeric(0x927c),
            "EXIF:MakerNote".to_string(),
            FormatFamily::EXIF,
            false,
            ValueType::Binary,
            "Manufacturer-specific metadata".to_string(),
            vec!["binary".to_string()],
        ),
    );

    registry.insert(
        "EXIF:ExposureMode",
        TagDescriptor::new(
            TagId::new_numeric(0xa402),
            "EXIF:ExposureMode".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Exposure mode (0=Auto, 1=Manual, 2=Bracket)".to_string(),
            vec!["0".to_string(), "1".to_string()],
        ),
    );

    registry.insert(
        "EXIF:DigitalZoomRatio",
        TagDescriptor::new(
            TagId::new_numeric(0xa404),
            "EXIF:DigitalZoomRatio".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Rational,
            "Digital zoom ratio".to_string(),
            vec!["1".to_string(), "2".to_string()],
        ),
    );

    registry.insert(
        "EXIF:SceneType",
        TagDescriptor::new(
            TagId::new_numeric(0xa301),
            "EXIF:SceneType".to_string(),
            FormatFamily::EXIF,
            false,
            ValueType::Binary,
            "Scene type (1=Directly photographed)".to_string(),
            vec!["1".to_string()],
        ),
    );

    registry.insert(
        "EXIF:CustomRendered",
        TagDescriptor::new(
            TagId::new_numeric(0xa401),
            "EXIF:CustomRendered".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Custom rendering (0=Normal, 1=Custom)".to_string(),
            vec!["0".to_string()],
        ),
    );

    registry.insert(
        "EXIF:DeviceSettingDescription",
        TagDescriptor::new(
            TagId::new_numeric(0xa40b),
            "EXIF:DeviceSettingDescription".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Binary,
            "Device settings description".to_string(),
            vec!["binary".to_string()],
        ),
    );

    registry.insert(
        "EXIF:SubjectDistanceRange",
        TagDescriptor::new(
            TagId::new_numeric(0xa40c),
            "EXIF:SubjectDistanceRange".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Subject distance range".to_string(),
            vec!["2".to_string()],
        ),
    );

    registry.insert(
        "EXIF:LensInfo",
        TagDescriptor::new(
            TagId::new_numeric(0xa432),
            "EXIF:LensInfo".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Rational,
            "Lens info (min/max focal, aperture)".to_string(),
            vec!["24".to_string(), "70".to_string()],
        ),
    );

    // ===========================
    // MAKER NOTES TAGS (90 total)
    // ===========================

    // --- Canon MakerNotes (30 tags) ---
    registry.insert(
        "Canon:ModelID",
        TagDescriptor::new(
            TagId::new_numeric(0x0010),
            "Canon:ModelID".to_string(),
            FormatFamily::MakerNotes,
            false,
            ValueType::Integer,
            "Canon model ID number".to_string(),
            vec!["0x80000001".to_string(), "0x80000287".to_string()],
        ),
    );

    registry.insert(
        "Canon:CanonFirmwareVersion",
        TagDescriptor::new(
            TagId::new_numeric(0x0007),
            "Canon:CanonFirmwareVersion".to_string(),
            FormatFamily::MakerNotes,
            false,
            ValueType::String,
            "Canon firmware version".to_string(),
            vec!["1.0.0".to_string(), "1.2.5".to_string()],
        ),
    );

    registry.insert(
        "Canon:CanonImageType",
        TagDescriptor::new(
            TagId::new_numeric(0x0006),
            "Canon:CanonImageType".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::String,
            "Canon image type".to_string(),
            vec![
                "IMG:EOS 5D Mark IV JPEG".to_string(),
                "CRW:EOS-1D X RAW".to_string(),
            ],
        ),
    );

    registry.insert(
        "Canon:OwnerName",
        TagDescriptor::new(
            TagId::new_numeric(0x0009),
            "Canon:OwnerName".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::String,
            "Camera owner name".to_string(),
            vec!["John Doe".to_string()],
        ),
    );

    registry.insert(
        "Canon:SerialNumber",
        TagDescriptor::new(
            TagId::new_numeric(0x000c),
            "Canon:SerialNumber".to_string(),
            FormatFamily::MakerNotes,
            false,
            ValueType::Integer,
            "Camera serial number".to_string(),
            vec!["1234567890".to_string()],
        ),
    );

    registry.insert(
        "Canon:InternalSerialNumber",
        TagDescriptor::new(
            TagId::new_numeric(0x0096),
            "Canon:InternalSerialNumber".to_string(),
            FormatFamily::MakerNotes,
            false,
            ValueType::String,
            "Internal serial number".to_string(),
            vec!["ABC123XYZ".to_string()],
        ),
    );

    registry.insert(
        "Canon:LensModel",
        TagDescriptor::new(
            TagId::new_numeric(0x0095),
            "Canon:LensModel".to_string(),
            FormatFamily::MakerNotes,
            false,
            ValueType::String,
            "Lens model name".to_string(),
            vec!["EF 24-70mm f/2.8L II USM".to_string()],
        ),
    );

    registry.insert(
        "Canon:ShortFocalLength",
        TagDescriptor::new(
            TagId::new_numeric(0x0002),
            "Canon:ShortFocalLength".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Integer,
            "Short focal length in focal units".to_string(),
            vec!["24".to_string()],
        ),
    );

    registry.insert(
        "Canon:LongFocalLength",
        TagDescriptor::new(
            TagId::new_numeric(0x0003),
            "Canon:LongFocalLength".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Integer,
            "Long focal length in focal units".to_string(),
            vec!["70".to_string()],
        ),
    );

    registry.insert(
        "Canon:FocalUnits",
        TagDescriptor::new(
            TagId::new_numeric(0x0025),
            "Canon:FocalUnits".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Integer,
            "Focal length units per mm".to_string(),
            vec!["1".to_string()],
        ),
    );

    registry.insert(
        "Canon:MaxAperture",
        TagDescriptor::new(
            TagId::new_numeric(0x0004),
            "Canon:MaxAperture".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Integer,
            "Maximum aperture".to_string(),
            vec!["28".to_string(), "56".to_string()],
        ),
    );

    registry.insert(
        "Canon:MinAperture",
        TagDescriptor::new(
            TagId::new_numeric(0x0005),
            "Canon:MinAperture".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Integer,
            "Minimum aperture".to_string(),
            vec!["320".to_string()],
        ),
    );

    registry.insert(
        "Canon:FlashBits",
        TagDescriptor::new(
            TagId::new_numeric(0x0004),
            "Canon:FlashBits".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Integer,
            "Flash status bits".to_string(),
            vec!["0".to_string(), "16".to_string()],
        ),
    );

    registry.insert(
        "Canon:FocusMode",
        TagDescriptor::new(
            TagId::new_numeric(0x0007),
            "Canon:FocusMode".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Integer,
            "Focus mode (0=One-shot, 1=AI Servo, 2=AI Focus)".to_string(),
            vec!["0".to_string(), "1".to_string()],
        ),
    );

    registry.insert(
        "Canon:AFPoint",
        TagDescriptor::new(
            TagId::new_numeric(0x000e),
            "Canon:AFPoint".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Integer,
            "Active AF point".to_string(),
            vec!["3".to_string(), "5".to_string()],
        ),
    );

    registry.insert(
        "Canon:CanonExposureMode",
        TagDescriptor::new(
            TagId::new_numeric(0x0014),
            "Canon:CanonExposureMode".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Integer,
            "Canon exposure mode".to_string(),
            vec!["0".to_string(), "1".to_string()],
        ),
    );

    registry.insert(
        "Canon:LongExposureNoiseReduction",
        TagDescriptor::new(
            TagId::new_numeric(0x0001),
            "Canon:LongExposureNoiseReduction".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Integer,
            "Long exposure NR (0=Off, 1=On)".to_string(),
            vec!["0".to_string(), "1".to_string()],
        ),
    );

    registry.insert(
        "Canon:WBBracketMode",
        TagDescriptor::new(
            TagId::new_numeric(0x0009),
            "Canon:WBBracketMode".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Integer,
            "WB bracket mode".to_string(),
            vec!["0".to_string(), "1".to_string()],
        ),
    );

    registry.insert(
        "Canon:FilterEffect",
        TagDescriptor::new(
            TagId::new_numeric(0x0027),
            "Canon:FilterEffect".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Integer,
            "Filter effect".to_string(),
            vec!["0".to_string()],
        ),
    );

    registry.insert(
        "Canon:ToningEffect",
        TagDescriptor::new(
            TagId::new_numeric(0x0028),
            "Canon:ToningEffect".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Integer,
            "Toning effect".to_string(),
            vec!["0".to_string()],
        ),
    );

    registry.insert(
        "Canon:MacroMode",
        TagDescriptor::new(
            TagId::new_numeric(0x0001),
            "Canon:MacroMode".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Integer,
            "Macro mode (1=Macro, 2=Normal)".to_string(),
            vec!["1".to_string(), "2".to_string()],
        ),
    );

    registry.insert(
        "Canon:SelfTimer",
        TagDescriptor::new(
            TagId::new_numeric(0x0002),
            "Canon:SelfTimer".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Integer,
            "Self timer delay in 0.1s".to_string(),
            vec!["0".to_string(), "100".to_string()],
        ),
    );

    registry.insert(
        "Canon:Quality",
        TagDescriptor::new(
            TagId::new_numeric(0x0003),
            "Canon:Quality".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Integer,
            "Image quality setting".to_string(),
            vec!["2".to_string(), "3".to_string()],
        ),
    );

    registry.insert(
        "Canon:CanonFlashMode",
        TagDescriptor::new(
            TagId::new_numeric(0x0004),
            "Canon:CanonFlashMode".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Integer,
            "Flash mode".to_string(),
            vec!["0".to_string(), "1".to_string()],
        ),
    );

    registry.insert(
        "Canon:ContinuousDrive",
        TagDescriptor::new(
            TagId::new_numeric(0x0005),
            "Canon:ContinuousDrive".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Integer,
            "Continuous drive mode".to_string(),
            vec!["0".to_string(), "1".to_string()],
        ),
    );

    registry.insert(
        "Canon:FocusRange",
        TagDescriptor::new(
            TagId::new_numeric(0x0007),
            "Canon:FocusRange".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Integer,
            "Focus range setting".to_string(),
            vec!["1".to_string()],
        ),
    );

    registry.insert(
        "Canon:ImageSize",
        TagDescriptor::new(
            TagId::new_numeric(0x000a),
            "Canon:ImageSize".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Integer,
            "Image size setting".to_string(),
            vec!["0".to_string(), "1".to_string()],
        ),
    );

    registry.insert(
        "Canon:EasyMode",
        TagDescriptor::new(
            TagId::new_numeric(0x000b),
            "Canon:EasyMode".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Integer,
            "Easy shooting mode".to_string(),
            vec!["0".to_string(), "1".to_string()],
        ),
    );

    registry.insert(
        "Canon:DigitalZoom",
        TagDescriptor::new(
            TagId::new_numeric(0x000c),
            "Canon:DigitalZoom".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Integer,
            "Digital zoom setting".to_string(),
            vec!["0".to_string(), "1".to_string()],
        ),
    );

    registry.insert(
        "Canon:Contrast",
        TagDescriptor::new(
            TagId::new_numeric(0x000d),
            "Canon:Contrast".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Integer,
            "Contrast setting".to_string(),
            vec!["0".to_string(), "1".to_string()],
        ),
    );

    // --- Nikon MakerNotes (30 tags) ---
    registry.insert(
        "Nikon:MakerNoteVersion",
        TagDescriptor::new(
            TagId::new_numeric(0x0001),
            "Nikon:MakerNoteVersion".to_string(),
            FormatFamily::MakerNotes,
            false,
            ValueType::String,
            "Nikon maker note version".to_string(),
            vec!["0210".to_string(), "0220".to_string()],
        ),
    );

    registry.insert(
        "Nikon:ISO2",
        TagDescriptor::new(
            TagId::new_numeric(0x0002),
            "Nikon:ISO2".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Integer,
            "ISO setting".to_string(),
            vec!["100".to_string(), "400".to_string()],
        ),
    );

    registry.insert(
        "Nikon:ColorMode",
        TagDescriptor::new(
            TagId::new_numeric(0x0003),
            "Nikon:ColorMode".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::String,
            "Color mode".to_string(),
            vec!["MODE1".to_string(), "MODE2".to_string()],
        ),
    );

    registry.insert(
        "Nikon:Quality",
        TagDescriptor::new(
            TagId::new_numeric(0x0004),
            "Nikon:Quality".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::String,
            "Image quality".to_string(),
            vec!["FINE".to_string(), "NORMAL".to_string()],
        ),
    );

    registry.insert(
        "Nikon:WhiteBalance",
        TagDescriptor::new(
            TagId::new_numeric(0x0005),
            "Nikon:WhiteBalance".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::String,
            "White balance".to_string(),
            vec!["AUTO".to_string(), "DAYLIGHT".to_string()],
        ),
    );

    registry.insert(
        "Nikon:Sharpness",
        TagDescriptor::new(
            TagId::new_numeric(0x0006),
            "Nikon:Sharpness".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::String,
            "Sharpness setting".to_string(),
            vec!["AUTO".to_string(), "NORMAL".to_string()],
        ),
    );

    registry.insert(
        "Nikon:FocusMode",
        TagDescriptor::new(
            TagId::new_numeric(0x0007),
            "Nikon:FocusMode".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::String,
            "Focus mode".to_string(),
            vec!["AF-S".to_string(), "AF-C".to_string()],
        ),
    );

    registry.insert(
        "Nikon:FlashSetting",
        TagDescriptor::new(
            TagId::new_numeric(0x0008),
            "Nikon:FlashSetting".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::String,
            "Flash setting".to_string(),
            vec!["NORMAL".to_string(), "RED-EYE".to_string()],
        ),
    );

    registry.insert(
        "Nikon:FlashType",
        TagDescriptor::new(
            TagId::new_numeric(0x0009),
            "Nikon:FlashType".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::String,
            "Flash type".to_string(),
            vec!["Built-in".to_string(), "External".to_string()],
        ),
    );

    registry.insert(
        "Nikon:WhiteBalanceFine",
        TagDescriptor::new(
            TagId::new_numeric(0x000b),
            "Nikon:WhiteBalanceFine".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Integer,
            "White balance fine tune".to_string(),
            vec!["0".to_string(), "+1".to_string()],
        ),
    );

    registry.insert(
        "Nikon:WB_RBLevels",
        TagDescriptor::new(
            TagId::new_numeric(0x000c),
            "Nikon:WB_RBLevels".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Rational,
            "WB red/blue levels".to_string(),
            vec!["2.5".to_string(), "1.8".to_string()],
        ),
    );

    registry.insert(
        "Nikon:ProgramShift",
        TagDescriptor::new(
            TagId::new_numeric(0x000d),
            "Nikon:ProgramShift".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Rational,
            "Program shift value".to_string(),
            vec!["0".to_string()],
        ),
    );

    registry.insert(
        "Nikon:ExposureDifference",
        TagDescriptor::new(
            TagId::new_numeric(0x000e),
            "Nikon:ExposureDifference".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Rational,
            "Exposure difference".to_string(),
            vec!["0".to_string(), "0.3".to_string()],
        ),
    );

    registry.insert(
        "Nikon:ISOSelection",
        TagDescriptor::new(
            TagId::new_numeric(0x000f),
            "Nikon:ISOSelection".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::String,
            "ISO selection".to_string(),
            vec!["AUTO".to_string(), "MANUAL".to_string()],
        ),
    );

    registry.insert(
        "Nikon:DataDump",
        TagDescriptor::new(
            TagId::new_numeric(0x0010),
            "Nikon:DataDump".to_string(),
            FormatFamily::MakerNotes,
            false,
            ValueType::Binary,
            "Nikon data dump".to_string(),
            vec!["binary".to_string()],
        ),
    );

    registry.insert(
        "Nikon:PreviewIFD",
        TagDescriptor::new(
            TagId::new_numeric(0x0011),
            "Nikon:PreviewIFD".to_string(),
            FormatFamily::MakerNotes,
            false,
            ValueType::Integer,
            "Preview image IFD pointer".to_string(),
            vec!["0".to_string()],
        ),
    );

    registry.insert(
        "Nikon:FlashExposureComp",
        TagDescriptor::new(
            TagId::new_numeric(0x0012),
            "Nikon:FlashExposureComp".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Rational,
            "Flash exposure compensation".to_string(),
            vec!["0".to_string(), "-0.7".to_string()],
        ),
    );

    registry.insert(
        "Nikon:ISOSetting",
        TagDescriptor::new(
            TagId::new_numeric(0x0013),
            "Nikon:ISOSetting".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Integer,
            "ISO setting value".to_string(),
            vec!["100".to_string(), "1600".to_string()],
        ),
    );

    registry.insert(
        "Nikon:ColorBalanceA",
        TagDescriptor::new(
            TagId::new_numeric(0x0014),
            "Nikon:ColorBalanceA".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Binary,
            "Color balance A data".to_string(),
            vec!["binary".to_string()],
        ),
    );

    registry.insert(
        "Nikon:ImageBoundary",
        TagDescriptor::new(
            TagId::new_numeric(0x0016),
            "Nikon:ImageBoundary".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Integer,
            "Image boundary".to_string(),
            vec!["0".to_string()],
        ),
    );

    registry.insert(
        "Nikon:ExternalFlashExposureComp",
        TagDescriptor::new(
            TagId::new_numeric(0x0017),
            "Nikon:ExternalFlashExposureComp".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Rational,
            "External flash exposure comp".to_string(),
            vec!["0".to_string()],
        ),
    );

    registry.insert(
        "Nikon:FlashExposureBracketValue",
        TagDescriptor::new(
            TagId::new_numeric(0x0018),
            "Nikon:FlashExposureBracketValue".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Rational,
            "Flash bracket value".to_string(),
            vec!["0".to_string()],
        ),
    );

    registry.insert(
        "Nikon:ExposureTuning",
        TagDescriptor::new(
            TagId::new_numeric(0x0019),
            "Nikon:ExposureTuning".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Rational,
            "Exposure tuning".to_string(),
            vec!["0".to_string()],
        ),
    );

    registry.insert(
        "Nikon:LensType",
        TagDescriptor::new(
            TagId::new_numeric(0x0083),
            "Nikon:LensType".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Integer,
            "Lens type code".to_string(),
            vec!["0".to_string(), "1".to_string()],
        ),
    );

    registry.insert(
        "Nikon:Lens",
        TagDescriptor::new(
            TagId::new_numeric(0x0084),
            "Nikon:Lens".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Rational,
            "Lens info".to_string(),
            vec!["50".to_string(), "1.8".to_string()],
        ),
    );

    registry.insert(
        "Nikon:ManualFocusDistance",
        TagDescriptor::new(
            TagId::new_numeric(0x0085),
            "Nikon:ManualFocusDistance".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Rational,
            "Manual focus distance".to_string(),
            vec!["1.5".to_string(), "inf".to_string()],
        ),
    );

    registry.insert(
        "Nikon:DigitalZoom",
        TagDescriptor::new(
            TagId::new_numeric(0x0086),
            "Nikon:DigitalZoom".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Rational,
            "Digital zoom ratio".to_string(),
            vec!["1".to_string(), "2".to_string()],
        ),
    );

    registry.insert(
        "Nikon:FlashMode",
        TagDescriptor::new(
            TagId::new_numeric(0x0087),
            "Nikon:FlashMode".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Integer,
            "Flash mode".to_string(),
            vec!["0".to_string(), "1".to_string()],
        ),
    );

    registry.insert(
        "Nikon:AFInfo",
        TagDescriptor::new(
            TagId::new_numeric(0x0088),
            "Nikon:AFInfo".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Binary,
            "AF information".to_string(),
            vec!["binary".to_string()],
        ),
    );

    registry.insert(
        "Nikon:ShootingMode",
        TagDescriptor::new(
            TagId::new_numeric(0x0089),
            "Nikon:ShootingMode".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Integer,
            "Shooting mode".to_string(),
            vec!["0".to_string(), "1".to_string()],
        ),
    );

    // --- Sony MakerNotes (30 tags) ---
    registry.insert(
        "Sony:SonyModelID",
        TagDescriptor::new(
            TagId::new_numeric(0x0010),
            "Sony:SonyModelID".to_string(),
            FormatFamily::MakerNotes,
            false,
            ValueType::Integer,
            "Sony model ID".to_string(),
            vec!["2".to_string(), "256".to_string()],
        ),
    );

    registry.insert(
        "Sony:AFMode",
        TagDescriptor::new(
            TagId::new_numeric(0x0114),
            "Sony:AFMode".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Integer,
            "AF mode".to_string(),
            vec!["0".to_string(), "1".to_string()],
        ),
    );

    registry.insert(
        "Sony:AFIlluminator",
        TagDescriptor::new(
            TagId::new_numeric(0x0115),
            "Sony:AFIlluminator".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Integer,
            "AF illuminator".to_string(),
            vec!["0".to_string(), "1".to_string()],
        ),
    );

    registry.insert(
        "Sony:JPEGQuality",
        TagDescriptor::new(
            TagId::new_numeric(0x0102),
            "Sony:JPEGQuality".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Integer,
            "JPEG quality level".to_string(),
            vec!["2".to_string(), "3".to_string()],
        ),
    );

    registry.insert(
        "Sony:FlashLevel",
        TagDescriptor::new(
            TagId::new_numeric(0x0111),
            "Sony:FlashLevel".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Integer,
            "Flash level".to_string(),
            vec!["0".to_string(), "1".to_string()],
        ),
    );

    registry.insert(
        "Sony:ReleaseMode",
        TagDescriptor::new(
            TagId::new_numeric(0x0112),
            "Sony:ReleaseMode".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Integer,
            "Release mode".to_string(),
            vec!["0".to_string(), "1".to_string()],
        ),
    );

    registry.insert(
        "Sony:SequenceNumber",
        TagDescriptor::new(
            TagId::new_numeric(0x0113),
            "Sony:SequenceNumber".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Integer,
            "Sequence number".to_string(),
            vec!["0".to_string(), "1".to_string()],
        ),
    );

    registry.insert(
        "Sony:WhiteBalanceFineTune",
        TagDescriptor::new(
            TagId::new_numeric(0x0116),
            "Sony:WhiteBalanceFineTune".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Integer,
            "WB fine tune value".to_string(),
            vec!["0".to_string(), "+3".to_string()],
        ),
    );

    registry.insert(
        "Sony:ColorTemperature",
        TagDescriptor::new(
            TagId::new_numeric(0x0117),
            "Sony:ColorTemperature".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Integer,
            "Color temperature in K".to_string(),
            vec!["5500".to_string(), "6500".to_string()],
        ),
    );

    registry.insert(
        "Sony:ColorCompensationFilter",
        TagDescriptor::new(
            TagId::new_numeric(0x0118),
            "Sony:ColorCompensationFilter".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Integer,
            "CC filter value".to_string(),
            vec!["0".to_string()],
        ),
    );

    registry.insert(
        "Sony:SceneMode",
        TagDescriptor::new(
            TagId::new_numeric(0x0119),
            "Sony:SceneMode".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Integer,
            "Scene mode".to_string(),
            vec!["0".to_string(), "1".to_string()],
        ),
    );

    registry.insert(
        "Sony:ZoneMatching",
        TagDescriptor::new(
            TagId::new_numeric(0x011a),
            "Sony:ZoneMatching".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Integer,
            "Zone matching mode".to_string(),
            vec!["0".to_string(), "1".to_string()],
        ),
    );

    registry.insert(
        "Sony:DynamicRangeOptimizer",
        TagDescriptor::new(
            TagId::new_numeric(0x011b),
            "Sony:DynamicRangeOptimizer".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Integer,
            "DRO setting".to_string(),
            vec!["0".to_string(), "1".to_string()],
        ),
    );

    registry.insert(
        "Sony:ImageStabilization",
        TagDescriptor::new(
            TagId::new_numeric(0x011c),
            "Sony:ImageStabilization".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Integer,
            "Image stabilization".to_string(),
            vec!["0".to_string(), "1".to_string()],
        ),
    );

    registry.insert(
        "Sony:LensID",
        TagDescriptor::new(
            TagId::new_numeric(0x011d),
            "Sony:LensID".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Integer,
            "Lens ID number".to_string(),
            vec!["0".to_string(), "1".to_string()],
        ),
    );

    registry.insert(
        "Sony:MinoltaMakerNote",
        TagDescriptor::new(
            TagId::new_numeric(0x0114),
            "Sony:MinoltaMakerNote".to_string(),
            FormatFamily::MakerNotes,
            false,
            ValueType::Binary,
            "Minolta maker note data".to_string(),
            vec!["binary".to_string()],
        ),
    );

    registry.insert(
        "Sony:ColorMode",
        TagDescriptor::new(
            TagId::new_numeric(0x0115),
            "Sony:ColorMode".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Integer,
            "Color mode".to_string(),
            vec!["0".to_string(), "1".to_string()],
        ),
    );

    registry.insert(
        "Sony:FullImageSize",
        TagDescriptor::new(
            TagId::new_numeric(0x0116),
            "Sony:FullImageSize".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Integer,
            "Full image size".to_string(),
            vec!["3000x2000".to_string()],
        ),
    );

    registry.insert(
        "Sony:PreviewImageSize",
        TagDescriptor::new(
            TagId::new_numeric(0x0117),
            "Sony:PreviewImageSize".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Integer,
            "Preview image size".to_string(),
            vec!["640x480".to_string()],
        ),
    );

    registry.insert(
        "Sony:Macro",
        TagDescriptor::new(
            TagId::new_numeric(0x0118),
            "Sony:Macro".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Integer,
            "Macro mode".to_string(),
            vec!["0".to_string(), "1".to_string()],
        ),
    );

    registry.insert(
        "Sony:ExposureMode",
        TagDescriptor::new(
            TagId::new_numeric(0x0119),
            "Sony:ExposureMode".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Integer,
            "Exposure mode".to_string(),
            vec!["0".to_string(), "1".to_string()],
        ),
    );

    registry.insert(
        "Sony:Quality2",
        TagDescriptor::new(
            TagId::new_numeric(0x011a),
            "Sony:Quality2".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Integer,
            "Quality setting 2".to_string(),
            vec!["0".to_string(), "1".to_string()],
        ),
    );

    registry.insert(
        "Sony:ImageQuality2",
        TagDescriptor::new(
            TagId::new_numeric(0x011b),
            "Sony:ImageQuality2".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Integer,
            "Image quality 2".to_string(),
            vec!["0".to_string(), "1".to_string()],
        ),
    );

    registry.insert(
        "Sony:Rotation",
        TagDescriptor::new(
            TagId::new_numeric(0x011c),
            "Sony:Rotation".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Integer,
            "Image rotation".to_string(),
            vec!["0".to_string(), "90".to_string()],
        ),
    );

    registry.insert(
        "Sony:FNumber2",
        TagDescriptor::new(
            TagId::new_numeric(0x011d),
            "Sony:FNumber2".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Rational,
            "F-number 2".to_string(),
            vec!["2.8".to_string(), "5.6".to_string()],
        ),
    );

    registry.insert(
        "Sony:ExposureTime2",
        TagDescriptor::new(
            TagId::new_numeric(0x011e),
            "Sony:ExposureTime2".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Rational,
            "Exposure time 2".to_string(),
            vec!["1/125".to_string(), "1/250".to_string()],
        ),
    );

    registry.insert(
        "Sony:FreeMemoryCardImages",
        TagDescriptor::new(
            TagId::new_numeric(0x011f),
            "Sony:FreeMemoryCardImages".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Integer,
            "Free card images".to_string(),
            vec!["100".to_string(), "500".to_string()],
        ),
    );

    registry.insert(
        "Sony:BrightnessValue",
        TagDescriptor::new(
            TagId::new_numeric(0x0120),
            "Sony:BrightnessValue".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Rational,
            "Brightness value".to_string(),
            vec!["5.2".to_string(), "7.8".to_string()],
        ),
    );

    registry.insert(
        "Sony:ColorReproduction",
        TagDescriptor::new(
            TagId::new_numeric(0x0121),
            "Sony:ColorReproduction".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::String,
            "Color reproduction".to_string(),
            vec!["REAL".to_string(), "VIVID".to_string()],
        ),
    );

    registry.insert(
        "Sony:Contrast2",
        TagDescriptor::new(
            TagId::new_numeric(0x0122),
            "Sony:Contrast2".to_string(),
            FormatFamily::MakerNotes,
            true,
            ValueType::Integer,
            "Contrast setting 2".to_string(),
            vec!["0".to_string(), "1".to_string()],
        ),
    );

    // ===========================
    // GPS TAGS (30+ total)
    // ===========================

    // --- Additional GPS Tags (12 tags) ---
    registry.insert(
        "GPS:GPSDestLatitudeRef",
        TagDescriptor::new(
            TagId::new_numeric(0x0013),
            "GPS:GPSDestLatitudeRef".to_string(),
            FormatFamily::GPS,
            true,
            ValueType::String,
            "Destination latitude reference".to_string(),
            vec!["N".to_string(), "S".to_string()],
        ),
    );

    registry.insert(
        "GPS:GPSDestLatitude",
        TagDescriptor::new(
            TagId::new_numeric(0x0014),
            "GPS:GPSDestLatitude".to_string(),
            FormatFamily::GPS,
            true,
            ValueType::Rational,
            "Destination latitude".to_string(),
            vec!["37 46 30.5".to_string()],
        ),
    );

    registry.insert(
        "GPS:GPSDestLongitudeRef",
        TagDescriptor::new(
            TagId::new_numeric(0x0015),
            "GPS:GPSDestLongitudeRef".to_string(),
            FormatFamily::GPS,
            true,
            ValueType::String,
            "Destination longitude reference".to_string(),
            vec!["E".to_string(), "W".to_string()],
        ),
    );

    registry.insert(
        "GPS:GPSDestLongitude",
        TagDescriptor::new(
            TagId::new_numeric(0x0016),
            "GPS:GPSDestLongitude".to_string(),
            FormatFamily::GPS,
            true,
            ValueType::Rational,
            "Destination longitude".to_string(),
            vec!["122 25 9.8".to_string()],
        ),
    );

    registry.insert(
        "GPS:GPSDestBearingRef",
        TagDescriptor::new(
            TagId::new_numeric(0x0017),
            "GPS:GPSDestBearingRef".to_string(),
            FormatFamily::GPS,
            true,
            ValueType::String,
            "Destination bearing reference".to_string(),
            vec!["T".to_string(), "M".to_string()],
        ),
    );

    registry.insert(
        "GPS:GPSDestBearing",
        TagDescriptor::new(
            TagId::new_numeric(0x0018),
            "GPS:GPSDestBearing".to_string(),
            FormatFamily::GPS,
            true,
            ValueType::Rational,
            "Destination bearing in degrees".to_string(),
            vec!["45.5".to_string(), "180".to_string()],
        ),
    );

    registry.insert(
        "GPS:GPSDestDistanceRef",
        TagDescriptor::new(
            TagId::new_numeric(0x0019),
            "GPS:GPSDestDistanceRef".to_string(),
            FormatFamily::GPS,
            true,
            ValueType::String,
            "Destination distance reference".to_string(),
            vec!["K".to_string(), "M".to_string(), "N".to_string()],
        ),
    );

    registry.insert(
        "GPS:GPSDestDistance",
        TagDescriptor::new(
            TagId::new_numeric(0x001a),
            "GPS:GPSDestDistance".to_string(),
            FormatFamily::GPS,
            true,
            ValueType::Rational,
            "Destination distance".to_string(),
            vec!["5.5".to_string(), "10.2".to_string()],
        ),
    );

    registry.insert(
        "GPS:GPSProcessingMethod",
        TagDescriptor::new(
            TagId::new_numeric(0x001b),
            "GPS:GPSProcessingMethod".to_string(),
            FormatFamily::GPS,
            true,
            ValueType::Binary,
            "GPS processing method".to_string(),
            vec!["GPS".to_string(), "CELLID".to_string()],
        ),
    );

    registry.insert(
        "GPS:GPSAreaInformation",
        TagDescriptor::new(
            TagId::new_numeric(0x001c),
            "GPS:GPSAreaInformation".to_string(),
            FormatFamily::GPS,
            true,
            ValueType::Binary,
            "GPS area information".to_string(),
            vec!["San Francisco".to_string()],
        ),
    );

    registry.insert(
        "GPS:GPSDifferential",
        TagDescriptor::new(
            TagId::new_numeric(0x001e),
            "GPS:GPSDifferential".to_string(),
            FormatFamily::GPS,
            true,
            ValueType::Integer,
            "GPS differential correction".to_string(),
            vec!["0".to_string(), "1".to_string()],
        ),
    );

    registry.insert(
        "GPS:GPSHPositioningError",
        TagDescriptor::new(
            TagId::new_numeric(0x001f),
            "GPS:GPSHPositioningError".to_string(),
            FormatFamily::GPS,
            true,
            ValueType::Rational,
            "Horizontal positioning error".to_string(),
            vec!["10".to_string(), "25".to_string()],
        ),
    );

    // ===========================
    // XMP TAGS (100+ total)
    // ===========================

    // --- Additional XMP Tags (80 tags) ---
    registry.insert(
        "XMP:Source",
        TagDescriptor::new(
            TagId::new_named("XMP-dc:source"),
            "XMP:Source".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Resource from which document is derived".to_string(),
            vec!["Original photograph".to_string()],
        ),
    );

    registry.insert(
        "XMP:Type",
        TagDescriptor::new(
            TagId::new_named("XMP-dc:type"),
            "XMP:Type".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Resource type".to_string(),
            vec!["Image".to_string(), "Text".to_string()],
        ),
    );

    registry.insert(
        "XMP:Coverage",
        TagDescriptor::new(
            TagId::new_named("XMP-dc:coverage"),
            "XMP:Coverage".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Spatial or temporal topic".to_string(),
            vec!["San Francisco, 2024".to_string()],
        ),
    );

    registry.insert(
        "XMP:Relation",
        TagDescriptor::new(
            TagId::new_named("XMP-dc:relation"),
            "XMP:Relation".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Related resource".to_string(),
            vec!["Part of series".to_string()],
        ),
    );

    registry.insert(
        "XMP:Identifier",
        TagDescriptor::new(
            TagId::new_named("XMP-dc:identifier"),
            "XMP:Identifier".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Unambiguous reference to resource".to_string(),
            vec!["ISBN:123456".to_string()],
        ),
    );

    registry.insert(
        "XMP:Audience",
        TagDescriptor::new(
            TagId::new_named("XMP-dc:audience"),
            "XMP:Audience".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Intended audience".to_string(),
            vec!["General public".to_string()],
        ),
    );

    registry.insert(
        "XMP:Instruction",
        TagDescriptor::new(
            TagId::new_named("XMP-dc:instruction"),
            "XMP:Instruction".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Usage instructions".to_string(),
            vec!["Credit required".to_string()],
        ),
    );

    registry.insert(
        "XMP:Source",
        TagDescriptor::new(
            TagId::new_named("XMP-dc:source"),
            "XMP:Source".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Original source".to_string(),
            vec!["Digital camera".to_string()],
        ),
    );

    registry.insert(
        "XMP:AltTitle",
        TagDescriptor::new(
            TagId::new_named("XMP-dc:title-alt"),
            "XMP:AltTitle".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Alternative title".to_string(),
            vec!["Alt title".to_string()],
        ),
    );

    registry.insert(
        "XMP:AltDescription",
        TagDescriptor::new(
            TagId::new_named("XMP-dc:description-alt"),
            "XMP:AltDescription".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Alternative description".to_string(),
            vec!["Alt desc".to_string()],
        ),
    );

    registry.insert(
        "XMP:AltRights",
        TagDescriptor::new(
            TagId::new_named("XMP-dc:rights-alt"),
            "XMP:AltRights".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Alternative rights statement".to_string(),
            vec!["Alt rights".to_string()],
        ),
    );

    registry.insert(
        "XMP:CreatorName",
        TagDescriptor::new(
            TagId::new_named("XMP-dc:creator-name"),
            "XMP:CreatorName".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Creator name".to_string(),
            vec!["John Doe".to_string()],
        ),
    );

    registry.insert(
        "XMP:ContributorName",
        TagDescriptor::new(
            TagId::new_named("XMP-dc:contributor-name"),
            "XMP:ContributorName".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Contributor name".to_string(),
            vec!["Jane Smith".to_string()],
        ),
    );

    registry.insert(
        "XMP:PublisherName",
        TagDescriptor::new(
            TagId::new_named("XMP-dc:publisher-name"),
            "XMP:PublisherName".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Publisher name".to_string(),
            vec!["Acme Corp".to_string()],
        ),
    );

    registry.insert(
        "XMP:DateSubmitted",
        TagDescriptor::new(
            TagId::new_named("XMP-dc:date-submitted"),
            "XMP:DateSubmitted".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::DateTime,
            "Date submitted".to_string(),
            vec!["2024-03-15".to_string()],
        ),
    );

    registry.insert(
        "XMP:Location",
        TagDescriptor::new(
            TagId::new_named("XMP-iptcCore:Location"),
            "XMP:Location".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Location shown in image".to_string(),
            vec!["San Francisco".to_string()],
        ),
    );

    registry.insert(
        "XMP:CountryCode",
        TagDescriptor::new(
            TagId::new_named("XMP-iptcCore:CountryCode"),
            "XMP:CountryCode".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "ISO country code".to_string(),
            vec!["USA".to_string(), "GBR".to_string()],
        ),
    );

    registry.insert(
        "XMP:CountryName",
        TagDescriptor::new(
            TagId::new_named("XMP-iptcCore:CountryName"),
            "XMP:CountryName".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Country name".to_string(),
            vec!["United States".to_string()],
        ),
    );

    registry.insert(
        "XMP:ProvinceState",
        TagDescriptor::new(
            TagId::new_named("XMP-iptcCore:ProvinceState"),
            "XMP:ProvinceState".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Province or state".to_string(),
            vec!["California".to_string()],
        ),
    );

    registry.insert(
        "XMP:City",
        TagDescriptor::new(
            TagId::new_named("XMP-iptcCore:City"),
            "XMP:City".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "City name".to_string(),
            vec!["San Francisco".to_string()],
        ),
    );

    registry.insert(
        "XMP:IntellectualGenre",
        TagDescriptor::new(
            TagId::new_named("XMP-iptcCore:IntellectualGenre"),
            "XMP:IntellectualGenre".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Intellectual genre".to_string(),
            vec!["News".to_string()],
        ),
    );

    registry.insert(
        "XMP:Scene",
        TagDescriptor::new(
            TagId::new_named("XMP-iptcCore:Scene"),
            "XMP:Scene".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "IPTC scene code".to_string(),
            vec!["011100".to_string()],
        ),
    );

    registry.insert(
        "XMP:SubjectCode",
        TagDescriptor::new(
            TagId::new_named("XMP-iptcCore:SubjectCode"),
            "XMP:SubjectCode".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "IPTC subject code".to_string(),
            vec!["04000000".to_string()],
        ),
    );

    registry.insert(
        "XMP:CreatorContactInfo",
        TagDescriptor::new(
            TagId::new_named("XMP-iptcCore:CreatorContactInfo"),
            "XMP:CreatorContactInfo".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::Struct,
            "Creator contact information".to_string(),
            vec!["Contact struct".to_string()],
        ),
    );

    registry.insert(
        "XMP:CiAdrExtadr",
        TagDescriptor::new(
            TagId::new_named("XMP-iptcCore:CiAdrExtadr"),
            "XMP:CiAdrExtadr".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Creator address".to_string(),
            vec!["123 Main St".to_string()],
        ),
    );

    registry.insert(
        "XMP:CiAdrCity",
        TagDescriptor::new(
            TagId::new_named("XMP-iptcCore:CiAdrCity"),
            "XMP:CiAdrCity".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Creator city".to_string(),
            vec!["San Francisco".to_string()],
        ),
    );

    registry.insert(
        "XMP:CiAdrRegion",
        TagDescriptor::new(
            TagId::new_named("XMP-iptcCore:CiAdrRegion"),
            "XMP:CiAdrRegion".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Creator region".to_string(),
            vec!["California".to_string()],
        ),
    );

    registry.insert(
        "XMP:CiAdrPcode",
        TagDescriptor::new(
            TagId::new_named("XMP-iptcCore:CiAdrPcode"),
            "XMP:CiAdrPcode".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Creator postal code".to_string(),
            vec!["94102".to_string()],
        ),
    );

    registry.insert(
        "XMP:CiAdrCtry",
        TagDescriptor::new(
            TagId::new_named("XMP-iptcCore:CiAdrCtry"),
            "XMP:CiAdrCtry".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Creator country".to_string(),
            vec!["USA".to_string()],
        ),
    );

    registry.insert(
        "XMP:CiTelWork",
        TagDescriptor::new(
            TagId::new_named("XMP-iptcCore:CiTelWork"),
            "XMP:CiTelWork".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Creator phone".to_string(),
            vec!["+1-415-555-1234".to_string()],
        ),
    );

    registry.insert(
        "XMP:CiEmailWork",
        TagDescriptor::new(
            TagId::new_named("XMP-iptcCore:CiEmailWork"),
            "XMP:CiEmailWork".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Creator email".to_string(),
            vec!["creator@example.com".to_string()],
        ),
    );

    registry.insert(
        "XMP:CiUrlWork",
        TagDescriptor::new(
            TagId::new_named("XMP-iptcCore:CiUrlWork"),
            "XMP:CiUrlWork".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Creator website".to_string(),
            vec!["https://example.com".to_string()],
        ),
    );

    registry.insert(
        "XMP:CreatorWorkEmail",
        TagDescriptor::new(
            TagId::new_named("XMP-iptcCore:CreatorWorkEmail"),
            "XMP:CreatorWorkEmail".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Creator work email".to_string(),
            vec!["work@example.com".to_string()],
        ),
    );

    registry.insert(
        "XMP:CreatorWorkURL",
        TagDescriptor::new(
            TagId::new_named("XMP-iptcCore:CreatorWorkURL"),
            "XMP:CreatorWorkURL".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Creator work URL".to_string(),
            vec!["https://portfolio.example.com".to_string()],
        ),
    );

    registry.insert(
        "XMP:IPTCScene",
        TagDescriptor::new(
            TagId::new_named("XMP-iptcCore:Scene"),
            "XMP:IPTCScene".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "IPTC scene".to_string(),
            vec!["outdoor".to_string()],
        ),
    );

    registry.insert(
        "XMP:Temperature",
        TagDescriptor::new(
            TagId::new_named("XMP-crs:Temperature"),
            "XMP:Temperature".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::Integer,
            "Color temperature".to_string(),
            vec!["5500".to_string(), "6500".to_string()],
        ),
    );

    registry.insert(
        "XMP:Tint",
        TagDescriptor::new(
            TagId::new_named("XMP-crs:Tint"),
            "XMP:Tint".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::Integer,
            "Tint adjustment".to_string(),
            vec!["-10".to_string(), "+15".to_string()],
        ),
    );

    registry.insert(
        "XMP:Exposure2012",
        TagDescriptor::new(
            TagId::new_named("XMP-crs:Exposure2012"),
            "XMP:Exposure2012".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::Float,
            "Exposure adjustment 2012".to_string(),
            vec!["+0.50".to_string(), "-1.00".to_string()],
        ),
    );

    registry.insert(
        "XMP:Contrast2012",
        TagDescriptor::new(
            TagId::new_named("XMP-crs:Contrast2012"),
            "XMP:Contrast2012".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::Integer,
            "Contrast 2012".to_string(),
            vec!["0".to_string(), "+25".to_string()],
        ),
    );

    registry.insert(
        "XMP:Highlights2012",
        TagDescriptor::new(
            TagId::new_named("XMP-crs:Highlights2012"),
            "XMP:Highlights2012".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::Integer,
            "Highlights 2012".to_string(),
            vec!["-50".to_string(), "0".to_string()],
        ),
    );

    registry.insert(
        "XMP:Shadows2012",
        TagDescriptor::new(
            TagId::new_named("XMP-crs:Shadows2012"),
            "XMP:Shadows2012".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::Integer,
            "Shadows 2012".to_string(),
            vec!["+50".to_string(), "0".to_string()],
        ),
    );

    registry.insert(
        "XMP:Whites2012",
        TagDescriptor::new(
            TagId::new_named("XMP-crs:Whites2012"),
            "XMP:Whites2012".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::Integer,
            "Whites 2012".to_string(),
            vec!["0".to_string(), "+25".to_string()],
        ),
    );

    registry.insert(
        "XMP:Blacks2012",
        TagDescriptor::new(
            TagId::new_named("XMP-crs:Blacks2012"),
            "XMP:Blacks2012".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::Integer,
            "Blacks 2012".to_string(),
            vec!["-25".to_string(), "0".to_string()],
        ),
    );

    registry.insert(
        "XMP:Clarity2012",
        TagDescriptor::new(
            TagId::new_named("XMP-crs:Clarity2012"),
            "XMP:Clarity2012".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::Integer,
            "Clarity 2012".to_string(),
            vec!["+15".to_string(), "0".to_string()],
        ),
    );

    registry.insert(
        "XMP:Vibrance",
        TagDescriptor::new(
            TagId::new_named("XMP-crs:Vibrance"),
            "XMP:Vibrance".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::Integer,
            "Vibrance adjustment".to_string(),
            vec!["+10".to_string(), "0".to_string()],
        ),
    );

    registry.insert(
        "XMP:Saturation",
        TagDescriptor::new(
            TagId::new_named("XMP-crs:Saturation"),
            "XMP:Saturation".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::Integer,
            "Saturation adjustment".to_string(),
            vec!["0".to_string(), "+15".to_string()],
        ),
    );

    registry.insert(
        "XMP:ParametricShadows",
        TagDescriptor::new(
            TagId::new_named("XMP-crs:ParametricShadows"),
            "XMP:ParametricShadows".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::Integer,
            "Parametric shadows".to_string(),
            vec!["0".to_string()],
        ),
    );

    registry.insert(
        "XMP:ParametricDarks",
        TagDescriptor::new(
            TagId::new_named("XMP-crs:ParametricDarks"),
            "XMP:ParametricDarks".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::Integer,
            "Parametric darks".to_string(),
            vec!["0".to_string()],
        ),
    );

    registry.insert(
        "XMP:ParametricLights",
        TagDescriptor::new(
            TagId::new_named("XMP-crs:ParametricLights"),
            "XMP:ParametricLights".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::Integer,
            "Parametric lights".to_string(),
            vec!["0".to_string()],
        ),
    );

    registry.insert(
        "XMP:ParametricHighlights",
        TagDescriptor::new(
            TagId::new_named("XMP-crs:ParametricHighlights"),
            "XMP:ParametricHighlights".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::Integer,
            "Parametric highlights".to_string(),
            vec!["0".to_string()],
        ),
    );

    registry.insert(
        "XMP:ParametricShadowSplit",
        TagDescriptor::new(
            TagId::new_named("XMP-crs:ParametricShadowSplit"),
            "XMP:ParametricShadowSplit".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::Integer,
            "Shadow split point".to_string(),
            vec!["25".to_string()],
        ),
    );

    registry.insert(
        "XMP:ParametricMidtoneSplit",
        TagDescriptor::new(
            TagId::new_named("XMP-crs:ParametricMidtoneSplit"),
            "XMP:ParametricMidtoneSplit".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::Integer,
            "Midtone split point".to_string(),
            vec!["50".to_string()],
        ),
    );

    registry.insert(
        "XMP:ParametricHighlightSplit",
        TagDescriptor::new(
            TagId::new_named("XMP-crs:ParametricHighlightSplit"),
            "XMP:ParametricHighlightSplit".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::Integer,
            "Highlight split point".to_string(),
            vec!["75".to_string()],
        ),
    );

    registry.insert(
        "XMP:Sharpness",
        TagDescriptor::new(
            TagId::new_named("XMP-crs:Sharpness"),
            "XMP:Sharpness".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::Integer,
            "Sharpness amount".to_string(),
            vec!["40".to_string(), "60".to_string()],
        ),
    );

    registry.insert(
        "XMP:LuminanceSmoothing",
        TagDescriptor::new(
            TagId::new_named("XMP-crs:LuminanceSmoothing"),
            "XMP:LuminanceSmoothing".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::Integer,
            "Luminance noise reduction".to_string(),
            vec!["0".to_string(), "25".to_string()],
        ),
    );

    registry.insert(
        "XMP:ColorNoiseReduction",
        TagDescriptor::new(
            TagId::new_named("XMP-crs:ColorNoiseReduction"),
            "XMP:ColorNoiseReduction".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::Integer,
            "Color noise reduction".to_string(),
            vec!["25".to_string(), "50".to_string()],
        ),
    );

    registry.insert(
        "XMP:HueAdjustmentRed",
        TagDescriptor::new(
            TagId::new_named("XMP-crs:HueAdjustmentRed"),
            "XMP:HueAdjustmentRed".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::Integer,
            "Red hue adjustment".to_string(),
            vec!["0".to_string(), "+10".to_string()],
        ),
    );

    registry.insert(
        "XMP:HueAdjustmentOrange",
        TagDescriptor::new(
            TagId::new_named("XMP-crs:HueAdjustmentOrange"),
            "XMP:HueAdjustmentOrange".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::Integer,
            "Orange hue adjustment".to_string(),
            vec!["0".to_string()],
        ),
    );

    registry.insert(
        "XMP:HueAdjustmentYellow",
        TagDescriptor::new(
            TagId::new_named("XMP-crs:HueAdjustmentYellow"),
            "XMP:HueAdjustmentYellow".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::Integer,
            "Yellow hue adjustment".to_string(),
            vec!["0".to_string()],
        ),
    );

    registry.insert(
        "XMP:SaturationAdjustmentRed",
        TagDescriptor::new(
            TagId::new_named("XMP-crs:SaturationAdjustmentRed"),
            "XMP:SaturationAdjustmentRed".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::Integer,
            "Red saturation adjustment".to_string(),
            vec!["0".to_string()],
        ),
    );

    registry.insert(
        "XMP:Credit",
        TagDescriptor::new(
            TagId::new_named("XMP-photoshop:Credit"),
            "XMP:Credit".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Credit line".to_string(),
            vec!["AP Photo".to_string()],
        ),
    );

    registry.insert(
        "XMP:Source",
        TagDescriptor::new(
            TagId::new_named("XMP-photoshop:Source"),
            "XMP:Source".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Source".to_string(),
            vec!["Reuters".to_string()],
        ),
    );

    registry.insert(
        "XMP:Headline",
        TagDescriptor::new(
            TagId::new_named("XMP-photoshop:Headline"),
            "XMP:Headline".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Headline".to_string(),
            vec!["Breaking News".to_string()],
        ),
    );

    registry.insert(
        "XMP:City",
        TagDescriptor::new(
            TagId::new_named("XMP-photoshop:City"),
            "XMP:City".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "City".to_string(),
            vec!["San Francisco".to_string()],
        ),
    );

    registry.insert(
        "XMP:State",
        TagDescriptor::new(
            TagId::new_named("XMP-photoshop:State"),
            "XMP:State".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "State/province".to_string(),
            vec!["California".to_string()],
        ),
    );

    registry.insert(
        "XMP:Country",
        TagDescriptor::new(
            TagId::new_named("XMP-photoshop:Country"),
            "XMP:Country".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Country".to_string(),
            vec!["United States".to_string()],
        ),
    );

    registry.insert(
        "XMP:TransmissionReference",
        TagDescriptor::new(
            TagId::new_named("XMP-photoshop:TransmissionReference"),
            "XMP:TransmissionReference".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Transmission reference".to_string(),
            vec!["JOB123".to_string()],
        ),
    );

    registry.insert(
        "XMP:Instructions",
        TagDescriptor::new(
            TagId::new_named("XMP-photoshop:Instructions"),
            "XMP:Instructions".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Special instructions".to_string(),
            vec!["Handle with care".to_string()],
        ),
    );

    registry.insert(
        "XMP:Category",
        TagDescriptor::new(
            TagId::new_named("XMP-photoshop:Category"),
            "XMP:Category".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Category".to_string(),
            vec!["POL".to_string(), "SPO".to_string()],
        ),
    );

    registry.insert(
        "XMP:SupplementalCategories",
        TagDescriptor::new(
            TagId::new_named("XMP-photoshop:SupplementalCategories"),
            "XMP:SupplementalCategories".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Supplemental categories".to_string(),
            vec!["election".to_string()],
        ),
    );

    registry.insert(
        "XMP:Urgency",
        TagDescriptor::new(
            TagId::new_named("XMP-photoshop:Urgency"),
            "XMP:Urgency".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::Integer,
            "Urgency".to_string(),
            vec!["1".to_string(), "5".to_string()],
        ),
    );

    registry.insert(
        "XMP:DateCreated",
        TagDescriptor::new(
            TagId::new_named("XMP-photoshop:DateCreated"),
            "XMP:DateCreated".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::DateTime,
            "Date created".to_string(),
            vec!["2024-03-15".to_string()],
        ),
    );

    registry.insert(
        "XMP:AuthorsPosition",
        TagDescriptor::new(
            TagId::new_named("XMP-photoshop:AuthorsPosition"),
            "XMP:AuthorsPosition".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Author's position".to_string(),
            vec!["Staff Photographer".to_string()],
        ),
    );

    registry.insert(
        "XMP:CaptionWriter",
        TagDescriptor::new(
            TagId::new_named("XMP-photoshop:CaptionWriter"),
            "XMP:CaptionWriter".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Caption writer".to_string(),
            vec!["Jane Smith".to_string()],
        ),
    );

    registry.insert(
        "XMP:ColorMode",
        TagDescriptor::new(
            TagId::new_named("XMP-photoshop:ColorMode"),
            "XMP:ColorMode".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::Integer,
            "Color mode".to_string(),
            vec!["3".to_string(), "4".to_string()],
        ),
    );

    registry.insert(
        "XMP:UsageTerms",
        TagDescriptor::new(
            TagId::new_named("XMP-xmpRights:UsageTerms"),
            "XMP:UsageTerms".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Usage terms and conditions".to_string(),
            vec!["For editorial use only".to_string()],
        ),
    );

    registry.insert(
        "XMP:WebStatement",
        TagDescriptor::new(
            TagId::new_named("XMP-xmpRights:WebStatement"),
            "XMP:WebStatement".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Web statement of rights".to_string(),
            vec!["https://example.com/rights".to_string()],
        ),
    );

    registry.insert(
        "XMP:Marked",
        TagDescriptor::new(
            TagId::new_named("XMP-xmpRights:Marked"),
            "XMP:Marked".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Copyrighted status".to_string(),
            vec!["True".to_string(), "False".to_string()],
        ),
    );

    registry.insert(
        "XMP:Owner",
        TagDescriptor::new(
            TagId::new_named("XMP-xmpRights:Owner"),
            "XMP:Owner".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Rights owner".to_string(),
            vec!["John Doe".to_string()],
        ),
    );

    registry.insert(
        "XMP:Certificate",
        TagDescriptor::new(
            TagId::new_named("XMP-xmpRights:Certificate"),
            "XMP:Certificate".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Rights certificate URL".to_string(),
            vec!["https://example.com/cert".to_string()],
        ),
    );

    // ===========================
    // IPTC TAGS (50+ total)
    // ===========================

    registry.insert(
        "IPTC:ApplicationRecordVersion",
        TagDescriptor::new(
            TagId::new_numeric(0x0000),
            "IPTC:ApplicationRecordVersion".to_string(),
            FormatFamily::IPTC,
            true,
            ValueType::Integer,
            "IPTC application record version".to_string(),
            vec!["2".to_string(), "4".to_string()],
        ),
    );

    registry.insert(
        "IPTC:ObjectTypeReference",
        TagDescriptor::new(
            TagId::new_numeric(0x0003),
            "IPTC:ObjectTypeReference".to_string(),
            FormatFamily::IPTC,
            true,
            ValueType::String,
            "Object type reference".to_string(),
            vec!["News".to_string()],
        ),
    );

    registry.insert(
        "IPTC:ObjectAttributeReference",
        TagDescriptor::new(
            TagId::new_numeric(0x0004),
            "IPTC:ObjectAttributeReference".to_string(),
            FormatFamily::IPTC,
            true,
            ValueType::String,
            "Object attribute reference".to_string(),
            vec!["001".to_string()],
        ),
    );

    registry.insert(
        "IPTC:ObjectName",
        TagDescriptor::new(
            TagId::new_numeric(0x0005),
            "IPTC:ObjectName".to_string(),
            FormatFamily::IPTC,
            true,
            ValueType::String,
            "Object name or title".to_string(),
            vec!["News Photo".to_string()],
        ),
    );

    registry.insert(
        "IPTC:EditStatus",
        TagDescriptor::new(
            TagId::new_numeric(0x0007),
            "IPTC:EditStatus".to_string(),
            FormatFamily::IPTC,
            true,
            ValueType::String,
            "Edit status".to_string(),
            vec!["Edited".to_string()],
        ),
    );

    registry.insert(
        "IPTC:Urgency",
        TagDescriptor::new(
            TagId::new_numeric(0x000a),
            "IPTC:Urgency".to_string(),
            FormatFamily::IPTC,
            true,
            ValueType::String,
            "Editorial urgency (1-8)".to_string(),
            vec!["5".to_string(), "1".to_string()],
        ),
    );

    registry.insert(
        "IPTC:Category",
        TagDescriptor::new(
            TagId::new_numeric(0x000f),
            "IPTC:Category".to_string(),
            FormatFamily::IPTC,
            true,
            ValueType::String,
            "Subject category".to_string(),
            vec!["POL".to_string(), "SPO".to_string()],
        ),
    );

    registry.insert(
        "IPTC:Keywords",
        TagDescriptor::new(
            TagId::new_numeric(0x0019),
            "IPTC:Keywords".to_string(),
            FormatFamily::IPTC,
            true,
            ValueType::String,
            "Keywords".to_string(),
            vec!["politics".to_string(), "sports".to_string()],
        ),
    );

    registry.insert(
        "IPTC:DateCreated",
        TagDescriptor::new(
            TagId::new_numeric(0x0037),
            "IPTC:DateCreated".to_string(),
            FormatFamily::IPTC,
            true,
            ValueType::String,
            "Date created (YYYYMMDD)".to_string(),
            vec!["20240315".to_string()],
        ),
    );

    registry.insert(
        "IPTC:By-line",
        TagDescriptor::new(
            TagId::new_numeric(0x0050),
            "IPTC:By-line".to_string(),
            FormatFamily::IPTC,
            true,
            ValueType::String,
            "Creator/photographer name".to_string(),
            vec!["John Doe".to_string()],
        ),
    );

    registry.insert(
        "IPTC:Headline",
        TagDescriptor::new(
            TagId::new_numeric(0x0069),
            "IPTC:Headline".to_string(),
            FormatFamily::IPTC,
            true,
            ValueType::String,
            "Headline".to_string(),
            vec!["Breaking News".to_string()],
        ),
    );

    registry.insert(
        "IPTC:CopyrightNotice",
        TagDescriptor::new(
            TagId::new_numeric(0x0074),
            "IPTC:CopyrightNotice".to_string(),
            FormatFamily::IPTC,
            true,
            ValueType::String,
            "Copyright notice".to_string(),
            vec!["© 2024 John Doe".to_string()],
        ),
    );

    registry.insert(
        "IPTC:Caption-Abstract",
        TagDescriptor::new(
            TagId::new_numeric(0x0078),
            "IPTC:Caption-Abstract".to_string(),
            FormatFamily::IPTC,
            true,
            ValueType::String,
            "Full description".to_string(),
            vec!["A detailed caption".to_string()],
        ),
    );

    registry.insert(
        "IPTC:SupplementalCategories",
        TagDescriptor::new(
            TagId::new_numeric(0x0014),
            "IPTC:SupplementalCategories".to_string(),
            FormatFamily::IPTC,
            true,
            ValueType::String,
            "Supplemental categories".to_string(),
            vec!["election".to_string(), "campaign".to_string()],
        ),
    );

    registry.insert(
        "IPTC:FixtureIdentifier",
        TagDescriptor::new(
            TagId::new_numeric(0x0016),
            "IPTC:FixtureIdentifier".to_string(),
            FormatFamily::IPTC,
            true,
            ValueType::String,
            "Fixture identifier".to_string(),
            vec!["FIX12345".to_string()],
        ),
    );

    registry.insert(
        "IPTC:ObjectCycle",
        TagDescriptor::new(
            TagId::new_numeric(0x004b),
            "IPTC:ObjectCycle".to_string(),
            FormatFamily::IPTC,
            true,
            ValueType::String,
            "Object cycle (a=morning, p=evening, b=both)".to_string(),
            vec!["a".to_string(), "p".to_string()],
        ),
    );

    registry.insert(
        "IPTC:By-lineTitle",
        TagDescriptor::new(
            TagId::new_numeric(0x0055),
            "IPTC:By-lineTitle".to_string(),
            FormatFamily::IPTC,
            true,
            ValueType::String,
            "Creator's job title".to_string(),
            vec!["Staff Photographer".to_string()],
        ),
    );

    registry.insert(
        "IPTC:City",
        TagDescriptor::new(
            TagId::new_numeric(0x005a),
            "IPTC:City".to_string(),
            FormatFamily::IPTC,
            true,
            ValueType::String,
            "City name".to_string(),
            vec!["San Francisco".to_string(), "New York".to_string()],
        ),
    );

    registry.insert(
        "IPTC:Sub-location",
        TagDescriptor::new(
            TagId::new_numeric(0x005c),
            "IPTC:Sub-location".to_string(),
            FormatFamily::IPTC,
            true,
            ValueType::String,
            "Sub-location".to_string(),
            vec!["Downtown".to_string(), "City Hall".to_string()],
        ),
    );

    registry.insert(
        "IPTC:Province-State",
        TagDescriptor::new(
            TagId::new_numeric(0x005f),
            "IPTC:Province-State".to_string(),
            FormatFamily::IPTC,
            true,
            ValueType::String,
            "Province/State".to_string(),
            vec!["California".to_string(), "New York".to_string()],
        ),
    );

    registry.insert(
        "IPTC:Country-PrimaryLocationCode",
        TagDescriptor::new(
            TagId::new_numeric(0x0064),
            "IPTC:Country-PrimaryLocationCode".to_string(),
            FormatFamily::IPTC,
            true,
            ValueType::String,
            "Country code (ISO 3166)".to_string(),
            vec!["USA".to_string(), "GBR".to_string()],
        ),
    );

    registry.insert(
        "IPTC:Country-PrimaryLocationName",
        TagDescriptor::new(
            TagId::new_numeric(0x0065),
            "IPTC:Country-PrimaryLocationName".to_string(),
            FormatFamily::IPTC,
            true,
            ValueType::String,
            "Country name".to_string(),
            vec!["United States".to_string(), "United Kingdom".to_string()],
        ),
    );

    registry.insert(
        "IPTC:OriginalTransmissionReference",
        TagDescriptor::new(
            TagId::new_numeric(0x0067),
            "IPTC:OriginalTransmissionReference".to_string(),
            FormatFamily::IPTC,
            true,
            ValueType::String,
            "Job ID or transmission reference".to_string(),
            vec!["JOB123".to_string()],
        ),
    );

    registry.insert(
        "IPTC:Credit",
        TagDescriptor::new(
            TagId::new_numeric(0x006e),
            "IPTC:Credit".to_string(),
            FormatFamily::IPTC,
            true,
            ValueType::String,
            "Credit line".to_string(),
            vec!["AP Photo".to_string()],
        ),
    );

    registry.insert(
        "IPTC:Source",
        TagDescriptor::new(
            TagId::new_numeric(0x0073),
            "IPTC:Source".to_string(),
            FormatFamily::IPTC,
            true,
            ValueType::String,
            "Original source".to_string(),
            vec!["Reuters".to_string()],
        ),
    );

    registry.insert(
        "IPTC:Writer-Editor",
        TagDescriptor::new(
            TagId::new_numeric(0x007a),
            "IPTC:Writer-Editor".to_string(),
            FormatFamily::IPTC,
            true,
            ValueType::String,
            "Caption writer/editor".to_string(),
            vec!["Jane Smith".to_string()],
        ),
    );

    registry.insert(
        "IPTC:SpecialInstructions",
        TagDescriptor::new(
            TagId::new_numeric(0x0028),
            "IPTC:SpecialInstructions".to_string(),
            FormatFamily::IPTC,
            true,
            ValueType::String,
            "Special instructions".to_string(),
            vec!["Handle with care".to_string()],
        ),
    );

    registry.insert(
        "IPTC:ActionAdvised",
        TagDescriptor::new(
            TagId::new_numeric(0x002a),
            "IPTC:ActionAdvised".to_string(),
            FormatFamily::IPTC,
            true,
            ValueType::String,
            "Action advised".to_string(),
            vec!["01".to_string(), "02".to_string()],
        ),
    );

    registry.insert(
        "IPTC:ReferenceService",
        TagDescriptor::new(
            TagId::new_numeric(0x002d),
            "IPTC:ReferenceService".to_string(),
            FormatFamily::IPTC,
            true,
            ValueType::String,
            "Reference service".to_string(),
            vec!["AP".to_string()],
        ),
    );

    registry.insert(
        "IPTC:ReferenceDate",
        TagDescriptor::new(
            TagId::new_numeric(0x002f),
            "IPTC:ReferenceDate".to_string(),
            FormatFamily::IPTC,
            true,
            ValueType::String,
            "Reference date".to_string(),
            vec!["20240315".to_string()],
        ),
    );

    registry.insert(
        "IPTC:ReferenceNumber",
        TagDescriptor::new(
            TagId::new_numeric(0x0032),
            "IPTC:ReferenceNumber".to_string(),
            FormatFamily::IPTC,
            true,
            ValueType::String,
            "Reference number".to_string(),
            vec!["123456".to_string()],
        ),
    );

    registry.insert(
        "IPTC:TimeCreated",
        TagDescriptor::new(
            TagId::new_numeric(0x003c),
            "IPTC:TimeCreated".to_string(),
            FormatFamily::IPTC,
            true,
            ValueType::String,
            "Time created (HHMMSS+HHMM)".to_string(),
            vec!["143045+0000".to_string()],
        ),
    );

    registry.insert(
        "IPTC:DigitalCreationDate",
        TagDescriptor::new(
            TagId::new_numeric(0x003e),
            "IPTC:DigitalCreationDate".to_string(),
            FormatFamily::IPTC,
            true,
            ValueType::String,
            "Digital creation date".to_string(),
            vec!["20240315".to_string()],
        ),
    );

    registry.insert(
        "IPTC:DigitalCreationTime",
        TagDescriptor::new(
            TagId::new_numeric(0x003f),
            "IPTC:DigitalCreationTime".to_string(),
            FormatFamily::IPTC,
            true,
            ValueType::String,
            "Digital creation time".to_string(),
            vec!["143045+0000".to_string()],
        ),
    );

    registry.insert(
        "IPTC:OriginatingProgram",
        TagDescriptor::new(
            TagId::new_numeric(0x0041),
            "IPTC:OriginatingProgram".to_string(),
            FormatFamily::IPTC,
            true,
            ValueType::String,
            "Originating program".to_string(),
            vec!["Photoshop".to_string()],
        ),
    );

    registry.insert(
        "IPTC:ProgramVersion",
        TagDescriptor::new(
            TagId::new_numeric(0x0046),
            "IPTC:ProgramVersion".to_string(),
            FormatFamily::IPTC,
            true,
            ValueType::String,
            "Program version".to_string(),
            vec!["2024".to_string()],
        ),
    );

    registry.insert(
        "IPTC:ObjectPreviewFileFormat",
        TagDescriptor::new(
            TagId::new_numeric(0x00c8),
            "IPTC:ObjectPreviewFileFormat".to_string(),
            FormatFamily::IPTC,
            true,
            ValueType::Integer,
            "Preview file format".to_string(),
            vec!["1".to_string(), "11".to_string()],
        ),
    );

    registry.insert(
        "IPTC:ObjectPreviewFileVersion",
        TagDescriptor::new(
            TagId::new_numeric(0x00c9),
            "IPTC:ObjectPreviewFileVersion".to_string(),
            FormatFamily::IPTC,
            true,
            ValueType::Integer,
            "Preview file version".to_string(),
            vec!["1".to_string()],
        ),
    );

    registry.insert(
        "IPTC:ObjectPreviewData",
        TagDescriptor::new(
            TagId::new_numeric(0x00ca),
            "IPTC:ObjectPreviewData".to_string(),
            FormatFamily::IPTC,
            true,
            ValueType::Binary,
            "Preview image data".to_string(),
            vec!["binary".to_string()],
        ),
    );

    registry.insert(
        "IPTC:LanguageIdentifier",
        TagDescriptor::new(
            TagId::new_numeric(0x0087),
            "IPTC:LanguageIdentifier".to_string(),
            FormatFamily::IPTC,
            true,
            ValueType::String,
            "Language identifier".to_string(),
            vec!["en".to_string(), "fr".to_string()],
        ),
    );

    registry.insert(
        "IPTC:ContentLocationCode",
        TagDescriptor::new(
            TagId::new_numeric(0x001a),
            "IPTC:ContentLocationCode".to_string(),
            FormatFamily::IPTC,
            true,
            ValueType::String,
            "Content location code".to_string(),
            vec!["CA".to_string(), "NY".to_string()],
        ),
    );

    registry.insert(
        "IPTC:ContentLocationName",
        TagDescriptor::new(
            TagId::new_numeric(0x001b),
            "IPTC:ContentLocationName".to_string(),
            FormatFamily::IPTC,
            true,
            ValueType::String,
            "Content location name".to_string(),
            vec!["California".to_string()],
        ),
    );

    registry.insert(
        "IPTC:ReleaseDate",
        TagDescriptor::new(
            TagId::new_numeric(0x001e),
            "IPTC:ReleaseDate".to_string(),
            FormatFamily::IPTC,
            true,
            ValueType::String,
            "Release date".to_string(),
            vec!["20240315".to_string()],
        ),
    );

    registry.insert(
        "IPTC:ReleaseTime",
        TagDescriptor::new(
            TagId::new_numeric(0x0023),
            "IPTC:ReleaseTime".to_string(),
            FormatFamily::IPTC,
            true,
            ValueType::String,
            "Release time".to_string(),
            vec!["120000+0000".to_string()],
        ),
    );

    registry.insert(
        "IPTC:ExpirationDate",
        TagDescriptor::new(
            TagId::new_numeric(0x0025),
            "IPTC:ExpirationDate".to_string(),
            FormatFamily::IPTC,
            true,
            ValueType::String,
            "Expiration date".to_string(),
            vec!["20250315".to_string()],
        ),
    );

    registry.insert(
        "IPTC:ExpirationTime",
        TagDescriptor::new(
            TagId::new_numeric(0x0026),
            "IPTC:ExpirationTime".to_string(),
            FormatFamily::IPTC,
            true,
            ValueType::String,
            "Expiration time".to_string(),
            vec!["235959+0000".to_string()],
        ),
    );

    registry.insert(
        "IPTC:ImageType",
        TagDescriptor::new(
            TagId::new_numeric(0x0082),
            "IPTC:ImageType".to_string(),
            FormatFamily::IPTC,
            true,
            ValueType::String,
            "Image type".to_string(),
            vec!["0M".to_string(), "1M".to_string()],
        ),
    );

    registry.insert(
        "IPTC:ImageOrientation",
        TagDescriptor::new(
            TagId::new_numeric(0x0083),
            "IPTC:ImageOrientation".to_string(),
            FormatFamily::IPTC,
            true,
            ValueType::String,
            "Image orientation".to_string(),
            vec!["P".to_string(), "L".to_string()],
        ),
    );

    registry.insert(
        "IPTC:LocalCaption",
        TagDescriptor::new(
            TagId::new_numeric(0x0079),
            "IPTC:LocalCaption".to_string(),
            FormatFamily::IPTC,
            true,
            ValueType::String,
            "Local caption".to_string(),
            vec!["Local description".to_string()],
        ),
    );

    registry.insert(
        "IPTC:EnvelopeRecordVersion",
        TagDescriptor::new(
            TagId::new_numeric(0x0000),
            "IPTC:EnvelopeRecordVersion".to_string(),
            FormatFamily::IPTC,
            true,
            ValueType::Integer,
            "Envelope record version".to_string(),
            vec!["2".to_string()],
        ),
    );

    // ===========================
    // PDF TAGS (11 total)
    // ===========================

    registry.insert(
        "PDF:Title",
        TagDescriptor::new(
            TagId::new_named("Title"),
            "PDF:Title".to_string(),
            FormatFamily::PDF,
            true,
            ValueType::String,
            "Document title".to_string(),
            vec!["My Document".to_string()],
        ),
    );

    registry.insert(
        "PDF:Author",
        TagDescriptor::new(
            TagId::new_named("Author"),
            "PDF:Author".to_string(),
            FormatFamily::PDF,
            true,
            ValueType::String,
            "Document author".to_string(),
            vec!["John Doe".to_string()],
        ),
    );

    registry.insert(
        "PDF:Subject",
        TagDescriptor::new(
            TagId::new_named("Subject"),
            "PDF:Subject".to_string(),
            FormatFamily::PDF,
            true,
            ValueType::String,
            "Document subject".to_string(),
            vec!["Annual Report".to_string()],
        ),
    );

    registry.insert(
        "PDF:Keywords",
        TagDescriptor::new(
            TagId::new_named("Keywords"),
            "PDF:Keywords".to_string(),
            FormatFamily::PDF,
            true,
            ValueType::String,
            "Document keywords".to_string(),
            vec!["finance, report, 2024".to_string()],
        ),
    );

    registry.insert(
        "PDF:Creator",
        TagDescriptor::new(
            TagId::new_named("Creator"),
            "PDF:Creator".to_string(),
            FormatFamily::PDF,
            true,
            ValueType::String,
            "Application that created document".to_string(),
            vec!["Microsoft Word".to_string()],
        ),
    );

    registry.insert(
        "PDF:Producer",
        TagDescriptor::new(
            TagId::new_named("Producer"),
            "PDF:Producer".to_string(),
            FormatFamily::PDF,
            true,
            ValueType::String,
            "PDF creation software".to_string(),
            vec!["Adobe PDF Library".to_string()],
        ),
    );

    registry.insert(
        "PDF:CreateDate",
        TagDescriptor::new(
            TagId::new_named("CreationDate"),
            "PDF:CreateDate".to_string(),
            FormatFamily::PDF,
            true,
            ValueType::DateTime,
            "Document creation date".to_string(),
            vec!["2024-03-15T14:30:45Z".to_string()],
        ),
    );

    registry.insert(
        "PDF:ModifyDate",
        TagDescriptor::new(
            TagId::new_named("ModDate"),
            "PDF:ModifyDate".to_string(),
            FormatFamily::PDF,
            true,
            ValueType::DateTime,
            "Last modification date".to_string(),
            vec!["2024-03-16T09:15:00Z".to_string()],
        ),
    );

    registry.insert(
        "PDF:Trapped",
        TagDescriptor::new(
            TagId::new_named("Trapped"),
            "PDF:Trapped".to_string(),
            FormatFamily::PDF,
            true,
            ValueType::String,
            "Trapping information".to_string(),
            vec![
                "True".to_string(),
                "False".to_string(),
                "Unknown".to_string(),
            ],
        ),
    );

    registry.insert(
        "PDF:SourceModified",
        TagDescriptor::new(
            TagId::new_named("SourceModified"),
            "PDF:SourceModified".to_string(),
            FormatFamily::PDF,
            true,
            ValueType::DateTime,
            "Source file modification date".to_string(),
            vec!["2024-03-15".to_string()],
        ),
    );

    registry.insert(
        "PDF:AppleKeywords",
        TagDescriptor::new(
            TagId::new_named("AAPL:Keywords"),
            "PDF:AppleKeywords".to_string(),
            FormatFamily::PDF,
            true,
            ValueType::String,
            "Keywords written by Apple utilities".to_string(),
            vec!["keyword1, keyword2".to_string()],
        ),
    );

    // ===========================
    // QUICKTIME TAGS (11 total)
    // ===========================

    registry.insert(
        "QuickTime:Title",
        TagDescriptor::new(
            TagId::new_named("©nam"),
            "QuickTime:Title".to_string(),
            FormatFamily::QuickTime,
            true,
            ValueType::String,
            "Media title".to_string(),
            vec!["My Video".to_string()],
        ),
    );

    registry.insert(
        "QuickTime:Artist",
        TagDescriptor::new(
            TagId::new_named("©ART"),
            "QuickTime:Artist".to_string(),
            FormatFamily::QuickTime,
            true,
            ValueType::String,
            "Artist name".to_string(),
            vec!["John Doe".to_string()],
        ),
    );

    registry.insert(
        "QuickTime:Album",
        TagDescriptor::new(
            TagId::new_named("©alb"),
            "QuickTime:Album".to_string(),
            FormatFamily::QuickTime,
            true,
            ValueType::String,
            "Album name".to_string(),
            vec!["My Album".to_string()],
        ),
    );

    registry.insert(
        "QuickTime:Comment",
        TagDescriptor::new(
            TagId::new_named("©cmt"),
            "QuickTime:Comment".to_string(),
            FormatFamily::QuickTime,
            true,
            ValueType::String,
            "User comment".to_string(),
            vec!["Great video".to_string()],
        ),
    );

    registry.insert(
        "QuickTime:Copyright",
        TagDescriptor::new(
            TagId::new_named("©cpy"),
            "QuickTime:Copyright".to_string(),
            FormatFamily::QuickTime,
            true,
            ValueType::String,
            "Copyright statement".to_string(),
            vec!["© 2024 John Doe".to_string()],
        ),
    );

    registry.insert(
        "QuickTime:ContentCreateDate",
        TagDescriptor::new(
            TagId::new_named("©day"),
            "QuickTime:ContentCreateDate".to_string(),
            FormatFamily::QuickTime,
            true,
            ValueType::String,
            "Content creation date".to_string(),
            vec!["2024".to_string()],
        ),
    );

    registry.insert(
        "QuickTime:Director",
        TagDescriptor::new(
            TagId::new_named("©dir"),
            "QuickTime:Director".to_string(),
            FormatFamily::QuickTime,
            true,
            ValueType::String,
            "Director name".to_string(),
            vec!["Jane Smith".to_string()],
        ),
    );

    registry.insert(
        "QuickTime:Make",
        TagDescriptor::new(
            TagId::new_named("make"),
            "QuickTime:Make".to_string(),
            FormatFamily::QuickTime,
            true,
            ValueType::String,
            "Camera manufacturer".to_string(),
            vec!["Canon".to_string(), "Sony".to_string()],
        ),
    );

    registry.insert(
        "QuickTime:Model",
        TagDescriptor::new(
            TagId::new_named("model"),
            "QuickTime:Model".to_string(),
            FormatFamily::QuickTime,
            true,
            ValueType::String,
            "Camera model".to_string(),
            vec!["EOS 5D Mark IV".to_string()],
        ),
    );

    registry.insert(
        "QuickTime:GPSCoordinates",
        TagDescriptor::new(
            TagId::new_named("location.ISO6709"),
            "QuickTime:GPSCoordinates".to_string(),
            FormatFamily::QuickTime,
            true,
            ValueType::String,
            "GPS coordinates in ISO 6709 format".to_string(),
            vec!["+37.7749-122.4194/".to_string()],
        ),
    );

    registry.insert(
        "QuickTime:CreationTime",
        TagDescriptor::new(
            TagId::new_named("creation_time"),
            "QuickTime:CreationTime".to_string(),
            FormatFamily::QuickTime,
            true,
            ValueType::DateTime,
            "Creation timestamp".to_string(),
            vec!["2024-03-15T14:30:45Z".to_string()],
        ),
    );

    // --- More EXIF Tags (40 tags) ---
    registry.insert(
        "EXIF:MinSampleValue",
        TagDescriptor::new(
            TagId::new_numeric(0x0118),
            "EXIF:MinSampleValue".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Minimum component value".to_string(),
            vec!["0".to_string()],
        ),
    );

    registry.insert(
        "EXIF:MaxSampleValue",
        TagDescriptor::new(
            TagId::new_numeric(0x0119),
            "EXIF:MaxSampleValue".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Maximum component value".to_string(),
            vec!["255".to_string(), "65535".to_string()],
        ),
    );

    registry.insert(
        "EXIF:StripRowCounts",
        TagDescriptor::new(
            TagId::new_numeric(0x022F),
            "EXIF:StripRowCounts".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Number of rows per strip".to_string(),
            vec!["8".to_string()],
        ),
    );

    registry.insert(
        "EXIF:ApplicationNotes",
        TagDescriptor::new(
            TagId::new_numeric(0x02BC),
            "EXIF:ApplicationNotes".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Binary,
            "Application-specific data".to_string(),
            vec!["binary data".to_string()],
        ),
    );

    registry.insert(
        "EXIF:JPEGTables",
        TagDescriptor::new(
            TagId::new_numeric(0x015B),
            "EXIF:JPEGTables".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Binary,
            "JPEG quantization and/or Huffman tables".to_string(),
            vec!["binary".to_string()],
        ),
    );

    registry.insert(
        "EXIF:OPIProxy",
        TagDescriptor::new(
            TagId::new_numeric(0x015F),
            "EXIF:OPIProxy".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "OPI proxy indicator".to_string(),
            vec!["0".to_string(), "1".to_string()],
        ),
    );

    registry.insert(
        "EXIF:GlobalParametersIFD",
        TagDescriptor::new(
            TagId::new_numeric(0x0190),
            "EXIF:GlobalParametersIFD".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Pointer to global parameters IFD".to_string(),
            vec!["0".to_string()],
        ),
    );

    registry.insert(
        "EXIF:ProfileType",
        TagDescriptor::new(
            TagId::new_numeric(0x0191),
            "EXIF:ProfileType".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Profile type".to_string(),
            vec!["0".to_string()],
        ),
    );

    registry.insert(
        "EXIF:FaxProfile",
        TagDescriptor::new(
            TagId::new_numeric(0x0192),
            "EXIF:FaxProfile".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Fax profile".to_string(),
            vec!["0".to_string(), "1".to_string()],
        ),
    );

    registry.insert(
        "EXIF:CodingMethods",
        TagDescriptor::new(
            TagId::new_numeric(0x0193),
            "EXIF:CodingMethods".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Coding methods".to_string(),
            vec!["1".to_string()],
        ),
    );

    registry.insert(
        "EXIF:VersionYear",
        TagDescriptor::new(
            TagId::new_numeric(0x0194),
            "EXIF:VersionYear".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Binary,
            "Version year".to_string(),
            vec!["2024".to_string()],
        ),
    );

    registry.insert(
        "EXIF:ModeNumber",
        TagDescriptor::new(
            TagId::new_numeric(0x0195),
            "EXIF:ModeNumber".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Mode number".to_string(),
            vec!["0".to_string()],
        ),
    );

    registry.insert(
        "EXIF:Decode",
        TagDescriptor::new(
            TagId::new_numeric(0x01B1),
            "EXIF:Decode".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Rational,
            "Decode array".to_string(),
            vec!["0 1".to_string()],
        ),
    );

    registry.insert(
        "EXIF:DefaultImageColor",
        TagDescriptor::new(
            TagId::new_numeric(0x01B2),
            "EXIF:DefaultImageColor".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Default image color".to_string(),
            vec!["0".to_string()],
        ),
    );

    registry.insert(
        "EXIF:T82Options",
        TagDescriptor::new(
            TagId::new_numeric(0x01B3),
            "EXIF:T82Options".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "T82 compression options".to_string(),
            vec!["0".to_string()],
        ),
    );

    registry.insert(
        "EXIF:ImageLayer",
        TagDescriptor::new(
            TagId::new_numeric(0x87AC),
            "EXIF:ImageLayer".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Image layer information".to_string(),
            vec!["0".to_string()],
        ),
    );

    registry.insert(
        "EXIF:HasselbladExif",
        TagDescriptor::new(
            TagId::new_numeric(0xC61A),
            "EXIF:HasselbladExif".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Binary,
            "Hasselblad EXIF data".to_string(),
            vec!["binary".to_string()],
        ),
    );

    registry.insert(
        "EXIF:HasselbladRawImage",
        TagDescriptor::new(
            TagId::new_numeric(0xC61B),
            "EXIF:HasselbladRawImage".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Binary,
            "Hasselblad raw image data".to_string(),
            vec!["binary".to_string()],
        ),
    );

    registry.insert(
        "EXIF:FlashpixReady",
        TagDescriptor::new(
            TagId::new_numeric(0xA20E),
            "EXIF:FlashpixReady".to_string(),
            FormatFamily::EXIF,
            false,
            ValueType::Integer,
            "FlashPix ready flag".to_string(),
            vec!["0".to_string(), "1".to_string()],
        ),
    );

    registry.insert(
        "EXIF:LensFirmwareVersion",
        TagDescriptor::new(
            TagId::new_numeric(0xA437),
            "EXIF:LensFirmwareVersion".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::String,
            "Lens firmware version".to_string(),
            vec!["1.0.0".to_string()],
        ),
    );

    registry.insert(
        "EXIF:ImageSourceData",
        TagDescriptor::new(
            TagId::new_numeric(0x935C),
            "EXIF:ImageSourceData".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Binary,
            "Original raw file data".to_string(),
            vec!["binary".to_string()],
        ),
    );

    registry.insert(
        "EXIF:StoNits",
        TagDescriptor::new(
            TagId::new_numeric(0x37F),
            "EXIF:StoNits".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Rational,
            "Sample to nits conversion".to_string(),
            vec!["1.0".to_string()],
        ),
    );

    registry.insert(
        "EXIF:CFALayout",
        TagDescriptor::new(
            TagId::new_numeric(0xA302),
            "EXIF:CFALayout".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "CFA layout (1=Rectangular, 2=Even columns offset, 3=Even rows offset, 4=Odd columns offset, 5=Odd rows offset)".to_string(),
            vec!["1".to_string()],
        ),
    );

    registry.insert(
        "EXIF:LinearizationTable",
        TagDescriptor::new(
            TagId::new_numeric(0xC618),
            "EXIF:LinearizationTable".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Linearization table for raw images".to_string(),
            vec!["0".to_string()],
        ),
    );

    registry.insert(
        "EXIF:BlackLevelRepeatDim",
        TagDescriptor::new(
            TagId::new_numeric(0xC619),
            "EXIF:BlackLevelRepeatDim".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "Black level repeat dimensions".to_string(),
            vec!["2 2".to_string()],
        ),
    );

    registry.insert(
        "EXIF:BlackLevel",
        TagDescriptor::new(
            TagId::new_numeric(0xC61A),
            "EXIF:BlackLevel".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Rational,
            "Black level values".to_string(),
            vec!["256".to_string()],
        ),
    );

    registry.insert(
        "EXIF:BlackLevelDeltaH",
        TagDescriptor::new(
            TagId::new_numeric(0xC61B),
            "EXIF:BlackLevelDeltaH".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Rational,
            "Black level delta horizontal".to_string(),
            vec!["0".to_string()],
        ),
    );

    registry.insert(
        "EXIF:BlackLevelDeltaV",
        TagDescriptor::new(
            TagId::new_numeric(0xC61C),
            "EXIF:BlackLevelDeltaV".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Rational,
            "Black level delta vertical".to_string(),
            vec!["0".to_string()],
        ),
    );

    registry.insert(
        "EXIF:WhiteLevel",
        TagDescriptor::new(
            TagId::new_numeric(0xC61D),
            "EXIF:WhiteLevel".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Integer,
            "White level".to_string(),
            vec!["4095".to_string(), "16383".to_string()],
        ),
    );

    registry.insert(
        "EXIF:DefaultScale",
        TagDescriptor::new(
            TagId::new_numeric(0xC61E),
            "EXIF:DefaultScale".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Rational,
            "Default scale for raw images".to_string(),
            vec!["1 1".to_string()],
        ),
    );

    registry.insert(
        "EXIF:DefaultCropOrigin",
        TagDescriptor::new(
            TagId::new_numeric(0xC61F),
            "EXIF:DefaultCropOrigin".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Rational,
            "Default crop origin".to_string(),
            vec!["0 0".to_string()],
        ),
    );

    registry.insert(
        "EXIF:DefaultCropSize",
        TagDescriptor::new(
            TagId::new_numeric(0xC620),
            "EXIF:DefaultCropSize".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Rational,
            "Default crop size".to_string(),
            vec!["4000 3000".to_string()],
        ),
    );

    registry.insert(
        "EXIF:ColorMatrix1",
        TagDescriptor::new(
            TagId::new_numeric(0xC621),
            "EXIF:ColorMatrix1".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Rational,
            "Color matrix 1".to_string(),
            vec!["1.0 0.0 0.0".to_string()],
        ),
    );

    registry.insert(
        "EXIF:ColorMatrix2",
        TagDescriptor::new(
            TagId::new_numeric(0xC622),
            "EXIF:ColorMatrix2".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Rational,
            "Color matrix 2".to_string(),
            vec!["1.0 0.0 0.0".to_string()],
        ),
    );

    registry.insert(
        "EXIF:CameraCalibration1",
        TagDescriptor::new(
            TagId::new_numeric(0xC623),
            "EXIF:CameraCalibration1".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Rational,
            "Camera calibration matrix 1".to_string(),
            vec!["1.0".to_string()],
        ),
    );

    registry.insert(
        "EXIF:CameraCalibration2",
        TagDescriptor::new(
            TagId::new_numeric(0xC624),
            "EXIF:CameraCalibration2".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Rational,
            "Camera calibration matrix 2".to_string(),
            vec!["1.0".to_string()],
        ),
    );

    registry.insert(
        "EXIF:ReductionMatrix1",
        TagDescriptor::new(
            TagId::new_numeric(0xC625),
            "EXIF:ReductionMatrix1".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Rational,
            "Reduction matrix 1".to_string(),
            vec!["1.0".to_string()],
        ),
    );

    registry.insert(
        "EXIF:ReductionMatrix2",
        TagDescriptor::new(
            TagId::new_numeric(0xC626),
            "EXIF:ReductionMatrix2".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Rational,
            "Reduction matrix 2".to_string(),
            vec!["1.0".to_string()],
        ),
    );

    registry.insert(
        "EXIF:AnalogBalance",
        TagDescriptor::new(
            TagId::new_numeric(0xC627),
            "EXIF:AnalogBalance".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Rational,
            "Analog balance values".to_string(),
            vec!["1.0 1.0 1.0".to_string()],
        ),
    );

    registry.insert(
        "EXIF:AsShotNeutral",
        TagDescriptor::new(
            TagId::new_numeric(0xC628),
            "EXIF:AsShotNeutral".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Rational,
            "As shot neutral coordinates".to_string(),
            vec!["0.5 0.5 0.5".to_string()],
        ),
    );

    // --- More XMP Tags (6 tags) ---
    registry.insert(
        "XMP:History",
        TagDescriptor::new(
            TagId::new_named("XMP-xmp:History"),
            "XMP:History".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::Struct,
            "History of processing steps".to_string(),
            vec!["Edit history".to_string()],
        ),
    );

    registry.insert(
        "XMP:DerivedFrom",
        TagDescriptor::new(
            TagId::new_named("XMP-xmp:DerivedFrom"),
            "XMP:DerivedFrom".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::Struct,
            "Source document reference".to_string(),
            vec!["Original file reference".to_string()],
        ),
    );

    registry.insert(
        "XMP:CountryName",
        TagDescriptor::new(
            TagId::new_named("XMP-iptcExt:LocationCreated"),
            "XMP:CountryName".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Country where content was created".to_string(),
            vec!["United States".to_string()],
        ),
    );

    registry.insert(
        "XMP:PersonInImage",
        TagDescriptor::new(
            TagId::new_named("XMP-iptcExt:PersonInImage"),
            "XMP:PersonInImage".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Names of people shown in image".to_string(),
            vec!["John Doe".to_string()],
        ),
    );

    registry.insert(
        "XMP:Event",
        TagDescriptor::new(
            TagId::new_named("XMP-iptcExt:Event"),
            "XMP:Event".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Event depicted in content".to_string(),
            vec!["Conference 2024".to_string()],
        ),
    );

    registry.insert(
        "XMP:OrganisationInImageName",
        TagDescriptor::new(
            TagId::new_named("XMP-iptcExt:OrganisationInImageName"),
            "XMP:OrganisationInImageName".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Organisation name shown in image".to_string(),
            vec!["Acme Corp".to_string()],
        ),
    );

    // ===========================
    // GPS TAGS (32 total)
    // ===========================

    registry.insert(
        "GPS:GPSLatitudeRef",
        TagDescriptor::new(
            TagId::new_numeric(0x0001),
            "GPS:GPSLatitudeRef".to_string(),
            FormatFamily::GPS,
            true,
            ValueType::String,
            "Latitude reference (N=North, S=South)".to_string(),
            vec!["N".to_string(), "S".to_string()],
        ),
    );

    registry.insert(
        "GPS:GPSLatitude",
        TagDescriptor::new(
            TagId::new_numeric(0x0002),
            "GPS:GPSLatitude".to_string(),
            FormatFamily::GPS,
            true,
            ValueType::Rational,
            "Latitude in degrees, minutes, seconds".to_string(),
            vec!["37 46 30.5".to_string(), "51 30 26.5".to_string()],
        ),
    );

    registry.insert(
        "GPS:GPSLongitudeRef",
        TagDescriptor::new(
            TagId::new_numeric(0x0003),
            "GPS:GPSLongitudeRef".to_string(),
            FormatFamily::GPS,
            true,
            ValueType::String,
            "Longitude reference (E=East, W=West)".to_string(),
            vec!["W".to_string(), "E".to_string()],
        ),
    );

    registry.insert(
        "GPS:GPSLongitude",
        TagDescriptor::new(
            TagId::new_numeric(0x0004),
            "GPS:GPSLongitude".to_string(),
            FormatFamily::GPS,
            true,
            ValueType::Rational,
            "Longitude in degrees, minutes, seconds".to_string(),
            vec!["122 25 9.8".to_string(), "0 7 39.9".to_string()],
        ),
    );

    registry.insert(
        "GPS:GPSAltitudeRef",
        TagDescriptor::new(
            TagId::new_numeric(0x0005),
            "GPS:GPSAltitudeRef".to_string(),
            FormatFamily::GPS,
            true,
            ValueType::Integer,
            "Altitude reference (0=Above sea level, 1=Below sea level)".to_string(),
            vec!["0".to_string(), "1".to_string()],
        ),
    );

    registry.insert(
        "GPS:GPSAltitude",
        TagDescriptor::new(
            TagId::new_numeric(0x0006),
            "GPS:GPSAltitude".to_string(),
            FormatFamily::GPS,
            true,
            ValueType::Rational,
            "Altitude in meters".to_string(),
            vec!["150.5".to_string(), "1234.2".to_string()],
        ),
    );

    registry.insert(
        "GPS:GPSTimeStamp",
        TagDescriptor::new(
            TagId::new_numeric(0x0007),
            "GPS:GPSTimeStamp".to_string(),
            FormatFamily::GPS,
            true,
            ValueType::Rational,
            "GPS time as UTC (hours, minutes, seconds)".to_string(),
            vec!["14 30 45.5".to_string(), "09 15 22.3".to_string()],
        ),
    );

    registry.insert(
        "GPS:GPSDateStamp",
        TagDescriptor::new(
            TagId::new_numeric(0x001D),
            "GPS:GPSDateStamp".to_string(),
            FormatFamily::GPS,
            true,
            ValueType::String,
            "GPS date in YYYY:MM:DD format".to_string(),
            vec!["2024:03:15".to_string(), "2024:11:25".to_string()],
        ),
    );

    registry.insert(
        "GPS:GPSSatellites",
        TagDescriptor::new(
            TagId::new_numeric(0x0008),
            "GPS:GPSSatellites".to_string(),
            FormatFamily::GPS,
            true,
            ValueType::String,
            "GPS satellites used for measurement".to_string(),
            vec!["12".to_string(), "08".to_string()],
        ),
    );

    registry.insert(
        "GPS:GPSStatus",
        TagDescriptor::new(
            TagId::new_numeric(0x0009),
            "GPS:GPSStatus".to_string(),
            FormatFamily::GPS,
            true,
            ValueType::String,
            "GPS receiver status (A=Active, V=Void)".to_string(),
            vec!["A".to_string(), "V".to_string()],
        ),
    );

    registry.insert(
        "GPS:GPSMeasureMode",
        TagDescriptor::new(
            TagId::new_numeric(0x000A),
            "GPS:GPSMeasureMode".to_string(),
            FormatFamily::GPS,
            true,
            ValueType::String,
            "GPS measurement mode (2=2D, 3=3D)".to_string(),
            vec!["3".to_string(), "2".to_string()],
        ),
    );

    registry.insert(
        "GPS:GPSDOP",
        TagDescriptor::new(
            TagId::new_numeric(0x000B),
            "GPS:GPSDOP".to_string(),
            FormatFamily::GPS,
            true,
            ValueType::Rational,
            "GPS dilution of precision".to_string(),
            vec!["1.5".to_string(), "2.3".to_string()],
        ),
    );

    registry.insert(
        "GPS:GPSSpeedRef",
        TagDescriptor::new(
            TagId::new_numeric(0x000C),
            "GPS:GPSSpeedRef".to_string(),
            FormatFamily::GPS,
            true,
            ValueType::String,
            "GPS speed reference (K=km/h, M=mph, N=knots)".to_string(),
            vec!["K".to_string(), "M".to_string()],
        ),
    );

    registry.insert(
        "GPS:GPSSpeed",
        TagDescriptor::new(
            TagId::new_numeric(0x000D),
            "GPS:GPSSpeed".to_string(),
            FormatFamily::GPS,
            true,
            ValueType::Rational,
            "GPS receiver speed".to_string(),
            vec!["45.5".to_string(), "0".to_string()],
        ),
    );

    registry.insert(
        "GPS:GPSTrackRef",
        TagDescriptor::new(
            TagId::new_numeric(0x000E),
            "GPS:GPSTrackRef".to_string(),
            FormatFamily::GPS,
            true,
            ValueType::String,
            "GPS track reference (T=True north, M=Magnetic north)".to_string(),
            vec!["T".to_string(), "M".to_string()],
        ),
    );

    registry.insert(
        "GPS:GPSTrack",
        TagDescriptor::new(
            TagId::new_numeric(0x000F),
            "GPS:GPSTrack".to_string(),
            FormatFamily::GPS,
            true,
            ValueType::Rational,
            "GPS direction of movement in degrees".to_string(),
            vec!["135.5".to_string(), "270".to_string()],
        ),
    );

    registry.insert(
        "GPS:GPSImgDirectionRef",
        TagDescriptor::new(
            TagId::new_numeric(0x0010),
            "GPS:GPSImgDirectionRef".to_string(),
            FormatFamily::GPS,
            true,
            ValueType::String,
            "GPS image direction reference (T=True north, M=Magnetic north)".to_string(),
            vec!["T".to_string(), "M".to_string()],
        ),
    );

    registry.insert(
        "GPS:GPSImgDirection",
        TagDescriptor::new(
            TagId::new_numeric(0x0011),
            "GPS:GPSImgDirection".to_string(),
            FormatFamily::GPS,
            true,
            ValueType::Rational,
            "GPS direction of image when captured in degrees".to_string(),
            vec!["90.5".to_string(), "180".to_string()],
        ),
    );

    registry.insert(
        "GPS:GPSMapDatum",
        TagDescriptor::new(
            TagId::new_numeric(0x0012),
            "GPS:GPSMapDatum".to_string(),
            FormatFamily::GPS,
            true,
            ValueType::String,
            "GPS geodetic survey data used".to_string(),
            vec!["WGS-84".to_string(), "NAD-83".to_string()],
        ),
    );

    registry.insert(
        "GPS:GPSVersionID",
        TagDescriptor::new(
            TagId::new_numeric(0x0000),
            "GPS:GPSVersionID".to_string(),
            FormatFamily::GPS,
            false,
            ValueType::Binary,
            "GPS tag version".to_string(),
            vec!["2.3.0.0".to_string(), "2.2.0.0".to_string()],
        ),
    );

    // ===========================
    // XMP TAGS (100+ total)
    // ===========================

    // --- Dublin Core (10 tags) ---
    registry.insert(
        "XMP:Creator",
        TagDescriptor::new(
            TagId::new_named("XMP-dc:Creator"),
            "XMP:Creator".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Creator or author of the document".to_string(),
            vec!["John Doe".to_string(), "Jane Smith".to_string()],
        ),
    );

    registry.insert(
        "XMP:Rights",
        TagDescriptor::new(
            TagId::new_named("XMP-dc:Rights"),
            "XMP:Rights".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Copyright and usage rights information".to_string(),
            vec!["© 2024 John Doe. All rights reserved.".to_string()],
        ),
    );

    registry.insert(
        "XMP:Title",
        TagDescriptor::new(
            TagId::new_named("XMP-dc:Title"),
            "XMP:Title".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Title or name of the document".to_string(),
            vec![
                "Mountain Landscape".to_string(),
                "City Skyline at Dusk".to_string(),
            ],
        ),
    );

    registry.insert(
        "XMP:Description",
        TagDescriptor::new(
            TagId::new_named("XMP-dc:Description"),
            "XMP:Description".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Textual description of the content".to_string(),
            vec!["A beautiful sunset over the mountains".to_string()],
        ),
    );

    registry.insert(
        "XMP:Subject",
        TagDescriptor::new(
            TagId::new_named("XMP-dc:Subject"),
            "XMP:Subject".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Keywords or topics describing the content".to_string(),
            vec![
                "landscape, nature, mountains".to_string(),
                "urban, architecture".to_string(),
            ],
        ),
    );

    registry.insert(
        "XMP:Publisher",
        TagDescriptor::new(
            TagId::new_named("XMP-dc:Publisher"),
            "XMP:Publisher".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Publisher of the document".to_string(),
            vec![
                "Photography Magazine".to_string(),
                "Nature Publications".to_string(),
            ],
        ),
    );

    registry.insert(
        "XMP:Contributor",
        TagDescriptor::new(
            TagId::new_named("XMP-dc:Contributor"),
            "XMP:Contributor".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Contributors to the content".to_string(),
            vec![
                "Assistant Editor".to_string(),
                "Photo Retoucher".to_string(),
            ],
        ),
    );

    registry.insert(
        "XMP:Date",
        TagDescriptor::new(
            TagId::new_named("XMP-dc:Date"),
            "XMP:Date".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::DateTime,
            "Date associated with the content".to_string(),
            vec!["2024-03-15T14:30:45Z".to_string(), "2024-11-25".to_string()],
        ),
    );

    registry.insert(
        "XMP:Format",
        TagDescriptor::new(
            TagId::new_named("XMP-dc:Format"),
            "XMP:Format".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "MIME type of the resource".to_string(),
            vec!["image/jpeg".to_string(), "image/png".to_string()],
        ),
    );

    registry.insert(
        "XMP:Language",
        TagDescriptor::new(
            TagId::new_named("XMP-dc:Language"),
            "XMP:Language".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Language of the content".to_string(),
            vec!["en-US".to_string(), "fr-FR".to_string()],
        ),
    );

    // --- XMP Basic (10 tags) ---
    registry.insert(
        "XMP:CreateDate",
        TagDescriptor::new(
            TagId::new_named("XMP-xmp:CreateDate"),
            "XMP:CreateDate".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::DateTime,
            "Date and time the resource was created".to_string(),
            vec![
                "2024-03-15T14:30:45-07:00".to_string(),
                "2024-11-25T10:20:30Z".to_string(),
            ],
        ),
    );

    registry.insert(
        "XMP:ModifyDate",
        TagDescriptor::new(
            TagId::new_named("XMP-xmp:ModifyDate"),
            "XMP:ModifyDate".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::DateTime,
            "Date and time the resource was last modified".to_string(),
            vec!["2024-03-16T09:15:00-07:00".to_string()],
        ),
    );

    registry.insert(
        "XMP:MetadataDate",
        TagDescriptor::new(
            TagId::new_named("XMP-xmp:MetadataDate"),
            "XMP:MetadataDate".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::DateTime,
            "Date and time the metadata was last changed".to_string(),
            vec!["2024-03-17T12:00:00Z".to_string()],
        ),
    );

    registry.insert(
        "XMP:CreatorTool",
        TagDescriptor::new(
            TagId::new_named("XMP-xmp:CreatorTool"),
            "XMP:CreatorTool".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Name of the tool that created the resource".to_string(),
            vec![
                "Adobe Photoshop 2024".to_string(),
                "GIMP 2.10.34".to_string(),
            ],
        ),
    );

    registry.insert(
        "XMP:Rating",
        TagDescriptor::new(
            TagId::new_named("XMP-xmp:Rating"),
            "XMP:Rating".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::Integer,
            "User-assigned rating (typically 1-5)".to_string(),
            vec!["5".to_string(), "3".to_string(), "0".to_string()],
        ),
    );

    registry.insert(
        "XMP:Label",
        TagDescriptor::new(
            TagId::new_named("XMP-xmp:Label"),
            "XMP:Label".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "User-defined label or color classification".to_string(),
            vec!["Red".to_string(), "Green".to_string(), "Select".to_string()],
        ),
    );

    registry.insert(
        "XMP:Nickname",
        TagDescriptor::new(
            TagId::new_named("XMP-xmp:Nickname"),
            "XMP:Nickname".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Short informal name for the resource".to_string(),
            vec!["sunset_best".to_string(), "IMG_final".to_string()],
        ),
    );

    registry.insert(
        "XMP:Identifier",
        TagDescriptor::new(
            TagId::new_named("XMP-xmp:Identifier"),
            "XMP:Identifier".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Unique identifier for the resource".to_string(),
            vec!["urn:uuid:12345678-1234-1234-1234-123456789012".to_string()],
        ),
    );

    registry.insert(
        "XMP:BaseURL",
        TagDescriptor::new(
            TagId::new_named("XMP-xmp:BaseURL"),
            "XMP:BaseURL".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Base URL for relative URLs in the document".to_string(),
            vec!["https://example.com/images/".to_string()],
        ),
    );

    registry.insert(
        "XMP:Thumbnails",
        TagDescriptor::new(
            TagId::new_named("XMP-xmp:Thumbnails"),
            "XMP:Thumbnails".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::Struct,
            "Thumbnail images for the resource".to_string(),
            vec!["160x120 thumbnail".to_string()],
        ),
    );

    registry
});

/// Retrieves a tag descriptor by its canonical name.
///
/// # Arguments
/// * `name` - The canonical tag name (e.g., "EXIF:Make", "GPS:GPSLatitude", "XMP:Creator")
///
/// # Returns
/// * `Some(&TagDescriptor)` if the tag is registered
/// * `None` if the tag is not found in the registry
///
/// # Examples
/// ```
/// use exiftool_rs::tag_db::tag_registry::get_tag_descriptor;
///
/// let tag = get_tag_descriptor("EXIF:Make");
/// assert!(tag.is_some());
/// assert_eq!(tag.unwrap().name(), "EXIF:Make");
///
/// let unknown = get_tag_descriptor("UNKNOWN:Tag");
/// assert!(unknown.is_none());
/// ```
pub fn get_tag_descriptor(name: &str) -> Option<&TagDescriptor> {
    // Try direct lookup first
    if let Some(descriptor) = TAG_REGISTRY.get(name) {
        return Some(descriptor);
    }

    // Try generated registry direct match
    if let Some(descriptor) = GENERATED_TAG_REGISTRY.get(name) {
        return Some(descriptor);
    }

    // Handle IFD prefix mapping for validation
    // When reading metadata, parsers output "IFD0:Make" but registry has "EXIF:Make"
    // Need to normalize IFD0/IFD1/ExifIFD/GPS prefixes to EXIF/GPS for lookup
    let normalized_name = if name.starts_with("IFD0:")
        || name.starts_with("IFD1:")
        || name.starts_with("ExifIFD:")
        || name.starts_with("InteropIFD:")
    {
        // Replace IFD prefix with "EXIF:" prefix
        if let Some(colon_pos) = name.find(':') {
            let tag_base_name = &name[colon_pos + 1..];
            format!("EXIF:{}", tag_base_name)
        } else {
            return None;
        }
    } else {
        // GPS and other families stay as-is
        // Try generated registry before giving up
        return GENERATED_TAG_REGISTRY.get(name);
    };

    TAG_REGISTRY
        .get(normalized_name.as_str())
        .or_else(|| GENERATED_TAG_REGISTRY.get(normalized_name.as_str()))
}

/// Returns the total number of tags in the registry.
///
/// This should return 500+ tags for the expanded implementation.
pub fn tag_count() -> usize {
    TAG_REGISTRY.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_count() {
        let count = tag_count();
        assert!(
            count >= 500,
            "Registry must contain at least 500 tags, found {}",
            count
        );
    }

    #[test]
    fn test_exif_make_lookup() {
        let tag = get_tag_descriptor("EXIF:Make");
        assert!(tag.is_some(), "EXIF:Make should be registered");
        let tag = tag.unwrap();
        assert_eq!(tag.name(), "EXIF:Make");
        assert_eq!(tag.format(), FormatFamily::EXIF);
        assert_eq!(tag.value_type(), ValueType::String);
        assert!(tag.is_writable());
    }

    #[test]
    fn test_exif_model_lookup() {
        let tag = get_tag_descriptor("EXIF:Model");
        assert!(tag.is_some());
        assert_eq!(tag.unwrap().format(), FormatFamily::EXIF);
    }

    #[test]
    fn test_exif_datetime_lookup() {
        let tag = get_tag_descriptor("EXIF:DateTime");
        assert!(tag.is_some());
        let tag = tag.unwrap();
        assert_eq!(tag.value_type(), ValueType::DateTime);
        assert!(tag.is_writable());
    }

    #[test]
    fn test_exif_exposure_time_lookup() {
        let tag = get_tag_descriptor("EXIF:ExposureTime");
        assert!(tag.is_some());
        let tag = tag.unwrap();
        assert_eq!(tag.value_type(), ValueType::Rational);
        assert_eq!(tag.format(), FormatFamily::EXIF);
    }

    #[test]
    fn test_exif_fnumber_lookup() {
        let tag = get_tag_descriptor("EXIF:FNumber");
        assert!(tag.is_some());
        assert_eq!(tag.unwrap().value_type(), ValueType::Rational);
    }

    #[test]
    fn test_exif_iso_lookup() {
        let tag = get_tag_descriptor("EXIF:ISO");
        assert!(tag.is_some());
        let tag = tag.unwrap();
        assert_eq!(tag.value_type(), ValueType::Integer);
        assert!(tag.is_writable());
    }

    #[test]
    fn test_gps_latitude_lookup() {
        let tag = get_tag_descriptor("GPS:GPSLatitude");
        assert!(tag.is_some());
        let tag = tag.unwrap();
        assert_eq!(tag.format(), FormatFamily::GPS);
        assert_eq!(tag.value_type(), ValueType::Rational);
        assert!(tag.is_writable());
    }

    #[test]
    fn test_gps_longitude_lookup() {
        let tag = get_tag_descriptor("GPS:GPSLongitude");
        assert!(tag.is_some());
        assert_eq!(tag.unwrap().format(), FormatFamily::GPS);
    }

    #[test]
    fn test_gps_altitude_lookup() {
        let tag = get_tag_descriptor("GPS:GPSAltitude");
        assert!(tag.is_some());
        let tag = tag.unwrap();
        assert_eq!(tag.format(), FormatFamily::GPS);
        assert_eq!(tag.value_type(), ValueType::Rational);
    }

    #[test]
    fn test_xmp_creator_lookup() {
        let tag = get_tag_descriptor("XMP:Creator");
        assert!(tag.is_some());
        let tag = tag.unwrap();
        assert_eq!(tag.format(), FormatFamily::XMP);
        assert_eq!(tag.value_type(), ValueType::String);
        assert!(tag.is_writable());
    }

    #[test]
    fn test_xmp_rights_lookup() {
        let tag = get_tag_descriptor("XMP:Rights");
        assert!(tag.is_some());
        assert_eq!(tag.unwrap().format(), FormatFamily::XMP);
    }

    #[test]
    fn test_xmp_create_date_lookup() {
        let tag = get_tag_descriptor("XMP:CreateDate");
        assert!(tag.is_some());
        let tag = tag.unwrap();
        assert_eq!(tag.value_type(), ValueType::DateTime);
        assert_eq!(tag.format(), FormatFamily::XMP);
    }

    #[test]
    fn test_unknown_tag_returns_none() {
        let tag = get_tag_descriptor("UNKNOWN:Tag");
        assert!(tag.is_none(), "Unknown tags should return None");
    }

    #[test]
    fn test_case_sensitive_lookup() {
        let tag = get_tag_descriptor("exif:make");
        assert!(tag.is_none(), "Tag names should be case-sensitive");
    }

    #[test]
    fn test_all_tags_have_valid_properties() {
        for (name, descriptor) in TAG_REGISTRY.iter() {
            assert_eq!(descriptor.name(), *name, "Tag name mismatch for {}", name);
            assert!(
                !descriptor.description().is_empty(),
                "Tag {} has empty description",
                name
            );
            assert!(
                !descriptor.examples().is_empty(),
                "Tag {} has no examples",
                name
            );
        }
    }

    #[test]
    fn test_tag_distribution() {
        let mut exif_count = 0;
        let mut gps_count = 0;
        let mut xmp_count = 0;
        let mut iptc_count = 0;
        let mut pdf_count = 0;
        let mut quicktime_count = 0;
        let mut makernotes_count = 0;

        for descriptor in TAG_REGISTRY.values() {
            match descriptor.format() {
                FormatFamily::EXIF => exif_count += 1,
                FormatFamily::GPS => gps_count += 1,
                FormatFamily::XMP => xmp_count += 1,
                FormatFamily::IPTC => iptc_count += 1,
                FormatFamily::PDF => pdf_count += 1,
                FormatFamily::QuickTime => quicktime_count += 1,
                FormatFamily::MakerNotes => makernotes_count += 1,
                _ => {}
            }
        }

        assert!(
            exif_count >= 150,
            "Expected at least 150 EXIF tags, found {}",
            exif_count
        );
        assert!(
            makernotes_count >= 90,
            "Expected at least 90 MakerNotes tags, found {}",
            makernotes_count
        );
        assert!(
            gps_count >= 30,
            "Expected at least 30 GPS tags, found {}",
            gps_count
        );
        assert!(
            xmp_count >= 100,
            "Expected at least 100 XMP tags, found {}",
            xmp_count
        );
        assert!(
            iptc_count >= 50,
            "Expected at least 50 IPTC tags, found {}",
            iptc_count
        );
        assert!(
            pdf_count >= 10,
            "Expected at least 10 PDF tags, found {}",
            pdf_count
        );
        assert!(
            quicktime_count >= 10,
            "Expected at least 10 QuickTime tags, found {}",
            quicktime_count
        );

        let total = exif_count
            + makernotes_count
            + gps_count
            + xmp_count
            + iptc_count
            + pdf_count
            + quicktime_count;
        assert!(
            total >= 500,
            "Total tag count should be at least 500, found {}",
            total
        );
    }

    #[test]
    fn test_numeric_tag_ids_for_exif_gps() {
        let exif_make = get_tag_descriptor("EXIF:Make").unwrap();
        assert!(
            exif_make.id().is_numeric(),
            "EXIF tags should have numeric IDs"
        );

        let gps_lat = get_tag_descriptor("GPS:GPSLatitude").unwrap();
        assert!(
            gps_lat.id().is_numeric(),
            "GPS tags should have numeric IDs"
        );
    }

    #[test]
    fn test_named_tag_ids_for_xmp() {
        let xmp_creator = get_tag_descriptor("XMP:Creator").unwrap();
        assert!(
            xmp_creator.id().is_named(),
            "XMP tags should have named IDs"
        );
    }

    // Additional test cases for new tags across all families

    // EXIF tags tests
    #[test]
    fn test_exif_subfile_type_lookup() {
        let tag = get_tag_descriptor("EXIF:SubfileType");
        assert!(tag.is_some(), "EXIF:SubfileType should be registered");
        assert_eq!(tag.unwrap().format(), FormatFamily::EXIF);
    }

    #[test]
    fn test_exif_tile_width_lookup() {
        let tag = get_tag_descriptor("EXIF:TileWidth");
        assert!(tag.is_some());
        assert_eq!(tag.unwrap().value_type(), ValueType::Integer);
    }

    #[test]
    fn test_exif_spectral_sensitivity_lookup() {
        let tag = get_tag_descriptor("EXIF:SpectralSensitivity");
        assert!(tag.is_some());
    }

    #[test]
    fn test_exif_gamma_value_lookup() {
        let tag = get_tag_descriptor("EXIF:GammaValue");
        assert!(tag.is_some());
        assert_eq!(tag.unwrap().value_type(), ValueType::Rational);
    }

    #[test]
    fn test_exif_image_unique_id_lookup() {
        let tag = get_tag_descriptor("EXIF:ImageUniqueID");
        assert!(tag.is_some());
        assert_eq!(tag.unwrap().value_type(), ValueType::String);
    }

    // Canon MakerNotes tests
    #[test]
    fn test_canon_model_id_lookup() {
        let tag = get_tag_descriptor("Canon:ModelID");
        assert!(tag.is_some(), "Canon:ModelID should be registered");
        assert_eq!(tag.unwrap().format(), FormatFamily::MakerNotes);
    }

    #[test]
    fn test_canon_firmware_version_lookup() {
        let tag = get_tag_descriptor("Canon:CanonFirmwareVersion");
        assert!(tag.is_some());
        assert_eq!(tag.unwrap().format(), FormatFamily::MakerNotes);
    }

    #[test]
    fn test_canon_lens_model_lookup() {
        let tag = get_tag_descriptor("Canon:LensModel");
        assert!(tag.is_some());
    }

    #[test]
    fn test_canon_focus_mode_lookup() {
        let tag = get_tag_descriptor("Canon:FocusMode");
        assert!(tag.is_some());
    }

    #[test]
    fn test_canon_macro_mode_lookup() {
        let tag = get_tag_descriptor("Canon:MacroMode");
        assert!(tag.is_some());
    }

    // Nikon MakerNotes tests
    #[test]
    fn test_nikon_maker_note_version_lookup() {
        let tag = get_tag_descriptor("Nikon:MakerNoteVersion");
        assert!(tag.is_some(), "Nikon:MakerNoteVersion should be registered");
        assert_eq!(tag.unwrap().format(), FormatFamily::MakerNotes);
    }

    #[test]
    fn test_nikon_color_mode_lookup() {
        let tag = get_tag_descriptor("Nikon:ColorMode");
        assert!(tag.is_some());
    }

    #[test]
    fn test_nikon_white_balance_lookup() {
        let tag = get_tag_descriptor("Nikon:WhiteBalance");
        assert!(tag.is_some());
    }

    #[test]
    fn test_nikon_lens_type_lookup() {
        let tag = get_tag_descriptor("Nikon:LensType");
        assert!(tag.is_some());
    }

    #[test]
    fn test_nikon_shooting_mode_lookup() {
        let tag = get_tag_descriptor("Nikon:ShootingMode");
        assert!(tag.is_some());
    }

    // Sony MakerNotes tests
    #[test]
    fn test_sony_model_id_lookup() {
        let tag = get_tag_descriptor("Sony:SonyModelID");
        assert!(tag.is_some(), "Sony:SonyModelID should be registered");
        assert_eq!(tag.unwrap().format(), FormatFamily::MakerNotes);
    }

    #[test]
    fn test_sony_af_mode_lookup() {
        let tag = get_tag_descriptor("Sony:AFMode");
        assert!(tag.is_some());
    }

    #[test]
    fn test_sony_color_temperature_lookup() {
        let tag = get_tag_descriptor("Sony:ColorTemperature");
        assert!(tag.is_some());
    }

    #[test]
    fn test_sony_lens_id_lookup() {
        let tag = get_tag_descriptor("Sony:LensID");
        assert!(tag.is_some());
    }

    #[test]
    fn test_sony_brightness_value_lookup() {
        let tag = get_tag_descriptor("Sony:BrightnessValue");
        assert!(tag.is_some());
    }

    // GPS tags tests
    #[test]
    fn test_gps_dest_latitude_lookup() {
        let tag = get_tag_descriptor("GPS:GPSDestLatitude");
        assert!(tag.is_some(), "GPS:GPSDestLatitude should be registered");
        assert_eq!(tag.unwrap().format(), FormatFamily::GPS);
    }

    #[test]
    fn test_gps_dest_bearing_lookup() {
        let tag = get_tag_descriptor("GPS:GPSDestBearing");
        assert!(tag.is_some());
    }

    #[test]
    fn test_gps_processing_method_lookup() {
        let tag = get_tag_descriptor("GPS:GPSProcessingMethod");
        assert!(tag.is_some());
    }

    #[test]
    fn test_gps_area_information_lookup() {
        let tag = get_tag_descriptor("GPS:GPSAreaInformation");
        assert!(tag.is_some());
    }

    #[test]
    fn test_gps_h_positioning_error_lookup() {
        let tag = get_tag_descriptor("GPS:GPSHPositioningError");
        assert!(tag.is_some());
        assert_eq!(tag.unwrap().value_type(), ValueType::Rational);
    }

    // XMP tags tests
    #[test]
    fn test_xmp_source_lookup() {
        let tag = get_tag_descriptor("XMP:Source");
        assert!(tag.is_some(), "XMP:Source should be registered");
        assert_eq!(tag.unwrap().format(), FormatFamily::XMP);
    }

    #[test]
    fn test_xmp_location_lookup() {
        let tag = get_tag_descriptor("XMP:Location");
        assert!(tag.is_some());
    }

    #[test]
    fn test_xmp_country_code_lookup() {
        let tag = get_tag_descriptor("XMP:CountryCode");
        assert!(tag.is_some());
    }

    #[test]
    fn test_xmp_temperature_lookup() {
        let tag = get_tag_descriptor("XMP:Temperature");
        assert!(tag.is_some());
    }

    #[test]
    fn test_xmp_exposure_2012_lookup() {
        let tag = get_tag_descriptor("XMP:Exposure2012");
        assert!(tag.is_some());
        assert_eq!(tag.unwrap().value_type(), ValueType::Float);
    }

    #[test]
    fn test_xmp_credit_lookup() {
        let tag = get_tag_descriptor("XMP:Credit");
        assert!(tag.is_some());
    }

    #[test]
    fn test_xmp_usage_terms_lookup() {
        let tag = get_tag_descriptor("XMP:UsageTerms");
        assert!(tag.is_some());
    }

    // IPTC tags tests
    #[test]
    fn test_iptc_headline_lookup() {
        let tag = get_tag_descriptor("IPTC:Headline");
        assert!(tag.is_some(), "IPTC:Headline should be registered");
        assert_eq!(tag.unwrap().format(), FormatFamily::IPTC);
    }

    #[test]
    fn test_iptc_caption_abstract_lookup() {
        let tag = get_tag_descriptor("IPTC:Caption-Abstract");
        assert!(tag.is_some());
    }

    #[test]
    fn test_iptc_keywords_lookup() {
        let tag = get_tag_descriptor("IPTC:Keywords");
        assert!(tag.is_some());
    }

    #[test]
    fn test_iptc_by_line_lookup() {
        let tag = get_tag_descriptor("IPTC:By-line");
        assert!(tag.is_some());
    }

    #[test]
    fn test_iptc_city_lookup() {
        let tag = get_tag_descriptor("IPTC:City");
        assert!(tag.is_some());
    }

    #[test]
    fn test_iptc_country_name_lookup() {
        let tag = get_tag_descriptor("IPTC:Country-PrimaryLocationName");
        assert!(tag.is_some());
    }

    // PDF tags tests
    #[test]
    fn test_pdf_title_lookup() {
        let tag = get_tag_descriptor("PDF:Title");
        assert!(tag.is_some(), "PDF:Title should be registered");
        assert_eq!(tag.unwrap().format(), FormatFamily::PDF);
    }

    #[test]
    fn test_pdf_author_lookup() {
        let tag = get_tag_descriptor("PDF:Author");
        assert!(tag.is_some());
    }

    #[test]
    fn test_pdf_subject_lookup() {
        let tag = get_tag_descriptor("PDF:Subject");
        assert!(tag.is_some());
    }

    #[test]
    fn test_pdf_creator_lookup() {
        let tag = get_tag_descriptor("PDF:Creator");
        assert!(tag.is_some());
    }

    #[test]
    fn test_pdf_producer_lookup() {
        let tag = get_tag_descriptor("PDF:Producer");
        assert!(tag.is_some());
    }

    // QuickTime tags tests
    #[test]
    fn test_quicktime_title_lookup() {
        let tag = get_tag_descriptor("QuickTime:Title");
        assert!(tag.is_some(), "QuickTime:Title should be registered");
        assert_eq!(tag.unwrap().format(), FormatFamily::QuickTime);
    }

    #[test]
    fn test_quicktime_artist_lookup() {
        let tag = get_tag_descriptor("QuickTime:Artist");
        assert!(tag.is_some());
    }

    #[test]
    fn test_quicktime_copyright_lookup() {
        let tag = get_tag_descriptor("QuickTime:Copyright");
        assert!(tag.is_some());
    }

    #[test]
    fn test_quicktime_make_lookup() {
        let tag = get_tag_descriptor("QuickTime:Make");
        assert!(tag.is_some());
    }

    #[test]
    fn test_quicktime_gps_coordinates_lookup() {
        let tag = get_tag_descriptor("QuickTime:GPSCoordinates");
        assert!(tag.is_some());
    }
}
