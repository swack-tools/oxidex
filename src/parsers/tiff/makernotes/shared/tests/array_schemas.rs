#[cfg(test)]
mod tests {
    use super::super::super::array_schemas::*;
    use crate::const_decoder;
    use std::collections::HashMap;

    const_decoder!(TEST_MODE, i16, [(1, "Mode1"), (2, "Mode2"), (3, "Mode3"),]);

    #[test]
    fn test_array_schema_with_decoder() {
        static SCHEMA: ArraySchema = ArraySchema {
            name: "TestSettings",
            indices: &[
                ArrayIndexDef::with_i16_decoder(0, "Mode", &TEST_MODE),
                ArrayIndexDef::raw(1, "RawValue"),
            ],
        };

        let array = vec![2i16, 42];
        let mut tags = HashMap::new();

        SCHEMA.process_i16_array(&array, "Test", &mut tags);

        assert_eq!(
            tags.get("Test:TestSettings:Mode"),
            Some(&"Mode2".to_string())
        );
        assert_eq!(
            tags.get("Test:TestSettings:RawValue"),
            Some(&"42".to_string())
        );
    }

    #[test]
    fn test_array_schema_missing_indices() {
        static SCHEMA: ArraySchema = ArraySchema {
            name: "TestSettings",
            indices: &[
                ArrayIndexDef::raw(0, "First"),
                ArrayIndexDef::raw(5, "OutOfBounds"),
            ],
        };

        let array = vec![100i16];
        let mut tags = HashMap::new();

        SCHEMA.process_i16_array(&array, "Test", &mut tags);

        assert_eq!(
            tags.get("Test:TestSettings:First"),
            Some(&"100".to_string())
        );
        assert_eq!(tags.get("Test:TestSettings:OutOfBounds"), None);
    }

    #[test]
    fn test_u16_array_processing() {
        static SCHEMA: ArraySchema = ArraySchema {
            name: "U16Settings",
            indices: &[ArrayIndexDef::raw(0, "Value")],
        };

        let array = vec![65535u16];
        let mut tags = HashMap::new();

        SCHEMA.process_u16_array(&array, "Test", &mut tags);

        assert_eq!(
            tags.get("Test:U16Settings:Value"),
            Some(&"65535".to_string())
        );
    }
}
