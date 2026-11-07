//! Config assertions over `.github/workflows/` that no command checks: how the jobs that save
//! rust-cache, and every job that restores what they save, keep the cache usable, and how the
//! test jobs keep insta's pending snapshots from a failed run out of the tree and in the run's
//! artifact. CI runs these tests like any other, so a later edit that drops or changes one of
//! these steps fails the pull request that makes it.

#[cfg(test)]
mod tests {
    const CI: &str = include_str!("../../.github/workflows/ci.yml");
    const TOOLCHAIN: &str = include_str!("../../rust-toolchain.toml");

    /// The jobs that restore and, on `main`, save a rust-cache entry, with their shared keys.
    const SAVERS: [(&str, &str); 3] =
        [("logic-linux", "linux"), ("windows-clippy", "win-clippy"), ("windows-test", "win-test")];
    /// The condition rust-cache's `save-if` uses in every saving job.
    const ON_MAIN: &str = "github.ref == 'refs/heads/main'";
    /// Jobs, by workflow file, that restore a saved key but still keep the runner image's other
    /// toolchains, so their key differs from the one `main` saves and they find no full match.
    // FIXME(#144): the Linux desktop lane gains the step in its own pull request; this list then
    // empties, and the test below fails until it does.
    const WITHOUT_THE_TOOLCHAIN_STEP: [(&str, &str); 1] = [("linux-desktop.yml", "session")];

    fn indent_of(line: &str) -> usize {
        line.len() - line.trim_start().len()
    }

    /// The names of a workflow's jobs, in order.
    fn jobs(workflow: &str) -> Vec<&str> {
        workflow
            .lines()
            .skip_while(|line| *line != "jobs:")
            .skip(1)
            .filter(|line| indent_of(line) == 2 && !line.trim().starts_with('#'))
            .filter_map(|line| line.trim().strip_suffix(':'))
            .collect()
    }

