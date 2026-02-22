//! Declarative UI DSL hooks used by the `ui!` macro.

extern crate alloc;

/// Shared options for stack-like containers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StackOpts {
    pub gap: u16,
    pub pad: u16,
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
}

#[cfg(test)]
mod tests {
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
                "vstack:end",
            ]
        );
    }
}
