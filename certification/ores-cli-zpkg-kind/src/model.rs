use std::collections::BTreeMap;

use serde_json::Value;

#[derive(Debug, Clone)]
pub struct Finding {
    pub code: String,
    pub message: String,
    pub severity: Severity,
    pub target: Option<String>,
    pub details: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

impl Finding {
    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(code, message, Severity::Error)
    }

    pub fn warning(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(code, message, Severity::Warning)
    }

    fn new(code: impl Into<String>, message: impl Into<String>, severity: Severity) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            severity,
            target: None,
            details: BTreeMap::new(),
        }
    }

    pub fn with_target(mut self, target: impl Into<String>) -> Self {
        self.target = Some(target.into());
        self
    }

    pub fn with_detail(mut self, key: impl Into<String>, value: Value) -> Self {
        self.details.insert(key.into(), value);
        self
    }
}

#[derive(Debug, Clone)]
pub struct CommandReport {
    pub command: String,
    pub findings: Vec<Finding>,
    pub metadata: BTreeMap<String, Value>,
}

impl CommandReport {
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            findings: Vec::new(),
            metadata: BTreeMap::new(),
        }
    }

    pub fn push(&mut self, finding: Finding) {
        self.findings.push(finding);
    }

    pub fn insert_metadata(&mut self, key: impl Into<String>, value: Value) {
        self.metadata.insert(key.into(), value);
    }

    #[must_use]
    pub fn finalize(self) -> Self {
        self
    }

    #[must_use]
    pub fn issue_count(&self) -> usize {
        self.findings
            .iter()
            .filter(|finding| finding.severity == Severity::Error)
            .count()
    }
}
