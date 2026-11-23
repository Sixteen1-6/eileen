//! WP0's three fixture machines (BUILD.md section 6.2, phase B), which WP1's plan tests and the
//! fake probe share: each a fact set as `machine\facts.toml` holds it, and a week of hourly
//! memory summaries in `memory-hours.jsonl` form with the usual memory they give.
//!
//! Every name in them is made up for the fixture: no real processor, adapter or endpoint.

use std::collections::BTreeSet;

use eileen_contracts::connectors::Platform;
use eileen_contracts::ids::{Personal, Untrusted, WallTime};
use eileen_contracts::machine::{
    AdapterFacts, AdapterKind, AudioFacts, CoreClass, CpuFacts, CpuFeature, CpuVendor, DiskFacts,
    DiskMedia, EndpointBus, EndpointFacts, EndpointForm, MachineFacts, PowerState, UsualMemory,
};

/// The end of the fixture week, 2026-10-26T00:00:00Z, Unix ms.
const WEEK_END_MS: u64 = 1_792_972_800_000;
/// One hour, ms.
const HOUR_MS: u64 = 3_600_000;
/// Hours in the fixture week.
const WEEK_HOURS: u64 = 168;
/// Memory above the usual figure in each four-hour cycle, MB. The usual hour comes first in
/// every cycle, so the 25th percentile of the hourly medians is the usual figure itself.
const CYCLE_MB: [u32; 4] = [0, 250, 600, 1_100];
/// The samples of a full hour, one every 5 s.
const FULL_HOUR_SAMPLES: u32 = 720;
/// The samples of the hour after each night's start, under the 360 that `usual` counts.
const SHORT_HOUR_SAMPLES: u32 = 120;

/// A fixture machine: its facts, a week of its hourly memory summaries, and the usual memory
/// `eileen_buildguard::samples::usual` gives over that week.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixtureMachine {
    /// A short name for test output.
    pub name: &'static str,
    /// The facts, as `machine\facts.toml` holds them once every component has reported.
    pub facts: MachineFacts,
    /// The week's hourly summaries, one `memory-hours.jsonl` line each, oldest first, the last
    /// ending at 2026-10-26T00:00:00Z. The hour after each night's start holds 120 samples, so
    /// `usual` leaves those seven out and counts the other 161.
    pub memory_hours: Vec<String>,
    /// What `usual` gives over that week, as the core receives it. What the PC usually has free
    /// without Eileen's own extras is `available_mb` plus `plan_items_mb`.
    pub usual: UsualMemory,
}

/// The memory a fixture's week centres on.
#[derive(Debug, Clone, Copy)]
struct UsualWeek {
    available_mb: u32,
    plan_items_mb: u32,
    headroom_p10_mb: u32,
    paging_p75_kb_s: u32,
}

impl UsualWeek {
    /// The week's lines, oldest first.
    fn lines(self) -> Vec<String> {
        (0..WEEK_HOURS)
            .zip(CYCLE_MB.iter().cycle())
            .map(|(hour, &up)| self.line(hour, up))
            .collect()
    }

    /// One hour's line: the hour after each night's start is short and Eileen's items are not
    /// loaded yet; every other hour is full and sits `up` MB above the usual figure.
    fn line(self, hour: u64, up: u32) -> String {
        let at_ms = WEEK_END_MS - (WEEK_HOURS - 1 - hour) * HOUR_MS;
        let short = hour % 24 == 1;
        let (p50, plan_items, samples) = if short {
            (self.available_mb / 2, 0, SHORT_HOUR_SAMPLES)
        } else {
            (self.available_mb + up, self.plan_items_mb, FULL_HOUR_SAMPLES)
        };
        let slot_held_pct = if hour % 24 == 15 { 30 } else { 0 };
        format!(
            "{{\"at_ms\": {at_ms}, \"avail_p10_mb\": {}, \"avail_p50_mb\": {p50}, \
             \"avail_p90_mb\": {}, \"headroom_p10_mb\": {}, \"paging_p75_kb_s\": {}, \
             \"plan_items_mb\": {plan_items}, \"slot_held_pct\": {slot_held_pct}, \
             \"samples\": {samples}, \"src\": \"runtime\"}}",
            p50 / 4 * 3,
            p50 + p50 / 2 + 200,
            self.headroom_p10_mb + up,
            self.paging_p75_kb_s + up / 4,
        )
    }

