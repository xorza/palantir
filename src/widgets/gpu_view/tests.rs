use super::*;
use crate::ui::harness::UiHarness;

use crate::input::sense::Sense;
use crate::layout::types::align::{Align, HAlign, VAlign};
use crate::layout::types::sizing::Sizing;
use crate::primitives::rect::Rect;
use crate::primitives::widget_id::WidgetId;
use crate::renderer::frontend::Frontend;
use crate::renderer::gpu_paint::gpu_frame_ctx::GpuFrameCtx;
use crate::renderer::render_plan::RenderPlan;
use crate::scene::damage::Damage;
use crate::scene::damage::region::DamageRegion;
use crate::scene::layer::Layer;
use crate::scene::node::configure::Configure;
use crate::scene::shapes::paint::ImageSource;
use crate::scene::shapes::record::ShapeRecord;
use crate::widgets::panel::Panel;
use glam::{UVec2, Vec2};

#[derive(Debug)]
struct NoopPaint;
impl GpuPaint for NoopPaint {
    fn paint(&mut self, _ctx: &mut GpuFrameCtx<'_>) {}
}

fn scene() -> Rc<RefCell<NoopPaint>> {
    Rc::new(RefCell::new(NoopPaint))
}

/// Records exactly one `GpuView` shape on its node, arranged at the
/// committed size — the layout half of the widget, GPU-free.
#[test]
fn records_one_gpu_view_shape_at_committed_size() {
    let mut h = UiHarness::new(UVec2::new(200, 120));
    let node = h.frame_value(|ui| {
        Panel::hstack()
            .auto_id()
            .show(ui, |ui| {
                GpuView::new(&scene())
                    .size((Sizing::fixed(150.0), Sizing::fixed(90.0)))
                    .show(ui)
                    .node()
            })
            .inner
    });
    let tree = h.ui.tree(Layer::Main);
    let mut shapes = tree.shapes_of(node);
    assert!(
        matches!(
            shapes.next(),
            Some(ShapeRecord::Image {
                source: ImageSource::GpuView { .. },
                ..
            }),
        ),
        "records exactly one view-sourced image shape",
    );
    assert!(shapes.next().is_none());
    let r = h.ui.arranged_rect(Layer::Main, node);
    assert_eq!((r.size.w, r.size.h), (150.0, 90.0));
}

/// Default sizing fills the parent — a viewport has no intrinsic size.
#[test]
fn default_fills_parent() {
    let mut h = UiHarness::new(UVec2::new(160, 100));
    let node = h.frame_value(|ui| GpuView::new(&scene()).show(ui).node());
    let r = h.ui.arranged_rect(Layer::Main, node);
    assert_eq!((r.size.w, r.size.h), (160.0, 100.0));
}

/// Retention is keyed on what the frame *recorded*, painting on what it
/// *damaged* — so a view culled out of a partial repaint keeps its
/// off-screen target, and `GpuPaint::init` is not re-run when it next
/// paints.
///
/// Composed twice off one recorded frame: a `Full` plan draws the view,
/// a `Partial` plan whose region sits nowhere near it does not. The live
/// roster is the same both times, which is the whole property — it must
/// not move with the damage.
#[test]
fn the_live_roster_lists_a_view_the_damage_plan_culls() {
    let mut h = UiHarness::new(UVec2::new(200, 200));
    h.frame(|ui| {
        GpuView::new(&scene())
            .id(WidgetId::from_hash("gpu_view_retained"))
            .size((Sizing::fixed(100.0), Sizing::fixed(100.0)))
            .align(Align::new(HAlign::Right, VAlign::Bottom))
            .show(ui);
    });

    let mut frontend = Frontend::for_test();
    let plan = |damage| RenderPlan {
        clear: h.ui.theme().window_clear,
        damage,
    };

    frontend.build(h.ui.frame_scene(), plan(Damage::Full));
    assert_eq!(
        frontend.buffer.frame_targets.len(),
        1,
        "a full repaint paints the view",
    );
    let live = frontend.buffer.live_targets.clone();
    assert_eq!(
        live,
        [frontend.buffer.frame_targets[0].id],
        "the roster names the view's target",
    );

    // Damage confined to the opposite corner from the bottom-right view.
    let elsewhere = DamageRegion::from(Rect::new(0.0, 0.0, 20.0, 20.0)).unmeasured();
    frontend.build(h.ui.frame_scene(), plan(Damage::Partial(elsewhere)));
    assert!(
        frontend.buffer.frame_targets.is_empty(),
        "the view is outside the damage region, so nothing repaints it",
    );
    assert_eq!(
        frontend.buffer.live_targets, live,
        "but it is still recorded, so its target must not be freed",
    );
}

/// Doesn't sense by default, but a caller can opt in via
/// `Configure::sense` and read clicks off the returned `Response`.
#[test]
fn senses_click_when_opted_in() {
    let id = WidgetId::from_hash("gpu_view_hitbox");
    let surface = UVec2::new(200, 100);
    let mut h = UiHarness::new(surface);
    h.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            GpuView::new(&scene())
                .id(id)
                .sense(Sense::CLICK)
                .size((Sizing::fixed(100.0), Sizing::fixed(50.0)))
                .show(ui);
        });
    });
    h.click_at(Vec2::new(50.0, 25.0));
    let mut clicked = false;
    h.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            clicked |= GpuView::new(&scene())
                .id(id)
                .sense(Sense::CLICK)
                .size((Sizing::fixed(100.0), Sizing::fixed(50.0)))
                .show(ui)
                .left
                .clicked();
        });
    });
    assert!(clicked, "GpuView senses clicks when sense is set");
}
