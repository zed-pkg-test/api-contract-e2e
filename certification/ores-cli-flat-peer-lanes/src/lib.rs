use std::collections::BTreeMap;
use std::path::PathBuf;

pub mod model {
    use std::collections::BTreeMap;

    use serde_json::Value;

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum Severity {
        Error,
        Info,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Finding {
        pub code: String,
        pub message: String,
        pub target: Option<String>,
        pub severity: Severity,
    }

    impl Finding {
        pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
            Self {
                code: code.into(),
                message: message.into(),
                target: None,
                severity: Severity::Error,
            }
        }

        pub fn info(code: impl Into<String>, message: impl Into<String>) -> Self {
            Self {
                code: code.into(),
                message: message.into(),
                target: None,
                severity: Severity::Info,
            }
        }

        pub fn with_target(mut self, target: impl Into<String>) -> Self {
            self.target = Some(target.into());
            self
        }
    }

    #[derive(Debug, Clone)]
    pub struct CommandReport {
        pub findings: Vec<Finding>,
        pub metadata: BTreeMap<String, Value>,
        pub label: String,
    }

    impl CommandReport {
        pub fn new(label: impl Into<String>) -> Self {
            Self {
                findings: Vec::new(),
                metadata: BTreeMap::new(),
                label: label.into(),
            }
        }

        pub fn push(&mut self, finding: Finding) {
            self.findings.push(finding);
        }

        pub fn insert_metadata(&mut self, key: impl Into<String>, value: Value) {
            self.metadata.insert(key.into(), value);
        }

        pub fn issue_count(&self) -> usize {
            self.findings
                .iter()
                .filter(|finding| finding.severity == Severity::Error)
                .count()
        }

        pub fn finalize(self) -> Self {
            self
        }
    }
}

pub mod audit {
    use std::path::PathBuf;

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct RepositoryAuditOptions {
        pub path: PathBuf,
        pub profile: String,
        pub additional_required_paths: Vec<String>,
    }

    #[path = "split_peer_contracts.rs"]
    pub mod split_peer_contracts;
}

pub fn smoke_options(path: PathBuf) -> audit::RepositoryAuditOptions {
    audit::RepositoryAuditOptions {
        path,
        profile: "baseline".to_owned(),
        additional_required_paths: Vec::new(),
    }
}

pub fn empty_metadata() -> BTreeMap<String, serde_json::Value> {
    BTreeMap::new()
}
