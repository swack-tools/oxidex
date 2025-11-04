//! Apple format family tags (auto-generated)

use crate::core::tag_descriptor::{FormatFamily, TagDescriptor, TagId, ValueType};
use once_cell::sync::Lazy;
use std::collections::HashMap;

static TAGS: Lazy<Vec<TagDescriptor>> = Lazy::new(|| vec![
    TagDescriptor::new(TagId::new_numeric(0x0001), "Apple:MakerNoteVersion".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "MakerNoteVersion tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "Apple:AEMatrix".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "AEMatrix tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "Apple:RunTime".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "RunTime tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "Apple:AEStable".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "AEStable tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0005), "Apple:AETarget".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "AETarget tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0006), "Apple:AEAverage".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "AEAverage tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0007), "Apple:AFStable".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "AFStable tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0008), "Apple:AccelerationVector".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "AccelerationVector tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000A), "Apple:HDRImageType".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "HDRImageType tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "Apple:Original Image".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Original Image tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000B), "Apple:BurstUUID".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "BurstUUID tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000C), "Apple:FocusDistanceRange".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "FocusDistanceRange tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000F), "Apple:OISMode".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "OISMode tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0011), "Apple:ContentIdentifier".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ContentIdentifier tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0014), "Apple:ImageCaptureType".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ImageCaptureType tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "Apple:Portrait".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Portrait tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x000A), "Apple:Photo".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Photo tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0015), "Apple:ImageUniqueID".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ImageUniqueID tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0017), "Apple:LivePhotoVideoIndex".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "LivePhotoVideoIndex tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0019), "Apple:ImageProcessingFlags".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ImageProcessingFlags tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x001A), "Apple:QualityHint".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "QualityHint tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x001D), "Apple:LuminanceNoiseAmplitude".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "LuminanceNoiseAmplitude tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x001F), "Apple:PhotosAppFeatureFlags".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "PhotosAppFeatureFlags tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0020), "Apple:ImageCaptureRequestID".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ImageCaptureRequestID tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0021), "Apple:HDRHeadroom".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "HDRHeadroom tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0023), "Apple:AFPerformance".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "AFPerformance tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0025), "Apple:SceneFlags".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "SceneFlags tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0026), "Apple:SignalToNoiseRatioType".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "SignalToNoiseRatioType tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0027), "Apple:SignalToNoiseRatio".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "SignalToNoiseRatio tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x002B), "Apple:PhotoIdentifier".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "PhotoIdentifier tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x002D), "Apple:ColorTemperature".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ColorTemperature tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x002E), "Apple:CameraType".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "CameraType tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "Apple:Back Normal".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Back Normal tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0006), "Apple:Front".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Front tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x002F), "Apple:FocusPosition".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "FocusPosition tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0030), "Apple:HDRGain".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "HDRGain tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0038), "Apple:AFMeasuredDepth".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "AFMeasuredDepth tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x003D), "Apple:AFConfidence".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "AFConfidence tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x003E), "Apple:ColorCorrectionMatrix".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "ColorCorrectionMatrix tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x003F), "Apple:GreenGhostMitigationStatus".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "GreenGhostMitigationStatus tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0040), "Apple:SemanticStyle".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "SemanticStyle tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0041), "Apple:SemanticStyleRenderingVer".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "SemanticStyleRenderingVer tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0042), "Apple:SemanticStylePreset".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "SemanticStylePreset tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x004E), "Apple:Apple_0x004e".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Apple_0x004e tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x004F), "Apple:Apple_0x004f".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Apple_0x004f tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0054), "Apple:Apple_0x0054".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Apple_0x0054 tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x005A), "Apple:Apple_0x005a".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Apple_0x005a tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "Apple:Has been rounded".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Has been rounded tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0002), "Apple:Positive infinity".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Positive infinity tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0003), "Apple:Negative infinity".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Negative infinity tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0004), "Apple:Indefinite".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Indefinite tag".to_string(), vec!["Example".to_string()]),
    TagDescriptor::new(TagId::new_numeric(0x0001), "Apple:Apple:RunTimeScale".to_string(), FormatFamily::MakerNotes, false, ValueType::String, "Apple:RunTimeScale tag".to_string(), vec!["Example".to_string()]),
]);

pub fn get_tags() -> &'static HashMap<String, TagDescriptor> {
    static MAP: Lazy<HashMap<String, TagDescriptor>> = Lazy::new(|| {
        let mut map = HashMap::with_capacity(TAGS.len());
        for tag in TAGS.iter() {
            map.insert(tag.tag_name.clone(), tag.clone());
        }
        map
    });
    &MAP
}
