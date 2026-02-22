//! Minimal, modern UI framework for embedded-graphics targets.
//! E-ink optimized: high contrast, no animations, type-safe.

pub mod activity;
pub mod components;
pub mod helpers;
pub mod runtime;
pub mod theme;

pub use activity::{Activity, ActivityRefreshMode, ActivityResult};
pub use components::{Button, Header, List, Modal, Toast};
pub use theme::{Theme, ThemeMetrics};
