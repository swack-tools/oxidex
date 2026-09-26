//! Compile-checks the write signatures `docs/reference/api-reference.md`
//! states (its ```rust,ignore blocks are not doctests), and
//! `library_write_codex_threads::the_api_reference_states_the_write_outcome_signatures`
//! checks the page states these. A signature change that is not also made
//! on that page fails one of the two.

use oxidex::Metadata;
use oxidex::core::operations::{
    CopyReport, clear_all_metadata, copy_metadata, copy_metadata_report, modify_tag, remove_tag,
    write_metadata,
};
use oxidex::core::{MetadataMap, TagValue, WriteOutcome};
use oxidex::error::Result;
use std::path::Path;

#[test]
fn write_signatures_match_the_api_reference() {
    let _: fn(&Path, &str, TagValue) -> Result<WriteOutcome> = modify_tag;
    let _: fn(&Path, &str) -> Result<WriteOutcome> = remove_tag;
    let _: fn(&Path, &MetadataMap) -> Result<WriteOutcome> = write_metadata;
    let _: fn(&Path) -> Result<WriteOutcome> = clear_all_metadata;
    let _: fn(&Path, &Path, Option<&[String]>) -> Result<WriteOutcome> = copy_metadata;
    let _: fn(&Path, &Path, Option<&[String]>) -> Result<CopyReport> = copy_metadata_report;
    let _: fn(&Metadata) -> Result<WriteOutcome> = Metadata::save;
    fn write_to(metadata: &Metadata, path: &Path) -> Result<WriteOutcome> {
        metadata.write_to(path)
    }
    let _: fn(&Metadata, &Path) -> Result<WriteOutcome> = write_to;
}
