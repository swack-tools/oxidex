//! Runtime for source-derived `WriteExif` new-directory mandatory defaults.
//!
//! This has no public writer route.  All tag IDs, values, directory names and
//! JFIF substitutions are generated operands.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MandatoryValue {
    Integer(i64),
    Text(&'static str),
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MandatoryDefault {
    pub tag_id: u16,
    pub value: MandatoryValue,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MandatoryDirectory {
    pub directory: &'static str,
    pub defaults: &'static [MandatoryDefault],
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct JfifAssignment {
    pub tag_id: u16,
    pub property: &'static str,
    pub adjustment: i64,
}
/// The source-selected direct `WriteValue` packing format for one IFD0
/// default.  The generator refuses every other native format.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DefaultEncoding {
    pub tag_id: u16,
    pub format_name: &'static str,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MandatoryRecipe {
    pub writer_source_file: &'static str,
    pub writer_source_sha256: &'static str,
    pub write_value_source_sha256: &'static str,
    pub perl_version: &'static str,
    pub no_mandatory_guard: bool,
    pub directories: &'static [MandatoryDirectory],
    pub jfif_directory: &'static str,
    pub jfif_probe: &'static str,
    pub jfif_assignments: &'static [JfifAssignment],
    pub encodings: &'static [DefaultEncoding],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TiffByteOrder { Little, Big }

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct EncodedMandatoryDefault {
    pub tag_id: u16,
    /// TIFF type selected from the captured row's Writable operand.
    pub tiff_type: u16,
    pub count: u32,
    pub bytes: Vec<u8>,
}

/// Execute the admitted direct `WriteValue` packing path for generated IFD0
/// mandatory operands.  It intentionally has no public writer entry point.
pub(crate) fn encode_ifd0_defaults(
    recipe: &MandatoryRecipe,
    defaults: &[MandatoryDefault],
    byte_order: TiffByteOrder,
) -> Result<Vec<EncodedMandatoryDefault>, String> {
    let mut encoded = Vec::with_capacity(defaults.len());
    for default in defaults {
        let format = recipe.encodings.iter().find(|item| item.tag_id == default.tag_id)
            .ok_or_else(|| refusal("mandatory default has no generated WriteValue operand"))?;
        let value = match default.value {
            MandatoryValue::Integer(value) => value,
            MandatoryValue::Text(_) => return Err(refusal("text mandatory default is outside numeric encoder scope")),
        };
        let (tiff_type, count, bytes) = match format.format_name {
            "int16u" => {
                let value = u16::try_from(value).map_err(|_| refusal("int16u mandatory operand is outside native range"))?;
                let bytes = match byte_order { TiffByteOrder::Little => value.to_le_bytes(), TiffByteOrder::Big => value.to_be_bytes() };
                (3, 1, bytes.to_vec())
            }
            "rational64u" => {
                let value = u32::try_from(value).map_err(|_| refusal("rational64u mandatory operand is outside native range"))?;
                let mut bytes = Vec::with_capacity(8);
                match byte_order { TiffByteOrder::Little => { bytes.extend(value.to_le_bytes()); bytes.extend(1u32.to_le_bytes()); }, TiffByteOrder::Big => { bytes.extend(value.to_be_bytes()); bytes.extend(1u32.to_be_bytes()); } }
                (5, 1, bytes)
            }
            _ => return Err(refusal("mandatory WriteValue format is unsupported")),
        };
        encoded.push(EncodedMandatoryDefault { tag_id: default.tag_id, tiff_type, count, bytes });
    }
    Ok(encoded)
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct JfifValues {
    pub x: i64,
    pub y: i64,
    pub resolution_unit: i64,
}
fn refusal(reason: &str) -> String {
    format!("generated mandatory defaults refused: {reason}")
}
/// Apply the captured new-directory branch. A caller must provide the already
/// observed JFIF fields; this runtime does not discover tags or defaults.
pub(crate) fn defaults_for_new_directory(
    recipe: &MandatoryRecipe,
    directory: &str,
    no_mandatory: bool,
    num_entries: u32,
    jfif: Option<JfifValues>,
) -> Result<Vec<MandatoryDefault>, String> {
    if recipe.writer_source_file != "Image/ExifTool/WriteExif.pl"
        || recipe.writer_source_sha256.len() != 64
    {
        return Err(refusal("writer provenance is unresolved"));
    }
    if num_entries != 0 || (recipe.no_mandatory_guard && no_mandatory) {
        return Ok(Vec::new());
    }
    let source = recipe
        .directories
        .iter()
        .find(|item| item.directory == directory)
        .ok_or_else(|| refusal("directory has no generated mandatory defaults"))?;
    let mut values = source.defaults.to_vec();
    if directory == recipe.jfif_directory {
        if let Some(jfif) = jfif {
            if recipe.jfif_probe != "JFIFYResolution" || recipe.jfif_assignments.len() != 3 {
                return Err(refusal("JFIF substitution operands are unresolved"));
            }
            for assignment in recipe.jfif_assignments {
                let raw = match assignment.property {
                    "JFIFXResolution" => jfif.x,
                    "JFIFYResolution" => jfif.y,
                    "JFIFResolutionUnit" => jfif.resolution_unit,
                    _ => return Err(refusal("JFIF substitution property is unsupported")),
                };
                let value = raw
                    .checked_add(assignment.adjustment)
                    .ok_or_else(|| refusal("JFIF substitution overflow"))?;
                if let Some(target) = values
                    .iter_mut()
                    .find(|item| item.tag_id == assignment.tag_id)
                {
                    target.value = MandatoryValue::Integer(value);
                } else {
                    values.push(MandatoryDefault {
                        tag_id: assignment.tag_id,
                        value: MandatoryValue::Integer(value),
                    });
                }
            }
        }
    }
    Ok(values)
}
