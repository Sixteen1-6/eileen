//! Grounding, UI Automation, input, capture and text insertion (`eileen-desktop-win`).

use eileen_contracts::core_io::Verification;
use eileen_contracts::desktop::{
    DesktopChange, ElementInfo, Monitor, Point, PresentationState, Selection, Selector, WindowInfo,
    WindowRef,
};
use eileen_contracts::tools::desktop::{InsertTarget, MouseButton, ShotTarget};

use crate::{Deadline, PlatformError, RgbaImage, Subscription, VirtualKey};

/// How much of a window's element tree to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TreeLimits {
    /// The deepest level read, the window being level 0.
    pub max_depth: u16,
    /// The most elements read.
    pub max_nodes: u32,
}

impl Default for TreeLimits {
    fn default() -> Self {
        Self { max_depth: 12, max_nodes: 500 }
    }
}

/// An element, by its window and UI Automation runtime id.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ElementHandle {
    /// Its window.
    pub hwnd: i64,
    /// Its runtime id.
    pub runtime_id: Vec<i32>,
}

/// One element of a tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementNode {
    /// How to reach it again.
    pub handle: ElementHandle,
    /// The index of its parent in the tree's nodes; `None` for the root.
    pub parent: Option<u32>,
    /// What it is.
    pub info: ElementInfo,
}

/// A window's element tree, parents before children.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ElementTree {
    /// The elements read.
    pub nodes: Vec<ElementNode>,
    /// True when a limit cut the tree short.
    pub truncated: bool,
}

/// An element a selector matched, with how well.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementCandidate {
    /// How to reach it.
    pub handle: ElementHandle,
    /// What it is.
    pub info: ElementInfo,
    /// The match score, higher is better.
    pub score: u16,
}

/// What [`Input::restore`] puts back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RestoreToken {
    /// The window that was in front before Eileen took the foreground.
    pub previous_foreground: Option<i64>,
    /// Where the pointer was.
    pub cursor: Option<Point>,
}

/// The answer to a foreground request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForegroundOutcome {
    /// Eileen has the window in front; the token restores what was there.
    Taken(RestoreToken),
    /// Windows refused (another app keeps the foreground lock).
    Refused,
}

/// One synthetic input event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputEvent {
    /// A key goes down or up.
    Key {
        /// The key.
        key: VirtualKey,
        /// Down, or up.
        down: bool,
    },
    /// A character typed as Unicode input.
    Char(char),
    /// The pointer moves to a point on the virtual screen.
    MouseMove(Point),
    /// A mouse button goes down or up.
    Button {
        /// The button.
        button: MouseButton,
        /// Down, or up.
        down: bool,
    },
    /// The wheel turns.
    Wheel {
        /// Wheel units (120 per notch).
        delta: i32,
        /// The horizontal wheel.
        horizontal: bool,
    },
}

/// What [`Input::send`] checks before each event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForegroundGuard {
    /// The window that must stay in front.
    pub hwnd: i64,
    /// Stop at the first physical input from the user.
    pub stop_on_physical_input: bool,
}

/// How a send ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendOutcome {
    /// Every event was sent.
    Completed {
        /// Events sent.
        sent: u32,
    },
    /// The user moved a physical device; the rest was not sent.
    StoppedByUser {
        /// Events sent before the stop.
        sent: u32,
    },
    /// The guarded window lost the foreground; the rest was not sent.
    FocusLost {
        /// Events sent before the stop.
        sent: u32,
    },
}

/// How text goes into a field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InsertMethod {
    /// UI Automation's value or text pattern.
    Uia,
    /// The clipboard and a paste, restoring the clipboard after.
    Clipboard,
    /// Typed as Unicode input.
    Keys,
}

/// How an insertion ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertOutcome {
    /// The text went in.
    Inserted {
        /// The method that worked.
        method: InsertMethod,
    },
    /// The field refused it (read-only, a password field the policy forbids, gone).
    Refused {
        /// Why.
        reason: &'static str,
    },
}

