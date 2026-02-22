//! Command-buffer runtime integration for existing UI components.

extern crate alloc;

use alloc::boxed::Box;
use alloc::string::ToString;

use crate::core::{Color, FontStyle, FontWeight, LineHeight, Point, Rect, TextStyle};
use crate::dsl::{RefreshMode as DslRefreshMode, StackOpts, UiDsl};
use crate::refresh::RefreshHint;
use crate::render_ir::{CmdBuffer, DrawCmd};
use crate::ui::theme::Theme;

/// Runtime that records DSL/component output into a fixed command buffer.
pub struct UiRuntime<'rt, const N: usize> {
    pub cmds: &'rt mut CmdBuffer<'static, N>,
    cursor: Point,
    width: u16,
    stack: heapless::Vec<(u16, u16), 16>,
    next_refresh: RefreshHint,
}

impl<'rt, const N: usize> UiRuntime<'rt, N> {
    pub fn new(cmds: &'rt mut CmdBuffer<'static, N>, width: u16) -> Self {
        Self {
            cmds,
            cursor: Point { x: 0, y: 0 },
            width,
            stack: heapless::Vec::new(),
            next_refresh: RefreshHint::Adaptive,
        }
    }

    pub fn set_refresh_hint(&mut self, hint: RefreshHint) {
        self.next_refresh = hint;
    }

    pub fn take_refresh_hint(&mut self) -> RefreshHint {
        let hint = self.next_refresh;
        self.next_refresh = RefreshHint::Adaptive;
        hint
    }

    pub fn draw_divider(&mut self) {
        let y = self.cursor.y;
        let rect = Rect {
            x: 0,
            y,
            width: self.width,
            height: 1,
        };
        let _ = self.cmds.push(
            DrawCmd::FillRect {
                rect,
                color: Color::Black,
            },
            rect,
        );
        self.cursor.y += 2;
    }

    pub fn paragraph_styled(&mut self, text: &'static str, style: TextStyle) {
        let pos = self.cursor;
        let region = Rect {
            x: 0,
            y: pos.y.saturating_sub(14),
            width: self.width,
            height: 20,
        };
        let _ = self
            .cmds
            .push(DrawCmd::DrawText { pos, text, style }, region);
        self.cursor.y += 22;
    }

    pub fn draw_status_bar(&mut self, left: &'static str, right: &'static str) {
        let style = default_text_style();
        let left_pos = Point { x: 8, y: 20 };
        let right_pos = Point {
            x: self
                .width
                .saturating_sub((right.len() as u16).saturating_mul(8)) as i16,
            y: 20,
        };
        let region = Rect {
            x: 0,
            y: 0,
            width: self.width,
            height: 28,
        };
        let _ = self.cmds.push(
            DrawCmd::FillRect {
                rect: region,
                color: Color::White,
            },
            region,
        );
        let _ = self.cmds.push(
            DrawCmd::DrawText {
                pos: left_pos,
                text: left,
                style,
            },
            region,
        );
        let _ = self.cmds.push(
            DrawCmd::DrawText {
                pos: right_pos,
                text: right,
                style,
            },
            region,
        );
        self.cursor.y = 30;
    }

    pub fn label_with_theme(&mut self, text: &'static str, _theme: &Theme) {
        self.label(text);
    }
}

impl<'rt, const N: usize> UiDsl for UiRuntime<'rt, N> {
    fn vstack<F>(&mut self, opts: StackOpts, f: F)
    where
        F: FnOnce(&mut Self),
    {
        self.cursor.x += opts.pad as i16;
        self.cursor.y += opts.pad as i16;
        let _ = self.stack.push((opts.gap, opts.pad));
        f(self);
        let _ = self.stack.pop();
        self.cursor.x -= opts.pad as i16;
        self.cursor.y += opts.pad as i16;
    }

    fn hstack<F>(&mut self, opts: StackOpts, f: F)
    where
        F: FnOnce(&mut Self),
    {
        let start = self.cursor;
        let _ = self.stack.push((opts.gap, opts.pad));
        f(self);
        let _ = self.stack.pop();
        self.cursor = Point {
            x: start.x,
            y: self.cursor.y + opts.gap as i16,
        };
    }

    fn label(&mut self, text: impl core::fmt::Display) {
        let owned = text.to_string();
        let leaked: &'static str = Box::leak(owned.into_boxed_str());
        let region = Rect {
            x: self.cursor.x,
            y: self.cursor.y.saturating_sub(16),
            width: self.width,
            height: 20,
        };
        let _ = self.cmds.push(
            DrawCmd::DrawText {
                pos: self.cursor,
                text: leaked,
                style: default_text_style(),
            },
            region,
        );
        let gap = self.stack.last().map(|(gap, _)| *gap as i16).unwrap_or(4);
        self.cursor.y += 16 + gap;
    }

    fn spacer(&mut self) {
        let gap = self.stack.last().map(|(gap, _)| *gap as i16).unwrap_or(8);
        self.cursor.y += gap;
    }

    fn divider(&mut self) {
        self.draw_divider();
    }

    fn status_bar(&mut self, left: impl core::fmt::Display, right: impl core::fmt::Display) {
        let left_owned = left.to_string();
        let right_owned = right.to_string();
        let left_ref: &'static str = Box::leak(left_owned.into_boxed_str());
        let right_ref: &'static str = Box::leak(right_owned.into_boxed_str());
        self.draw_status_bar(left_ref, right_ref);
    }

    fn paragraph(&mut self, text: impl core::fmt::Display) {
        let owned = text.to_string();
        let leaked: &'static str = Box::leak(owned.into_boxed_str());
        self.paragraph_styled(leaked, default_text_style());
    }

    fn with_refresh<F>(&mut self, mode: DslRefreshMode, f: F)
    where
        F: FnOnce(&mut Self),
    {
        self.set_refresh_hint(match mode {
            DslRefreshMode::Full => RefreshHint::Full,
            DslRefreshMode::Partial => RefreshHint::Partial,
            DslRefreshMode::Fast => RefreshHint::Fast,
        });
        f(self);
    }
}

fn default_text_style() -> TextStyle {
    TextStyle {
        font: 0,
        size_px: 14.0,
        color: Color::Black,
        line_height: LineHeight::Multiplier(1.2),
        letter_spacing: 0,
        weight: FontWeight::Regular,
        style: FontStyle::Normal,
    }
}
