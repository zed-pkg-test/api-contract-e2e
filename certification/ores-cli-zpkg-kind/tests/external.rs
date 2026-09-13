use std::fs;
use std::path::Path;

use ores_cli_zpkg_kind_certification::audit::run;
use tempfile::tempdir;

const GOOD: &str = r#"[package]
org = "zed-pkg"
name = "zed-e2e"
version = "0.1.0"
description = "Zed validation fixture"
license = "MIT"
language = "typescript"

[package.repository]
vcs = "git"
url = "https://github.com/zed-pkg/zed-e2e"

[install]
adapter = "none"
dir = ".vendor/.zed"

[targets.repository]
dir = "."

[scripts]
test = "true"
"#;

fn write(root: &Path, body: &str) {
    fs::write(root.join(".zpkg.toml"), body).expect("manifest");
}

#[test]
fn exact_lint_accepts_good_and_rejects_package_kind() {
    let good = tempdir().expect("good repo");
    write(good.path(), GOOD);
    let report = run(good.path());
    assert_eq!(report.issue_count(), 0, "{:#?}", report.findings);

    let bad = tempdir().expect("bad repo");
    let bad_manifest = GOOD.replace("version = \"0.1.0\"", "version = \"0.1.0\"\nkind = \"pub-lib-core\"");
    write(bad.path(), &bad_manifest);
    let report = run(bad.path());
    assert!(report.findings.iter().any(|finding| finding.code == "zpkg-package-kind-noncanonical"));
}
