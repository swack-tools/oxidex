//! File-scoped generated-runtime state regressions.
//!
//! These tests exercise the public `Session` lifecycle directly. The IFD
//! engine tests cover the same scope while walking real generated tables;
//! this file keeps the lifetime contract small enough to diagnose on its own.

use oxidex::exiftool_tables::session::{ByteOrder, MemberVal, Session};

#[test]
fn nested_directories_restore_only_directory_state() {
    let mut session = Session::new();
    session
        .set_member("ParentBytes", MemberVal::from_bytes(vec![0xff, 0x00, 0x80]))
        .unwrap();
    session.set_member("Compression", MemberVal::Undef).unwrap();
    session
        .set_member("DIR_NAME", MemberVal::Float(-0.0))
        .unwrap();
    session.byte_order = Some(ByteOrder::LittleEndian);
    session.count = Some(7);
    session.format = Some("outer".to_string());

    {
        let mut child = session.enter_directory(ByteOrder::BigEndian, Some("Child"));
        assert_eq!(
            child.member("ParentBytes"),
            MemberVal::Bytes(vec![0xff, 0x00, 0x80])
        );
        assert_eq!(
            child.member("DIR_NAME"),
            MemberVal::Str("Child".to_string())
        );
        assert_eq!(child.member("Compression"), MemberVal::Str(String::new()));
        assert_eq!(child.member("SubfileType"), MemberVal::Str(String::new()));
        assert_eq!(child.byte_order, Some(ByteOrder::BigEndian));
        assert_eq!(child.count, None);
        assert_eq!(child.format, None);

        child
            .set_member("ParentBytes", MemberVal::from_bytes(vec![0xfe, 0x81]))
            .unwrap();
        child
            .set_member("Compression", MemberVal::Str("JPEG".to_string()))
            .unwrap();
        child.count = Some(1);
        child.format = Some("int16u".to_string());

        {
            let mut grandchild = child.enter_directory(ByteOrder::LittleEndian, Some("Grandchild"));
            grandchild
                .set_member("Compression", MemberVal::Float(-0.0))
                .unwrap();
            grandchild.count = Some(2);
            grandchild.format = Some("undef".to_string());
        }

        assert_eq!(
            child.member("ParentBytes"),
            MemberVal::Bytes(vec![0xfe, 0x81])
        );
        assert_eq!(
            child.member("Compression"),
            MemberVal::Str("JPEG".to_string())
        );
        assert_eq!(child.count, Some(1));
        assert_eq!(child.format.as_deref(), Some("int16u"));
    }

    // File-scoped DataMembers survive the directory. Directory-local state
    // returns to its exact prior shape, including present-undef vs absent.
    assert_eq!(
        session.member("ParentBytes"),
        MemberVal::Bytes(vec![0xfe, 0x81])
    );
    assert!(session.has_member("Compression"));
    assert_eq!(session.member("Compression"), MemberVal::Undef);
    assert!(!session.has_member("SubfileType"));
    match session.member("DIR_NAME") {
        MemberVal::Float(value) => assert_eq!(value.to_bits(), (-0.0f64).to_bits()),
        other => panic!("negative zero was not restored exactly: {other:?}"),
    }
    assert_eq!(session.byte_order, Some(ByteOrder::LittleEndian));
    assert_eq!(session.count, Some(7));
    assert_eq!(session.format.as_deref(), Some("outer"));
}

fn leave_directory_early(session: &mut Session) -> Result<(), &'static str> {
    let mut scope = session.enter_directory(ByteOrder::BigEndian, Some("ErrorChild"));
    scope
        .set_member("Compression", MemberVal::Str("temporary".to_string()))
        .unwrap();
    scope.warn(MemberVal::Str("file warning".to_string()));
    scope.set_option("Validate", MemberVal::Int(1));
    Err("ordinary error")
}

#[test]
fn directory_scope_drops_on_error_without_rolling_back_file_effects() {
    let mut session = Session::new();
    session
        .set_member("Compression", MemberVal::Str("outer".to_string()))
        .unwrap();

    assert_eq!(leave_directory_early(&mut session), Err("ordinary error"));
    assert_eq!(
        session.member("Compression"),
        MemberVal::Str("outer".to_string())
    );
    assert_eq!(session.warnings().len(), 1);
    assert_eq!(session.option("Validate"), MemberVal::Int(1));
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Outcome {
    Report,
    Suppress,
    Decline,
}

fn leave_directory_normally(session: &mut Session, outcome: Outcome) -> Outcome {
    let mut scope = session.enter_directory(ByteOrder::BigEndian, Some("Temporary"));
    scope
        .set_member("Compression", MemberVal::Str("temporary".to_string()))
        .unwrap();
    scope.count = Some(99);
    scope.format = Some("temporary-format".to_string());
    outcome
}

#[test]
fn directory_scope_restores_after_every_conversion_outcome() {
    for outcome in [Outcome::Report, Outcome::Suppress, Outcome::Decline] {
        let mut session = Session::new();
        session
            .set_member("Compression", MemberVal::Str("outer".to_string()))
            .unwrap();
        session.count = Some(7);
        session.format = Some("outer-format".to_string());

        assert_eq!(leave_directory_normally(&mut session, outcome), outcome);
        assert_eq!(
            session.member("Compression"),
            MemberVal::Str("outer".to_string())
        );
        assert_eq!(session.count, Some(7));
        assert_eq!(session.format.as_deref(), Some("outer-format"));
        assert!(!session.has_member("DIR_NAME"));
    }
}
