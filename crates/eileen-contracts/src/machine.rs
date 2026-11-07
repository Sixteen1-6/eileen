//! The machine profile (BUILD.md sections 2.16 and 3.3.17): what Eileen read about the PC, its
//! usual memory, the model catalogue the plan reads, the short benchmarks, and the plan, with a
//! reason for every choice.
//!
//! The facts go to `machine\facts.toml` as one [`MachineFacts`] in TOML, the plan to
//! `machine\plan.toml` as one [`MachinePlan`], and the benchmarks to `machine\bench.jsonl`, one
//! [`MachineBench`] per line. The core hears the facts, the usual figure, the load and each
//! benchmark as `Input::Machine`, asks for benchmarks and writes the plan through
//! `Effect::Machine`, and publishes a changed plan as `BusEvent::MachinePlanChanged`.
//!
//! Never read: serial numbers, hardware ids, the machine's name, user names or accounts.
//! Processor and adapter names are [`Untrusted`], since the OS passes on whatever the firmware
//! or the driver says, and audio endpoint names are [`Personal`], since a Bluetooth headset can
//! name a person. The plain strings reachable from `Input` (the build, a driver version,
//! provider, engine and model names, Eileen's version, the names of degraded facts, a failed
//! benchmark's reason) are written by Eileen's own code or the OS, and `text-allowlist.toml`
//! lists each with its reason.

use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::connectors::Platform;
use crate::ids::{Personal, Untrusted, WallTime};
use crate::schema::Registry;

/// The version [`MachinePlan::v`] carries.
pub const PLAN_VERSION: u16 = 1;

/// What Eileen read about this PC (section 2.16), stored in `machine\facts.toml`. Never a
/// serial number, a hardware id, the machine's name, a user name or an account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MachineFacts {
    /// When the facts were read.
    pub read_at: WallTime,
    /// How long reading them took, ms.
    pub probe_ms: u32,
    /// The operating system.
    pub os: Platform,
    /// The build and its update revision, for example `26200.6899`.
    pub os_build: String,
    /// Total physical memory, MB.
    pub memory_total_mb: u32,
    /// The commit limit, MB.
    pub commit_limit_mb: u32,
    /// The processor.
    pub cpu: CpuFacts,
    /// The GPUs and NPUs, from `DXCore`.
    pub adapters: Vec<AdapterFacts>,
    /// The ONNX Runtime execution providers the shipped library can create here, as the speech
    /// process reported them; empty until it has.
    pub execution_providers: Vec<String>,
    /// The volume that holds the data folder.
    pub disk: DiskFacts,
    /// The power source and battery saver.
    pub power: PowerState,
    /// The audio endpoints, as the audio component reported them.
    pub audio: AudioFacts,
    /// How many monitors there are.
    pub monitors: u8,
    /// The facts that could not be read: unknown, never reported as zero.
    pub degraded: Vec<String>,
}

/// The processor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CpuFacts {
    /// The processor name string, as the OS reports it.
    pub name: Untrusted<String>,
    /// Who made it.
    pub vendor: CpuVendor,
    /// Logical processors.
    pub logical: u16,
    /// Physical cores.
    pub physical: u16,
    /// Cores per efficiency class, highest (fastest) class first.
    pub classes: Vec<CoreClass>,
    /// The instruction-set features int8 inference uses.
    pub features: BTreeSet<CpuFeature>,
}

impl CpuFacts {
    /// The performance cores: the physical cores of the highest efficiency class, which on a
    /// CPU with one class are all of them. With no class read, the physical count.
    #[must_use]
    pub fn performance_cores(&self) -> u16 {
        self.classes
            .iter()
            .max_by_key(|class| class.efficiency_class)
            .map_or(self.physical, |class| class.physical)
    }
}

/// One efficiency class of cores (`GetLogicalProcessorInformationEx`; a higher class is faster,
/// and a CPU with one core type has one class).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub struct CoreClass {
    /// The class, as the OS numbers it.
    pub efficiency_class: u8,
    /// Physical cores of this class.
    pub physical: u16,
    /// Logical processors of this class.
    pub logical: u16,
}

/// Who made the processor.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum CpuVendor {
    /// Intel.
    Intel,
    /// AMD.
    Amd,
    /// An Arm design.
    Arm,
    /// Anyone else.
    Other,
}

/// An instruction-set feature int8 inference uses.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum CpuFeature {
    /// AVX2.
    Avx2,
    /// AVX-512.
    Avx512,
    /// AVX-VNNI.
    AvxVnni,
    /// Arm NEON.
    Neon,
    /// Arm's dot-product extension.
    DotProd,
}

