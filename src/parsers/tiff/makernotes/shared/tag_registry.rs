//! Tag registry system for organizing and managing MakerNote tags
//!
//! This module provides a structured way to define and manage tag definitions,
//! reducing the need for large match statements and repetitive tag handling code.
//!
//! ## Design Philosophy
//! Instead of scattered tag definitions and individual decoder functions,
//! the registry provides:
//! 1. **Centralized tag definitions** - All tags in one place
//! 2. **Type-safe tag handling** - Compile-time validation
//! 3. **Builder pattern** - Easy, readable tag registration
//! 4. **Automatic decoding** - Tags know how to decode themselves
//!
//! ## Before & After Example
//!
//! **Before** (scattered definitions):
//! ```ignore
//! const TAG_QUALITY: u16 = 0x0001;
//! const TAG_MODE: u16 = 0x0002;
//!
//! fn get_tag_name(tag_id: u16) -> &'static str {
//!     match tag_id {
//!         TAG_QUALITY => "Quality",
//!         TAG_MODE => "Mode",
//!         _ => "Unknown",
//!     }
//! }
//!
//! fn decode_tag(tag_id: u16, value: i16) -> String {
//!     match tag_id {
//!         TAG_QUALITY => decode_quality(value),
//!         TAG_MODE => decode_mode(value),
//!         _ => format!("Unknown ({})", value),
//!     }
//! }
//! ```
//!
//! **After** (using TagRegistry):
//! ```ignore
//! use super::shared::tag_registry::TagRegistry;
//!
//! let registry = TagRegistry::new()
//!     .register_simple(0x0001, "Quality", &QUALITY_DECODER)
//!     .register_simple(0x0002, "Mode", &MODE_DECODER);
//!
//! // Usage:
//! let name = registry.get_tag_name(0x0001); // "Quality"
//! let decoded = registry.decode_i16(0x0001, value);
//! ```

use super::array_schemas::ArraySchema;
use super::generic_decoders::SimpleValueDecoder;
use std::collections::HashMap;

/// Type alias for decoder functions that take an i16 and return a String
pub type I16Decoder = fn(i16) -> String;

/// Type alias for decoder functions that take an i32 and return a String
pub type I32Decoder = fn(i32) -> String;

/// Type alias for decoder functions that take a u16 and return a String
pub type U16Decoder = fn(u16) -> String;

/// Type alias for decoder functions that take a u32 and return a String
pub type U32Decoder = fn(u32) -> String;

/// Represents a single tag's metadata and decoder
///
/// Each tag has:
/// - An ID (u16) that identifies it in the MakerNote
/// - A human-readable name
/// - An optional decoder function for converting raw values to strings
#[derive(Clone)]
pub struct TagDefinition {
    /// The tag's unique identifier
    pub id: u16,
    /// Human-readable tag name (e.g., "Scene Optimizer", "White Balance")
    pub name: &'static str,
    /// Optional decoder function for this tag's values
    pub decoder: Option<TagDecoder>,
}