    /// What `usual` gives over the week: 161 qualifying hours, the usual figure at their 25th
    /// percentile, the plan's items at their median.
    const fn usual(self) -> UsualMemory {
        UsualMemory {
            hours: 161,
            available_mb: self.available_mb,
            plan_items_mb: self.plan_items_mb,
            headroom_p10_mb: self.headroom_p10_mb,
        }
    }
}

/// A processor of `classes`, each `(efficiency class, physical, logical)`, highest first.
fn cpu(
    name: &str,
    vendor: CpuVendor,
    classes: &[(u8, u16, u16)],
    features: &[CpuFeature],
) -> CpuFacts {
    CpuFacts {
        name: Untrusted::new(name.to_owned()),
        vendor,
        logical: classes.iter().map(|&(_, _, logical)| logical).sum(),
        physical: classes.iter().map(|&(_, physical, _)| physical).sum(),
        classes: classes
            .iter()
            .map(|&(efficiency_class, physical, logical)| CoreClass {
                efficiency_class,
                physical,
                logical,
            })
            .collect(),
        features: features.iter().copied().collect::<BTreeSet<_>>(),
    }
}

/// An adapter.
fn adapter(
    name: &str,
    kind: AdapterKind,
    dedicated_mb: u32,
    shared_mb: u32,
    driver: &str,
) -> AdapterFacts {
    AdapterFacts {
        name: Untrusted::new(name.to_owned()),
        kind,
        dedicated_mb,
        shared_mb,
        driver: driver.to_owned(),
    }
}

/// An audio endpoint at the usual 48 kHz mix rate.
fn endpoint(name: &str, form: EndpointForm, bus: EndpointBus, channels: u8) -> EndpointFacts {
    EndpointFacts { name: Personal::new(name.to_owned()), form, bus, mix_hz: 48_000, channels }
}

/// One capture and one render endpoint, both the defaults, with the loopback open.
fn audio(capture: EndpointFacts, render: EndpointFacts) -> AudioFacts {
    AudioFacts {
        capture: vec![capture],
        render: vec![render],
        default_capture: Some(0),
        default_render: Some(0),
        loopback: true,
    }
}

impl FixtureMachine {
    /// The owner's shape: 16 GB, 6 performance cores among 16 and 22 logical processors, an
    /// integrated GPU and an NPU, a solid-state disk, on mains power, usually 1.8 GB free.
    #[must_use]
    pub fn owners_shape() -> Self {
        let week = UsualWeek {
            available_mb: 800,
            plan_items_mb: 1_000,
            headroom_p10_mb: 5_500,
            paging_p75_kb_s: 4_000,
        };
        let facts = MachineFacts {
            read_at: WallTime(1_792_972_800_000_000),
            probe_ms: 140,
            os: Platform::Windows,
            os_build: "26200.6899".to_owned(),
            memory_total_mb: 15_700,
            commit_limit_mb: 25_000,
            cpu: cpu(
                "Fixture mobile processor, 6 performance and 10 efficient cores",
                CpuVendor::Intel,
                &[(1, 6, 12), (0, 10, 10)],
                &[CpuFeature::Avx2, CpuFeature::AvxVnni],
            ),
            adapters: vec![
                adapter(
                    "Fixture integrated GPU",
                    AdapterKind::IntegratedGpu,
                    128,
                    7_850,
                    "32.0.101.6079",
                ),
                adapter("Fixture NPU", AdapterKind::Npu, 0, 7_850, "32.0.100.3104"),
            ],
            execution_providers: vec!["CPUExecutionProvider".to_owned()],
            disk: DiskFacts {
                data_free_mb: 342_000,
                data_total_mb: 951_000,
                media: DiskMedia::Ssd,
            },
            power: PowerState { has_battery: true, on_battery: false, battery_saver: false },
            audio: audio(
                endpoint(
                    "Fixture microphone array",
                    EndpointForm::Microphone,
                    EndpointBus::Builtin,
                    2,
                ),
                endpoint("Fixture speakers", EndpointForm::Speakers, EndpointBus::Builtin, 2),
            ),
            monitors: 1,
            degraded: Vec::new(),
        };
        Self { name: "owners-shape", facts, memory_hours: week.lines(), usual: week.usual() }
    }

