//! The apps toolset's arguments and op results.
//!
//! Provisional: WP6 pins this module in its first pull request (BUILD.md section 3.3).
//!
//! A tool whose argument struct chooses its operation through one enum field (`op` or
//! `action`) has one op per variant, named by the variant's `snake_case` tag. The result
//! structs are the ops' `output_schema`s, which recipes point into.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{ToolInput, input};
use crate::desktop::Rect;
use crate::schema::Registry;

const MODULE: &str = "tools/apps";

/// `apps_media`: the system's media sessions. `Status` tier 0, else 1.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AppsMedia {
    /// What to do.
    pub action: MediaAction,
    /// The session; the current one when left out.
    pub session: Option<String>,
}

/// What `apps_media` does.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MediaAction {
    /// Read what is playing.
    Status,
    /// Play.
    Play,
    /// Pause.
    Pause,
    /// Toggle play and pause.
    Toggle,
    /// The next track.
    Next,
    /// The previous track.
    Previous,
    /// Seek to a position.
    Seek {
        /// Seconds from the start.
        seconds: f64,
    },
    /// Seek by an amount.
    SeekBy {
        /// Seconds, forward or back.
        seconds: f64,
    },
}

/// What `apps_media` `status` returns.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MediaStatus {
    /// What is playing.
    pub title: String,
    /// The position, seconds.
    pub position_s: Option<f64>,
    /// Whether it is playing.
    pub playing: bool,
    /// The session.
    pub session: String,
}

/// `apps_open_url`: https only. Tier 1, by arguments for the brain (section 2.12's URL rule)
/// and for tainted workers (egress). With `reuse_playing`, a media session already showing
/// `wait_for_media_title` is sought to `start_s` instead of opening the URL again; idempotent
/// then.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AppsOpenUrl {
    /// The URL.
    pub url: String,
    /// Where to start a video, seconds.
    pub start_s: Option<u32>,
    /// Match-only: it only recognises the session, never picks what opens.
    #[schemars(extend("x-eileen-match" = true))]
    pub wait_for_media_title: Option<String>,
    /// Reuse a session already playing it.
    #[serde(default)]
    pub reuse_playing: bool,
}

/// What `apps_open_url` returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct OpenUrlOutput {
    /// A media session showed the expected title.
    pub media_title_seen: bool,
    /// An existing session was reused.
    pub reused: bool,
}

/// `apps_open_uri`: allowlisted schemes only (`ms-settings:`, `mailto:` compose,
/// `permissions.uri_schemes`). Tier 1.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AppsOpenUri {
    /// The URI.
    pub uri: String,
}

/// `apps_open_path`, by arguments (section 2.7): 1 for a document or folder; 2, HUD only, when
/// it runs code or `with_app` is a shell or interpreter; 3 for a credential, Eileen-data or
/// browser-profile path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AppsOpenPath {
    /// The file or folder.
    #[schemars(extend("x-eileen-path" = true))]
    pub path: String,
    /// The app to open it with.
    pub with_app: Option<String>,
}

/// `apps_resolve`: the Resolve adapter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AppsResolve {
    /// The op.
    pub op: ResolveOp,
}