/// What is on screen.
pub trait Grounding: Send + Sync {
    /// The monitors.
    fn monitors(&self) -> Result<Vec<Monitor>, PlatformError>;
    /// The top-level windows, front to back.
    fn windows(&self, include_minimized: bool) -> Result<Vec<WindowInfo>, PlatformError>;
    /// The foreground window.
    fn foreground(&self) -> Result<Option<WindowRef>, PlatformError>;
    /// The window under a point.
    fn window_from_point(&self, p: Point) -> Result<Option<WindowRef>, PlatformError>;
    /// The focused element.
    fn focused_element(&self, deadline: Deadline) -> Result<Option<ElementInfo>, PlatformError>;
    /// The element under a point.
    fn element_from_point(
        &self,
        p: Point,
        deadline: Deadline,
    ) -> Result<Option<ElementInfo>, PlatformError>;
    /// The user's selection, from the clipboard only when `allow_clipboard`.
    fn selection(
        &self,
        allow_clipboard: bool,
        deadline: Deadline,
    ) -> Result<Option<Selection>, PlatformError>;
    /// Whether a presentation, a call or a full-screen app is on.
    fn presentation_state(&self) -> Result<PresentationState, PlatformError>;
    /// Whether another app is using the microphone.
    fn mic_in_use_elsewhere(&self) -> Result<bool, PlatformError>;
    /// Calls `sink` with every change.
    fn subscribe(
        &self,
        sink: Box<dyn Fn(DesktopChange) + Send + Sync>,
    ) -> Result<Subscription, PlatformError>;
}

/// UI Automation, on its own MTA thread, abandoned and replaced on a hang.
pub trait Automation: Send + Sync {
    /// A window's element tree, within `limits`.
    fn tree(
        &self,
        hwnd: i64,
        limits: TreeLimits,
        deadline: Deadline,
    ) -> Result<ElementTree, PlatformError>;
    /// Invokes an element (its invoke, toggle or select pattern) and checks the effect.
    fn invoke(
        &self,
        element: &ElementHandle,
        deadline: Deadline,
    ) -> Result<Verification, PlatformError>;
    /// Sets an element's value and reads it back.
    fn set_value(
        &self,
        element: &ElementHandle,
        value: &str,
        deadline: Deadline,
    ) -> Result<Verification, PlatformError>;
    /// The elements of a window a selector matches, best first.
    fn find(
        &self,
        hwnd: i64,
        selector: &Selector,
        deadline: Deadline,
    ) -> Result<Vec<ElementCandidate>, PlatformError>;
}

/// Synthetic input under the input lock.
pub trait Input: Send + Sync {
    /// Physical idle time from Raw Input; synthetic input does not reset it. The only other
    /// source of "physical" events is the `test-physical` Cargo feature, which no shipped
    /// binary enables.
    fn user_idle_ms(&self) -> Result<u32, PlatformError>;
    /// Brings `hwnd` to the front.
    fn take_foreground(&self, hwnd: i64) -> Result<ForegroundOutcome, PlatformError>;
    /// Sends events one at a time; stops within one event when physical input or a focus
    /// change is seen.
    fn send(
        &self,
        hwnd: i64,
        events: &[InputEvent],
        guard: ForegroundGuard,
    ) -> Result<SendOutcome, PlatformError>;
    /// Puts back the foreground, and the pointer when the user has not moved it.
    fn restore(&self, token: RestoreToken, cursor_if_unmoved: bool) -> Result<(), PlatformError>;
}

/// Screenshots.
pub trait Capture: Send + Sync {
    /// Captures `target`.
    fn capture(&self, target: &ShotTarget) -> Result<RgbaImage, PlatformError>;
}

/// Text into a field.
pub trait TextInsertion: Send + Sync {
    /// Inserts `text` into `target` with `method`, falling back as the implementation allows.
    fn insert(
        &self,
        text: &str,
        target: &InsertTarget,
        method: InsertMethod,
    ) -> Result<InsertOutcome, PlatformError>;
}

#[cfg(any(test, feature = "fake"))]
pub use fake::{FakeAutomation, FakeCapture, FakeGrounding, FakeInput, FakeTextInsertion};

