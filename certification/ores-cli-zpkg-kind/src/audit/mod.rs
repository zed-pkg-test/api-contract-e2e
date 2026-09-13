use std::path::{Path, PathBuf};

use crate::model::CommandReport;

mod zpkg_manifest;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryAuditOptions {
    pub path: PathBuf,
    pub profile: String,
    pub additional_required_paths: Vec<String>,
}

#[must_use]
pub fn run(root: &Path) -> CommandReport {
    zpkg_manifest::augment_zpkg_manifest_audit(
        &RepositoryAuditOptions {
            path: root.to_path_buf(),
            profile: "baseline".to_owned(),
            additional_required_paths: Vec::new(),
        },
        CommandReport::new("audit repo"),
    )
}