/// A GPU or an NPU.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AdapterFacts {
    /// Its name, as the driver reports it.
    pub name: Untrusted<String>,
    /// What it is.
    pub kind: AdapterKind,
    /// Dedicated memory, MB.
    pub dedicated_mb: u32,
    /// Shared memory, MB.
    pub shared_mb: u32,
    /// The driver version.
    pub driver: String,
}

/// What an adapter is.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum AdapterKind {
    /// A GPU that shares the system's memory.
    IntegratedGpu,
    /// A GPU with memory of its own.
    DiscreteGpu,
    /// An adapter with the generic machine-learning attribute and no graphics attribute.
    Npu,
}

/// The volume that holds the data folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct DiskFacts {
    /// Free space, MB.
    pub data_free_mb: u64,
    /// Total space, MB.
    pub data_total_mb: u64,
    /// What the volume is (the seek-penalty query).
    pub media: DiskMedia,
}

/// What a volume is.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum DiskMedia {
    /// A solid-state disk.
    Ssd,
    /// A spinning disk.
    Hdd,
    /// The query gave no answer.
    Unknown,
}

/// The power source and battery saver.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct PowerState {
    /// The PC has a battery.
    pub has_battery: bool,
    /// It runs on the battery now.
    pub on_battery: bool,
    /// Battery saver is on.
    pub battery_saver: bool,
}

/// The audio endpoints (section 2.16), as the audio component reported them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AudioFacts {
    /// The capture endpoints.
    pub capture: Vec<EndpointFacts>,
    /// The render endpoints.
    pub render: Vec<EndpointFacts>,
    /// The default capture endpoint, an index into `capture`.
    pub default_capture: Option<u8>,
    /// The default render endpoint, an index into `render`.
    pub default_render: Option<u8>,
    /// The default render endpoint's loopback opened (the echo reference of section 2.2).
    pub loopback: bool,
}

/// One audio endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct EndpointFacts {
    /// Its friendly name, which can name a person.
    pub name: Personal<String>,
    /// Its form factor.
    pub form: EndpointForm,
    /// How it is connected.
    pub bus: EndpointBus,
    /// Its mix format's sample rate, Hz.
    pub mix_hz: u32,
    /// Its mix format's channel count.
    pub channels: u8,
}

/// An endpoint's form factor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EndpointForm {
    /// A microphone.
    Microphone,
    /// A headset, microphone and earphones together.
    Headset,
    /// Headphones.
    Headphones,
    /// Speakers.
    Speakers,
    /// A line input.
    LineIn,
    /// Anything else.
    Other,
}

/// How an endpoint is connected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EndpointBus {
    /// Built into the PC.
    Builtin,
    /// USB.
    Usb,
    /// Bluetooth.
    Bluetooth,
    /// Anything else.
    Other,
}

/// The PC's usual memory: `samples::usual` over 7 days of hourly summaries (section 2.16). What
/// the PC would usually have free without Eileen's own extras is `available_mb` plus
/// `plan_items_mb`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct UsualMemory {
    /// The qualifying hours behind it; 0 means the first minute's reading after start.
    pub hours: u16,
    /// The 25th percentile of the hourly medians of available memory, MB.
    pub available_mb: u32,
    /// The median of what the plan's own items held at those samples, MB.
    pub plan_items_mb: u32,
    /// The usual 10th percentile of commit headroom, MB.
    pub headroom_p10_mb: u32,
}

/// One entry of the model catalogue the plan reads, from `models.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ModelFit {
    /// The model's name in `models.toml`.
    pub name: String,
    /// What it does.
    pub role: ModelRole,
    /// Its resident size, MB.
    pub resident_mb: u32,
    /// It is the low-memory choice for its role.
    pub low_memory: bool,
    /// WP4 adopted it.
    pub adopted: bool,
    /// Its accuracy on WP4's command corpus, per mille (row 1 of the plan).
    pub command_accuracy_per_mille: Option<u16>,
}

/// What a model does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ModelRole {
    /// Voice activity detection.
    Vad,
    /// Recognition.
    Asr,
    /// End of turn.
    Turn,
    /// Synthesis.
    Tts,
    /// Voice cloning.
    Clone,
    /// The L2 embedding model.
    Embed,
    /// The speaker check.
    Speaker,
}

/// One short benchmark, with the conditions it ran under.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MachineBench {
    /// When it ran.
    pub at: WallTime,
    /// What it ran.
    pub kind: BenchKind,
    /// How long it took, ms.
    pub took_ms: u32,
    /// What the PC was doing meanwhile.
    pub conditions: BenchConditions,
    /// What it measured.
    pub result: BenchResult,
}

