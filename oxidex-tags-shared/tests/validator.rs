use oxidex_tags_shared::{
    Tag, TagDatabase, TagTable, render_domain_summary, render_table_preview, validate_database,
};

fn sample_tag(name: &str) -> Tag {
    Tag {
        id: format!("0x{name}"),
        name: name.to_string(),
        writable: true,
        type_name: Some("string".into()),
        description: Some(format!("{name} description")),
    }
}

fn valid_db() -> TagDatabase {
    TagDatabase {
        tables: vec![TagTable {
            name: "Sample::Main".into(),
            tags: vec![sample_tag("Foo"), sample_tag("Bar")],
        }],
    }
}

#[test]
fn passes_validation_for_well_formed_schema() {
    let db = valid_db();
    validate_database(&db).expect("valid database should pass validation");
}

#[test]
fn detects_empty_tag_id() {
    let mut db = valid_db();
    db.tables[0].tags[0].id.clear();

    let err = validate_database(&db).expect_err("empty tag ids should fail validation");
    assert!(
        err.issues()
            .iter()
            .any(|issue| issue.message.contains("Tag id must not be empty")),
        "expected empty id warning, got {:?}",
        err.issues()
    );
}

#[test]
fn renders_domain_summary() {
    let db = valid_db();
    let summary = render_domain_summary("Camera", &db);
    assert!(
        summary.contains("# Camera Tag Domain"),
        "summary includes heading"
    );

    let preview = render_table_preview(&db.tables[0], 1);
    assert!(
        preview.contains("Sample::Main"),
        "preview should include table name"
    );
    assert!(preview.contains("..."), "preview indicates truncation");
}
