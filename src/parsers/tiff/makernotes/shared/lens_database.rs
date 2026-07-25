//! Unified lens database infrastructure
//!
//! Provides a common interface for lens lookups across different manufacturers,
//! eliminating duplicated lens database implementations.

/// Trait for lens database lookups
pub trait LensDatabase {
    /// Look up a lens name by its ID
    fn lookup(&self, lens_id: u16) -> Option<&'static str>;

    /// Look up a lens by ID range (for lenses that span multiple IDs)
    fn lookup_range(&self, id_min: u16, id_max: u16) -> Option<&'static str> {
        // Default implementation checks if any ID in range matches
        for id in id_min..=id_max {
            if let Some(name) = self.lookup(id) {
                return Some(name);
            }
        }
        None
    }
}

/// Static lens database implementation backed by a const array
///
/// Most efficient for manufacturers with < 1000 lenses.
/// Uses linear search which is fine for typical database sizes.
pub struct StaticLensDb {
    entries: &'static [(u16, &'static str)],
}

impl StaticLensDb {
    /// Create a new static lens database
    pub const fn new(entries: &'static [(u16, &'static str)]) -> Self {
        Self { entries }
    }
}

impl LensDatabase for StaticLensDb {
    fn lookup(&self, lens_id: u16) -> Option<&'static str> {
        self.entries
            .iter()
            .find(|(id, _)| *id == lens_id)
            .map(|(_, name)| *name)
    }
}

/// Range-based lens database for lenses identified by ID ranges
///
/// Example: Lens IDs 100-105 all map to "Canon EF 50mm f/1.8"
pub struct RangeLensDb {
    entries: &'static [(u16, u16, &'static str)], // (min_id, max_id, name)
}

impl RangeLensDb {
    /// Create a new range-based lens database
    pub const fn new(entries: &'static [(u16, u16, &'static str)]) -> Self {
        Self { entries }
    }
}

impl LensDatabase for RangeLensDb {
    fn lookup(&self, lens_id: u16) -> Option<&'static str> {
        self.entries
            .iter()
            .find(|(min, max, _)| lens_id >= *min && lens_id <= *max)
            .map(|(_, _, name)| *name)
    }
}

/// Combined lens database that checks multiple sources
///
/// Useful when a manufacturer has both exact ID matches and ranges.
pub struct CombinedLensDb {
    static_db: Option<&'static StaticLensDb>,
    range_db: Option<&'static RangeLensDb>,
}

impl CombinedLensDb {
    /// Create a new combined lens database from optional static and range databases
    ///
    /// # Arguments
    /// * `static_db` - Optional static lens database for exact ID matches
    /// * `range_db` - Optional range-based lens database for ID ranges
    pub const fn new(
        static_db: Option<&'static StaticLensDb>,
        range_db: Option<&'static RangeLensDb>,
    ) -> Self {
        Self {
            static_db,
            range_db,
        }
    }
}

impl LensDatabase for CombinedLensDb {
    fn lookup(&self, lens_id: u16) -> Option<&'static str> {
        // Try static database first
        if let Some(db) = self.static_db
            && let Some(name) = db.lookup(lens_id)
        {
            return Some(name);
        }

        // Fall back to range database
        if let Some(db) = self.range_db {
            return db.lookup(lens_id);
        }

        None
    }
}
