//! Tests for TagRegistry array schema support

#[cfg(test)]
mod tests {
    use super::super::super::array_schemas::*;
    use super::super::super::tag_registry::*;
    use crate::const_decoder;
    use std::collections::HashMap;

    // Define test decoders using the const_decoder macro
    const_decoder!(QUALITY, i16, [(1, "Low"), (2, "Normal"), (3, "High"),]);

    const_decoder!(MACRO_MODE, i16, [(1, "Macro"), (2, "Normal"),]);

    #[test]
    fn test_registry_with_array_schema() {
        // Define a test array schema
        static SETTINGS_SCHEMA: ArraySchema = ArraySchema {
            name: "CameraSettings",
            indices: &[
                ArrayIndexDef::with_i16_decoder(1, "Quality", &QUALITY),
                ArrayIndexDef::raw(2, "ISO"),
            ],
        };

        // Create registry and register the array schema
        let registry = TagRegistry::new().register_array_schema(0x0001, &SETTINGS_SCHEMA);

        // Test that the tag is registered with correct name
        assert_eq!(registry.get_tag_name(0x0001), Some("CameraSettings"));
        assert!(registry.has_tag(0x0001));

        // Create test array data
        let array = vec![0i16, 2, 400]; // index 0 unused, 1=Normal quality, 2=ISO 400
        let mut tags = HashMap::new();

        // Decode the array using the registry
        registry.decode_array_i16(0x0001, &array, "Canon", &mut tags);

        // Verify the decoded values
        assert_eq!(
            tags.get("Canon:CameraSettings:Quality"),
            Some(&"Normal".to_string())
        );
        assert_eq!(
            tags.get("Canon:CameraSettings:ISO"),
            Some(&"400".to_string())
        );
    }

    #[test]
    fn test_registry_array_schema_with_multiple_decoders() {
        // Define a more complex schema with multiple decoders
        static CAMERA_SETTINGS: ArraySchema = ArraySchema {
            name: "CameraSettings",
            indices: &[
                ArrayIndexDef::with_i16_decoder(0, "MacroMode", &MACRO_MODE),
                ArrayIndexDef::with_i16_decoder(1, "Quality", &QUALITY),
                ArrayIndexDef::raw(2, "FlashMode"),
                ArrayIndexDef::raw(3, "DriveMode"),
            ],
        };

        let registry = TagRegistry::new().register_array_schema(0x0001, &CAMERA_SETTINGS);

        let array = vec![2i16, 3, 1, 0]; // Normal macro, High quality, flash 1, drive 0
        let mut tags = HashMap::new();

        registry.decode_array_i16(0x0001, &array, "Test", &mut tags);

        assert_eq!(
            tags.get("Test:CameraSettings:MacroMode"),
            Some(&"Normal".to_string())
        );
        assert_eq!(
            tags.get("Test:CameraSettings:Quality"),
            Some(&"High".to_string())
        );
        assert_eq!(
            tags.get("Test:CameraSettings:FlashMode"),
            Some(&"1".to_string())
        );
        assert_eq!(
            tags.get("Test:CameraSettings:DriveMode"),
            Some(&"0".to_string())
        );
    }

    #[test]
    fn test_registry_array_schema_partial_array() {
        // Test with an array that doesn't have all indices
        static SETTINGS_SCHEMA: ArraySchema = ArraySchema {
            name: "Settings",
            indices: &[
                ArrayIndexDef::raw(0, "First"),
                ArrayIndexDef::raw(5, "OutOfBounds"), // This index doesn't exist
            ],
        };

        let registry = TagRegistry::new().register_array_schema(0x0001, &SETTINGS_SCHEMA);

        let array = vec![100i16]; // Only has index 0
        let mut tags = HashMap::new();

        registry.decode_array_i16(0x0001, &array, "Test", &mut tags);

        // First index should be present
        assert_eq!(tags.get("Test:Settings:First"), Some(&"100".to_string()));
        // Out of bounds index should not be present
        assert_eq!(tags.get("Test:Settings:OutOfBounds"), None);
    }

    #[test]
    fn test_registry_array_schema_u16() {
        // Test u16 array processing
        const_decoder!(U16_MODE, u16, [(1, "Mode1"), (2, "Mode2"),]);

        static U16_SETTINGS: ArraySchema = ArraySchema {
            name: "U16Settings",
            indices: &[
                ArrayIndexDef::with_u16_decoder(0, "Mode", &U16_MODE),
                ArrayIndexDef::raw(1, "Value"),
            ],
        };

        let registry = TagRegistry::new().register_array_schema(0x0001, &U16_SETTINGS);

        let array = vec![2u16, 65535]; // Mode2, max u16 value
        let mut tags = HashMap::new();

        registry.decode_array_u16(0x0001, &array, "Test", &mut tags);

        assert_eq!(
            tags.get("Test:U16Settings:Mode"),
            Some(&"Mode2".to_string())
        );
        assert_eq!(
            tags.get("Test:U16Settings:Value"),
            Some(&"65535".to_string())
        );
    }

    #[test]
    fn test_registry_array_schema_wrong_tag_id() {
        // Test that decoding with wrong tag ID does nothing
        static SETTINGS_SCHEMA: ArraySchema = ArraySchema {
            name: "Settings",
            indices: &[ArrayIndexDef::raw(0, "Value")],
        };

        let registry = TagRegistry::new().register_array_schema(0x0001, &SETTINGS_SCHEMA);

        let array = vec![100i16];
        let mut tags = HashMap::new();

        // Try to decode with a different tag ID
        registry.decode_array_i16(0x9999, &array, "Test", &mut tags);

        // Tags should be empty
        assert!(tags.is_empty());
    }

    #[test]
    fn test_registry_mixed_tags() {
        // Test registry with both regular tags and array schema tags
        static ARRAY_SCHEMA: ArraySchema = ArraySchema {
            name: "ArrayTag",
            indices: &[ArrayIndexDef::raw(0, "Value")],
        };

        let registry = TagRegistry::new()
            .register_simple_i16(0x0001, "SimpleTag", &QUALITY)
            .register_array_schema(0x0002, &ARRAY_SCHEMA)
            .register_raw(0x0003, "RawTag");

        // Test simple tag
        assert_eq!(registry.get_tag_name(0x0001), Some("SimpleTag"));
        assert_eq!(registry.decode_i16(0x0001, 2), "Normal");

        // Test array schema tag
        assert_eq!(registry.get_tag_name(0x0002), Some("ArrayTag"));
        let array = vec![42i16];
        let mut tags = HashMap::new();
        registry.decode_array_i16(0x0002, &array, "Test", &mut tags);
        assert_eq!(tags.get("Test:ArrayTag:Value"), Some(&"42".to_string()));

        // Test raw tag
        assert_eq!(registry.get_tag_name(0x0003), Some("RawTag"));
        assert_eq!(registry.decode_i16(0x0003, 100), "100");

        // Verify total count
        assert_eq!(registry.len(), 3);
    }

    #[test]
    fn test_decode_array_on_non_array_tag() {
        // Test that calling decode_array on a non-array tag does nothing
        let registry = TagRegistry::new().register_simple_i16(0x0001, "SimpleTag", &QUALITY);

        let array = vec![1i16, 2, 3];
        let mut tags = HashMap::new();

        registry.decode_array_i16(0x0001, &array, "Test", &mut tags);

        // Tags should be empty because tag 0x0001 is not an ArraySchema
        assert!(tags.is_empty());
    }
}
