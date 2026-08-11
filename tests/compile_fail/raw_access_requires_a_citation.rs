//! `RawAccess::new` requires a `&'static PerlCitation` on every call (Step
//! 10, D2: "required, no exceptions"). A call that supplies only the field
//! and the acknowledgment -- no citation -- must not compile.

use oxidex::exiftool_tables::{Acknowledged, RawAccess, decode_binary_table, find_table};
use oxidex::io::ByteOrder;

fn main() {
    let table = find_table("PhotoCD", "Main").expect("generated PhotoCD::Main table");
    let decode = decode_binary_table(table, &[0u8; 2048], ByteOrder::Big);
    let field = &decode.fields()[0];
    let _ = RawAccess::new(field, Acknowledged::VALUE_CONV);
}
