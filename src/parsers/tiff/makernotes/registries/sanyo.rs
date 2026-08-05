//! Sanyo tag registry
//!
//! Registry of Sanyo MakerNote tags with their metadata and decoders,
//! transcribed from ExifTool's `Sanyo.pm` `%Image::ExifTool::Sanyo::Main`
//! (verified against the pinned 13.59 oracle).
//!
//! The previous version of this registry used tag IDs 0x0100-0x010B with
//! names ("Quality", "FocusMode", "ColorMode", ...) that do not appear
//! anywhere in `Sanyo.pm` -- it was not transcribed from ExifTool at all, so
//! every one of `combined-samples/Sanyo.jpg`'s 21 `[Sanyo]`-grouped tags
//! (verified via `exiftool -G1 -s -a`) was either silently dropped (no
//! tag matched a registered ID) or would have printed a fabricated
//! PrintConv had a real ID collided with one of the invented ones.

use super::super::shared::generic_decoders::SimpleValueDecoder;
use super::super::shared::tag_registry::TagRegistry;

/// Sanyo.pm:19-22 (`%offOn`, reused by many of this table's tags).
const OFF_ON: SimpleValueDecoder<u16> = SimpleValueDecoder::new(&[(0, "Off"), (1, "On")]);

/// Sanyo.pm:78-87 (`Macro`).
const MACRO: SimpleValueDecoder<u16> =
    SimpleValueDecoder::new(&[(0, "Normal"), (1, "Macro"), (2, "View"), (3, "Manual")]);

/// Sanyo.pm:95-104 (`SequentialShot`).
const SEQUENTIAL_SHOT: SimpleValueDecoder<u16> = SimpleValueDecoder::new(&[
    (0, "None"),
    (1, "Standard"),
    (2, "Best"),
    (3, "Adjust Exposure"),
]);

/// Sanyo.pm:131-138 (`RecordShutterRelease`).
const RECORD_SHUTTER_RELEASE: SimpleValueDecoder<u16> = SimpleValueDecoder::new(&[
    (0, "Record while down"),
    (1, "Press start, press stop"),
]);

/// Sanyo.pm:159-166 (`Resaved`).
const RESAVED: SimpleValueDecoder<u16> = SimpleValueDecoder::new(&[(0, "No"), (1, "Yes")]);

/// Sanyo.pm:167-179 (`SceneSelect`).
const SCENE_SELECT: SimpleValueDecoder<u16> = SimpleValueDecoder::new(&[
    (0, "Off"),
    (1, "Sport"),
    (2, "TV"),
    (3, "Night"),
    (4, "User 1"),
    (5, "User 2"),
    (6, "Lamp"),
]);

/// Sanyo.pm:190-199 (`SequenceShotInterval`).
const SEQUENCE_SHOT_INTERVAL: SimpleValueDecoder<u16> = SimpleValueDecoder::new(&[
    (0, "5 frames/s"),
    (1, "10 frames/s"),
    (2, "15 frames/s"),
    (3, "20 frames/s"),
]);

/// Sanyo.pm:200-209 (`FlashMode`).
const FLASH_MODE: SimpleValueDecoder<u16> =
    SimpleValueDecoder::new(&[(0, "Auto"), (1, "Force"), (2, "Disabled"), (3, "Red eye")]);

fn decode_off_on(value: u16) -> String {
    OFF_ON.decode(value)
}
fn decode_macro(value: u16) -> String {
    MACRO.decode(value)
}
fn decode_sequential_shot(value: u16) -> String {
    SEQUENTIAL_SHOT.decode(value)
}
fn decode_record_shutter_release(value: u16) -> String {
    RECORD_SHUTTER_RELEASE.decode(value)
}
fn decode_resaved(value: u16) -> String {
    RESAVED.decode(value)
}
fn decode_scene_select(value: u16) -> String {
    SCENE_SELECT.decode(value)
}
fn decode_sequence_shot_interval(value: u16) -> String {
    SEQUENCE_SHOT_INTERVAL.decode(value)
}
fn decode_flash_mode(value: u16) -> String {
    FLASH_MODE.decode(value)
}

