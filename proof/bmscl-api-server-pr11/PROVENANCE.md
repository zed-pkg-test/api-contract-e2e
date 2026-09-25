# bmscl-api-server PR #11 targeted alternate proof

Parent repository Actions failed before any job steps/logs were created, and the first BeamScale test-org fallback had the same pre-run failure. This proof therefore uses a separate organization budget.

Source repository: `beamscale/bmscl-api-server.rs`
Source pull request: `#11`
Source commit: `055016b42e7540a33a1e2cb6760cf52c6efbd3a3`

Exact source blobs used:
- `Cargo.toml`: `d9ef4b7a0e385fdb5a135e810aa9033c6fa079ae`
- changed Rust module `src/security.rs`: `33706dd384ce07578fe6bcc8a2a97d3a9966f6db`
- source CI workflow: `18f06cbea9d51e16ca4dced6017382beb4714ed4`

PR #11 changes only `src/security.rs` plus documentation. The unchanged `main.rs` / placement path was already validated in the immediately preceding merged security/placement work. This fallback compiles the exact changed module as a library and runs the source repository's substantive Rust gates: stable Rust, clippy with `-D warnings`, all-target tests, and rustfmt.

`src/lib.rs` in this proof directory is test harness glue only (`pub mod security;`) and is not source code proposed for the BeamScale repository.