/// The Resolve ops. Tier 0: `Hello` to `IsRendering`. Tier 1: `OpenPage` to `MarkerAdd`
/// (`DeleteFusionComp` only for names starting `eileen-`). By arguments: `LoadProject` (2 if
/// the open project has unsaved changes), `RenderStart` (2 if it would overwrite),
/// `ImportFusionComp` (1 when its inverse was recorded in the same job, else 2; preview first)
/// and `ApplyDrx` (1 after `AddVersion` in the same job, else 2; preview first). The `act`
/// toolset exposes only the tier-0 ops and `OpenPage`, `Goto`, `GotoTimecode`, `SetTimeline`,
/// `LoadProject`, `RenderStart` and `MarkerAdd`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ResolveOp {
    /// Check the bridge and Resolve's version.
    Hello,
    /// The current timeline and playhead.
    TimelineInfo,
    /// Recent projects.
    ProjectsRecent,
    /// The project's timelines.
    Timelines,
    /// The clip under the playhead.
    CurrentItem,
    /// The clip's versions.
    Versions,
    /// The clip's Fusion compositions.
    FusionComps,
    /// The timeline's markers.
    Markers,
    /// The render queue.
    RenderStatus,
    /// Whether a render runs.
    IsRendering,
    /// Open a page.
    OpenPage {
        /// The page.
        page: ResolvePage,
    },
    /// Move the playhead to an offset from the timeline's start.
    Goto {
        /// Seconds.
        offset_s: f64,
    },
    /// Move the playhead to a timecode.
    GotoTimecode {
        /// The timecode.
        tc: String,
    },
    /// Switch timeline.
    SetTimeline {
        /// The timeline.
        name: String,
    },
    /// Export the frame under the playhead into the job's temp folder.
    ExportStill,
    /// Export frames over a range.
    ExportStills {
        /// Start, seconds.
        from_offset_s: f64,
        /// End, seconds.
        to_offset_s: f64,
        /// Every n-th frame.
        every_n_frames: Option<u16>,
    },
    /// Add a version to the clip.
    AddVersion {
        /// Its name.
        name: Option<String>,
    },
    /// Load a version.
    LoadVersion {
        /// Its name.
        name: String,
    },
    /// Load a Fusion composition.
    LoadFusionComp {
        /// Its name.
        name: String,
    },
    /// Delete a Fusion composition Eileen made.
    DeleteFusionComp {
        /// Its name.
        name: String,
    },
    /// Add a marker.
    MarkerAdd {
        /// Seconds from the start.
        offset_s: f64,
        /// Its name.
        name: String,
        /// Its colour, as Resolve names it.
        color: String,
    },
    /// Open a project.
    LoadProject {
        /// The project.
        name: String,
    },
    /// Start a render.
    RenderStart {
        /// The render preset.
        preset: String,
        /// The output file name.
        file_name: String,
    },
    /// Import a Fusion template over tracked boxes.
    ImportFusionComp {
        /// The template.
        template: FusionTemplate,
        /// The boxes, per frame.
        boxes: Vec<FrameBox>,
        /// How strong.
        amount: Choice,
    },
    /// Apply a grade from a DRX template.
    ApplyDrx {
        /// The template.
        template: DrxTemplate,
        /// A window to limit it to.
        window: Option<Rect>,
        /// Apply to every layer.
        all_layers: bool,
    },
}

/// A Resolve page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ResolvePage {
    /// Media.
    Media,
    /// Cut.
    Cut,
    /// Edit.
    Edit,
    /// Fusion.
    Fusion,
    /// Color.
    Color,
    /// Fairlight.
    Fairlight,
    /// Deliver.
    Deliver,
}

/// Eileen's Fusion templates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FusionTemplate {
    /// Sharpen inside the boxes.
    Sharpen,
    /// Blur inside the boxes.
    Blur,
}

/// Eileen's DRX grade templates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DrxTemplate {
    /// Sharpen.
    Sharpen,
    /// Soften.
    Soften,
}

/// How strong an effect is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Choice {
    /// A little.
    Subtle,
    /// Clearly.
    Medium,
    /// A lot.
    Strong,
}

/// A box on one frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FrameBox {
    /// The frame.
    pub frame: u64,
    /// The box, in the frame's pixels.
    pub rect: Rect,
}

/// What `goto` returns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GotoOutput {
    /// The timecode reached.
    pub target_tc: String,
    /// The frame reached.
    pub target_frame: u64,
}

/// What `timeline_info` returns.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TimelineInfoOutput {
    /// The timeline.
    pub timeline: String,
    /// Its start timecode.
    pub start_tc: String,
    /// Frames per second.
    pub fps: f64,
    /// Whether it uses drop-frame timecode.
    pub drop_frame: bool,
    /// The open page.
    pub page: ResolvePage,
    /// The playhead's frame.
    pub playhead_frame: u64,
    /// The playhead's timecode.
    pub playhead_tc: String,
}

/// What `export_still` returns, and one entry of `export_stills`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ExportStillOutput {
    /// The file, in the job's temp folder.
    pub path: String,
    /// Seconds from the start.
    pub offset_s: f64,
    /// The frame.
    pub frame: u64,
}

/// What `export_stills` returns.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ExportStillsOutput {
    /// The stills.
    pub stills: Vec<ExportStillOutput>,
}

