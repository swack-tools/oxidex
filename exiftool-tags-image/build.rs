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
    println!("cargo:rerun-if-changed=src/image_tags.yaml");

    // Read the YAML source file
    let yaml_path = "src/image_tags.yaml";
    let yaml_content = fs::read_to_string(yaml_path).expect("Failed to read image_tags.yaml");

    // Deserialize YAML into TagDatabase structure
    // We need to use the same types that will be used at runtime
    #[derive(serde::Deserialize, serde::Serialize)]
    struct Tag {
        id: String,
        name: String,
        writable: bool,
        #[serde(rename = "type")]
        type_name: Option<String>,
        description: Option<String>,
    }

    #[derive(serde::Deserialize, serde::Serialize)]
    struct TagTable {
        name: String,
        tags: Vec<Tag>,
    }

    #[derive(serde::Deserialize, serde::Serialize)]
    struct TagDatabase {
        tables: Vec<TagTable>,
    }

    let tag_database: TagDatabase =
        serde_yaml::from_str(&yaml_content).expect("Failed to parse image_tags.yaml during build");

    // Serialize to binary format using bincode
    let binary_data = bincode::serialize(&tag_database)
        .expect("Failed to serialize tag database to binary format");

    // Write binary data to OUT_DIR
    let out_dir = env::var("OUT_DIR").expect("OUT_DIR not set");
    let dest_path = Path::new(&out_dir).join("image_tags.bin");

    fs::write(&dest_path, binary_data).expect("Failed to write binary tag database file");

    println!(
        "cargo:warning=Pre-compiled image_tags.yaml to binary format ({} bytes)",
        fs::metadata(&dest_path).unwrap().len()
    );
}
