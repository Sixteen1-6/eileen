//! Whether a process is still running.

/// True while the process `pid` runs. Off Windows, from `/proc` (a zombie counts as gone).
#[must_use]
pub fn process_alive(pid: u32) -> bool {
    #[cfg(windows)]
    {
        crate::ffi::process_alive(pid)
    }
    #[cfg(not(windows))]
    {
        std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| {
            // The state follows the parenthesised command name, which may itself hold spaces.
            stat.rsplit_once(')')
                .and_then(|(_, rest)| rest.split_whitespace().next())
                .is_some_and(|s| s != "Z" && s != "X")
        })
    }
}

#[cfg(test)]
mod tests {
    use super::process_alive;

    #[test]
    fn this_process_is_alive_and_a_made_up_one_is_not() {
        assert!(process_alive(std::process::id()), "this process");
        assert!(!process_alive(u32::MAX - 1), "no such process");
    }
}
