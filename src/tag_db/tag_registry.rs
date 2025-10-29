//! Tag Registry - Initial 100 Common Metadata Tags
//!
//! This module provides a static registry of the 100 most commonly used metadata tags
//! covering EXIF (60+), GPS (20+), and XMP (20+) formats. This is a manual implementation
//! that will later be replaced by automated tag generation in build.rs (task I5.T5).

use crate::core::tag_descriptor::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

/// Static registry containing all 100 registered metadata tags.
/// Uses lazy initialization for zero-cost abstraction until first access.
static TAG_REGISTRY: Lazy<HashMap<&'static str, TagDescriptor>> = Lazy::new(|| {
    let mut registry = HashMap::with_capacity(100);

    // ===========================
    // EXIF TAGS (60 total)
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
    // GPS TAGS (20 total)
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
    // XMP TAGS (20 total)
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
    TAG_REGISTRY.get(name)
}

/// Returns the total number of tags in the registry.
///
/// This should always return 100 for the initial implementation.
pub fn tag_count() -> usize {
    TAG_REGISTRY.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_count() {
        assert_eq!(tag_count(), 100, "Registry must contain exactly 100 tags");
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

        for descriptor in TAG_REGISTRY.values() {
            match descriptor.format() {
                FormatFamily::EXIF => exif_count += 1,
                FormatFamily::GPS => gps_count += 1,
                FormatFamily::XMP => xmp_count += 1,
                _ => panic!("Unexpected format family in registry"),
            }
        }

        assert!(
            exif_count >= 60,
            "Expected at least 60 EXIF tags, found {}",
            exif_count
        );
        assert!(
            gps_count >= 20,
            "Expected at least 20 GPS tags, found {}",
            gps_count
        );
        assert!(
            xmp_count >= 20,
            "Expected at least 20 XMP tags, found {}",
            xmp_count
        );
        assert_eq!(
            exif_count + gps_count + xmp_count,
            100,
            "Total tag count mismatch"
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
}
