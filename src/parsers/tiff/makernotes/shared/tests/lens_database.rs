#[cfg(test)]
mod tests {
    use super::super::super::lens_database::*;

    const TEST_LENSES: [(u16, &str); 3] = [
        (1, "Test Lens 50mm f/1.8"),
        (2, "Test Lens 85mm f/1.4"),
        (3, "Test Lens 24-70mm f/2.8"),
    ];

    const TEST_RANGES: [(u16, u16, &str); 2] =
        [(100, 105, "Range Lens A"), (200, 210, "Range Lens B")];

    #[test]
    fn test_static_lens_db_lookup() {
        let db = StaticLensDb::new(&TEST_LENSES);

        assert_eq!(db.lookup(1), Some("Test Lens 50mm f/1.8"));
        assert_eq!(db.lookup(2), Some("Test Lens 85mm f/1.4"));
        assert_eq!(db.lookup(99), None);
    }

    #[test]
    fn test_range_lens_db_lookup() {
        let db = RangeLensDb::new(&TEST_RANGES);

        assert_eq!(db.lookup(100), Some("Range Lens A"));
        assert_eq!(db.lookup(103), Some("Range Lens A"));
        assert_eq!(db.lookup(105), Some("Range Lens A"));
        assert_eq!(db.lookup(106), None);
        assert_eq!(db.lookup(205), Some("Range Lens B"));
    }

    #[test]
    fn test_combined_lens_db() {
        static STATIC_DB: StaticLensDb = StaticLensDb::new(&TEST_LENSES);
        static RANGE_DB: RangeLensDb = RangeLensDb::new(&TEST_RANGES);
        let combined = CombinedLensDb::new(Some(&STATIC_DB), Some(&RANGE_DB));

        // Should find in static DB
        assert_eq!(combined.lookup(1), Some("Test Lens 50mm f/1.8"));

        // Should find in range DB
        assert_eq!(combined.lookup(102), Some("Range Lens A"));

        // Should not find
        assert_eq!(combined.lookup(999), None);
    }

    #[test]
    fn test_lens_db_trait_range_method() {
        let db = StaticLensDb::new(&TEST_LENSES);

        // Range lookup should find lens 2
        assert_eq!(db.lookup_range(1, 5), Some("Test Lens 50mm f/1.8"));
    }
}
