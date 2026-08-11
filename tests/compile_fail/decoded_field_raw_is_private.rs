//! `DecodedField::raw` is a private field (Step 10, D1/D2): the only value
//! accessors are `DecodedField::emit` and `RawAccess`. A direct `.raw` read
//! from outside `oxidex::exiftool_tables::runtime` must not compile.

use oxidex::exiftool_tables::{decode_binary_table, find_table};
use oxidex::io::ByteOrder;

fn main() {
    let table = find_table("PhotoCD", "Main").expect("generated PhotoCD::Main table");
    let decode = decode_binary_table(table, &[0u8; 2048], ByteOrder::Big);
    let field = &decode.fields()[0];
    let _ = &field.raw;
}