#[cfg(any(test, feature = "fake"))]
mod fake {
    use std::collections::BTreeMap;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

    use eileen_contracts::core_io::Verification;
    use eileen_contracts::desktop::{
        DesktopChange, ElementInfo, Monitor, Point, PresentationState, Selection, Selector,
        WindowInfo, WindowRef,
    };
    use eileen_contracts::tools::desktop::{InsertTarget, ShotTarget};

    use super::{
        Automation, Capture, ElementCandidate, ElementHandle, ElementTree, ForegroundGuard,
        ForegroundOutcome, Grounding, Input, InputEvent, InsertMethod, InsertOutcome, RestoreToken,
        SendOutcome, TextInsertion, TreeLimits,
    };
    use crate::fake_util::{Sinks, lock};
    use crate::{Deadline, PlatformError, RgbaImage, Subscription};

    /// A desktop a test describes; changes reach subscribers through [`Self::emit`].
    #[derive(Debug)]
    pub struct FakeGrounding {
        /// The monitors.
        pub monitors: Mutex<Vec<Monitor>>,
        /// The windows, front to back.
        pub windows: Mutex<Vec<WindowInfo>>,
        /// The foreground window.
        pub foreground: Mutex<Option<WindowRef>>,
        /// The focused element.
        pub focused: Mutex<Option<ElementInfo>>,
        /// The selection.
        pub selection: Mutex<Option<Selection>>,
        /// The presentation state.
        pub presentation: Mutex<PresentationState>,
        /// Another app is using the microphone.
        pub mic_elsewhere: AtomicBool,
        sinks: Sinks<DesktopChange>,
    }

    impl Default for FakeGrounding {
        fn default() -> Self {
            Self {
                monitors: Mutex::default(),
                windows: Mutex::default(),
                foreground: Mutex::default(),
                focused: Mutex::default(),
                selection: Mutex::default(),
                presentation: Mutex::default(),
                mic_elsewhere: AtomicBool::new(false),
                sinks: Sinks::new(),
            }
        }
    }

    impl FakeGrounding {
        /// Sends a change to every subscriber.
        pub fn emit(&self, change: &DesktopChange) {
            self.sinks.emit(change);
        }

        /// How many subscribers there are.
        #[must_use]
        pub fn subscribers(&self) -> usize {
            self.sinks.len()
        }
    }

    impl Grounding for FakeGrounding {
        fn monitors(&self) -> Result<Vec<Monitor>, PlatformError> {
            Ok(lock(&self.monitors).clone())
        }

        fn windows(&self, include_minimized: bool) -> Result<Vec<WindowInfo>, PlatformError> {
            let windows = lock(&self.windows);
            Ok(windows.iter().filter(|w| include_minimized || !w.is_minimized).cloned().collect())
        }

        fn foreground(&self) -> Result<Option<WindowRef>, PlatformError> {
            Ok(*lock(&self.foreground))
        }

        fn window_from_point(&self, p: Point) -> Result<Option<WindowRef>, PlatformError> {
            let hit = lock(&self.windows)
                .iter()
                .find(|w| {
                    let r = w.frame_rect;
                    !w.is_minimized
                        && p.x >= r.x
                        && p.y >= r.y
                        && i64::from(p.x) < i64::from(r.x) + i64::from(r.w)
                        && i64::from(p.y) < i64::from(r.y) + i64::from(r.h)
                })
                .map(|w| WindowRef { hwnd: w.hwnd, pid: w.pid });
            Ok(hit)
        }

        fn focused_element(&self, _: Deadline) -> Result<Option<ElementInfo>, PlatformError> {
            Ok(lock(&self.focused).clone())
        }

        fn element_from_point(
            &self,
            _: Point,
            _: Deadline,
        ) -> Result<Option<ElementInfo>, PlatformError> {
            Ok(lock(&self.focused).clone())
        }

        fn selection(&self, _: bool, _: Deadline) -> Result<Option<Selection>, PlatformError> {
            Ok(lock(&self.selection).clone())
        }

