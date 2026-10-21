//! The HUD's drawing surface.

use eileen_contracts::desktop::{Point, Rect};
use eileen_contracts::hud::HudInput;

use crate::{PlatformError, RgbaImage};

/// One thing to draw, in the overlay's pixels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DrawItem {
    /// A filled rectangle.
    Fill {
        /// Where.
        rect: Rect,
        /// Colour, `0xRRGGBBAA`.
        rgba: u32,
    },
    /// A line of text.
    Text {
        /// Its top-left corner.
        at: Point,
        /// What it says.
        text: String,
        /// Its size, pixels.
        px: u16,
        /// Colour, `0xRRGGBBAA`.
        rgba: u32,
    },
    /// An image scaled into a rectangle.
    Image {
        /// Where.
        rect: Rect,
        /// The pixels.
        image: RgbaImage,
    },
}

/// A frame for the HUD, back to front. `eileen-hud` lays it out; the surface draws it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DrawList {
    /// The items, back to front.
    pub items: Vec<DrawItem>,
}

/// The overlay (`eileen-hud-win`, driven by `eileen-hud`). Not `Send`: it lives on the HUD's
/// own thread with its window.
pub trait HudSurface {
    /// Draws a frame.
    fn draw(&mut self, list: &DrawList) -> Result<(), PlatformError>;
    /// Shows a live thumbnail of `hwnd` in `rect` (DWM), or hides it with `None`.
    fn set_watch(&mut self, hwnd: Option<i64>, rect: Rect) -> Result<(), PlatformError>;
    /// The next input on the overlay, and whether it came from a physical device.
    fn next_input(&mut self) -> Option<(HudInput, bool)>;
}

#[cfg(any(test, feature = "fake"))]
pub use fake::FakeHudSurface;

#[cfg(any(test, feature = "fake"))]
mod fake {
    use std::collections::VecDeque;

    use eileen_contracts::desktop::Rect;
    use eileen_contracts::hud::HudInput;

    use super::{DrawList, HudSurface};
    use crate::PlatformError;

    /// A surface that keeps what it drew and plays queued input.
    #[derive(Debug, Default)]
    pub struct FakeHudSurface {
        /// Every frame drawn, in order.
        pub frames: Vec<DrawList>,
        /// The current watch, if any.
        pub watch: Option<(i64, Rect)>,
        /// Input to deliver, with its physical flag.
        pub input: VecDeque<(HudInput, bool)>,
    }

    impl HudSurface for FakeHudSurface {
        fn draw(&mut self, list: &DrawList) -> Result<(), PlatformError> {
            self.frames.push(list.clone());
            Ok(())
        }

        fn set_watch(&mut self, hwnd: Option<i64>, rect: Rect) -> Result<(), PlatformError> {
            self.watch = hwnd.map(|h| (h, rect));
            Ok(())
        }

        fn next_input(&mut self) -> Option<(HudInput, bool)> {
            self.input.pop_front()
        }
    }
}

#[cfg(test)]
mod tests {
    use eileen_contracts::desktop::Rect;

    use super::{DrawItem, DrawList, FakeHudSurface, HudSurface};

    #[test]
    fn frames_and_watches_are_kept() {
        let mut hud = FakeHudSurface::default();
        let list = DrawList { items: vec![DrawItem::Fill { rect: Rect::default(), rgba: 0xff }] };
        hud.draw(&list).unwrap();
        hud.set_watch(Some(42), Rect::default()).unwrap();
        assert_eq!(hud.frames, vec![list], "frame kept");
        assert_eq!(hud.watch.map(|w| w.0), Some(42), "watching");
        assert!(hud.next_input().is_none(), "no input queued");
    }
}
