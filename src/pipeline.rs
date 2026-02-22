//! End-to-end command recording, diffing, and refresh execution.

use crate::refresh::{EinkDisplay, RefreshHint, RefreshMode, RefreshScheduler};
use crate::render_ir::{CmdBuffer, DrawCmd, diff_cmd_buffers};
use crate::ui::runtime::UiRuntime;

/// Frame pipeline with previous-frame command history.
pub struct FramePipeline<const N: usize, const PREV: usize> {
    current: CmdBuffer<'static, N>,
    previous: CmdBuffer<'static, PREV>,
    scheduler: RefreshScheduler,
}

impl<const N: usize, const PREV: usize> FramePipeline<N, PREV> {
    pub const fn new(partial_limit: u8) -> Self {
        Self {
            current: CmdBuffer::new(),
            previous: CmdBuffer::new(),
            scheduler: RefreshScheduler::new(partial_limit),
        }
    }

    pub fn begin_frame(&mut self) -> UiRuntime<'_, N> {
        self.current.clear();
        UiRuntime::new(&mut self.current, 480)
    }

    pub fn end_frame<D: EinkDisplay>(
        &mut self,
        display: &mut D,
        hint: RefreshHint,
    ) -> Result<RefreshMode, D::Error> {
        self.scheduler.set_hint(hint);
        diff_cmd_buffers(&self.previous, &self.current, self.scheduler.tracker_mut());
        let mode = self.scheduler.flush(display, &self.current)?;
        self.swap_history();
        Ok(mode)
    }

    pub fn current_commands(&self) -> &[DrawCmd<'static>] {
        self.current.as_slice()
    }

    fn swap_history(&mut self) {
        self.previous.clear();
        for (idx, cmd) in self.current.as_slice().iter().enumerate() {
            if idx >= PREV {
                break;
            }
            let Some(region) = self
                .current
                .regions()
                .iter()
                .find_map(|(rect, cmd_idx)| if *cmd_idx == idx { Some(*rect) } else { None })
            else {
                continue;
            };
            let _ = self.previous.push(cmd.clone(), region);
        }
    }
}

impl<const N: usize, const PREV: usize> Default for FramePipeline<N, PREV> {
    fn default() -> Self {
        Self::new(6)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Rect;
    use crate::dsl::UiDsl;

    struct MockDisplay {
        full: usize,
        partial: usize,
        fast: usize,
    }

    impl EinkDisplay for MockDisplay {
        type Error = core::convert::Infallible;
        fn full_refresh(&mut self) -> Result<(), Self::Error> {
            self.full += 1;
            Ok(())
        }
        fn partial_refresh(&mut self, _region: Rect) -> Result<(), Self::Error> {
            self.partial += 1;
            Ok(())
        }
        fn fast_refresh(&mut self, _region: Rect) -> Result<(), Self::Error> {
            self.fast += 1;
            Ok(())
        }
        fn width(&self) -> u16 {
            480
        }
        fn height(&self) -> u16 {
            800
        }
    }

    #[test]
    fn frame_pipeline_records_and_flushes() {
        let mut pipeline: FramePipeline<64, 64> = FramePipeline::new(4);
        {
            let mut ui = pipeline.begin_frame();
            ui.label("hello");
        }
        let mut display = MockDisplay {
            full: 0,
            partial: 0,
            fast: 0,
        };
        let mode = pipeline
            .end_frame(&mut display, RefreshHint::Partial)
            .expect("flush");
        assert!(matches!(mode, RefreshMode::Partial { .. }));
        assert!(display.partial > 0);
    }
}
