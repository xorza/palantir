//! Same widget tree as `pan_zoom`, but the tab self-drives synthetic
//! pointer + scroll + zoom input every frame — the exact oscillator
//! `benches/scrollzoom.rs` uses. Lets you watch the bench's workload
//! animate. Frame counter lives in `Ui::state` (rebuilt-arena safe);
//! continuous repaint comes from an animation whose target moves
//! every frame, so `repaint_requested` stays armed without any host
//! cooperation.

use glam::Vec2;
use palantir::{AnimSpec, InputEvent, Ui, WidgetId};

pub const NAME: &str = "pan+zoom auto";

pub fn build(ui: &mut Ui) {
    let id = WidgetId::from_hash("pz-auto-tick");
    let frame = ui.state_mut::<u32>(id);
    let i = *frame;
    *frame = frame.wrapping_add(1);

    let size = ui.display().logical_size();
    // Centre horizontally; bias vertically to land below the toolbar
    // + page header text and well inside the scroll viewport so the
    // Scroll widget latches as the scroll-target hit.
    let centre = Vec2::new(size.w * 0.5, size.h * 0.6);
    ui.on_input(InputEvent::PointerMoved(centre));

    let t = i as f32 * 0.05;
    ui.on_input(InputEvent::Scroll(Vec2::new(
        t.cos() * 5.0,
        (t * 0.7).cos() * 5.0,
    )));
    ui.on_input(InputEvent::Zoom(1.0 + t.cos() * 0.02));

    // Moving target → spring never settles → `repaint_requested` stays
    // true → host runs the next frame. Discarded value.
    let _ = ui.animate(id, "tick", t.sin(), Some(AnimSpec::FAST));

    crate::pan_zoom::build(ui);
}