/// Enum representing different types of decoders for various value types
///
/// This allows the registry to handle tags with different value types
/// (i16, i32, u16, u32, etc.) in a type-safe manner.
#[derive(Clone)]
pub enum TagDecoder {
    /// Decoder for 16-bit signed integer values
    I16(I16Decoder),
    /// Decoder for 32-bit signed integer values
    I32(I32Decoder),
    /// Decoder for 16-bit unsigned integer values
    U16(U16Decoder),
    /// Decoder for 32-bit unsigned integer values
    U32(U32Decoder),
    /// Decoder using SimpleValueDecoder<i16>
    SimpleI16(&'static SimpleValueDecoder<i16>),
    /// Decoder using SimpleValueDecoder<i32>
    SimpleI32(&'static SimpleValueDecoder<i32>),
    /// Decoder using SimpleValueDecoder<u16>
    SimpleU16(&'static SimpleValueDecoder<u16>),
    /// Decoder using SimpleValueDecoder<u32>
    SimpleU32(&'static SimpleValueDecoder<u32>),
    /// Array schema for processing array-type tags
    ArraySchema(&'static ArraySchema),
}

/// A registry that maps tag IDs to their definitions and decoders
///
/// The registry provides:
/// - Fast O(1) lookup of tag names by ID
/// - Automatic value decoding based on tag type
/// - Builder pattern for easy registration
/// - Support for various value types
///
/// # Example
/// ```ignore
/// use oxidex::parsers::tiff::makernotes::shared::tag_registry::TagRegistry;
/// use oxidex::parsers::tiff::makernotes::shared::generic_decoders::SimpleValueDecoder;
///
/// const QUALITY: SimpleValueDecoder<i16> = SimpleValueDecoder::new(&[
///     (1, "Low"),
///     (2, "Medium"),
///     (3, "High"),
/// ]);
///
/// let registry = TagRegistry::new()
///     .register_simple_i16(0x0001, "Quality", &QUALITY)
///     .register_raw(0x0002, "Mode"); // No decoder, returns raw value
///
/// assert_eq!(registry.get_tag_name(0x0001), Some("Quality"));
/// assert_eq!(registry.decode_i16(0x0001, 2), "Medium");
/// assert_eq!(registry.decode_i16(0x0002, 5), "5"); // Raw value
/// ```
pub struct TagRegistry {
    /// Map of tag ID to tag definition
    tags: HashMap<u16, TagDefinition>,
}

impl TagRegistry {
    /// Creates a new empty tag registry
    ///
    /// # Returns
    /// A new TagRegistry instance ready for tag registration
    pub fn new() -> Self {
        Self {
            tags: HashMap::new(),
        }
    }

    /// Creates a registry with pre-allocated capacity
    ///
    /// Use this when you know approximately how many tags you'll register
    /// to avoid reallocation overhead.
    ///
    /// # Arguments
    /// * `capacity` - Number of tags to pre-allocate space for
    ///
    /// # Returns
    /// A new TagRegistry with pre-allocated capacity
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            tags: HashMap::with_capacity(capacity),
        }
    }

    /// Registers a tag with a SimpleValueDecoder<i16>
    ///
    /// This is the most common registration method for simple enum-like tags.
    ///
    /// # Arguments
    /// * `id` - The tag ID
    /// * `name` - Human-readable tag name
    /// * `decoder` - Reference to a static SimpleValueDecoder<i16>
    ///
    /// # Returns
    /// Self for method chaining
    pub fn register_simple_i16(
        mut self,
        id: u16,
        name: &'static str,
        decoder: &'static SimpleValueDecoder<i16>,
    ) -> Self {
        self.tags.insert(
            id,
            TagDefinition {
                id,
                name,
                decoder: Some(TagDecoder::SimpleI16(decoder)),
            },
        );
        self
    }

    /// Registers a tag with a SimpleValueDecoder<i32>
    ///
    /// # Arguments
    /// * `id` - The tag ID
    /// * `name` - Human-readable tag name
    /// * `decoder` - Reference to a static SimpleValueDecoder<i32>
    ///
    /// # Returns
    /// Self for method chaining
    pub fn register_simple_i32(
        mut self,
        id: u16,
        name: &'static str,
        decoder: &'static SimpleValueDecoder<i32>,
    ) -> Self {
        self.tags.insert(
            id,
            TagDefinition {
                id,
                name,
                decoder: Some(TagDecoder::SimpleI32(decoder)),
            },
        );
        self
    }

    /// Registers a tag with a SimpleValueDecoder<u16>
    ///
    /// # Arguments
    /// * `id` - The tag ID
    /// * `name` - Human-readable tag name
    /// * `decoder` - Reference to a static SimpleValueDecoder<u16>
    ///
    /// # Returns
    /// Self for method chaining
    pub fn register_simple_u16(
        mut self,
        id: u16,
        name: &'static str,
        decoder: &'static SimpleValueDecoder<u16>,
    ) -> Self {
        self.tags.insert(
            id,
            TagDefinition {
                id,
                name,
                decoder: Some(TagDecoder::SimpleU16(decoder)),
            },
        );
        self
    }

    /// Registers a tag with a SimpleValueDecoder<u32>
    ///
    /// # Arguments
    /// * `id` - The tag ID
    /// * `name` - Human-readable tag name
    /// * `decoder` - Reference to a static SimpleValueDecoder<u32>
    ///
    /// # Returns
    /// Self for method chaining
    pub fn register_simple_u32(
        mut self,
        id: u16,
        name: &'static str,
        decoder: &'static SimpleValueDecoder<u32>,
    ) -> Self {
        self.tags.insert(
            id,
            TagDefinition {
                id,
                name,
                decoder: Some(TagDecoder::SimpleU32(decoder)),
            },
        );
        self
    }

    /// Registers a tag with a custom i16 decoder function
    ///
    /// Use this for tags that need custom decoding logic beyond simple
    /// value-to-string mappings.
    ///
    /// # Arguments
    /// * `id` - The tag ID
    /// * `name` - Human-readable tag name
    /// * `decoder` - Decoder function (i16 -> String)
    ///
    /// # Returns
    /// Self for method chaining
    pub fn register_i16(mut self, id: u16, name: &'static str, decoder: I16Decoder) -> Self {
        self.tags.insert(
            id,
            TagDefinition {
                id,
                name,
                decoder: Some(TagDecoder::I16(decoder)),
            },
        );
        self
    }

    /// Registers a tag with a custom i32 decoder function
    ///
    /// # Arguments
    /// * `id` - The tag ID
    /// * `name` - Human-readable tag name
    /// * `decoder` - Decoder function (i32 -> String)
    ///
    /// # Returns
    /// Self for method chaining
    pub fn register_i32(mut self, id: u16, name: &'static str, decoder: I32Decoder) -> Self {
        self.tags.insert(
            id,
            TagDefinition {
                id,
                name,
                decoder: Some(TagDecoder::I32(decoder)),
            },
        );
        self
    }

    /// Registers a tag with a custom u16 decoder function
    ///
    /// # Arguments
    /// * `id` - The tag ID
    /// * `name` - Human-readable tag name
    /// * `decoder` - Decoder function (u16 -> String)
    ///
    /// # Returns
    /// Self for method chaining
    pub fn register_u16(mut self, id: u16, name: &'static str, decoder: U16Decoder) -> Self {
        self.tags.insert(
            id,
            TagDefinition {
                id,
                name,
                decoder: Some(TagDecoder::U16(decoder)),
            },
        );
        self
    }

    /// Registers a tag with a custom u32 decoder function
    ///
    /// # Arguments
    /// * `id` - The tag ID
    /// * `name` - Human-readable tag name
    /// * `decoder` - Decoder function (u32 -> String)
    ///
    /// # Returns
    /// Self for method chaining
    pub fn register_u32(mut self, id: u16, name: &'static str, decoder: U32Decoder) -> Self {
        self.tags.insert(
            id,
            TagDefinition {
                id,
                name,
                decoder: Some(TagDecoder::U32(decoder)),
            },
        );
        self
    }

    /// Registers a tag without a decoder (returns raw value as string)
    ///
    /// Use this for tags that don't need decoding, such as numeric values
    /// that should be displayed as-is.
    ///
    /// # Arguments
    /// * `id` - The tag ID
    /// * `name` - Human-readable tag name
    ///
    /// # Returns
    /// Self for method chaining
    pub fn register_raw(mut self, id: u16, name: &'static str) -> Self {
        self.tags.insert(
            id,
            TagDefinition {
                id,
                name,
                decoder: None,
            },
        );
        self
    }

    /// Register a string tag (no decoder)
    ///
    /// String tags contain text values extracted directly from the MakerNote data.
    /// They require special handling in the parser because the value_offset field
    /// contains either inline data (≤4 bytes) or an offset to external data.
    ///
    /// # Arguments
    /// * `id` - The tag ID
    /// * `name` - Human-readable tag name
    ///
    /// # Returns
    /// Self for method chaining
    pub fn register_string_tag(mut self, id: u16, name: &'static str) -> Self {
        self.tags.insert(
            id,
            TagDefinition {
                id,
                name,
                decoder: None,
            },
        );
        self
    }

    /// Register an enumerated tag with an i32 decoder
    ///
    /// Enumerated tags use const_decoder! macros to map numeric values to
    /// human-readable strings (e.g., 1="Auto", 2="Manual").
    ///
    /// # Arguments
    /// * `id` - The tag ID
    /// * `name` - Human-readable tag name
    /// * `decoder` - Optional reference to a static SimpleValueDecoder<i32>
    ///
    /// # Returns
    /// Self for method chaining
    pub fn register_enum_tag(
        mut self,
        id: u16,
        name: &'static str,
        decoder: Option<&'static SimpleValueDecoder<i32>>,
    ) -> Self {
        let tag_decoder = decoder.map(TagDecoder::SimpleI32);
        self.tags.insert(
            id,
            TagDefinition {
                id,
                name,
                decoder: tag_decoder,
            },
        );
        self
    }

    /// Register an enumerated tag with an i32 decoder (non-optional variant)
    ///
    /// This is a convenience method for tags that always have a decoder.
    ///
    /// # Arguments
    /// * `id` - The tag ID
    /// * `name` - Human-readable tag name
    /// * `decoder` - Reference to a static SimpleValueDecoder<i32>
    ///
    /// # Returns
    /// Self for method chaining
    pub fn register_enum_tag_required(
        mut self,
        id: u16,
        name: &'static str,
        decoder: &'static SimpleValueDecoder<i32>,
    ) -> Self {
        self.tags.insert(
            id,
            TagDefinition {
                id,
                name,
                decoder: Some(TagDecoder::SimpleI32(decoder)),
            },
        );
        self
    }

    /// Register an integer/numeric tag (optionally with decoder)
    ///
    /// Integer tags contain numeric values that might be used directly or
    /// decoded through an optional decoder function.
    ///
    /// # Arguments
    /// * `id` - The tag ID
    /// * `name` - Human-readable tag name
    /// * `decoder` - Optional reference to a static SimpleValueDecoder<i32>
    ///
    /// # Returns
    /// Self for method chaining
    pub fn register_integer_tag(
        mut self,
        id: u16,
        name: &'static str,
        decoder: Option<&'static SimpleValueDecoder<i32>>,
    ) -> Self {
        let tag_decoder = decoder.map(TagDecoder::SimpleI32);
        self.tags.insert(
            id,
            TagDefinition {
                id,
                name,
                decoder: tag_decoder,
            },
        );
        self
    }

    /// Register an integer/numeric tag with an i32 decoder (non-optional variant)
    ///
    /// This is a convenience method for integer tags that always have a decoder.
    ///
    /// # Arguments
    /// * `id` - The tag ID
    /// * `name` - Human-readable tag name
    /// * `decoder` - Reference to a static SimpleValueDecoder<i32>
    ///
    /// # Returns
    /// Self for method chaining
    pub fn register_integer_tag_required(
        mut self,
        id: u16,
        name: &'static str,
        decoder: &'static SimpleValueDecoder<i32>,
    ) -> Self {
        self.tags.insert(
            id,
            TagDefinition {
                id,
                name,
                decoder: Some(TagDecoder::SimpleI32(decoder)),
            },
        );
        self
    }

    /// Register an array-based tag that uses an ArraySchema
    ///
    /// This method allows registration of tags that represent arrays of values,
    /// where each array index has specific meaning defined by the schema.
    ///
    /// # Arguments
    /// * `tag_id` - The tag ID
    /// * `schema` - Reference to a static ArraySchema defining the array structure
    ///
    /// # Returns
    /// Self for method chaining
    ///
    /// # Example
    /// ```ignore
    /// static CAMERA_SETTINGS: ArraySchema = ArraySchema {
    ///     name: "CameraSettings",
    ///     indices: &[
    ///         ArrayIndexDef::with_i16_decoder(1, "Quality", &QUALITY_DECODER),
    ///         ArrayIndexDef::raw(2, "ISO"),
    ///     ],
    /// };
    ///
    /// let registry = TagRegistry::new()
    ///     .register_array_schema(0x0001, &CAMERA_SETTINGS);
    /// ```
    pub fn register_array_schema(mut self, tag_id: u16, schema: &'static ArraySchema) -> Self {
        self.tags.insert(
            tag_id,
            TagDefinition {
                id: tag_id,
                name: schema.name,
                decoder: Some(TagDecoder::ArraySchema(schema)),
            },
        );
        self
    }

    /// Gets the human-readable name for a tag ID
    ///
    /// # Arguments
    /// * `tag_id` - The tag ID to look up
    ///
    /// # Returns
    /// The tag name, or None if the tag is not registered
    pub fn get_tag_name(&self, tag_id: u16) -> Option<&'static str> {
        self.tags.get(&tag_id).map(|tag| tag.name)
    }

    /// Gets the full tag definition for a tag ID
    ///
    /// # Arguments
    /// * `tag_id` - The tag ID to look up
    ///
    /// # Returns
    /// The tag definition, or None if not registered
    pub fn get_tag(&self, tag_id: u16) -> Option<&TagDefinition> {
        self.tags.get(&tag_id)
    }

    /// Checks if a tag is registered
    ///
    /// # Arguments
    /// * `tag_id` - The tag ID to check
    ///
    /// # Returns
    /// true if the tag is registered, false otherwise
    pub fn has_tag(&self, tag_id: u16) -> bool {
        self.tags.contains_key(&tag_id)
    }

    /// Decodes an i16 value for the given tag
    ///
    /// # Arguments
    /// * `tag_id` - The tag ID
    /// * `value` - The value to decode
    ///
    /// # Returns
    /// Decoded string, or the raw value as string if no decoder is registered
    pub fn decode_i16(&self, tag_id: u16, value: i16) -> String {
        match self.tags.get(&tag_id) {
            Some(tag) => match &tag.decoder {
                Some(TagDecoder::I16(decoder)) => decoder(value),
                Some(TagDecoder::SimpleI16(decoder)) => decoder.decode(value),
                _ => value.to_string(),
            },
            None => value.to_string(),
        }
    }

    /// Decodes an i32 value for the given tag
    ///
    /// # Arguments
    /// * `tag_id` - The tag ID
    /// * `value` - The value to decode
    ///
    /// # Returns
    /// Decoded string, or the raw value as string if no decoder is registered
    pub fn decode_i32(&self, tag_id: u16, value: i32) -> String {
        match self.tags.get(&tag_id) {
            Some(tag) => match &tag.decoder {
                Some(TagDecoder::I32(decoder)) => decoder(value),
                Some(TagDecoder::SimpleI32(decoder)) => decoder.decode(value),
                _ => value.to_string(),
            },
            None => value.to_string(),
        }
    }

    /// Decodes a u16 value for the given tag
    ///
    /// # Arguments
    /// * `tag_id` - The tag ID
    /// * `value` - The value to decode
    ///
    /// # Returns
    /// Decoded string, or the raw value as string if no decoder is registered
    pub fn decode_u16(&self, tag_id: u16, value: u16) -> String {
        match self.tags.get(&tag_id) {
            Some(tag) => match &tag.decoder {
                Some(TagDecoder::U16(decoder)) => decoder(value),
                Some(TagDecoder::SimpleU16(decoder)) => decoder.decode(value),
                _ => value.to_string(),
            },
            None => value.to_string(),
        }
    }

    /// Decodes a u32 value for the given tag
    ///
    /// # Arguments
    /// * `tag_id` - The tag ID
    /// * `value` - The value to decode
    ///
    /// # Returns
    /// Decoded string, or the raw value as string if no decoder is registered
    pub fn decode_u32(&self, tag_id: u16, value: u32) -> String {
        match self.tags.get(&tag_id) {
            Some(tag) => match &tag.decoder {
                Some(TagDecoder::U32(decoder)) => decoder(value),
                Some(TagDecoder::SimpleU32(decoder)) => decoder.decode(value),
                _ => value.to_string(),
            },
            None => value.to_string(),
        }
    }

    /// Decode and insert an i16 array tag using its schema
    ///
    /// This method processes an array of i16 values according to the ArraySchema
    /// registered for the given tag ID. Each array index defined in the schema
    /// will be extracted and inserted into the tags map with appropriate decoding.
    ///
    /// # Arguments
    /// * `tag_id` - The tag ID to look up in the registry
    /// * `array` - The i16 array to process
    /// * `prefix` - Prefix for tag names (e.g., "Canon", "Nikon")
    /// * `tags` - HashMap to insert the decoded values into
    ///
    /// # Behavior
    /// - If the tag is not registered, no action is taken
    /// - If the tag is registered but not with an ArraySchema, no action is taken
    /// - Only indices present in the array are processed (missing indices are skipped)
    ///
    /// # Example
    /// ```ignore
    /// let mut tags = HashMap::new();
    /// let settings = vec![0i16, 2, 400]; // index 0 unused, 1=quality, 2=ISO
    /// registry.decode_array_i16(0x0001, &settings, "Canon", &mut tags);
    /// // tags now contains "Canon:CameraSettings:Quality" and "Canon:CameraSettings:ISO"
    /// ```
    pub fn decode_array_i16(
        &self,
        tag_id: u16,
        array: &[i16],
        prefix: &str,
        tags: &mut HashMap<String, String>,
    ) {
        if let Some(def) = self.tags.get(&tag_id)
            && let Some(TagDecoder::ArraySchema(schema)) = def.decoder
        {
            schema.process_i16_array(array, prefix, tags);
        }
    }

    /// Decode and insert a u16 array tag using its schema
    ///
    /// This method processes an array of u16 values according to the ArraySchema
    /// registered for the given tag ID. Each array index defined in the schema
    /// will be extracted and inserted into the tags map with appropriate decoding.
    ///
    /// # Arguments
    /// * `tag_id` - The tag ID to look up in the registry
    /// * `array` - The u16 array to process
    /// * `prefix` - Prefix for tag names (e.g., "Canon", "Nikon")
    /// * `tags` - HashMap to insert the decoded values into
    ///
    /// # Behavior
    /// - If the tag is not registered, no action is taken
    /// - If the tag is registered but not with an ArraySchema, no action is taken
    /// - Only indices present in the array are processed (missing indices are skipped)
    ///
    /// # Example
    /// ```ignore
    /// let mut tags = HashMap::new();
    /// let settings = vec![0u16, 2, 400]; // index 0 unused, 1=quality, 2=ISO
    /// registry.decode_array_u16(0x0001, &settings, "Canon", &mut tags);
    /// // tags now contains "Canon:CameraSettings:Quality" and "Canon:CameraSettings:ISO"
    /// ```
    pub fn decode_array_u16(
        &self,
        tag_id: u16,
        array: &[u16],
        prefix: &str,
        tags: &mut HashMap<String, String>,
    ) {
        if let Some(def) = self.tags.get(&tag_id)
            && let Some(TagDecoder::ArraySchema(schema)) = def.decoder
        {
            schema.process_u16_array(array, prefix, tags);
        }
    }

    /// Decode and insert an i32 array tag using its schema
    ///
    /// This method processes an array of i32 values according to the ArraySchema
    /// registered for the given tag ID. Each array index defined in the schema
    /// will be extracted and inserted into the tags map with appropriate decoding.
    ///
    /// # Arguments
    /// * `tag_id` - The tag ID to look up in the registry
    /// * `array` - The i32 array to process
    /// * `prefix` - Prefix for tag names (e.g., "Olympus", "Panasonic")
    /// * `tags` - HashMap to insert the decoded values into
    ///
    /// # Behavior
    /// - If the tag is not registered, no action is taken
    /// - If the tag is registered but not with an ArraySchema, no action is taken
    /// - Only indices present in the array are processed (missing indices are skipped)
    pub fn decode_array_i32(
        &self,
        tag_id: u16,
        array: &[i32],
        prefix: &str,
        tags: &mut HashMap<String, String>,
    ) {
        if let Some(def) = self.tags.get(&tag_id)
            && let Some(TagDecoder::ArraySchema(schema)) = def.decoder
        {
            schema.process_i32_array(array, prefix, tags);
        }
    }

    /// Decode and insert a u32 array tag using its schema
    ///
    /// This method processes an array of u32 values according to the ArraySchema
    /// registered for the given tag ID. Each array index defined in the schema
    /// will be extracted and inserted into the tags map with appropriate decoding.
    ///
    /// # Arguments
    /// * `tag_id` - The tag ID to look up in the registry
    /// * `array` - The u32 array to process
    /// * `prefix` - Prefix for tag names (e.g., "Olympus", "Panasonic")
    /// * `tags` - HashMap to insert the decoded values into
    ///
    /// # Behavior
    /// - If the tag is not registered, no action is taken
    /// - If the tag is registered but not with an ArraySchema, no action is taken
    /// - Only indices present in the array are processed (missing indices are skipped)
    pub fn decode_array_u32(
        &self,
        tag_id: u16,
        array: &[u32],
        prefix: &str,
        tags: &mut HashMap<String, String>,
    ) {
        if let Some(def) = self.tags.get(&tag_id)
            && let Some(TagDecoder::ArraySchema(schema)) = def.decoder
        {
            schema.process_u32_array(array, prefix, tags);
        }
    }

    /// Returns the number of registered tags
    pub fn len(&self) -> usize {
        self.tags.len()
    }

    /// Checks if the registry is empty
    pub fn is_empty(&self) -> bool {
        self.tags.is_empty()
    }

    /// Returns an iterator over all tag IDs
    pub fn tag_ids(&self) -> impl Iterator<Item = u16> + '_ {
        self.tags.keys().copied()
    }

    /// Returns an iterator over all tag definitions
    pub fn tags(&self) -> impl Iterator<Item = &TagDefinition> + '_ {
        self.tags.values()
    }
}

