/**
 * Build script for C FFI integration test
 *
 * This script compiles the C test file and links it against the Rust library.
 */

fn main() {
    // Only build C test if the feature is enabled or we're in test mode
    if cfg!(test) {
        let out_dir = std::env::var("OUT_DIR").unwrap();

        // Compile the C test file
        cc::Build::new()
            .file("tests/ffi/c_integration_test.c")
            .include("include")
            .warnings(true)
            .extra_warnings(true)
            .flag_if_supported("-std=c99")
            .flag_if_supported("-pedantic")
            .compile("c_integration_test");

        // Tell cargo to link the Rust library
        println!("cargo:rustc-link-search=native={}", out_dir);
        println!("cargo:rustc-link-lib=static=exiftool_rs");

        // Rebuild if the C source changes
        println!("cargo:rerun-if-changed=tests/ffi/c_integration_test.c");
        println!("cargo:rerun-if-changed=include/exiftool_rs.h");
    }
}