/// What `current_item` returns.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CurrentItemOutput {
    /// The clip.
    pub name: String,
    /// Its start, seconds from the timeline's start.
    pub start_offset_s: f64,
    /// Its end, seconds from the timeline's start.
    pub end_offset_s: f64,
}

/// What `import_fusion_comp` returns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ImportFusionCompOutput {
    /// The new composition.
    pub name: String,
    /// The composition that was current before.
    pub previous_current: Option<String>,
}

/// What `fusion_comps` and `versions` return.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct NamesOutput {
    /// The names.
    pub names: Vec<String>,
    /// The current one.
    pub current: Option<String>,
}

/// What `add_version` returns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AddVersionOutput {
    /// The new version.
    pub name: String,
    /// The version that was current before.
    pub previous: Option<String>,
}

/// What `apply_drx` returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ApplyDrxOutput {
    /// Whether the grade was applied.
    pub applied: bool,
}

/// What `render_start` returns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RenderStartOutput {
    /// The render job.
    pub render_job: String,
}

/// `apps_image`: image analysis on files in the job's folders. Tier 0.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AppsImage {
    /// The op.
    pub op: ImageOp,
}

/// The image ops.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ImageOp {
    /// Find faces.
    Faces {
        /// The image.
        #[schemars(extend("x-eileen-path" = true))]
        path: String,
    },
    /// Pick one face by position.
    PickFace {
        /// The image.
        #[schemars(extend("x-eileen-path" = true))]
        path: String,
        /// Which one.
        which: Position,
    },
    /// Crop.
    Crop {
        /// The image.
        #[schemars(extend("x-eileen-path" = true))]
        path: String,
        /// The crop.
        rect: Rect,
    },
    /// Measure sharpness.
    Sharpness {
        /// The image.
        #[schemars(extend("x-eileen-path" = true))]
        path: String,
        /// Where.
        rect: Rect,
    },
    /// Track a face across stills.
    TrackFace {
        /// The stills.
        stills: Vec<StillRef>,
        /// The face on the first still.
        start: Rect,
    },
}

/// Which face, by position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Position {
    /// The leftmost.
    Left,
    /// The rightmost.
    Right,
    /// The most central.
    Center,
    /// The highest.
    Top,
    /// The lowest.
    Bottom,
    /// The largest.
    Largest,
}

/// One still for face tracking.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct StillRef {
    /// The image.
    #[schemars(extend("x-eileen-path" = true))]
    pub path: String,
    /// Its frame.
    pub frame: u64,
}

/// What `pick_face` returns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PickFaceOutput {
    /// The face.
    pub rect: Rect,
    /// A crop of it, in the job's temp folder.
    pub crop: String,
}

/// What `sharpness` returns.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SharpnessOutput {
    /// The variance of the Laplacian.
    pub sharpness: f64,
}

/// What `track_face` returns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TrackFaceOutput {
    /// The face's box per frame.
    pub boxes: Vec<FrameBox>,
}

/// `apps_office`: Word and Excel over COM. Tier 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AppsOffice {
    /// The op.
    pub op: OfficeOp,
}

/// The Office ops.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OfficeOp {
    /// The selection in Word.
    WordSelection,
    /// The active cell in Excel.
    ExcelActiveCell,
}

/// `apps_explorer`. Tier 0 or 1.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AppsExplorer {
    /// The op.
    pub op: ExplorerOp,
}

/// The Explorer ops.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExplorerOp {
    /// The selected files (tier 0).
    Selection,
    /// Open a folder (tier 1).
    OpenFolder {
        /// The folder.
        #[schemars(extend("x-eileen-path" = true))]
        path: String,
    },
}

/// `apps_recent`: recent files and media. Tier 0.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AppsRecent {
    /// What to look for.
    pub query: String,
    /// Which kinds; every kind when empty.
    #[serde(default)]
    pub kinds: Vec<FileKind>,
    /// At most this many.
    pub limit: Option<u8>,
    /// Include thumbnails.
    #[serde(default)]
    pub thumbnails: bool,
}

