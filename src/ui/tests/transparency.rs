//! Reduced transparency, as the system asks or a preference picks: every pane paints solid.

use glam::UVec2;

use crate::host::harness::Harness;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::backdrop_filter::BackdropFilter;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::reduce_preference::ReducePreference;
use crate::scene::layer::Layer;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::panel::Panel;

const SURFACE: UVec2 = UVec2::new(200, 100);

/// A frosted panel paints its filter under full transparency. The system's ask records the next
/// frame again with the pane solid, a pick of full over that ask brings the filter back, and a
/// pick of reduced over a system that asks nothing takes it away again.
#[test]
fn reduced_transparency_records_every_pane_solid() {
    let id = WidgetId::from_hash("pane");
    let pane = Chrome::fill(RgbaF32::hex(0x204060).with_alpha8(184))
        .with_backdrop_filter(BackdropFilter::frosted(20.0));
    let record = |ui: &mut Ui| {
        Panel::vstack()
            .id(id)
            .size((Sizing::fixed(40.0), Sizing::fixed(20.0)))
            .chrome(pane.clone())
            .show(ui, |_| {});
    };
    let filtered = |h: &Harness| {
        let node = h.node_of(id).expect("the pane recorded").node;
        let row = h.ui().tree(Layer::Main).chrome(node).expect("a chrome row");
        row.backdrop.is_some()
    };
    let mut h = Harness::mono(SURFACE);
    h.prime(2, record);
    assert!(filtered(&h), "full transparency");
    assert!(!h.ui().is_transparency_reduced());

    h.set_system_transparency_reduced(true);
    h.frame(record);
    assert!(h.ui().is_transparency_reduced());
    assert!(h.ui().system_transparency_reduced());
    assert!(!filtered(&h), "the system's ask");

    h.ui_mut()
        .set_transparency_preference(ReducePreference::Full);
    h.frame(record);
    assert!(!h.ui().is_transparency_reduced());
    assert!(filtered(&h), "a pick of full over the system's ask");

    h.set_system_transparency_reduced(false);
    h.ui_mut()
        .set_transparency_preference(ReducePreference::Reduced);
    h.frame(record);
    assert_eq!(h.ui().transparency_preference(), ReducePreference::Reduced);
    assert!(!filtered(&h), "a pick of reduced");
}