/// Create and return the Sanyo tag registry.
///
/// `SanyoQuality` (0x0201, its own large `PrintHex`-flagged map) and
/// `MakerNoteOffset` (0x00ff, `int32u`) are handled directly in
/// `sanyo.rs::parse_entry` rather than through this `u16`-keyed registry --
/// see the comments there.
pub fn sanyo_registry() -> TagRegistry {
    TagRegistry::new()
        .register_raw(0x0200, "SpecialMode")
        .register_u16(0x0202, "Macro", decode_macro)
        // DigitalZoom (0x0204) and ManualFocusDistance (0x0223) are
        // `rational64u` (Sanyo.pm:90-91,182-185): 8 bytes, always
        // out-of-line, so `entry.value_offset` is an offset pointer, not the
        // value -- `parse_entry`'s `extract_u16_value` has no rational
        // decoder and would print that pointer's low 16 bits as if it were
        // the tag's value. Neither is in this squad's assigned tag list, so
        // rather than ship that garbage under a real tag name they are left
        // unregistered (get_tag_name returns None, and parse_entry drops
        // the entry) until someone implements the rational read.
        //
        // SoftwareVersion (0x0207), PictInfo (0x0208) and CameraID (0x0209)
        // are bare `Name => '...'` entries with no `Writable`/`Format` in
        // Sanyo.pm, so their real on-the-wire type is whatever a given
        // camera actually writes -- unverified against any sample in this
        // corpus, and also not in this squad's list. Left unregistered for
        // the same reason.
        .register_u16(0x020E, "SequentialShot", decode_sequential_shot)
        .register_u16(0x020F, "WideRange", decode_off_on)
        .register_u16(0x0210, "ColorAdjustmentMode", decode_off_on)
        .register_u16(0x0213, "QuickShot", decode_off_on)
        .register_u16(0x0214, "SelfTimer", decode_off_on)
        .register_u16(0x0216, "VoiceMemo", decode_off_on)
        .register_u16(0x0217, "RecordShutterRelease", decode_record_shutter_release)
        .register_u16(0x0218, "FlickerReduce", decode_off_on)
        .register_u16(0x0219, "OpticalZoomOn", decode_off_on)
        .register_u16(0x021B, "DigitalZoomOn", decode_off_on)
        .register_u16(0x021D, "LightSourceSpecial", decode_off_on)
        .register_u16(0x021E, "Resaved", decode_resaved)
        .register_u16(0x021F, "SceneSelect", decode_scene_select)
        .register_u16(0x0224, "SequenceShotInterval", decode_sequence_shot_interval)
        .register_u16(0x0225, "FlashMode", decode_flash_mode)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_creation() {
        let registry = sanyo_registry();
        assert!(registry.has_tag(0x0219)); // OpticalZoomOn
        assert!(registry.has_tag(0x021F)); // SceneSelect
    }

    #[test]
    fn test_registry_tag_names() {
        let registry = sanyo_registry();
        assert_eq!(registry.get_tag_name(0x0218), Some("FlickerReduce"));
        assert_eq!(registry.get_tag_name(0x021D), Some("LightSourceSpecial"));
        assert_eq!(registry.get_tag_name(0x0217), Some("RecordShutterRelease"));
    }

    #[test]
    fn test_decoders() {
        assert_eq!(decode_off_on(0), "Off");
        assert_eq!(decode_off_on(1), "On");
        assert_eq!(decode_scene_select(3), "Night");
        assert_eq!(decode_sequence_shot_interval(0), "5 frames/s");
        assert_eq!(decode_flash_mode(3), "Red eye");
        assert_eq!(decode_record_shutter_release(0), "Record while down");
        assert_eq!(decode_resaved(0), "No");
    }

    #[test]
    fn test_unknown_tag() {
        let registry = sanyo_registry();
        assert!(!registry.has_tag(0xFFFF));
        assert_eq!(registry.get_tag_name(0xFFFF), None);
    }
}
