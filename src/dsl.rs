//! Declarative UI DSL hooks used by the `ui!` macro.

extern crate alloc;

/// Shared options for stack-like containers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StackOpts {
    pub gap: u16,
    pub pad: u16,
}

/// Node-level refresh override emitted by the `ui!` macro.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshMode {
    Full,
    Partial,
    Fast,
}

/// Runtime trait that receives commands emitted by the `ui!` macro.
pub trait UiDsl {
    fn vstack<F>(&mut self, opts: StackOpts, f: F)
    where
        F: FnOnce(&mut Self);

    fn hstack<F>(&mut self, opts: StackOpts, f: F)
    where
        F: FnOnce(&mut Self);

    fn label(&mut self, text: impl core::fmt::Display);

    fn spacer(&mut self);

    fn divider(&mut self) {}

    fn status_bar(&mut self, _left: impl core::fmt::Display, _right: impl core::fmt::Display) {}

    fn paragraph(&mut self, text: impl core::fmt::Display) {
        self.label(text);
    }

    fn with_refresh<F>(&mut self, _mode: RefreshMode, f: F)
    where
        F: FnOnce(&mut Self),
    {
        f(self);
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec;
    use alloc::vec::Vec;

    use super::*;
    use crate::ui;

    #[derive(Default)]
    struct TraceUi {
        ops: Vec<&'static str>,
    }

    impl UiDsl for TraceUi {
        fn vstack<F>(&mut self, _opts: StackOpts, f: F)
        where
            F: FnOnce(&mut Self),
        {
            self.ops.push("vstack:start");
            f(self);
            self.ops.push("vstack:end");
        }

        fn hstack<F>(&mut self, _opts: StackOpts, f: F)
        where
            F: FnOnce(&mut Self),
        {
            self.ops.push("hstack:start");
            f(self);
            self.ops.push("hstack:end");
        }

        fn label(&mut self, _text: impl core::fmt::Display) {
            self.ops.push("label");
        }

        fn spacer(&mut self) {
            self.ops.push("spacer");
        }

        fn divider(&mut self) {
            self.ops.push("divider");
        }

        fn status_bar(&mut self, _left: impl core::fmt::Display, _right: impl core::fmt::Display) {
            self.ops.push("status_bar");
        }

        fn paragraph(&mut self, _text: impl core::fmt::Display) {
            self.ops.push("paragraph");
        }

        fn with_refresh<F>(&mut self, mode: RefreshMode, f: F)
        where
            F: FnOnce(&mut Self),
        {
            match mode {
                RefreshMode::Full => self.ops.push("refresh:full:start"),
                RefreshMode::Partial => self.ops.push("refresh:partial:start"),
                RefreshMode::Fast => self.ops.push("refresh:fast:start"),
            }
            f(self);
            self.ops.push("refresh:end");
        }
    }

    #[test]
    fn ui_macro_expands_to_runtime_calls() {
        let mut ui = TraceUi::default();
        ui! {
            VStack gap=8 pad=4 {
                Label("A")
                HStack gap=2 {
                    Spacer
                    Label("B")
                }
                #[refresh = Full]
                Divider
                StatusBar { left: "L", right: "R" }
                #[refresh = Fast]
                Paragraph("Body")
            }
        }

        assert_eq!(
            ui.ops,
            vec![
                "vstack:start",
                "label",
                "hstack:start",
                "spacer",
                "label",
                "hstack:end",
                "refresh:full:start",
                "divider",
                "refresh:end",
                "status_bar",
                "refresh:fast:start",
                "paragraph",
                "refresh:end",
                "vstack:end",
            ]
        );
    }
}