        fn presentation_state(&self) -> Result<PresentationState, PlatformError> {
            Ok(*lock(&self.presentation))
        }

        fn mic_in_use_elsewhere(&self) -> Result<bool, PlatformError> {
            Ok(self.mic_elsewhere.load(Ordering::SeqCst))
        }

        fn subscribe(
            &self,
            sink: Box<dyn Fn(DesktopChange) + Send + Sync>,
        ) -> Result<Subscription, PlatformError> {
            Ok(self.sinks.add(sink))
        }
    }

    /// Element trees and find results a test sets. With `hang` set, every call answers
    /// [`PlatformError::DeadlineExceeded`], as an abandoned UIA thread does.
    #[derive(Debug, Default)]
    pub struct FakeAutomation {
        /// Trees by window.
        pub trees: Mutex<BTreeMap<i64, ElementTree>>,
        /// What `find` answers.
        pub candidates: Mutex<Vec<ElementCandidate>>,
        /// What `invoke` and `set_value` answer; `None` gives `Unavailable`.
        pub verification: Mutex<Option<Verification>>,
        /// Answer every call with a passed deadline.
        pub hang: AtomicBool,
        /// The values set, with their element.
        pub values_set: Mutex<Vec<(ElementHandle, String)>>,
    }

    impl FakeAutomation {
        fn check_hang(&self, call: &'static str) -> Result<(), PlatformError> {
            if self.hang.load(Ordering::SeqCst) {
                Err(PlatformError::DeadlineExceeded { call })
            } else {
                Ok(())
            }
        }

        fn verification(&self) -> Result<Verification, PlatformError> {
            lock(&self.verification)
                .clone()
                .ok_or(PlatformError::Unavailable { what: "verification" })
        }
    }

    impl Automation for FakeAutomation {
        fn tree(
            &self,
            hwnd: i64,
            limits: TreeLimits,
            _: Deadline,
        ) -> Result<ElementTree, PlatformError> {
            self.check_hang("tree")?;
            let mut tree = lock(&self.trees)
                .get(&hwnd)
                .cloned()
                .ok_or(PlatformError::NotFound { what: "window" })?;
            let max = usize::try_from(limits.max_nodes).unwrap_or(usize::MAX);
            if tree.nodes.len() > max {
                tree.nodes.truncate(max);
                tree.truncated = true;
            }
            Ok(tree)
        }

        fn invoke(&self, _: &ElementHandle, _: Deadline) -> Result<Verification, PlatformError> {
            self.check_hang("invoke")?;
            self.verification()
        }

        fn set_value(
            &self,
            element: &ElementHandle,
            value: &str,
            _: Deadline,
        ) -> Result<Verification, PlatformError> {
            self.check_hang("set_value")?;
            lock(&self.values_set).push((element.clone(), value.to_owned()));
            self.verification()
        }

        fn find(
            &self,
            _: i64,
            _: &Selector,
            _: Deadline,
        ) -> Result<Vec<ElementCandidate>, PlatformError> {
            self.check_hang("find")?;
            Ok(lock(&self.candidates).clone())
        }
    }

    /// Input that records what it sent. `physical_after` stops a send after that many events,
    /// as physical input from the user would.
    #[derive(Debug, Default)]
    pub struct FakeInput {
        /// What `user_idle_ms` answers.
        pub idle_ms: AtomicU32,
        /// Refuse foreground requests.
        pub refuse_foreground: AtomicBool,
        /// Stop each send after this many events (`None`: never).
        pub physical_after: Mutex<Option<u32>>,
        /// Every event sent, in order.
        pub sent: Mutex<Vec<InputEvent>>,
        /// Every restore, in order.
        pub restored: Mutex<Vec<RestoreToken>>,
    }

    impl Input for FakeInput {
        fn user_idle_ms(&self) -> Result<u32, PlatformError> {
            Ok(self.idle_ms.load(Ordering::SeqCst))
        }

        fn take_foreground(&self, _: i64) -> Result<ForegroundOutcome, PlatformError> {
            if self.refuse_foreground.load(Ordering::SeqCst) {
                Ok(ForegroundOutcome::Refused)
            } else {
                Ok(ForegroundOutcome::Taken(RestoreToken {
                    previous_foreground: None,
                    cursor: None,
                }))
            }
        }

