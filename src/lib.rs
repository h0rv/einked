//! Generic e-ink UI primitives built on embedded-graphics.
//! Shared by firmware, simulators, and app crates.

#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]
#![cfg_attr(
    not(test),
    deny(
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented,
        clippy::unreachable,
        clippy::unwrap_used
    )
)]

pub mod buffered_display;
pub mod diff;
pub mod embedded_fonts;
pub mod font_render;
pub mod input;
pub mod test_display;
pub mod ui;

/// UI display dimensions (portrait mode).
/// SSD1677 panel is 480x800 pixels in its native orientation.
pub const DISPLAY_WIDTH: u32 = 480;
pub const DISPLAY_HEIGHT: u32 = 800;

/// Normalize a draw target's size to portrait (width <= height).
pub fn portrait_dimensions<D: embedded_graphics::prelude::OriginDimensions>(
    display: &D,
) -> (u32, u32) {
    let size = display.size();
    let width = size.width.min(size.height);
    let height = size.width.max(size.height);
    (width, height)
}
