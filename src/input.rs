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

/// Button remapping configuration.
///
/// Allows users to customize button behavior for accessibility or preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ButtonConfig {
    /// Swap Left and Right buttons.
    pub swap_left_right: bool,
    /// Swap Up and Down buttons.
    pub swap_up_down: bool,
    /// Use volume buttons (Aux1/Aux2) for page turns instead of scroll.
    /// When enabled: Aux1 = Left (prev page), Aux2 = Right (next page).
    pub volume_for_pages: bool,
}

impl ButtonConfig {
    /// Remap a button press according to the configuration.
    ///
    /// The remapping order is:
    /// 1. Apply volume button mapping (if volume_for_pages is enabled)
    /// 2. Apply left/right swap (if swap_left_right is enabled)
    /// 3. Apply up/down swap (if swap_up_down is enabled)
    pub fn remap(&self, button: Button) -> Button {
        let mapped = if self.volume_for_pages {
            match button {
                Button::Aux1 => Button::Left,
                Button::Aux2 => Button::Right,
                other => other,
            }
        } else {
            button
        };

        let mapped = if self.swap_left_right {
            match mapped {
                Button::Left => Button::Right,
                Button::Right => Button::Left,
                other => other,
            }
        } else {
            mapped
        };

        if self.swap_up_down {
            match mapped {
                Button::Up => Button::Down,
                Button::Down => Button::Up,
                other => other,
            }
        } else {
            mapped
        }
    }

    /// Remap an input event according to the configuration.
    pub fn remap_event(&self, event: InputEvent) -> InputEvent {
        match event {
            InputEvent::Press(button) => InputEvent::Press(self.remap(button)),
        }
    }
}
