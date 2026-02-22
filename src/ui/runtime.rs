//! Command-buffer runtime integration for existing UI components.

use crate::core::{Color, FontStyle, FontWeight, LineHeight, Point, Rect, TextStyle};
use crate::dsl::{RefreshMode as DslRefreshMode, StackOpts, UiDsl};
use crate::refresh::RefreshHint;
use crate::render_ir::{CmdBuffer, DrawCmd, DrawTextBuf};
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

    pub fn paragraph_styled(&mut self, text: &str, style: TextStyle) {
        self.push_text(self.cursor, text, style);
        self.cursor.y += 22;
    }

    pub fn draw_status_bar(&mut self, left: &str, right: &str) {
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
        self.push_text(left_pos, left, style);
        self.push_text(right_pos, right, style);
        self.cursor.y = 30;
    }

    pub fn label_with_theme(&mut self, text: &str, _theme: &Theme) {
        self.label(text);
    }

    pub fn draw_text_at(&mut self, pos: Point, text: &str) {
        self.push_text(pos, text, default_text_style());
    }

    pub fn fill_rect(&mut self, rect: Rect, color: Color) {
        let _ = self.cmds.push(DrawCmd::FillRect { rect, color }, rect);
    }

    pub fn draw_line(&mut self, start: Point, end: Point, color: Color, width: u8) {
        let min_x = start.x.min(end.x);
        let min_y = start.y.min(end.y);
        let max_x = start.x.max(end.x);
        let max_y = start.y.max(end.y);
        let region = Rect {
            x: min_x,
            y: min_y,
            width: (max_x - min_x + 1) as u16,
            height: (max_y - min_y + 1) as u16,
        };
        let _ = self.cmds.push(
            DrawCmd::DrawLine {
                start,
                end,
                color,
                width,
            },
            region,
        );
    }

    fn push_text(&mut self, pos: Point, text: &str, style: TextStyle) {
        let mut buf = DrawTextBuf::new();
        for ch in text.chars() {
            if buf.push(ch).is_err() {
                break;
            }
        }
        let region = Rect {
            x: pos.x,
            y: pos.y.saturating_sub(16),
            width: self.width,
            height: 20,
        };
        let _ = self.cmds.push(
            DrawCmd::DrawText {
                pos,
                text: buf,
                style,
            },
            region,
        );
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
        let mut owned = DrawTextBuf::new();
        let _ = core::fmt::write(&mut owned, format_args!("{}", text));
        self.push_text(self.cursor, owned.as_str(), default_text_style());
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
        let mut left_owned = DrawTextBuf::new();
        let mut right_owned = DrawTextBuf::new();
        let _ = core::fmt::write(&mut left_owned, format_args!("{}", left));
        let _ = core::fmt::write(&mut right_owned, format_args!("{}", right));
        self.draw_status_bar(left_owned.as_str(), right_owned.as_str());
    }

    fn paragraph(&mut self, text: impl core::fmt::Display) {
        let mut owned = DrawTextBuf::new();
        let _ = core::fmt::write(&mut owned, format_args!("{}", text));
        self.paragraph_styled(owned.as_str(), default_text_style());
    }

    fn text_flow(&mut self, lines: impl core::fmt::Display) {
        self.paragraph(lines);
    }

    fn icon(&mut self, name: impl core::fmt::Display, value: impl core::fmt::Display) {
        self.label(format_args!("[icon {}:{}]", name, value));
    }

    fn page_indicator(&mut self, current: impl core::fmt::Display, total: impl core::fmt::Display) {
        self.label(format_args!("{}/{}", current, total));
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
