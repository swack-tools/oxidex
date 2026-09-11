"""Exercise the corpus guard check through its CLI on small Rust source trees."""
import pathlib
import subprocess
import sys
import tempfile
import unittest


CHECKER = pathlib.Path(__file__).with_name("check-corpus-guards.py")
FIXTURE = "/tmp/oxidex-exiftool-cache/combined-samples/Nikon.nef"


class CorpusGuardTests(unittest.TestCase):
    def check(self, source, expected):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            (root / "src").mkdir()
            (root / "src" / "lib.rs").write_text(source.replace("FIXTURE", FIXTURE))
            result = subprocess.run(
                [sys.executable, str(CHECKER)], cwd=root,
                capture_output=True, text=True,
            )
        self.assertEqual(result.returncode, expected, result.stdout + result.stderr)
        if expected:
            self.assertIn("src/lib.rs:", result.stdout)

    def test_module_constant_after_completed_test_is_not_a_read(self):
        self.check('''
#[test]
fn previous() { assert_eq!(1, 1); }
const IMAGE: &str = "FIXTURE";
''', 0)

    def test_module_constant_after_nested_blocks_is_not_a_read(self):
        self.check('''
#[test]
fn previous() {
    if true { assert_eq!("{", r###" } { "###); }
    /* a comment with { and /* nested } */ braces */
}
const IMAGE: &str = "FIXTURE";
''', 0)

    def test_real_read_after_previous_test_is_rejected(self):
        self.check('''
#[test]
fn previous() {
    if !crate::test_support::pinned_corpus_available() { return; }
}
#[test]
fn unguarded() { std::fs::read("FIXTURE").unwrap(); }
''', 1)

    def test_nested_block_does_not_end_test_early(self):
        self.check('''
#[test]
fn unguarded() {
    if true { let _ = "}"; }
    std::fs::read("FIXTURE").unwrap();
}
''', 1)

    def test_raw_string_does_not_end_test_early(self):
        self.check('''
#[test]
fn unguarded() {
    let _ = r##" } \" } // } "##;
    std::fs::read("FIXTURE").unwrap();
}
''', 1)

    def test_multiline_signature_remains_checked(self):
        self.check('''
#[test]
fn unguarded(
) {
    std::fs::read("FIXTURE").unwrap();
}
''', 1)

    def test_global_availability_early_return_is_accepted(self):
        self.check('''
#[test]
fn guarded() {
    if !crate::test_support::pinned_corpus_available() { return; }
    std::fs::read("FIXTURE").unwrap();
}
''', 0)

    def test_path_specific_early_return_is_accepted(self):
        self.check('''
#[test]
fn guarded() {
    if !std::path::Path::new("FIXTURE").is_file() {
        eprintln!("skipping: {}", "FIXTURE");
        return;
    }
    std::fs::read("FIXTURE").unwrap();
}
''', 0)

    def test_guard_mentions_in_comments_or_strings_do_not_count(self):
        for mention in (
            '// pinned_corpus_available() .exists()',
            'let _ = "pinned_corpus_available() .is_file()";',
        ):
            with self.subTest(mention=mention):
                self.check('''
#[test]
fn unguarded() {
    MENTION
    std::fs::read("FIXTURE").unwrap();
}
'''.replace("MENTION", mention), 1)

    def test_unrelated_existence_check_does_not_guard_read(self):
        self.check('''
#[test]
fn unguarded() {
    if !Path::new("other.jpg").exists() { return; }
    std::fs::read("FIXTURE").unwrap();
}
''', 1)

    def test_availability_call_without_early_return_is_not_a_guard(self):
        self.check('''
#[test]
fn unguarded() {
    let _ = crate::test_support::pinned_corpus_available();
    std::fs::read("FIXTURE").unwrap();
}
''', 1)

    def test_conditional_guard_does_not_guard_later_read(self):
        self.check('''
#[test]
fn unguarded() {
    if optional {
        if !crate::test_support::pinned_corpus_available() { return; }
    }
    std::fs::read("FIXTURE").unwrap();
}
''', 1)

    def test_closure_return_is_not_a_test_early_return(self):
        self.check('''
#[test]
fn unguarded() {
    if !crate::test_support::pinned_corpus_available() {
        let unused = || return;
    }
    std::fs::read("FIXTURE").unwrap();
}
''', 1)

    def test_return_tokens_inside_macro_arguments_are_not_a_guard(self):
        self.check('''
#[test]
fn unguarded() {
    if !crate::test_support::pinned_corpus_available() {
        discard_tokens!(anything; return;);
    }
    std::fs::read("FIXTURE").unwrap();
}
''', 1)

    def test_guard_in_uninvoked_expression_closure_does_not_count(self):
        self.check('''
#[test]
fn unguarded() {
    let check = || if !crate::test_support::pinned_corpus_available() { return; };
    std::fs::read("FIXTURE").unwrap();
}
''', 1)

    def test_short_circuited_guard_does_not_count(self):
        self.check('''
#[test]
fn unguarded() {
    let _ = false && if !crate::test_support::pinned_corpus_available() {
        return;
    } else { true };
    std::fs::read("FIXTURE").unwrap();
}
''', 1)

    def test_guard_after_read_does_not_hide_it(self):
        self.check('''
#[test]
fn unguarded() {
    std::fs::read("FIXTURE").unwrap();
    if !crate::test_support::pinned_corpus_available() { return; }
}
''', 1)

    def test_read_inside_print_arguments_is_not_a_diagnostic_string(self):
        self.check('''
#[test]
fn unguarded() {
    eprintln!("{:?}", std::fs::read("FIXTURE").unwrap());
}
''', 1)

    def test_character_and_comment_braces_do_not_end_test(self):
        self.check('''
#[test]
fn unguarded() {
    let close = '}'; // } fake end
    /* } and /* } */ } */
    std::fs::read(r#"FIXTURE"#).unwrap();
}
''', 1)

    def test_comments_are_not_reads(self):
        self.check('''
#[test]
fn comments() {
    // "FIXTURE"
    /* "FIXTURE" */
}
''', 0)


if __name__ == "__main__":
    unittest.main()
