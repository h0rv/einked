//! Core geometry, style, and theme types for v1 APIs.

/// 2D point in display coordinates.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Point {
    pub x: i16,
    pub y: i16,
}

/// Axis-aligned rectangle.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i16,
    pub y: i16,
    pub width: u16,
    pub height: u16,
}

impl Rect {
    pub const fn split_top(self, height: u16) -> (Rect, Rect) {
        let h = if height < self.height {
            height
        } else {
            self.height
        };
        (
            Rect {
                x: self.x,
                y: self.y,
                width: self.width,
                height: h,
            },
            Rect {
                x: self.x,
                y: self.y + h as i16,
                width: self.width,
                height: self.height.saturating_sub(h),
            },
        )
    }

    pub const fn split_bottom(self, height: u16) -> (Rect, Rect) {
        let h = if height < self.height {
            height
        } else {
            self.height
        };
        (
            Rect {
                x: self.x,
                y: self.y,
                width: self.width,
                height: self.height.saturating_sub(h),
            },
            Rect {
                x: self.x,
                y: self.y + self.height.saturating_sub(h) as i16,
                width: self.width,
                height: h,
            },
        )
    }

    pub const fn split_left(self, width: u16) -> (Rect, Rect) {
        let w = if width < self.width {
            width
        } else {
            self.width
        };
        (
            Rect {
                x: self.x,
                y: self.y,
                width: w,
                height: self.height,
            },
            Rect {
                x: self.x + w as i16,
                y: self.y,
                width: self.width.saturating_sub(w),
                height: self.height,
            },
        )
    }

    pub const fn split_right(self, width: u16) -> (Rect, Rect) {
        let w = if width < self.width {
            width
        } else {
            self.width
        };
        (
            Rect {
                x: self.x,
                y: self.y,
                width: self.width.saturating_sub(w),
                height: self.height,
            },
            Rect {
                x: self.x + self.width.saturating_sub(w) as i16,
                y: self.y,
                width: w,
                height: self.height,
            },
        )
    }

    pub const fn inset(self, h: u16, v: u16) -> Rect {
        let double_h = h.saturating_mul(2);
        let double_v = v.saturating_mul(2);
        Rect {
            x: self.x + h as i16,
            y: self.y + v as i16,
            width: self.width.saturating_sub(double_h),
            height: self.height.saturating_sub(double_v),
        }
    }

    pub const fn contains(self, p: Point) -> bool {
        let x2 = self.x + self.width as i16;
        let y2 = self.y + self.height as i16;
        p.x >= self.x && p.y >= self.y && p.x < x2 && p.y < y2
    }

    pub const fn intersects(self, other: Rect) -> bool {
        let self_x2 = self.x + self.width as i16;
        let self_y2 = self.y + self.height as i16;
        let other_x2 = other.x + other.width as i16;
        let other_y2 = other.y + other.height as i16;
        !(self_x2 <= other.x || other_x2 <= self.x || self_y2 <= other.y || other_y2 <= self.y)
    }

    pub const fn union(self, other: Rect) -> Rect {
        let min_x = if self.x < other.x { self.x } else { other.x };
        let min_y = if self.y < other.y { self.y } else { other.y };
        let self_x2 = self.x + self.width as i16;
        let self_y2 = self.y + self.height as i16;
        let other_x2 = other.x + other.width as i16;
        let other_y2 = other.y + other.height as i16;
        let max_x = if self_x2 > other_x2 {
            self_x2
        } else {
            other_x2
        };
        let max_y = if self_y2 > other_y2 {
            self_y2
        } else {
            other_y2
        };
        Rect {
            x: min_x,
            y: min_y,
            width: (max_x - min_x) as u16,
            height: (max_y - min_y) as u16,
        }
    }
}

/// Display-independent color.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Color {
    Black,
    White,
    Gray(u8),
    Red,
    Custom(u8),
}

/// Opaque font identifier.
pub type FontId = u8;

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum LineHeight {
    Multiplier(f32),
    Absolute(u16),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum FontWeight {
    Regular,
    Bold,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum FontStyle {
    Normal,
    Italic,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct TextStyle {
    pub font: FontId,
    pub size_px: f32,
    pub color: Color,
    pub line_height: LineHeight,
    pub letter_spacing: i8,
    pub weight: FontWeight,
    pub style: FontStyle,
}

/// Semantic theme contract used by activity stack APIs.
pub trait Theme {
    fn body(&self) -> TextStyle;
    fn heading(&self) -> TextStyle;
    fn caption(&self) -> TextStyle;
    fn ui_label(&self) -> TextStyle;
    fn background(&self) -> Color;
    fn foreground(&self) -> Color;
    fn accent(&self) -> Color;
}

/// Sensible monochrome defaults.
pub struct DefaultTheme;

impl Theme for DefaultTheme {
    fn body(&self) -> TextStyle {
        TextStyle {
            font: 0,
            size_px: 16.0,
            color: Color::Black,
            line_height: LineHeight::Multiplier(1.4),
            letter_spacing: 0,
            weight: FontWeight::Regular,
            style: FontStyle::Normal,
        }
    }

    fn heading(&self) -> TextStyle {
        TextStyle {
            size_px: 20.0,
            weight: FontWeight::Bold,
            ..self.body()
        }
    }

    fn caption(&self) -> TextStyle {
        TextStyle {
            size_px: 12.0,
            ..self.body()
        }
    }

    fn ui_label(&self) -> TextStyle {
        TextStyle {
            size_px: 14.0,
            weight: FontWeight::Bold,
            ..self.body()
        }
    }

    fn background(&self) -> Color {
        Color::White
    }

    fn foreground(&self) -> Color {
        Color::Black
    }

    fn accent(&self) -> Color {
        Color::Black
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_split_and_union() {
        let rect = Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 50,
        };
        let (top, rest) = rect.split_top(10);
        assert_eq!(top.height, 10);
        assert_eq!(rest.y, 10);
        assert_eq!(top.union(rest), rect);
    }
}
