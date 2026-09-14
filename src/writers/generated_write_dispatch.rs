//! Join generated public address planning to generated scalar file operations.
//! Public migration ownership and the final atomic file replacement live at
//! the application boundary; this layer never retries a handwritten tag rule.
use super::generated_scalar::Scalar;
use super::generated_setnewvalue_address_rules::{
    SET_NEW_VALUE_ADDRESS_CAPTURE, SET_NEW_VALUE_ADDRESSING, StaticSetNewValueAddress,
};
use super::tiff_surgical::generated_scalar::ResolvedScalarWriteRequest;
use crate::error::{ExifToolError, Result};

pub(crate) fn resolved_scalar_request(
    row: &StaticSetNewValueAddress,
    value: Scalar,
) -> Result<ResolvedScalarWriteRequest<'static>> {
    let capture = SET_NEW_VALUE_ADDRESS_CAPTURE.ok_or_else(|| {
        ExifToolError::unsupported_format("generated public address capture is unavailable")
    })?;
    let row = SET_NEW_VALUE_ADDRESSING
        .and_then(|rows| rows.get(row.index))
        .filter(|canonical| {
            canonical.index == row.index
                && canonical.module == row.module
                && canonical.table == row.table
                && canonical.full_name == row.full_name
                && canonical.raw_id == row.raw_id
                && canonical.name == row.name
                && canonical.group0 == row.group0
                && canonical.group1 == row.group1
                && canonical.write_group == row.write_group
        })
        .ok_or_else(|| {
            ExifToolError::unsupported_format("address is not in the selected generated capture")
        })?;
    Ok(ResolvedScalarWriteRequest {
        module: row.module,
        table: row.table,
        full_name: row.full_name,
        raw_id: row.raw_id,
        name: row.name,
        write_group: row.write_group,
        write_proc_source_sha256: capture.write_exif_source_sha256,
        registry_source_sha256: capture.exif_source_sha256,
        writer_source_sha256: capture.writer_source_sha256,
        value,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_rows_not_in_the_selected_capture() {
        let row = &SET_NEW_VALUE_ADDRESSING.unwrap()[0];
        let forged = StaticSetNewValueAddress {
            index: row.index,
            module: row.module,
            table: row.table,
            full_name: row.full_name,
            raw_id: row.raw_id,
            name: row.name,
            group0: row.group0,
            group1: row.group1,
            write_group: "forged-directory",
        };
        assert!(resolved_scalar_request(&forged, Scalar::Undefined).is_err());
        let valid = resolved_scalar_request(row, Scalar::Undefined).unwrap();
        assert_eq!(
            valid.writer_source_sha256,
            SET_NEW_VALUE_ADDRESS_CAPTURE.unwrap().writer_source_sha256
        );
    }
}