/// What a benchmark runs (section 2.16's table).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BenchKind {
    /// 10 s of a bundled clip through the voice activity detector.
    Vad,
    /// A bundled 8 s command clip through a recognizer.
    Asr {
        /// The recognizer, by its name in `models.toml`.
        engine: String,
        /// The threads it ran on.
        threads: u8,
    },
    /// One clause of three to six words through a voice.
    Tts {
        /// The voice engine, by its name in `models.toml`.
        engine: String,
        /// The precision it ran at.
        precision: Precision,
        /// The threads it ran on.
        threads: u8,
    },
    /// The clone timing on the bundled synthetic reference.
    CloneTtfa {
        /// The cloning engine, by its name in `models.toml`.
        engine: String,
    },
    /// One 12-word utterance through an embedding model, 20 times.
    Embed {
        /// The model, by its name in `models.toml`.
        model: String,
    },
    /// A sequential read of 64 MB of an installed model file.
    DiskRead,
}

/// A model's numeric precision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Precision {
    /// 32-bit floating point.
    Fp32,
    /// 8-bit integers.
    Int8,
    /// 8-bit quantised.
    Q8,
    /// 4-bit quantised.
    Q4,
}

/// What the PC was doing while a benchmark ran; a number taken while it pages is pessimistic.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct BenchConditions {
    /// Available memory, MB.
    pub available_mb: u32,
    /// CPU busy, percent.
    pub cpu_busy_pct: u8,
    /// The PC ran on its battery.
    pub on_battery: bool,
    /// Eileen's version.
    pub eileen_version: String,
}

/// What a benchmark measured.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BenchResult {
    /// CPU time.
    Cpu {
        /// Per mille of one core.
        per_mille_of_core: u16,
    },
    /// A speech stage.
    Speech {
        /// The real-time factor, per mille.
        rtf_per_mille: u16,
        /// The first clause or the first partial, ms.
        first_ms: u32,
        /// The recognizer's tail, ms.
        tail_ms: u32,
    },
    /// A round trip.
    Latency {
        /// The median, ms.
        p50_ms: u32,
        /// The 95th percentile, ms.
        p95_ms: u32,
    },
    /// A read.
    Read {
        /// MB per second.
        mb_s: u32,
    },
    /// The benchmark failed.
    Failed {
        /// Why, in Eileen's own words.
        reason: String,
    },
}

/// What the plan chose (section 2.16); in `machine\plan.toml`, on the bus and on the HUD.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MachinePlan {
    /// The plan's format version, [`PLAN_VERSION`].
    pub v: u16,
    /// When it was computed.
    pub at: WallTime,
    /// How much room the PC has.
    pub class: MachineClass,
    /// The usual memory it was computed from.
    pub usual: UsualMemory,
    /// The admission reserve, `memory.reserve_mb`, MB.
    pub reserve_mb: u32,
    /// `usual.available_mb + usual.plan_items_mb - reserve_mb`, MB; may be negative.
    pub budget_mb: i32,
    /// Rows 1 to 12 of section 2.16's table, in that order.
    pub choices: Vec<FitChoice>,
    /// Row 13: the staged features.
    pub staged: Vec<StagedFeature>,
    /// Items the live re-check let go, until they come back.
    pub paused: Vec<FitPause>,
    /// The summary sentence, from Eileen's own template.
    pub summary: String,
}

/// How much room a PC has (section 2.16).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum MachineClass {
    /// The budget covers neither the deep pool session nor the spare.
    Tight,
    /// Between the two.
    Standard,
    /// The budget covers every memory row with at least 2 GB to spare.
    Roomy,
}

/// One fitted setting's effective value and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FitChoice {
    /// The setting key, such as `routing.hot_spare`; `idle_work` for row 12, which no setting
    /// overrides.
    pub key: String,
    /// The value.
    pub value: FitValue,
    /// Who chose it.
    pub source: FitSource,
    /// False only for an override the budget does not cover.
    pub fits: bool,
    /// Why.
    pub reason: FitReason,
    /// The reason in words, from Eileen's own template.
    pub sentence: String,
}

/// A fitted setting's value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FitValue {
    /// On or off.
    Bool(bool),
    /// A count.
    Count(u8),
    /// An engine or model, by its name in `models.toml`.
    Engine(String),
    /// A precision.
    Precision(Precision),
    /// A size, MB.
    Mb(u32),
}

/// Who chose a fitted setting's value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FitSource {
    /// The machine profile: the key is `auto`.
    Auto,
    /// The user's override, which the plan never changes.
    Override,
}

