//! Cross-widget integration tests — behavior that spans several widgets
//! or pins a builder-wide contract. Per-widget tests live next to their
//! widget in `widgets/<name>/tests.rs`.

mod drag;
mod size_override;
mod track_caller;
mod visibility;
