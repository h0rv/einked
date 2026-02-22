//! Minimal fixed-size layout engine.

use crate::core::Rect;

/// Fixed-size layout slots.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Layout<const N: usize> {
    rects: [Rect; N],
}

impl<const N: usize> Layout<N> {
    pub const fn new(rects: [Rect; N]) -> Self {
        Self { rects }
    }

    pub const fn get(&self, slot: usize) -> Rect {
        self.rects[slot]
    }
}

/// Rect carving helper.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct LayoutBuilder {
    remaining: Rect,
}

impl LayoutBuilder {
    pub const fn new(screen: Rect) -> Self {
        Self { remaining: screen }
    }

    pub const fn header(self, height: u16) -> (Rect, Self) {
        let (header, remaining) = self.remaining.split_top(height);
        (header, Self { remaining })
    }

    pub const fn footer(self, height: u16) -> (Rect, Self) {
        let (remaining, footer) = self.remaining.split_bottom(height);
        (footer, Self { remaining })
    }

    pub const fn sidebar(self, width: u16) -> (Rect, Self) {
        let (sidebar, remaining) = self.remaining.split_left(width);
        (sidebar, Self { remaining })
    }

    pub const fn margin(self, h: u16, v: u16) -> Self {
        Self {
            remaining: self.remaining.inset(h, v),
        }
    }

    pub const fn body(self) -> Rect {
        self.remaining
    }

    pub const fn columns<const N: usize>(self) -> [Rect; N] {
        let mut out = [Rect {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        }; N];
        let each = if N == 0 {
            0
        } else {
            self.remaining.width / N as u16
        };
        let mut i = 0usize;
        while i < N {
            let x = self.remaining.x + (each as i16 * i as i16);
            let width = if i + 1 == N {
                self.remaining.width.saturating_sub(each * (N as u16 - 1))
            } else {
                each
            };
            out[i] = Rect {
                x,
                y: self.remaining.y,
                width,
                height: self.remaining.height,
            };
            i += 1;
        }
        out
    }

    pub const fn rows<const N: usize>(self) -> [Rect; N] {
        let mut out = [Rect {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        }; N];
        let each = if N == 0 {
            0
        } else {
            self.remaining.height / N as u16
        };
        let mut i = 0usize;
        while i < N {
            let y = self.remaining.y + (each as i16 * i as i16);
            let height = if i + 1 == N {
                self.remaining.height.saturating_sub(each * (N as u16 - 1))
            } else {
                each
            };
            out[i] = Rect {
                x: self.remaining.x,
                y,
                width: self.remaining.width,
                height,
            };
            i += 1;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_splits() {
        let screen = Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 80,
        };
        let (header, builder) = LayoutBuilder::new(screen).header(10);
        assert_eq!(header.height, 10);
        let cols = builder.columns::<2>();
        assert_eq!(cols[0].width + cols[1].width, 100);
    }
}
