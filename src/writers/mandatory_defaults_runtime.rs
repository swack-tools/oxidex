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
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MandatoryRecipe {
    pub writer_source_file: &'static str,
    pub writer_source_sha256: &'static str,
    pub perl_version: &'static str,
    pub no_mandatory_guard: bool,
    pub directories: &'static [MandatoryDirectory],
    pub jfif_directory: &'static str,
    pub jfif_probe: &'static str,
    pub jfif_assignments: &'static [JfifAssignment],
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
