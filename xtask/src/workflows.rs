//! Config assertions over `.github/workflows/ci.yml` that no command checks: how the jobs that
//! save rust-cache keep their cache usable. CI runs these tests like any other, so a later edit
//! that drops one of these steps fails the pull request that makes it.

#[cfg(test)]
mod tests {
    const CI: &str = include_str!("../../.github/workflows/ci.yml");
    const TOOLCHAIN: &str = include_str!("../../rust-toolchain.toml");

    /// The jobs that restore and, on `main`, save a rust-cache entry, with their shared keys.
    const SAVERS: [(&str, &str); 3] =
        [("logic-linux", "linux"), ("windows-clippy", "win-clippy"), ("windows-test", "win-test")];
    /// The condition rust-cache's `save-if` uses in every saving job.
    const ON_MAIN: &str = "github.ref == 'refs/heads/main'";

    fn indent_of(line: &str) -> usize {
        line.len() - line.trim_start().len()
    }

    /// The lines of one job under `jobs:`, from its `name:` line to the next job.
    fn job<'a>(workflow: &'a str, name: &str) -> Vec<&'a str> {
        let lines: Vec<&str> = workflow.lines().collect();
        let header = format!("{name}:");
        let start = lines
            .iter()
            .position(|line| indent_of(line) == 2 && line.trim() == header)
            .unwrap_or_else(|| panic!("ci.yml has no `{name}` job"));
        lines[start + 1..]
            .iter()
            .take_while(|line| line.trim().is_empty() || indent_of(line) > 2)
            .copied()
            .collect()
    }

    /// A job's steps, each as the text of its lines, in order.
    fn steps(job: &[&str]) -> Vec<String> {
        let at = job
            .iter()
            .position(|line| line.trim() == "steps:")
            .unwrap_or_else(|| panic!("a job without steps: {job:#?}"));
        let indent = indent_of(job[at]);
        let mut steps: Vec<String> = Vec::new();
        for line in &job[at + 1..] {
            if line.trim().is_empty() {
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
        steps
            .iter()
            .position(|step| needles.iter().all(|needle| step.contains(needle)))
            .unwrap_or_else(|| panic!("no step holds {needles:?}: {steps:#?}"))
    }

    fn on_windows(job: &[&str]) -> bool {
        job.iter().any(|line| line.trim() == "runs-on: windows-latest")
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
        for (name, shared) in SAVERS {
            let job = job(CI, name);
            let steps = steps(&job);
            let install = position(&steps, &["run: rustup toolchain install --no-self-update"]);
            let cache = position(&steps, &["uses: Swatinem/rust-cache@"]);
            let prune = position(&steps, &["rust-toolchain.toml", "rustup toolchain uninstall"]);
            assert!(install < prune && prune < cache, "{name}: install, prune, then rust-cache");
            // Every run prunes, pull requests too, so they compute the key `main` saved.
            assert!(!steps[prune].contains("if:"), "{name}: the pruning runs on every event");
            assert!(
                !steps[prune].contains(channel),
                "{name}: the pinned version is read from rust-toolchain.toml, not written here"
            );
            assert!(steps[cache].contains(&format!("shared-key: {shared}\n")), "{name}: its key");
            assert!(
                steps[cache].contains(&format!("save-if: ${{{{ {ON_MAIN} }}}}\n")),
                "{name}: saves only on main"
            );
            if on_windows(&job) {
                assert!(steps[prune].contains("shell: bash\n"), "{name}: bash, said explicitly");
            }
        }
    }

    #[test]
    fn cache_savers_date_their_artifacts_last_on_main() {
        for (name, _) in SAVERS {
            let job = job(CI, name);
            let steps = steps(&job);
            let last = steps.last().unwrap_or_else(|| panic!("{name} has no steps"));
            // rust-cache's restore purges entries of these folders older than a week when the
            // key does not fully match; its save runs after the job's last step.
            for folder in ["target/debug/build", "target/debug/.fingerprint", "target/debug/deps"] {
                assert!(last.contains(folder), "{name}: the last step dates {folder}: {last}");
            }
            assert!(last.contains("-mindepth 1 -maxdepth 1"), "{name}: top-level entries only");
            assert!(last.contains("touch -c"), "{name}: touches, never creates: {last}");
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
        assert!(text.contains("'.[1:][]"), "the newest of each key is kept: {text}");
        assert!(text.contains("gh cache delete"), "{text}");
        // The aggregate does not wait for maintenance, and its list is unchanged.
        let all_green = job_text("all-green");
        assert!(
            all_green
                .contains("needs: [plan, hygiene, logic-linux, windows-clippy, windows-test]\n"),
            "{all_green}"
        );
    }

    fn job_text(name: &str) -> String {
        job(CI, name).join("\n") + "\n"
    }
}
