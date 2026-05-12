//! Same widget tree as `pan_zoom`, but the tab self-drives synthetic
//! scroll + zoom input every frame — the exact oscillator
//! `benches/scrollzoom.rs` uses. Lets you watch the bench's workload
//! animate. Pointer is seeded over the viewport on the first frame
//! only so `scroll_target` latches without clobbering the real cursor
//! on subsequent frames (otherwise tab-bar clicks miss). Frame counter
//! lives in `Ui::state` (rebuilt-arena safe); continuous repaint comes
//! from `ui.request_repaint()` each frame so the host keeps scheduling
//! the next one.

use glam::Vec2;
use palantir::{InputEvent, Ui, WidgetId};

pub const NAME: &str = "pan+zoom auto";

pub fn build(ui: &mut Ui) {
    let id = WidgetId::from_hash("pz-auto-tick");
    let frame = ui.state_mut::<u32>(id);
    let i = *frame;
    *frame = frame.wrapping_add(1);

    // Seed the pointer over the scroll viewport on the first frame
    // only — enough to latch scroll_target. Re-injecting every frame
    // would clobber the real cursor and break clicks on the tab bar.
    if i == 0 {
        let size = ui.display().logical_size();
        let centre = Vec2::new(size.w * 0.5, size.h * 0.6);
        ui.on_input(InputEvent::PointerMoved(centre));
    }

    let t = i as f32 * 0.05;
    ui.on_input(InputEvent::Scroll(Vec2::new(
        t.cos() * 5.0,
        (t * 0.7).cos() * 5.0,
    )));
    ui.on_input(InputEvent::Zoom(1.0 + t.cos() * 0.02));

    ui.request_repaint();

    crate::showcase::complex_pan_zoom::build(ui);
}