        fn send(
            &self,
            _: i64,
            events: &[InputEvent],
            guard: ForegroundGuard,
        ) -> Result<SendOutcome, PlatformError> {
            let stop =
                if guard.stop_on_physical_input { *lock(&self.physical_after) } else { None };
            let limit = stop.map_or(events.len(), |s| {
                usize::try_from(s).unwrap_or(usize::MAX).min(events.len())
            });
            let sent_now = events.get(..limit).unwrap_or(events);
            lock(&self.sent).extend_from_slice(sent_now);
            let sent = u32::try_from(sent_now.len()).unwrap_or(u32::MAX);
            if sent_now.len() < events.len() {
                Ok(SendOutcome::StoppedByUser { sent })
            } else {
                Ok(SendOutcome::Completed { sent })
            }
        }

        fn restore(&self, token: RestoreToken, _: bool) -> Result<(), PlatformError> {
            lock(&self.restored).push(token);
            Ok(())
        }
    }

    /// Captures answer a blank image of a fixed size.
    #[derive(Debug, Clone, Copy)]
    pub struct FakeCapture {
        /// The width of every capture.
        pub width: u32,
        /// The height of every capture.
        pub height: u32,
    }

    impl Capture for FakeCapture {
        fn capture(&self, _: &ShotTarget) -> Result<RgbaImage, PlatformError> {
            let len = u64::from(self.width) * u64::from(self.height) * 4;
            let len = usize::try_from(len).map_err(|_| PlatformError::Invalid {
                what: "capture size",
                detail: format!("{}x{}", self.width, self.height),
            })?;
            RgbaImage::new(self.width, self.height, vec![0; len])
        }
    }

    /// Insertion that records the text and answers with the method asked for.
    #[derive(Debug, Default)]
    pub struct FakeTextInsertion {
        /// Every insertion, in order.
        pub inserted: Mutex<Vec<(String, InsertMethod)>>,
        /// Refuse every insertion.
        pub refuse: AtomicBool,
    }

    impl TextInsertion for FakeTextInsertion {
        fn insert(
            &self,
            text: &str,
            _: &InsertTarget,
            method: InsertMethod,
        ) -> Result<InsertOutcome, PlatformError> {
            if self.refuse.load(Ordering::SeqCst) {
                return Ok(InsertOutcome::Refused { reason: "read-only field" });
            }
            lock(&self.inserted).push((text.to_owned(), method));
            Ok(InsertOutcome::Inserted { method })
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use super::{
        Automation, ElementTree, FakeAutomation, FakeInput, ForegroundGuard, Input, InputEvent,
        SendOutcome, TreeLimits,
    };
    use crate::{Deadline, PlatformError, VirtualKey};

    #[test]
    fn physical_input_stops_a_send_within_one_event() {
        let input = FakeInput::default();
        *input.physical_after.lock().unwrap() = Some(2);
        let key = InputEvent::Key { key: VirtualKey(0x41), down: true };
        let guard = ForegroundGuard { hwnd: 1, stop_on_physical_input: true };
        let outcome = input.send(1, &[key, key, key, key], guard).unwrap();
        assert_eq!(outcome, SendOutcome::StoppedByUser { sent: 2 }, "stopped after two");
        assert_eq!(input.sent.lock().unwrap().len(), 2, "nothing past the stop");
    }

    #[test]
    fn a_hung_automation_answers_a_deadline_error() {
        let uia = FakeAutomation::default();
        uia.trees.lock().unwrap().insert(7, ElementTree::default());
        assert!(uia.tree(7, TreeLimits::default(), Deadline::UIA).is_ok(), "answers");
        uia.hang.store(true, Ordering::SeqCst);
        assert_eq!(
            uia.tree(7, TreeLimits::default(), Deadline::UIA),
            Err(PlatformError::DeadlineExceeded { call: "tree" }),
            "abandoned",
        );
    }
}
