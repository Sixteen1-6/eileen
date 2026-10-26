# Simulator regressions

Each file here is a minimised simulator failure, a `SimRegression` of BUILD.md section 3.3.16,
named `<name>.toml`: the profile, the seed that found it, the commit, the decoded inputs (one
JSON `InputRecord` per string, never a buffer of random bytes), every injected fault as an
explicit record, and the invariant that failed.

Every pull request replays every file here (`eileen-sim::it::regressions`), and each must pass:
a file is added together with the fix of the failure it records. Replay one with
`cargo xtask sim --replay crates/eileen-sim/regressions/<name>.toml`.
