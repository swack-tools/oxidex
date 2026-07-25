use oxidex_tags_shared::TagDatabase;
use std::env;
use std::fs;
use std::path::Path;

/// Build script that pre-compiles YAML tag definitions to binary format
///
/// This eliminates the 40ms cold start penalty from runtime YAML parsing by:
/// 1. Reading the YAML file at build time
/// 2. Deserializing it with serde_yaml
/// 3. Serializing it to efficient binary format with bincode
/// 4. Embedding the binary data in the compiled binary via include_bytes!
///
/// Trade-off: Slightly larger binary size for significantly faster startup time
fn main() {
    // Tell Cargo to rerun this build script if the YAML file changes
    println!("cargo:rerun-if-changed=src/specialty_tags.yaml");

    // Read the YAML source file
    let yaml_path = "src/specialty_tags.yaml";
    let yaml_content = fs::read_to_string(yaml_path).expect("Failed to read specialty_tags.yaml");

    let tag_database: TagDatabase = serde_yaml::from_str(&yaml_content)
        .expect("Failed to parse specialty_tags.yaml during build");

    // Serialize to binary format using bincode 2.0 serde API
    // Uses legacy() config for compatibility with bincode 1.x binary format
    let binary_data = bincode::serde::encode_to_vec(&tag_database, bincode::config::legacy())
        .expect("Failed to serialize tag database to binary format");

    // Write binary data to OUT_DIR
    let out_dir = env::var("OUT_DIR").expect("OUT_DIR not set");
    let dest_path = Path::new(&out_dir).join("specialty_tags.bin");

    fs::write(&dest_path, binary_data).expect("Failed to write binary tag database file");
}
