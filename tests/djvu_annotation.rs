use djvu_bzz::bzz_encode;
use oxidex::core::operations::read_metadata;
use tempfile::Builder;

fn chunk(id: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + data.len() + data.len() % 2);
    out.extend_from_slice(id);
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(data);
    if data.len() % 2 == 1 {
        out.push(0);
    }
    out
}

fn djvu_with_compressed_annotation(annotation: &str) -> Vec<u8> {
    let annotation = format!("(metadata (annote \"{annotation}\"))");
    let antz = chunk(b"ANTz", &bzz_encode(annotation.as_bytes()));

    let mut page = b"DJVI".to_vec();
    page.extend_from_slice(&antz);
    let page = chunk(b"FORM", &page);

    let mut file = b"AT&TFORM".to_vec();
    file.extend_from_slice(&((4 + page.len()) as u32).to_be_bytes());
    file.extend_from_slice(b"DJVM");
    file.extend_from_slice(&page);
    file
}

#[test]
fn extracts_annotation_from_a_compressed_nested_djvu_form() {
    let file = Builder::new()
        .suffix(".djvu")
        .tempfile()
        .expect("creates DjVu test file");
    std::fs::write(
        file.path(),
        djvu_with_compressed_annotation("Did you get this?"),
    )
    .expect("writes DjVu test file");

    let metadata = read_metadata(file.path()).expect("reads DjVu metadata");

    assert_eq!(
        metadata.get_string("DjVu:Annotation"),
        Some("Did you get this?")
    );
}
