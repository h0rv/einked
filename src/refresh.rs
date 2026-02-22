//! E-ink refresh policy and scheduler.

use crate::core::Rect;
use crate::render_ir::{CmdBuffer, DirtyTracker};
use heapless::Vec;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RefreshHint {
    Adaptive,
    Full,
    Partial,
    Fast,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RefreshMode {
    Full,
    Partial { regions: Vec<Rect, 16> },
    Fast { regions: Vec<Rect, 16> },
    Skip,
}

pub trait EinkDisplay {
    type Error;
    fn full_refresh(&mut self) -> Result<(), Self::Error>;
    fn partial_refresh(&mut self, region: Rect) -> Result<(), Self::Error>;
    fn fast_refresh(&mut self, region: Rect) -> Result<(), Self::Error>;
    fn width(&self) -> u16;
    fn height(&self) -> u16;
}

pub struct RefreshScheduler {
    tracker: DirtyTracker,
    hint: RefreshHint,
}

impl RefreshScheduler {
    pub const fn new(partial_limit: u8) -> Self {
        Self {
            tracker: DirtyTracker::new(partial_limit),
            hint: RefreshHint::Adaptive,
        }
    }

    pub fn tracker_mut(&mut self) -> &mut DirtyTracker {
        &mut self.tracker
    }

    pub fn set_hint(&mut self, hint: RefreshHint) {
        self.hint = hint;
    }

    pub fn decide(&self) -> RefreshMode {
        if self.tracker.is_clean() {
            RefreshMode::Skip
        } else if self.hint == RefreshHint::Full || self.tracker.should_full_refresh() {
            RefreshMode::Full
        } else {
            let regions = self.snapshot_regions();
            match self.hint {
                RefreshHint::Fast => RefreshMode::Fast { regions },
                _ => RefreshMode::Partial { regions },
            }
        }
    }

    pub fn flush<D: EinkDisplay, const N: usize>(
        &mut self,
        display: &mut D,
        _cmds: &CmdBuffer<'_, N>,
    ) -> Result<RefreshMode, D::Error> {
        let mode = self.decide();
        match &mode {
            RefreshMode::Skip => {}
            RefreshMode::Full => {
                display.full_refresh()?;
                self.tracker.reset_after_full();
            }
            RefreshMode::Partial { regions } => {
                for &region in regions.as_slice() {
                    display.partial_refresh(region)?;
                }
                self.tracker.on_partial_flush();
                self.tracker.reset();
            }
            RefreshMode::Fast { regions } => {
                for &region in regions.as_slice() {
                    display.fast_refresh(region)?;
                }
                self.tracker.on_partial_flush();
                self.tracker.reset();
            }
        }
        Ok(mode)
    }

    fn snapshot_regions(&self) -> Vec<Rect, 16> {
        let mut out = Vec::new();
        for &region in self.tracker.dirty_regions() {
            let _ = out.push(region);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheduler_skips_when_clean() {
        let scheduler = RefreshScheduler::new(5);
        assert!(matches!(scheduler.decide(), RefreshMode::Skip));
    }
}