impl Default for TagRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Builder Pattern Macros
// ============================================================================

/// Macro for building a tag registry declaratively
///
/// This macro provides a clean, declarative syntax for defining tag registries
/// with multiple tags.
///
/// # Syntax
/// ```ignore
/// tag_registry! {
///     REGISTRY_NAME {
///         tag_id1 => "Tag Name 1" : simple_i16(&DECODER1),
///         tag_id2 => "Tag Name 2" : i16(decoder_fn2),
///         tag_id3 => "Tag Name 3" : raw,
///     }
/// }
/// ```
///
/// # Example
/// ```ignore
/// use oxidex::tag_registry;
/// use oxidex::parsers::tiff::makernotes::shared::generic_decoders::SimpleValueDecoder;
///
/// const QUALITY: SimpleValueDecoder<i16> = SimpleValueDecoder::new(&[
///     (1, "Low"),
///     (2, "High"),
/// ]);
///
/// fn decode_mode(value: i16) -> String {
///     match value {
///         0 => "Auto".to_string(),
///         1 => "Manual".to_string(),
///         _ => format!("Unknown ({})", value),
///     }
/// }
///
/// tag_registry! {
///     MY_TAGS {
///         0x0001 => "Quality" : simple_i16(&QUALITY),
///         0x0002 => "Mode" : i16(decode_mode),
///         0x0003 => "ISO" : raw,
///     }
/// }
/// ```
#[macro_export]
macro_rules! tag_registry {
    // Main pattern: registry name followed by tag definitions
    (
        $name:ident {
            $(
                $tag_id:tt => $tag_name:tt : $decoder_type:ident $( ( $decoder:expr ) )? ,
            )*
        }
    ) => {
        fn $name() -> $crate::parsers::tiff::makernotes::shared::tag_registry::TagRegistry {
            $crate::parsers::tiff::makernotes::shared::tag_registry::TagRegistry::new()
            $(
                .tag_registry!(@register $tag_id, $tag_name, $decoder_type $( ( $decoder ) )? )
            )*
        }
    };

    // Internal rules for different decoder types
    (@register $tag_id:tt, $tag_name:tt, simple_i16 ( $decoder:expr ) ) => {
        register_simple_i16($tag_id, $tag_name, $decoder)
    };
    (@register $tag_id:tt, $tag_name:tt, simple_i32 ( $decoder:expr ) ) => {
        register_simple_i32($tag_id, $tag_name, $decoder)
    };
    (@register $tag_id:tt, $tag_name:tt, simple_u16 ( $decoder:expr ) ) => {
        register_simple_u16($tag_id, $tag_name, $decoder)
    };
    (@register $tag_id:tt, $tag_name:tt, simple_u32 ( $decoder:expr ) ) => {
        register_simple_u32($tag_id, $tag_name, $decoder)
    };
    (@register $tag_id:tt, $tag_name:tt, i16 ( $decoder:expr ) ) => {
        register_i16($tag_id, $tag_name, $decoder)
    };
    (@register $tag_id:tt, $tag_name:tt, i32 ( $decoder:expr ) ) => {
        register_i32($tag_id, $tag_name, $decoder)
    };
    (@register $tag_id:tt, $tag_name:tt, u16 ( $decoder:expr ) ) => {
        register_u16($tag_id, $tag_name, $decoder)
    };
    (@register $tag_id:tt, $tag_name:tt, u32 ( $decoder:expr ) ) => {
        register_u32($tag_id, $tag_name, $decoder)
    };
    (@register $tag_id:tt, $tag_name:tt, raw ) => {
        register_raw($tag_id, $tag_name)
    };
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::super::generic_decoders::SimpleValueDecoder;
    use super::*;

    const TEST_QUALITY: SimpleValueDecoder<i16> =
        SimpleValueDecoder::new(&[(1, "Low"), (2, "Medium"), (3, "High")]);

    const TEST_MODE: SimpleValueDecoder<i16> =
        SimpleValueDecoder::new(&[(0, "Auto"), (1, "Manual")]);

    fn custom_decoder(value: i16) -> String {
        format!("Custom: {}", value)
    }

    #[test]
    fn test_registry_creation() {
        let registry = TagRegistry::new();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
    }

    #[test]
    fn test_registry_with_capacity() {
        let registry = TagRegistry::with_capacity(10);
        assert!(registry.is_empty());
    }

    #[test]
    fn test_register_simple_i16() {
        let registry = TagRegistry::new().register_simple_i16(0x0001, "Quality", &TEST_QUALITY);

        assert_eq!(registry.len(), 1);
        assert_eq!(registry.get_tag_name(0x0001), Some("Quality"));
        assert_eq!(registry.decode_i16(0x0001, 2), "Medium");
    }

    #[test]
    fn test_register_custom_decoder() {
        let registry = TagRegistry::new().register_i16(0x0001, "CustomTag", custom_decoder);

        assert_eq!(registry.decode_i16(0x0001, 42), "Custom: 42");
    }

    #[test]
    fn test_register_raw() {
        let registry = TagRegistry::new().register_raw(0x0001, "RawValue");

        assert_eq!(registry.get_tag_name(0x0001), Some("RawValue"));
        assert_eq!(registry.decode_i16(0x0001, 123), "123");
    }

    #[test]
    fn test_chained_registration() {
        let registry = TagRegistry::new()
            .register_simple_i16(0x0001, "Quality", &TEST_QUALITY)
            .register_simple_i16(0x0002, "Mode", &TEST_MODE)
            .register_raw(0x0003, "ISO");

        assert_eq!(registry.len(), 3);
        assert_eq!(registry.decode_i16(0x0001, 1), "Low");
        assert_eq!(registry.decode_i16(0x0002, 0), "Auto");
        assert_eq!(registry.decode_i16(0x0003, 800), "800");
    }

    #[test]
    fn test_has_tag() {
        let registry = TagRegistry::new().register_simple_i16(0x0001, "Quality", &TEST_QUALITY);

        assert!(registry.has_tag(0x0001));
        assert!(!registry.has_tag(0x0002));
    }

    #[test]
    fn test_get_tag() {
        let registry = TagRegistry::new().register_simple_i16(0x0001, "Quality", &TEST_QUALITY);

        let tag = registry.get_tag(0x0001);
        assert!(tag.is_some());
        assert_eq!(tag.unwrap().name, "Quality");
    }

    #[test]
    fn test_unknown_tag_decode() {
        let registry = TagRegistry::new().register_simple_i16(0x0001, "Quality", &TEST_QUALITY);

        // Decoding an unregistered tag returns raw value
        assert_eq!(registry.decode_i16(0x9999, 42), "42");
    }

    #[test]
    fn test_tag_ids_iterator() {
        let registry = TagRegistry::new()
            .register_simple_i16(0x0001, "Tag1", &TEST_QUALITY)
            .register_simple_i16(0x0002, "Tag2", &TEST_MODE);

        let ids: Vec<u16> = registry.tag_ids().collect();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&0x0001));
        assert!(ids.contains(&0x0002));
    }

    #[test]
    fn test_tags_iterator() {
        let registry = TagRegistry::new()
            .register_simple_i16(0x0001, "Tag1", &TEST_QUALITY)
            .register_simple_i16(0x0002, "Tag2", &TEST_MODE);

        let tags: Vec<&TagDefinition> = registry.tags().collect();
        assert_eq!(tags.len(), 2);
    }

    #[test]
    fn test_different_value_types() {
        const TEST_U16: SimpleValueDecoder<u16> = SimpleValueDecoder::new(&[(1, "One")]);

        const TEST_I32: SimpleValueDecoder<i32> = SimpleValueDecoder::new(&[(100, "Hundred")]);

        const TEST_U32: SimpleValueDecoder<u32> = SimpleValueDecoder::new(&[(1000, "Thousand")]);

        let registry = TagRegistry::new()
            .register_simple_u16(0x0001, "U16Tag", &TEST_U16)
            .register_simple_i32(0x0002, "I32Tag", &TEST_I32)
            .register_simple_u32(0x0003, "U32Tag", &TEST_U32);

        assert_eq!(registry.decode_u16(0x0001, 1), "One");
        assert_eq!(registry.decode_i32(0x0002, 100), "Hundred");
        assert_eq!(registry.decode_u32(0x0003, 1000), "Thousand");
    }
}
