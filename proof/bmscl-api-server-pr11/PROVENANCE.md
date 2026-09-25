# bmscl-api-server PR #11 targeted alternate proof

Parent repository Actions failed before any job steps/logs were created, and the first BeamScale test-org fallback had the same pre-run failure. This proof therefore uses a separate organization budget.

Source repository: `beamscale/bmscl-api-server.rs`
Source pull request: `#11`
Final merged source head: `e45a7f6596545b2e0d6a59195ddf8b10a6589de9`
Merge commit: `bb141a676e3bdbe2b0037e6bbb44e512cc1d6b01`

Exact source blobs used:
- `Cargo.toml`: `557aa374bb0c9e3490271f7bafbbf16da14f633e`
- changed Rust module `src/security.rs`: `f06516ab3de5f0acccd9a20e80d1bcf28da7c4f2`
- source CI workflow: `18f06cbea9d51e16ca4dced6017382beb4714ed4`

PR #11 changes Rust security-boundary code plus documentation/build metadata. The unchanged `main.rs` / placement path was already validated in the immediately preceding merged security/placement work. This fallback compiles the exact final changed security module with the exact final Cargo manifest and runs the source repository's substantive Rust gates: stable Rust, clippy with `-D warnings`, all-target tests, and rustfmt.

`src/lib.rs` in this proof directory is test harness glue only (`pub mod security;`) and is not source code proposed for the BeamScale repository.
