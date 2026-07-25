use oxidex_tags::{camera, core, document, image, media, specialty};
use oxidex_tags_shared::render_domain_summary;
use std::env;
use std::fs;
use std::process;

fn main() {
    let mut args = env::args().skip(1);
    let domain = args.next().unwrap_or_else(|| usage());
    let output = args.next().unwrap_or_else(|| usage());

    if args.next().is_some() {
        usage();
    }

    let (display_name, db) = match domain.as_str() {
        "core" => ("Core Domain", &*core::CORE_TAGS),
        "camera" => ("Camera Manufacturers", &*camera::CAMERA_TAGS),
        "media" => ("Media Formats", &*media::MEDIA_TAGS),
        "image" => ("Image Formats", &*image::IMAGE_TAGS),
        "document" => ("Document Formats", &*document::DOCUMENT_TAGS),
        "specialty" => ("Specialty Formats", &*specialty::SPECIALTY_TAGS),
        other => {
            eprintln!(
                "Unknown domain '{other}'. Expected one of: core, camera, media, image, document, specialty"
            );
            process::exit(2);
        }
    };

    let markdown = render_domain_summary(display_name, db);
    fs::write(&output, markdown).expect("Failed to write domain summary");
    println!("Wrote {output}");
}

fn usage() -> ! {
    eprintln!("Usage: cargo run -p oxidex-tags --example render_domain -- <domain> <output-path>");
    eprintln!("Domains: core, camera, media, image, document, specialty");
    process::exit(1);
}
