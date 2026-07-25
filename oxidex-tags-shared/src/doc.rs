use crate::{Tag, TagDatabase, TagTable};

/// Render a Markdown summary for an entire tag domain.
///
/// Includes total table/tag counts and short previews for each table.
pub fn render_domain_summary(domain_name: &str, db: &TagDatabase) -> String {
    let total_tables = db.tables.len();
    let total_tags: usize = db.tables.iter().map(|table| table.tags.len()).sum();

    let mut output = format!("# {domain_name} Tag Domain\n\n");
    output.push_str(&format!(
        "- Tables: {total_tables}\n- Total tags: {total_tags}\n\n"
    ));

    for table in &db.tables {
        output.push_str(&render_table_preview(table, 100));
    }

    output
}

/// Render a Markdown snippet for a single tag table.
///
/// `preview_limit` controls how many tag entries to include before truncating.
pub fn render_table_preview(table: &TagTable, preview_limit: usize) -> String {
    let mut output = format!("## {} ({} tags)\n\n", table.name, table.tags.len());
    let limit = preview_limit.min(table.tags.len());

    for tag in table.tags.iter().take(limit) {
        output.push_str(&format_tag_entry(tag));
    }

    if table.tags.len() > limit {
        output.push_str(&format!(
            "- _... plus {} more tags_\n",
            table.tags.len() - limit
        ));
    }

    output.push('\n');
    output
}

fn format_tag_entry(tag: &Tag) -> String {
    let desc = tag
        .description
        .as_deref()
        .unwrap_or("No description provided");
    format!("- `{}` — {}\n", tag.name, desc)
}
