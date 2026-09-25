# ores-proc-isolation-cli PR #6 cross-org proof

Source repository: `ORESoftware/ores-proc-isolation-cli`
Source PR: `#6`
Final source head: `041aa4ce3fb451da0b30e5990caaa3e42f9fcb70`
Merge commit: `e06952ba12c67182bf9e982c63eded2a6026470c`

The source org's four final CI jobs failed before any job steps were created (`steps: null`), so they provided no code signal. This workflow checks out the exact final source commit directly and verifies its SHA before running the Linux portions of the source workflow: shell syntax, ShellCheck, rustfmt, clippy with `-D warnings`, all-target tests, config/contract validation, backend doctor, and sandbox smoke checks.

The cross-org workflow does not modify or mirror source code. A different source SHA invalidates this proof.
