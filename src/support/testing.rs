//! Cross-module test/bench fixtures that have no single natural home.
//! Co-located helpers live in `test_support` mods inside each
//! production file (e.g. `crate::ui::test_support`, `crate::input::test_support`).

#![cfg(any(test, feature = "internals"))]
#![allow(private_interfaces, private_bounds, dead_code)]

use crate::Ui;
use crate::forest::element::Configure;
use crate::forest::tree::NodeId;
use crate::layout::types::sizing::Sizing;
use crate::widgets::panel::Panel;
use glam::UVec2;

/// Wrap UUT inside a Fill HStack so the panel can express its own measured size.
/// Cross-cutting (drives a frame + builds a Panel) so it lives here rather than
/// in either `ui::test_support` or `widgets::panel::test_support`.
#[allow(private_interfaces)]
pub fn under_outer<F: FnMut(&mut Ui) -> NodeId>(ui: &mut Ui, surface: UVec2, mut f: F) -> NodeId {
    let mut inner = None;
    ui.run_at(surface, |ui| {
        Panel::hstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                inner = Some(f(ui));
            });
    });
    inner.unwrap()
}
