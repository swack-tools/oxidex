//! Integration test demonstrating array schemas, TagRegistry, and lens databases working together

#[cfg(test)]
mod tests {
    use super::super::super::{array_schemas::*, lens_database::*, tag_registry::*};
    use crate::const_decoder;
    use std::collections::HashMap;

    // Sample decoders
    const_decoder!(MACRO_MODE, i16, [(1, "Macro"), (2, "Normal")]);
    const_decoder!(QUALITY, i16, [(1, "Economy"), (2, "Normal"), (3, "Fine")]);

    // Sample array schema
    static CAMERA_SETTINGS: ArraySchema = ArraySchema {
        name: "CameraSettings",
        indices: &[
            ArrayIndexDef::with_i16_decoder(1, "MacroMode", &MACRO_MODE),
            ArrayIndexDef::with_i16_decoder(2, "Quality", &QUALITY),
            ArrayIndexDef::raw(3, "ISO"),
        ],
    };

    // Sample lens database
    static TEST_LENSES: [(u16, &str); 2] = [(1, "Test 50mm f/1.8"), (2, "Test 85mm f/1.4")];

    static LENS_DB: StaticLensDb = StaticLensDb::new(&TEST_LENSES);

    #[test]
    fn test_complete_parser_workflow() {
        // Create registry with array schema
        let registry = TagRegistry::new().register_array_schema(0x0001, &CAMERA_SETTINGS);

        // Simulate CameraSettings array from camera
        let settings = vec![0i16, 2, 3, 400]; // index 0 unused, 1=Normal, 2=Fine, 3=ISO 400

        let mut tags = HashMap::new();

        // Process array using registry
        registry.decode_array_i16(0x0001, &settings, "Test", &mut tags);

        // Verify extracted values
        assert_eq!(
            tags.get("Test:CameraSettings:MacroMode"),
            Some(&"Normal".to_string())
        );
        assert_eq!(
            tags.get("Test:CameraSettings:Quality"),
            Some(&"Fine".to_string())
        );
        assert_eq!(
            tags.get("Test:CameraSettings:ISO"),
            Some(&"400".to_string())
        );

        // Test lens lookup
        assert_eq!(LENS_DB.lookup(1), Some("Test 50mm f/1.8"));
        assert_eq!(LENS_DB.lookup(2), Some("Test 85mm f/1.4"));
        assert_eq!(LENS_DB.lookup(99), None);
    }

    #[test]
    fn test_schema_without_registry() {
        // Array schemas can be used standalone
        let settings = vec![0i16, 1, 2, 800];
        let mut tags = HashMap::new();

        CAMERA_SETTINGS.process_i16_array(&settings, "Standalone", &mut tags);

        assert_eq!(
            tags.get("Standalone:CameraSettings:MacroMode"),
            Some(&"Macro".to_string())
        );
        assert_eq!(
            tags.get("Standalone:CameraSettings:ISO"),
            Some(&"800".to_string())
        );
    }
}
