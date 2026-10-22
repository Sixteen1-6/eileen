//! The core process's platform implementations on Linux: what `eileen-shell-win` does on
//! Windows (BUILD.md section 3.4; docs/LINUX.md section 4). Phase L0 starts with the machine
//! profile's facts (BUILD.md section 2.16), which [`LinuxMachineProbe`] reads from `/proc`,
//! `/sys` and `/etc`, and the memory sample of section 2.5, which [`LinuxSystemMemory`] takes.
//!
//! # Owns
//!
//! - Reading the machine's facts on Linux: the processor and its core classes, memory totals,
//!   GPUs and NPUs, the data folder's volume, the OS, power and the monitor count; and its load,
//!   the CPU's busy share with the power state.
//! - [`facts`]: their shapes, field for field as BUILD.md section 3.3.17 gives them, until
//!   `eileen-contracts::machine` exists.
//! - Sampling memory on Linux into `eileen_contracts::jobs::MemoryPressure`: available memory,
//!   commit headroom under Linux's overcommit policy, the major-fault paging rate and the
//!   largest process outside Eileen's tree.
//! - [`SysRoot`]: the file-system root the reads are under (`/` on a real machine, a fixture
//!   tree in tests).
//! - [`default_data_dir`]: where Eileen's data folder lives on Linux (docs/LINUX.md section 5).
//!
//! # Does not own
//!
//! - The `MachineProbe` and `SystemMemory` traits (`eileen-platform`, WP0) and the fact types'
//!   final home (`eileen-contracts::machine`, WP0 phase B's `wp0/machine-contracts`). When those
//!   land, [`LinuxMachineProbe`] and [`LinuxSystemMemory`] implement the traits and [`facts`]
//!   becomes re-exports.
//! - The plan that reads the facts (`eileen_core::fit`, WP1); the runtime's sampler that turns
//!   samples into `Input::Memory` every 5 s, fills the effective paging limit and keeps the 30 s
//!   windows (WP2); and the build guard's own memory readings (WP0).
//! - The audio endpoints and the execution providers: their own components report them.
//!
//! # Invariants
//!
//! - Linux only: the crate is `cfg(target_os = "linux")` and compiles to an empty library
//!   elsewhere, as the `-win` crates do off Windows.
//! - Read only: it never writes, never starts a process and never needs elevation.
//! - A fact that cannot be read is named in `degraded` and left at its unknown value; it is
//!   never reported as a real zero.
//! - It never reads a serial-number file, the machine's name, user names or accounts, and never
//!   keeps a unit's id: of NVIDIA's information file, which also holds the GPU's UUID, only the
//!   `Model` line is kept. PCI vendor and device ids name a model, not a unit; they are used to
//!   look names up and are not kept either.
//! - `degraded` entries never hold a path, so the user's home folder never reaches the facts.
//!
//! # Boundaries
//!
//! - May depend on `eileen-contracts` (the text wrappers, `Platform`), `rustix` (`statvfs`)
//!   and `thiserror`. Nothing else in the workspace may depend on it except the composition
//!   root on Linux.
//! - Wire types: none of its own; the facts become contract types.
//!
//! # Testing
//!
//! - Fast command: `cargo nextest run -p eileen-shell-linux` (`cargo xtask test -p
//!   eileen-shell-linux` once `wp0/xtask` merges).
//! - Layers: unit tests for each parser over the kernel's text formats; integration tests over
//!   six fixture machines in `tests/fixtures/` (one recorded under WSL2, five synthetic: the
//!   owner's laptop shape, AMD on btrfs, NVIDIA on LVM, an Arm board, a QEMU virtual machine),
//!   each a `.tree` file the tests build into a folder; one live test that reads the machine
//!   the tests run on.
//! - A bug here starts as a machine panel line that does not match the PC, or a plan class
//!   (`Tight`, `Standard`, `Roomy`) that the PC's memory does not explain.
//!
//! # Traps
//!
//! - sysfs entries are symbolic links on a real machine and plain folders in the fixtures:
//!   read through paths, never with `read_link`, and take a device's driver from its `uevent`.
//! - Virtual disks call themselves rotational, or solid-state for a network volume. A guest is
//!   known by WSL's kernel release, the CPU's `hypervisor` flag or Xen's `/sys/hypervisor`,
//!   and, where none says so (an Arm guest), by virtio and Xen disk names and models such as
//!   `QEMU HARDDISK` and `Virtual Disk`. Its disk media is `Unknown` and degraded, never `Hdd`.
//!   Under WSL the GPU is `/dev/dxg` with no DRM card, which is degraded too, never "no GPU".
//! - An AMD GPU's VRAM size alone does not tell an APU: a Strix Halo's carve-out can be 96 GB
//!   and a small discrete card has 2 GB. The processor decides first (module `adapters`).
//! - `simpledrm` registers as the platform driver `simple-framebuffer`: a firmware framebuffer,
//!   not a GPU.
//! - A btrfs subvolume's device number matches no block device and not even its own line in
//!   `mountinfo`: the data folder's disk is found through the mount that holds the folder.
//! - Arm's `core_id` restarts in each cluster: physical cores are counted from
//!   `core_cpus_list`, never from `core_id`.
//! - Equal `cpu_capacity` on every core means one core class, not a missing fact. Where the
//!   kernel publishes both Intel's hybrid PMU lists and capacities, the PMU lists win: they
//!   give Windows' two classes, the capacities three.
//!
//! # Logs and tracing
//!
//! - None yet: the reads are short synchronous file reads. Spans from `eileen_trace::names`
//!   join with the trait implementation.
//!
//! # Do not change without a decision record
//!
//! - Which source each fact comes from (docs/LINUX.md section 4) and how the core classes are
//!   numbered (higher means faster, as Windows' efficiency classes are).

#![cfg(target_os = "linux")]
#![forbid(unsafe_code)]

mod adapters;
mod cpu;
pub mod dbus;
mod disk;
mod error;
pub mod facts;
mod load;
mod memory;
mod monitors;
mod os;
mod pci;
mod power;
mod pressure;
mod probe;
mod root;

pub use disk::{Volume, read_volume};
pub use error::ReadError;
pub use pressure::{LinuxSystemMemory, MIN_RATE_INTERVAL_MS};
pub use probe::{LinuxMachineProbe, default_data_dir};
pub use root::SysRoot;
