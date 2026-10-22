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