    /// An 8 GB laptop with 4 cores of one class, integrated graphics and 30 GB of free disk,
    /// on its battery, usually 0.9 GB free.
    #[must_use]
    pub fn one_class_laptop() -> Self {
        let week = UsualWeek {
            available_mb: 500,
            plan_items_mb: 400,
            headroom_p10_mb: 2_500,
            paging_p75_kb_s: 6_000,
        };
        let facts = MachineFacts {
            read_at: WallTime(1_792_972_800_000_000),
            probe_ms: 190,
            os: Platform::Windows,
            os_build: "26100.4652".to_owned(),
            memory_total_mb: 7_900,
            commit_limit_mb: 12_000,
            cpu: cpu(
                "Fixture laptop processor, 4 cores of one class",
                CpuVendor::Amd,
                &[(0, 4, 8)],
                &[CpuFeature::Avx2],
            ),
            adapters: vec![adapter(
                "Fixture integrated graphics",
                AdapterKind::IntegratedGpu,
                512,
                3_900,
                "31.0.21921.1000",
            )],
            execution_providers: vec!["CPUExecutionProvider".to_owned()],
            disk: DiskFacts { data_free_mb: 30_720, data_total_mb: 243_000, media: DiskMedia::Ssd },
            power: PowerState { has_battery: true, on_battery: true, battery_saver: false },
            audio: audio(
                endpoint(
                    "Fixture laptop microphone",
                    EndpointForm::Microphone,
                    EndpointBus::Builtin,
                    1,
                ),
                endpoint(
                    "Fixture laptop speakers",
                    EndpointForm::Speakers,
                    EndpointBus::Builtin,
                    2,
                ),
            ),
            monitors: 1,
            degraded: Vec::new(),
        };
        Self { name: "one-class-laptop", facts, memory_hours: week.lines(), usual: week.usual() }
    }

    /// A 32 GB desktop with 8 performance cores and a discrete GPU, no battery, a USB headset,
    /// usually 14 GB free.
    #[must_use]
    pub fn desktop() -> Self {
        let week = UsualWeek {
            available_mb: 12_800,
            plan_items_mb: 1_200,
            headroom_p10_mb: 20_000,
            paging_p75_kb_s: 500,
        };
        let facts = MachineFacts {
            read_at: WallTime(1_792_972_800_000_000),
            probe_ms: 95,
            os: Platform::Windows,
            os_build: "26200.6899".to_owned(),
            memory_total_mb: 32_600,
            commit_limit_mb: 37_400,
            cpu: cpu(
                "Fixture desktop processor, 8 performance and 12 efficient cores",
                CpuVendor::Intel,
                &[(1, 8, 16), (0, 12, 12)],
                &[CpuFeature::Avx2, CpuFeature::AvxVnni],
            ),
            adapters: vec![adapter(
                "Fixture discrete GPU",
                AdapterKind::DiscreteGpu,
                12_282,
                16_300,
                "32.0.15.6614",
            )],
            execution_providers: vec![
                "CPUExecutionProvider".to_owned(),
                "DmlExecutionProvider".to_owned(),
            ],
            disk: DiskFacts {
                data_free_mb: 1_400_000,
                data_total_mb: 1_907_000,
                media: DiskMedia::Ssd,
            },
            power: PowerState::default(),
            audio: audio(
                endpoint("Fixture USB headset", EndpointForm::Headset, EndpointBus::Usb, 1),
                endpoint("Fixture USB headset", EndpointForm::Headset, EndpointBus::Usb, 2),
            ),
            monitors: 2,
            degraded: Vec::new(),
        };
        Self { name: "desktop", facts, memory_hours: week.lines(), usual: week.usual() }
    }

    /// The three, in the order above.
    #[must_use]
    pub fn all() -> [Self; 3] {
        [Self::owners_shape(), Self::one_class_laptop(), Self::desktop()]
    }
}
