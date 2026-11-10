# Machine fixtures

Each `.tree` file describes one machine's `/proc`, `/sys`, `/etc`, `/usr` and `/var/lib` as the
probe reads them. The integration tests build the tree under Cargo's temporary folder for
integration tests (`CARGO_TARGET_TMPDIR`) and point `SysRoot` at it. On a real machine sysfs
entries are symbolic links; here every entry is a plain folder or file, which is why the probe
reads drivers from `uevent` and never follows links by name.

| Fixture | What it is |
|---|---|
| `wsl-ryzen-5600x` | Recorded: WSL2 on a Ryzen 5 5600X, trimmed to the files the probe reads |
| `intel-hybrid-laptop` | Synthetic: the owner's shape, a Core Ultra 7 155H laptop on battery with the power saver on |
| `amd-desktop-btrfs` | Synthetic: Fedora on btrfs, with an APU's Radeon and an RX 7900 XTX |
| `nvidia-desktop-lvm` | Synthetic: Ubuntu 24.04, NVIDIA's own driver, ext4 on LVM on a hard disk |
| `arm-rk3588-board` | Synthetic: an Arm big.LITTLE board with SoC GPU and NPU platform devices |
| `ubuntu-vm-qemu` | Synthetic: Ubuntu 26.04 in a QEMU/KVM virtual machine |

Synthetic fixtures follow the kernel's formats; their versions and figures are illustrative. A
recorded fixture never holds a serial number, a hardware id such as a GPU UUID, a host name, a
user name or a path from the recording machine.

## Format

- A line starting with `#`, and a blank line, is ignored.
- `path = value` writes a one-line file: `value` and a newline. `path =` writes an empty file.
  In a value, `\0` is a NUL byte (the device tree's lists).
- A `{list}` in the path, in the kernel's CPU-list syntax (`0-11`, `0,8`, `12-21`), writes one
  file per number, and `$n` in the value becomes that number:
  `sys/block/loop{0-7}/dev = 7:$n`.
- `<<< path` starts a file of several lines, kept as written, up to a line holding only `>>>`.

# Time zone files

`zoneinfo/` holds TZif files for the `zone` reader's tests. Under each zone's name is the file
Ubuntu 24.04's tzdata 2026c installs in `/usr/share/zoneinfo`, which is fat: every transition to
2037 is written out. Under `slim/` is the same zone as `zic -b slim` writes it from that package's
`tzdata.zi`, with the footer's rule alone after its last irregular change. The tz database and
the files built from it are in the public domain. `src/zone/tests.rs` holds what glibc 2.39 reads
from them for chosen years.

| Zone | Why |
|---|---|
| `Etc/UTC` | No transitions, `UTC0` |
| `America/New_York` | Version 2; local mean time, -4:56:02, until 1883 |
| `Europe/London` | British Standard Time, +1 all year from 1968 to 1971 |
| `Australia/Lord_Howe` | A half-hour change, south of the equator |
| `Asia/Kolkata` | +5:30 with no changes since 1945, and Madras time, +5:21:10, before |
| `Pacific/Chatham` | +12:45, a 45-minute offset, south of the equator |
| `America/Santiago` | Version 3: changes at 24:00, south of the equator |
| `America/Nuuk` | Version 3: changes at -1:00 and 0:00 |
