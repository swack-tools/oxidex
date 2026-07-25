use crate::{TagDatabase, TagTable};
use std::collections::HashSet;
use thiserror::Error;

/// Single validation failure describing where and why it occurred.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationIssue {
    pub table: String,
    pub tag: Option<String>,
    pub message: String,
}

impl ValidationIssue {
    fn new(table: impl Into<String>, tag: Option<String>, message: impl Into<String>) -> Self {
        Self {
            table: table.into(),
            tag,
            message: message.into(),
        }
    }
}

/// Aggregated validation error with all issues discovered.
#[derive(Debug, Error)]
#[error("tag database validation failed with {} issue(s)", .issues.len())]
pub struct ValidationError {
    issues: Vec<ValidationIssue>,
}

impl ValidationError {
    pub fn new(issues: Vec<ValidationIssue>) -> Self {
        Self { issues }
    }

    pub fn issues(&self) -> &[ValidationIssue] {
        &self.issues
    }

    pub fn issue_count(&self) -> usize {
        self.issues.len()
    }
}

/// Validate a tag database for empty names and duplicate identifiers.
pub fn validate_database(db: &TagDatabase) -> Result<(), ValidationError> {
    let mut issues = Vec::new();
    let mut seen_tables = HashSet::new();

    for table in &db.tables {
        if table.name.trim().is_empty() {
            issues.push(ValidationIssue::new(
                table.name.clone(),
                None,
                "Table name must not be empty",
            ));
        }

        if !seen_tables.insert(table.name.clone()) {
            issues.push(ValidationIssue::new(
                table.name.clone(),
                None,
                "Duplicate table name detected",
            ));
        }

        validate_table(&mut issues, table);
    }

    if issues.is_empty() {
        Ok(())
    } else {
        Err(ValidationError::new(issues))
    }
}

fn validate_table(issues: &mut Vec<ValidationIssue>, table: &TagTable) {
    for tag in &table.tags {
        if tag.id.trim().is_empty() {
            issues.push(ValidationIssue::new(
                table.name.clone(),
                Some(tag.name.clone()),
                "Tag id must not be empty",
            ));
        }

        if tag.name.trim().is_empty() {
            issues.push(ValidationIssue::new(
                table.name.clone(),
                Some(tag.name.clone()),
                "Tag name must not be empty",
            ));
        }
    }
}