/// Why a fitted setting has its value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FitReason {
    /// Kept or left out by the memory budget.
    Budget {
        /// The item's measured cost, MB.
        cost_mb: u32,
        /// The budget left after it, MB; may be negative.
        left_mb: i32,
    },
    /// A measurement on this PC.
    Measured {
        /// What ran.
        kind: BenchKind,
        /// What it measured.
        result: BenchResult,
    },
    /// The performance cores.
    Cores {
        /// How many there are.
        performance: u16,
    },
    /// The data volume's free space.
    Disk {
        /// Free space, MB.
        free_mb: u64,
    },
    /// Battery saver is on.
    BatterySaver,
    /// A measurement did not adopt it.
    NotAdopted {
        /// The measurement, such as "WP4 acceptance 14".
        by: String,
    },
    /// The user set it.
    UserChose,
    /// An override that admission's rule 1 keeps from starting.
    HeldBack,
    /// Nothing on this PC changes the starting value.
    Default,
}

/// A feature shown with its state and reason, not chosen by the plan (row 13).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct StagedFeature {
    /// The feature, such as `speaker_check`.
    pub feature: String,
    /// Its state.
    pub state: StagedState,
    /// Its state and reason in words, with any measured numbers, from Eileen's own template.
    pub sentence: String,
}

/// A staged feature's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StagedState {
    /// Not used.
    Off,
    /// Used when the user asks for it.
    OnDemand,
    /// Measured but not acted on.
    Shadow,
    /// In use.
    On,
}

/// An item the live re-check let go (section 2.16, "The live re-check").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FitPause {
    /// The setting key of the item, such as `routing.hot_spare`.
    pub key: String,
    /// Why it was let go.
    pub cause: PauseCause,
    /// Since when.
    pub since: WallTime,
}

/// Why the live re-check let an item go.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PauseCause {
    /// Available memory stayed under the reserve for 30 s.
    MemoryLow {
        /// The biggest process that is not Eileen's, and its working set in MB.
        largest: Option<(Personal<String>, u32)>,
    },
    /// Paging stayed above `memory.max_paging_kb_s` for 30 s.
    Paging {
        /// The 30 s average, KB/s.
        kb_s: u32,
    },
    /// Battery saver is on.
    BatterySaver,
}

pub(crate) fn register(r: &mut Registry) {
    r.add::<MachineFacts>();
    r.add::<CpuFacts>();
    r.add::<CoreClass>();
    r.add::<CpuVendor>();
    r.add::<CpuFeature>();
    r.add::<AdapterFacts>();
    r.add::<AdapterKind>();
    r.add::<DiskFacts>();
    r.add::<DiskMedia>();
    r.add::<PowerState>();
    r.add::<AudioFacts>();
    r.add::<EndpointFacts>();
    r.add::<EndpointForm>();
    r.add::<EndpointBus>();
    r.add::<UsualMemory>();
    r.add::<ModelFit>();
    r.add::<ModelRole>();
    r.add::<MachineBench>();
    r.add::<BenchKind>();
    r.add::<Precision>();
    r.add::<BenchConditions>();
    r.add::<BenchResult>();
    r.add::<MachinePlan>();
    r.add::<MachineClass>();
    r.add::<FitChoice>();
    r.add::<FitValue>();
    r.add::<FitSource>();
    r.add::<FitReason>();
    r.add::<StagedFeature>();
    r.add::<StagedState>();
    r.add::<FitPause>();
    r.add::<PauseCause>();
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{CoreClass, CpuFacts, CpuFeature, CpuVendor};
    use crate::ids::Untrusted;

    fn cpu(physical: u16, logical: u16, classes: &[(u8, u16, u16)]) -> CpuFacts {
        CpuFacts {
            name: Untrusted::new("fixture processor".to_owned()),
            vendor: CpuVendor::Intel,
            logical,
            physical,
            classes: classes
                .iter()
                .map(|&(efficiency_class, physical, logical)| CoreClass {
                    efficiency_class,
                    physical,
                    logical,
                })
                .collect(),
            features: BTreeSet::from([CpuFeature::Avx2, CpuFeature::AvxVnni]),
        }
    }

    #[test]
    fn performance_cores_is_six_for_the_owners_shape() {
        // 6 performance cores with two threads each, then 10 others: 16 cores, 22 logical.
        let owners = cpu(16, 22, &[(1, 6, 12), (0, 10, 10)]);
        assert_eq!(owners.performance_cores(), 6, "the highest class's physical cores");
        let listed_slowest_first = cpu(16, 22, &[(0, 10, 10), (1, 6, 12)]);
        assert_eq!(
            listed_slowest_first.performance_cores(),
            6,
            "the highest class wins whatever the order"
        );
    }

    #[test]
    fn performance_cores_is_four_for_the_one_class_laptop() {
        let laptop = cpu(4, 8, &[(0, 4, 8)]);
        assert_eq!(laptop.performance_cores(), 4, "one class: every core");
        let unread = cpu(4, 8, &[]);
        assert_eq!(unread.performance_cores(), 4, "no class read: the physical count");
    }
}
