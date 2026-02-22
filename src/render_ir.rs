//! Render IR, fixed command buffers, and dirty tracking.

use heapless::Vec;

use crate::core::{Color, Point, Rect, TextStyle};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ImageFormat {
    Mono1bpp,
    Gray8,
}

#[derive(Clone, Debug, PartialEq)]
pub enum DrawCmd<'a> {
    FillRect {
        rect: Rect,
        color: Color,
    },
    DrawLine {
        start: Point,
        end: Point,
        color: Color,
        width: u8,
    },
    DrawText {
        pos: Point,
        text: &'a str,
        style: TextStyle,
    },
    DrawImage {
        rect: Rect,
        data: &'a [u8],
        format: ImageFormat,
    },
    Clip {
        rect: Rect,
    },
    Unclip,
}

/// Fixed-size frame command buffer.
pub struct CmdBuffer<'a, const N: usize> {
    cmds: Vec<DrawCmd<'a>, N>,
    regions: Vec<(Rect, usize), N>,
}

impl<'a, const N: usize> CmdBuffer<'a, N> {
    pub const fn new() -> Self {
        Self {
            cmds: Vec::new(),
            regions: Vec::new(),
        }
    }

    pub fn push(&mut self, cmd: DrawCmd<'a>, region: Rect) -> Result<(), DrawCmd<'a>> {
        let idx = self.cmds.len();
        if self.cmds.push(cmd.clone()).is_err() {
            return Err(cmd);
        }
        if self.regions.push((region, idx)).is_err() {
            let _ = self.cmds.pop();
            return Err(cmd);
        }
        Ok(())
    }

    pub fn as_slice(&self) -> &[DrawCmd<'a>] {
        &self.cmds
    }

    pub fn regions(&self) -> &[(Rect, usize)] {
        &self.regions
    }

    pub fn clear(&mut self) {
        self.cmds.clear();
        self.regions.clear();
    }
}

impl<'a, const N: usize> Default for CmdBuffer<'a, N> {
    fn default() -> Self {
        Self::new()
    }
}

/// Mark dirty regions by diffing consecutive command buffers.
pub fn diff_cmd_buffers<'a, 'b, const A: usize, const B: usize>(
    previous: &CmdBuffer<'a, A>,
    current: &CmdBuffer<'b, B>,
    tracker: &mut DirtyTracker,
) {
    let prev_cmds = previous.as_slice();
    let cur_cmds = current.as_slice();
    let max = if prev_cmds.len() > cur_cmds.len() {
        prev_cmds.len()
    } else {
        cur_cmds.len()
    };

    let mut i = 0usize;
    while i < max {
        let changed = match (prev_cmds.get(i), cur_cmds.get(i)) {
            (Some(a), Some(b)) => a != b,
            (Some(_), None) | (None, Some(_)) => true,
            (None, None) => false,
        };
        if changed
            && let Some(region) = region_for_cmd_index(current.regions(), i)
                .or_else(|| region_for_cmd_index(previous.regions(), i))
        {
            tracker.mark_dirty(region, false);
        }
        i += 1;
    }
    tracker.coalesce();
}

fn region_for_cmd_index(regions: &[(Rect, usize)], index: usize) -> Option<Rect> {
    for (rect, cmd_idx) in regions {
        if *cmd_idx == index {
            return Some(*rect);
        }
    }
    None
}

/// Coalesced dirty-state between frames.
pub struct DirtyTracker {
    dirty: Vec<Rect, 16>,
    needs_full: bool,
    partial_count: u8,
    partial_limit: u8,
}

impl DirtyTracker {
    pub const fn new(partial_limit: u8) -> Self {
        Self {
            dirty: Vec::new(),
            needs_full: false,
            partial_count: 0,
            partial_limit,
        }
    }

    pub fn mark_dirty(&mut self, rect: Rect, requires_full: bool) {
        if requires_full {
            self.needs_full = true;
        }
        if self.dirty.push(rect).is_err() {
            self.needs_full = true;
        }
    }

    pub fn coalesce(&mut self) {
        if self.dirty.len() < 2 {
            return;
        }
        let mut merged = self.dirty[0];
        let mut i = 1usize;
        while i < self.dirty.len() {
            merged = merged.union(self.dirty[i]);
            i += 1;
        }
        self.dirty.clear();
        let _ = self.dirty.push(merged);
    }

    pub fn is_clean(&self) -> bool {
        self.dirty.is_empty() && !self.needs_full
    }

    pub fn should_full_refresh(&self) -> bool {
        self.needs_full || self.partial_count >= self.partial_limit
    }

    pub fn dirty_regions(&self) -> &[Rect] {
        &self.dirty
    }

    pub fn on_partial_flush(&mut self) {
        self.partial_count = self.partial_count.saturating_add(1);
    }

    pub fn reset_after_full(&mut self) {
        self.partial_count = 0;
        self.reset();
    }

    pub fn reset(&mut self) {
        self.dirty.clear();
        self.needs_full = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dirty_tracker_coalesces() {
        let mut tracker = DirtyTracker::new(3);
        tracker.mark_dirty(
            Rect {
                x: 0,
                y: 0,
                width: 10,
                height: 10,
            },
            false,
        );
        tracker.mark_dirty(
            Rect {
                x: 10,
                y: 0,
                width: 10,
                height: 10,
            },
            false,
        );
        tracker.coalesce();
        assert_eq!(tracker.dirty_regions().len(), 1);
    }
}
