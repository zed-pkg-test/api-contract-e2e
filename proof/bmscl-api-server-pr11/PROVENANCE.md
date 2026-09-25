# bmscl-api-server PR #11 targeted alternate proof

Parent repository Actions failed before any job steps/logs were created, and the first BeamScale test-org fallback had the same pre-run failure. This proof therefore uses a separate organization budget.

Source repository: `beamscale/bmscl-api-server.rs`
Source pull request: `#11`
Source commit: `c2854214c9c6d8687da2f49519a48c8985bbbf49`

Exact source blobs used:
- `Cargo.toml`: `557aa374bb0c9e3490271f7bafbbf16da14f633e`
- changed Rust module `src/security.rs`: `4442bf346bd3a7ed5b539a149aaf2dc2d7715c6c`
- source CI workflow: `18f06cbea9d51e16ca4dced6017382beb4714ed4`

PR #11 changes Rust security-boundary code plus documentation/build metadata. The unchanged `main.rs` / placement path was already validated in the immediately preceding merged security/placement work. This fallback compiles the exact current changed security module with the exact current Cargo manifest and runs the source repository's substantive Rust gates: stable Rust, clippy with `-D warnings`, all-target tests, and rustfmt.

`src/lib.rs` in this proof directory is test harness glue only (`pub mod security;`) and is not source code proposed for the BeamScale repository.