/// The kinds of recent item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FileKind {
    /// A document.
    Document,
    /// A spreadsheet.
    Spreadsheet,
    /// A presentation.
    Presentation,
    /// An image.
    Image,
    /// A video file.
    Video,
    /// An audio file.
    Audio,
    /// Source code.
    Code,
    /// A folder.
    Folder,
    /// Media the user watched or heard, in an app or a browser.
    Media,
}

/// What `apps_recent` returns.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RecentOutput {
    /// The best match, when there is one.
    pub best: Option<RecentHit>,
    /// Whether the best match is confident.
    pub confident: bool,
    /// Every match.
    pub items: Vec<RecentHit>,
}

/// One recent item.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RecentHit {
    /// Its title.
    pub title: String,
    /// Its URL, for web media.
    pub url: Option<String>,
    /// Its path, for a file.
    pub path: Option<String>,
    /// Its kind.
    pub kind: FileKind,
    /// How well it matches, 0 to 1.
    pub confidence: f64,
}

/// `apps_photoshop`. Tier 1; `SmartSharpen` previews first; Generative Fill and Neural Filters,
/// if ever added, are tier 2.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AppsPhotoshop {
    /// The op.
    pub op: PhotoshopOp,
}

/// The Photoshop ops.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PhotoshopOp {
    /// Recent documents.
    Recent,
    /// Open a document.
    Open {
        /// The document.
        #[schemars(extend("x-eileen-path" = true))]
        path: String,
    },
    /// Duplicate the current layer.
    DuplicateLayer,
    /// An elliptical selection.
    SelectEllipse {
        /// Its bounds.
        rect: Rect,
        /// Feather, pixels.
        feather_px: u16,
    },
    /// Smart Sharpen.
    SmartSharpen {
        /// Amount, percent.
        amount_pct: u16,
        /// Radius, pixels.
        radius_px: u16,
    },
}

pub(super) fn inputs(out: &mut BTreeMap<&'static str, ToolInput>) {
    input::<AppsMedia>(out, MODULE, "apps_media");
    input::<AppsOpenUrl>(out, MODULE, "apps_open_url");
    input::<AppsOpenUri>(out, MODULE, "apps_open_uri");
    input::<AppsOpenPath>(out, MODULE, "apps_open_path");
    input::<AppsResolve>(out, MODULE, "apps_resolve");
    input::<AppsImage>(out, MODULE, "apps_image");
    input::<AppsOffice>(out, MODULE, "apps_office");
    input::<AppsExplorer>(out, MODULE, "apps_explorer");
    input::<AppsRecent>(out, MODULE, "apps_recent");
    input::<AppsPhotoshop>(out, MODULE, "apps_photoshop");
}

pub(crate) fn register(r: &mut Registry) {
    r.add::<AppsMedia>();
    r.add::<MediaAction>();
    r.add::<MediaStatus>();
    r.add::<AppsOpenUrl>();
    r.add::<OpenUrlOutput>();
    r.add::<AppsOpenUri>();
    r.add::<AppsOpenPath>();
    r.add::<AppsResolve>();
    r.add::<ResolveOp>();
    r.add::<ResolvePage>();
    r.add::<FusionTemplate>();
    r.add::<DrxTemplate>();
    r.add::<Choice>();
    r.add::<FrameBox>();
    r.add::<GotoOutput>();
    r.add::<TimelineInfoOutput>();
    r.add::<ExportStillOutput>();
    r.add::<ExportStillsOutput>();
    r.add::<CurrentItemOutput>();
    r.add::<ImportFusionCompOutput>();
    r.add::<NamesOutput>();
    r.add::<AddVersionOutput>();
    r.add::<ApplyDrxOutput>();
    r.add::<RenderStartOutput>();
    r.add::<AppsImage>();
    r.add::<ImageOp>();
    r.add::<Position>();
    r.add::<StillRef>();
    r.add::<PickFaceOutput>();
    r.add::<SharpnessOutput>();
    r.add::<TrackFaceOutput>();
    r.add::<AppsOffice>();
    r.add::<OfficeOp>();
    r.add::<AppsExplorer>();
    r.add::<ExplorerOp>();
    r.add::<AppsRecent>();
    r.add::<FileKind>();
    r.add::<RecentOutput>();
    r.add::<RecentHit>();
    r.add::<AppsPhotoshop>();
    r.add::<PhotoshopOp>();
}
