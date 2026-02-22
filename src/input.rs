//! Generic button input abstraction.

/// Logical controls exposed by the runtime.
///
/// `Aux1..Aux3` are intentionally generic escape hatches for
/// device-specific buttons without coupling this crate to any board layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
    Up,
    Down,
    Confirm,
    Back,
    Aux1,
    Aux2,
    Aux3,
}

/// Input events
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputEvent {
    Press(Button),
}
