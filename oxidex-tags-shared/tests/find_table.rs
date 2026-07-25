use oxidex_tags_shared::{LookupError, Tag, TagDatabase, TagTable, find_table};

fn sample_db() -> TagDatabase {
    TagDatabase {
        tables: vec![TagTable {
            name: "Exif::Main".into(),
            tags: vec![Tag {
                id: "0x0001".into(),
                name: "InteropIndex".into(),
                writable: false,
                type_name: Some("string".into()),
                description: Some("Indicates the identification of interoperability rule".into()),
            }],
        }],
    }
}

#[test]
fn finds_existing_table() {
    let db = sample_db();
    let table = find_table(&db, "Exif::Main").unwrap();
    assert_eq!(table.name, "Exif::Main");
}

#[test]
fn errors_on_missing_table() {
    let db = sample_db();
    match find_table(&db, "GPS::Main") {
        Ok(_) => panic!("expected missing table to error"),
        Err(LookupError::NotFound(name)) => assert_eq!(name, "GPS::Main"),
    }
}
