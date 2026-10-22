//! The probe on the machine the tests run on: whatever the PC, its facts hold together and
//! carry nothing that names the user or the machine.

use std::path::Path;

use eileen_contracts::connectors::Platform;
use eileen_shell_linux::LinuxMachineProbe;

#[test]
fn this_machine_reads_facts_that_hold_together() {
    // A folder that does not exist yet: the volume is measured at its nearest existing parent.
    let data_dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("live").join("not-created");
    let probe = LinuxMachineProbe::system(&data_dir);
    let facts = probe.facts().unwrap();
    let (busy_pct, _) = probe.load().unwrap();
    assert!(busy_pct <= 100, "a share: {busy_pct}");
    assert_eq!(facts.os, Platform::Linux, "Linux");
    assert!(facts.os_build.contains(", kernel "), "the build names the kernel: {}", facts.os_build);
    assert!(facts.memory_total_mb > 0, "memory");

    let cpu = &facts.cpu;
    assert!(cpu.logical >= 1 && cpu.physical >= 1 && cpu.physical <= cpu.logical, "{cpu:?}");
    assert_eq!(
        cpu.classes.iter().map(|class| class.logical).sum::<u16>(),
        cpu.logical,
        "each CPU in one class"
    );
    assert_eq!(
        cpu.classes.iter().map(|class| class.physical).sum::<u16>(),
        cpu.physical,
        "each core in one class"
    );
    assert!(
        cpu.classes.windows(2).all(|pair| pair[0].efficiency_class > pair[1].efficiency_class),
        "the highest class first: {:?}",
        cpu.classes
    );

    assert!(facts.disk.data_total_mb > 0, "the volume has a size");
    assert!(facts.disk.data_free_mb <= facts.disk.data_total_mb, "free fits in the size");

    let text = format!("{facts:?}");
    let host = std::fs::read_to_string("/proc/sys/kernel/hostname").unwrap_or_default();
    let user = std::env::var("USER").unwrap_or_default();
    for private in [host.trim(), user.as_str()] {
        if private.len() >= 4 && !facts.os_build.contains(private) {
            assert!(!text.contains(private), "the facts name the host or the user");
        }
    }
    if let Ok(home) = std::env::var("HOME")
        && home.len() > 1
    {
        assert!(!text.contains(&home), "the facts hold the home folder's path");
    }
}
