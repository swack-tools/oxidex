//! `DecodedField::apply_print_conv_to_raw` is removed (Step 10): it was the
//! only method that consulted a `Field::omitted` flag before this step, and
//! it consulted just one of the five (`value_conv`). `DecodedField::emit` is
//! its replacement and the only remaining flag-consulting accessor.

use oxidex::exiftool_tables::{decode_binary_table, find_table};
use oxidex::io::ByteOrder;

fn main() {
    let table = find_table("PhotoCD", "Main").expect("generated PhotoCD::Main table");
    let decode = decode_binary_table(table, &[0u8; 2048], ByteOrder::Big);
    let field = &decode.fields()[0];
    let _ = field.apply_print_conv_to_raw();
}
