//! Compile-fail proof that Step 10's bypass-proof API cannot be bypassed.
//!
//! `OVERHAUL_OXIDEX_PLAN.md`'s Step 10 requires that a caller cannot reach a
//! `decode_binary_table` field's raw value without going through
//! [`oxidex::exiftool_tables::RawAccess`] -- not merely "reviewers should
//! reject a PR that does this", but "the crate does not compile if you try".
//! `trybuild` is what turns that claim into something CI actually checks: each
//! fixture under `tests/compile_fail/` is a small program that attempts one of
//! the bypasses the old API allowed, paired with the `rustc` error it must now
//! produce.
//!
//! Regenerate the `.stderr` files after a wording-only rustc/API change with:
//! `TRYBUILD=overwrite cargo test --test compile_fail`, then diff the result
//! before committing -- an overwrite that silently drops a fixture's failure
//! is the one outcome this test exists to catch.
#[test]
fn bypass_paths_do_not_compile() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile_fail/*.rs");
}