    /// The lines of one job under `jobs:`, from its `name:` line to the next job.
    fn job<'a>(workflow: &'a str, name: &str) -> Vec<&'a str> {
        let lines: Vec<&str> = workflow.lines().collect();
        let header = format!("{name}:");
        let start = lines
            .iter()
            .position(|line| indent_of(line) == 2 && line.trim() == header)
            .unwrap_or_else(|| panic!("the workflow has no `{name}` job"));
        lines[start + 1..]
            .iter()
            .take_while(|line| line.trim().is_empty() || indent_of(line) > 2)
            .copied()
            .collect()
    }

    /// A job's steps, each as the text of its lines, trimmed and without comment lines, in
    /// order, so a comment can neither satisfy nor fail a check. Empty for a job without steps,
    /// such as one that calls a reusable workflow.
    fn steps(job: &[&str]) -> Vec<String> {
        let Some(at) = job.iter().position(|line| line.trim() == "steps:") else {
            return Vec::new();
        };
        let indent = indent_of(job[at]);
        let mut steps: Vec<String> = Vec::new();
        for line in &job[at + 1..] {
            if line.trim().is_empty() || line.trim().starts_with('#') {
                continue;
            }
            if indent_of(line) <= indent {
                break;
            }
            if indent_of(line) == indent + 2 && line.trim_start().starts_with("- ") {
                steps.push(String::new());
            }
            if let Some(step) = steps.last_mut() {
                step.push_str(line.trim());
                step.push('\n');
            }
        }
        steps
    }

    /// The index of the first step that holds every one of `needles`.
    fn position(steps: &[String], needles: &[&str]) -> usize {
        try_position(steps, needles)
            .unwrap_or_else(|| panic!("no step holds {needles:?}: {steps:#?}"))
    }

    fn try_position(steps: &[String], needles: &[&str]) -> Option<usize> {
        steps.iter().position(|step| needles.iter().all(|needle| step.contains(needle)))
    }

    /// Whether one of the step's keys, not a comment or a script line, starts with `key`.
    fn has_key(step: &str, key: &str) -> bool {
        step.lines().any(|line| line.trim_start_matches("- ").starts_with(key))
    }

    /// Whether the text holds a version number such as `1.99`, however it is written.
    fn has_version_number(text: &str) -> bool {
        text.as_bytes()
            .windows(3)
            .any(|w| w[0].is_ascii_digit() && w[1] == b'.' && w[2].is_ascii_digit())
    }

    /// The value of a rust-cache step's `shared-key`, without quotes or a trailing comment.
    fn shared_key(step: &str) -> Option<&str> {
        step.lines().find_map(|line| {
            let value = line.trim_start_matches("- ").strip_prefix("shared-key:")?;
            let value = value.split(" #").next().unwrap_or_default().trim();
            Some(value.trim_matches(['"', '\'']))
        })
    }

    fn on_windows(job: &[&str]) -> bool {
        job.iter().any(|line| line.trim() == "runs-on: windows-latest")
    }

    const TOOLCHAIN_STEP_NEEDLES: [&str; 2] = ["rust-toolchain.toml", "rustup toolchain uninstall"];

    /// `logic-linux`'s step that keeps only the pinned toolchain: every other job that uses one
    /// of the saved keys runs the same text.
    fn reference_toolchain_step() -> String {
        let steps = steps(&job(CI, SAVERS[0].0));
        steps[position(&steps, &TOOLCHAIN_STEP_NEEDLES)].clone()
    }

    #[test]
    fn cache_savers_keep_only_the_pinned_toolchain() {
        let channel = TOOLCHAIN
            .lines()
            .find_map(|line| Some(line.trim().strip_prefix("channel = ")?.trim_matches('"')))
            .unwrap_or_else(|| panic!("rust-toolchain.toml has no channel"));
        // The key hashes `rustc -vV` of every installed toolchain, and with the environment hash
        // turned off it also drops the lockfile hash, so the cache would never be saved again.
        assert!(!CI.contains("add-rust-environment-hash-key"), "the key keeps its hashes");
        let reference = reference_toolchain_step();
        // The pinned version is read from rust-toolchain.toml, and that one is kept.
        assert!(
            reference.contains("pinned=\"$(sed -n ")
                && reference.contains(" rust-toolchain.toml)\""),
            "the pinned channel comes from rust-toolchain.toml: {reference}"
        );
        assert!(reference.contains("rustup default \"$pinned\"\n"), "{reference}");
        assert!(reference.contains("\"$pinned\"-*) ;;\n"), "the pinned one is kept: {reference}");
        assert!(
            reference.contains("*) rustup toolchain uninstall \"$toolchain\" ;;\n"),
            "every other one goes: {reference}"
        );
        assert!(
            !reference.contains(channel) && !has_version_number(&reference),
            "no version is written into the workflow: {reference}"
        );
        for (name, shared) in SAVERS {
            let job = job(CI, name);
            let steps = steps(&job);
            let install = position(&steps, &["run: rustup toolchain install --no-self-update"]);
            let cache = position(&steps, &["uses: Swatinem/rust-cache@"]);
            let toolchain = position(&steps, &TOOLCHAIN_STEP_NEEDLES);
            assert!(
                install < toolchain && toolchain < cache,
                "{name}: install, keep only the pinned toolchain, then rust-cache"
            );
            assert_eq!(steps[toolchain], reference, "{name}: the same step as {}", SAVERS[0].0);
            // Every run drops the other toolchains, pull requests too, so they compute the key
            // `main` saved.
            assert!(!has_key(&steps[toolchain], "if:"), "{name}: the step runs on every event");
            assert!(steps[cache].contains(&format!("shared-key: {shared}\n")), "{name}: its key");
            assert!(
                steps[cache].contains(&format!("save-if: ${{{{ {ON_MAIN} }}}}\n")),
                "{name}: saves only on main"
            );
            if on_windows(&job) {
                assert!(
                    steps[toolchain].contains("shell: bash\n"),
                    "{name}: bash, said explicitly"
                );
            }
        }
    }

    /// Every job in every workflow that restores a key the savers write computes the key the
    /// same way, or it never finds `main`'s entry once `prune-caches` removes the older ones.
    #[test]
    fn every_restore_of_a_saved_key_keeps_only_the_pinned_toolchain() {
        let reference = reference_toolchain_step();
        let folder = crate::repo_root().join(".github/workflows");
        let mut files: Vec<_> = std::fs::read_dir(&folder)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "yml" || ext == "yaml"))
            .collect();
        files.sort();
        let mut restorers = Vec::new();
        for path in files {
            let file = path.file_name().unwrap().to_string_lossy().into_owned();
            let workflow = std::fs::read_to_string(&path).unwrap();
            for name in jobs(&workflow) {
                let job = job(&workflow, name);
                let steps = steps(&job);
                let Some(cache) = try_position(&steps, &["uses: Swatinem/rust-cache@"]) else {
                    continue;
                };
                let saved = shared_key(&steps[cache])
                    .is_some_and(|key| SAVERS.iter().any(|(_, shared)| *shared == key));
                if !saved {
                    continue;
                }
                let pending =
                    WITHOUT_THE_TOOLCHAIN_STEP.iter().any(|(f, j)| *f == file && *j == name);
                let toolchain = steps.iter().position(|step| *step == reference);
                if pending {
                    assert!(toolchain.is_none(), "{file} {name} has the step now: empty the list");
                } else {
                    let toolchain = toolchain.unwrap_or_else(|| {
                        panic!("{file} {name}: no step identical to {}'s: {reference}", SAVERS[0].0)
                    });
                    assert!(toolchain < cache, "{file} {name}: the toolchain step comes first");
                }
                restorers.push((file.clone(), name.to_owned()));
            }
        }
        for (file, name) in WITHOUT_THE_TOOLCHAIN_STEP {
            assert!(
                restorers.iter().any(|(f, n)| f == file && n == name),
                "{file} {name} no longer restores a saved key: drop it from the list"
            );
        }
        for (name, _) in SAVERS {
            assert!(restorers.iter().any(|(f, n)| f == "ci.yml" && n == name), "{name} was seen");
        }
    }

    #[test]
    fn cache_savers_date_their_artifacts_last_on_main() {
        let reference = {
            let steps = steps(&job(CI, SAVERS[0].0));
            steps.last().cloned().unwrap_or_default()
        };
        for (name, _) in SAVERS {
            let job = job(CI, name);
            let steps = steps(&job);
            let last = steps.last().unwrap_or_else(|| panic!("{name} has no steps"));
            assert_eq!(*last, reference, "{name}: the same step as {}", SAVERS[0].0);
            // rust-cache's restore purges entries of these folders older than a week when the
            // key does not fully match; its save runs after the job's last step.
            for folder in ["target/debug/build", "target/debug/.fingerprint", "target/debug/deps"] {
                assert!(last.contains(folder), "{name}: the last step dates {folder}: {last}");
            }
            // Cargo rebuilds a unit whose dependency's output is newer than its own, so every
            // file and folder below them, at any depth, gets one and the same date.
            assert!(last.contains("stamp=\"$(date +%s)\"\n"), "{name}: one date: {last}");
            assert!(
                last.contains("find \"$dir\" -exec touch -c -d \"@$stamp\" -- {} +\n"),
                "{name}: everything, to that date, never creating a file: {last}"
            );
            assert!(!last.contains("-maxdepth") && !last.contains("-mindepth"), "{name}: {last}");
            assert!(
                last.contains(&format!("if: success() && {ON_MAIN}\n")),
                "{name}: only on the runs that save: {last}"
            );
            if on_windows(&job) {
                assert!(last.contains("shell: bash\n"), "{name}: bash, said explicitly");
            }
        }
    }

    #[test]
    fn superseded_caches_are_pruned_only_after_mains_saves() {
        let job = job(CI, "prune-caches");
        let text = job.join("\n");
        assert!(text.contains("needs: [logic-linux, windows-clippy, windows-test]\n"), "{text}");
        assert!(
            text.contains(&format!("if: github.event_name == 'push' && {ON_MAIN}\n")),
            "only pushes to main save, so only they prune: {text}"
        );
        // Job permissions replace the workflow's, so this is all the token may do.
        let permissions = job
            .iter()
            .position(|line| line.trim() == "permissions:")
            .unwrap_or_else(|| panic!("prune-caches names its permissions: {text}"));
        let granted: Vec<&str> = job[permissions + 1..]
            .iter()
            .take_while(|line| indent_of(line) > indent_of(job[permissions]))
            .map(|line| line.split('#').next().unwrap_or_default().trim())
            .collect();
        assert_eq!(granted, ["actions: write"], "{text}");
        assert!(!text.contains("actions/checkout@"), "nothing of the repository is needed");
        // Only rust-cache's own entries on main, one shared key at a time, never the others
        // (the Linux desktop lane's apt cache, for one).
        assert!(text.contains("--ref refs/heads/main"), "{text}");
        assert!(text.contains("for shared in linux win-clippy win-test; do"), "{text}");
        assert!(text.contains("--key \"v0-rust-$shared-\""), "{text}");
        assert!(text.contains("--sort created_at --order desc"), "newest first: {text}");
        // A later key that starts the same way, such as `linux-fuzz`, is left alone: only the
        // entries whose next part is rust-cache's OS name count, and the newest is kept.
        assert!(text.contains("grep -E \"^[0-9]+ v0-rust-$shared-(Linux|Windows_NT)-\""), "{text}");
        assert!(text.contains("tail -n +2)"), "the newest of each key is kept: {text}");
        assert!(!text.contains("[1:]"), "kept after the filter, not before: {text}");
        assert!(text.contains("gh cache delete"), "{text}");
        // The aggregate does not wait for maintenance.
        let all_green = job_text("all-green");
        assert!(!all_green.contains("prune-caches"), "{all_green}");
    }

    fn job_text(name: &str) -> String {
        job(CI, name).join("\n") + "\n"
    }

    /// Whether a step, as `steps` gives it, runs tests: `cargo nextest run` or `cargo test` on
    /// any of its lines but its name, so a command inside a `run: |` or `run: >-` block counts
    /// too, while a build-only `--no-run` does not.
    fn runs_tests(step: &str) -> bool {
        step.lines().filter(|line| !line.trim_start_matches("- ").starts_with("name: ")).any(
            |line| {
                (line.contains("cargo nextest run") || line.contains("cargo test"))
                    && !line.contains("--no-run")
            },
        )
    }

    #[test]
    fn a_test_step_is_found_in_every_run_form() {
        let workflow = "name: example
jobs:
  inline:
    steps:
      - name: Tests
        run: cargo nextest run --workspace
  folded:
    steps:
      - name: Build the tests
        run: cargo nextest run --workspace --no-run
      - name: The tests, on two lines
        run: >-
          cargo nextest run --workspace
          --locked --profile ci
  literal:
    steps:
      - name: Doc tests
        run: |
          echo first
          cargo test --workspace --doc
  elsewhere:
    steps:
      - name: Lint, as cargo test would build it
        # cargo test is only named in this comment
        run: cargo clippy --workspace --all-targets
";
        let found: Vec<(&str, Vec<bool>)> = jobs(workflow)
            .into_iter()
            .map(|name| {
                let steps = steps(&job(workflow, name));
                (name, steps.iter().map(|step| runs_tests(step)).collect())
            })
            .collect();
        assert_eq!(
            found,
            [
                ("inline", vec![true]),
                ("folded", vec![false, true]),
                ("literal", vec![true]),
                ("elsewhere", vec![false]),
            ]
        );
    }

    #[test]
    fn ci_keeps_pending_snapshots() {
        const PENDING_DIR: &str = "INSTA_PENDING_DIR: ${{ github.workspace }}/target/insta-pending";
        let mut testing = Vec::new();
        for name in jobs(CI) {
            let steps = steps(&job(CI, name));
            let tests: Vec<&String> = steps.iter().filter(|step| runs_tests(step)).collect();
            if tests.is_empty() {
                continue;
            }
            testing.push(name);
            // With `INSTA_UPDATE=new`, insta writes a failing or new snapshot as `.snap.new`, and
            // an inline one as `.<file>.pending-snap`, a hidden file, even when it detects CI.
            // `INSTA_PENDING_DIR` puts both under `target/`, at their repository paths, so none
            // lands in the tree, and the job's upload on failure keeps the folder.
            for step in tests {
                for line in ["INSTA_UPDATE: new", PENDING_DIR] {
                    assert!(step.lines().any(|l| l == line), "{name} sets `{line}`: {step}");
                }
            }
            let upload = steps
                .iter()
                .find(|step| has_key(step, "uses: actions/upload-artifact@"))
                .unwrap_or_else(|| panic!("{name} keeps what a failed run leaves: {steps:#?}"));
            for line in ["if: failure()", "target/insta-pending/", "include-hidden-files: true"] {
                assert!(upload.lines().any(|l| l == line), "{name} keeps `{line}`: {upload}");
            }
        }
        for name in ["logic-linux", "windows-test"] {
            assert!(testing.contains(&name), "{name} runs tests: {testing:?}");
        }
        // A pending file in the tree came from a hand copy, and no test notices it, so the
        // `hygiene` job, which runs on every pull request, rejects it.
        let hygiene = steps(&job(CI, "hygiene"));
        assert!(
            hygiene.iter().any(|step| {
                step.contains("git ls-files -- '*.snap.new' '*.pending-snap' | grep .")
            }),
            "hygiene rejects a committed pending snapshot: {hygiene:#?}"
        );
    }
}
