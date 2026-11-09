use super::*;
use crate::host::harness::Harness;

use crate::damage::Damage;
use crate::damage::region::DamageRegion;
use crate::gpu::gpu_paint::gpu_paint_ref::internals::NoopPaint;
use crate::image_registry::texture_limit::TextureLimit;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::input::sense::Sense;
use crate::primitives::layout::align::{Align, HAlign, VAlign};
use crate::renderer::frontend::{BuildPlan, Frontend};
use crate::renderer::render_plan::RenderPlan;
use crate::scene::layer::Layer;
use crate::shape::paint::image_source::ImageSource;
use crate::shape::record::ShapeRecord;
use crate::widget_core::configure::ThemeDefaults;
use crate::widgets::panel::Panel;
use glam::{UVec2, Vec2};
use std::num::NonZeroU32;

/// The renderer an application keeps and lends back each frame, one handle since a fresh one is a fresh view with its own target (see `GpuViews::record`).
fn scene() -> Rc<RefCell<NoopPaint>> {
    Rc::new(RefCell::new(NoopPaint))
}

/// Records exactly one `GpuView` shape on its node at the committed size: the GPU-free layout half.
#[test]
fn records_one_gpu_view_shape_at_committed_size() {
    let mut h = Harness::mono(UVec2::new(200, 120));
    let node = h.frame_value(|ui| {
        Panel::hstack()
            .auto_id()
            .show(ui, |ui| {
                GpuView::new(&scene())
                    .size((Sizing::fixed(150.0), Sizing::fixed(90.0)))
                    .show(ui)
                    .node(ui)
            })
            .inner
    });
    let tree = h.ui().tree(Layer::Main);
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
    let r = h.ui().arranged_rect(Layer::Main, node);
    assert_eq!((r.size.w, r.size.h), (150.0, 90.0));
}

/// `repaint(false)` keeps the epoch the view last painted at so the backend skips it; the default re-stamps it every frame. The first frame paints either way (new target).
#[test]
fn repaint_false_holds_the_epoch_of_the_last_paint() {
    let paint = scene();
    for (repaint, want_second) in [(true, 2), (false, 1)] {
        let mut h = Harness::mono(UVec2::new(200, 120));
        let mut epochs = Vec::new();
        for _ in 0..2 {
            let node = h.frame_value(|ui| GpuView::new(&paint).repaint(repaint).show(ui).node(ui));
            let epoch = h
                .ui()
                .tree(Layer::Main)
                .shapes_of(node)
                .find_map(|shape| match shape {
                    ShapeRecord::Image {
                        source: ImageSource::GpuView { epoch, .. },
                        ..
                    } => Some(*epoch),
                    _ => None,
                })
                .expect("the view records its shape");
            epochs.push(epoch);
        }
        assert_eq!(epochs, [1, want_second], "repaint {repaint}");
    }
}

/// Default sizing fills the parent, as a viewport has no intrinsic size; a wrapper's
/// `default_size` replaces it.
#[test]
fn default_fills_parent() {
    let mut h = Harness::mono(UVec2::new(160, 100));
    let node = h.frame_value(|ui| GpuView::new(&scene()).show(ui).node(ui));
    let r = h.ui().arranged_rect(Layer::Main, node);
    assert_eq!((r.size.w, r.size.h), (160.0, 100.0));

    let node = h.frame_value(|ui| GpuView::new(&scene()).default_size(50.0).show(ui).node(ui));
    let r = h.ui().arranged_rect(Layer::Main, node);
    assert_eq!((r.size.w, r.size.h), (50.0, 50.0));
}

/// A device attach moves the `GpuView` ceiling in the next build, as a recovery onto a smaller GPU
/// asks: a 100 px view paints whole under 8192 and at 50 px under 50.
#[test]
fn an_attach_moves_the_view_ceiling_in_the_next_build() {
    let mut h = Harness::mono(UVec2::new(200, 200));
    h.frame(|ui| {
        GpuView::new(&scene())
            .id(WidgetId::from_hash("gpu_view_ceiling"))
            .size((Sizing::fixed(100.0), Sizing::fixed(100.0)))
            .show(ui);
    });
    let plan = RenderPlan {
        clear: h.ui().theme().window_bg,
        damage: Damage::Full,
    };
    let mut frontend = Frontend::for_test();
    let target_size = |frontend: &mut Frontend| {
        frontend.build(h.ui().frame_scene(), BuildPlan::without_backdrops(plan));
        frontend.buffer.frame_targets[0].full
    };
    assert_eq!(target_size(&mut frontend), UVec2::splat(100));
    frontend.attach_device(TextureLimit::from_device(NonZeroU32::new(50).unwrap()));
    assert_eq!(target_size(&mut frontend), UVec2::splat(50));
}

/// Retention keys on what the frame recorded, painting on what it damaged: a view culled from a partial repaint keeps its target and `GpuPaint::init` doesn't re-run. One recorded frame composed twice (a `Full` plan draws it, a distant `Partial` doesn't) must give the same live roster, which the window driver lists from the window's views after each build.
#[test]
fn the_live_roster_lists_a_view_the_damage_plan_culls() {
    let mut h = Harness::mono(UVec2::new(200, 200));
    h.frame(|ui| {
        GpuView::new(&scene())
            .id(WidgetId::from_hash("gpu_view_retained"))
            .size((Sizing::fixed(100.0), Sizing::fixed(100.0)))
            .align(Align::new(HAlign::Right, VAlign::Bottom))
            .show(ui);
    });

    let mut frontend = Frontend::for_test();
    let plan = |damage| RenderPlan {
        clear: h.ui().theme().window_bg,
        damage,
    };

    frontend.build(
        h.frames().ui().frame_scene(),
        BuildPlan::without_backdrops(plan(Damage::Full)),
    );
    h.frames()
        .gpu_views()
        .collect_live_targets(&mut frontend.buffer.live_targets);
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

    // Damage confined to the corner opposite the bottom-right view.
    let elsewhere = DamageRegion::from(Rect::new(0.0, 0.0, 20.0, 20.0)).unmeasured();
    frontend.build(
        h.frames().ui().frame_scene(),
        BuildPlan::without_backdrops(plan(Damage::Partial(elsewhere))),
    );
    h.frames()
        .gpu_views()
        .collect_live_targets(&mut frontend.buffer.live_targets);
    assert!(
        frontend.buffer.frame_targets.is_empty(),
        "the view is outside the damage region, so nothing repaints it",
    );
    assert_eq!(
        frontend.buffer.live_targets, live,
        "but it is still recorded, so its target must not be freed",
    );
}

/// Senses nothing by default; a caller can opt in via `Configure::sense` and read clicks off the `Response`.
#[test]
fn senses_click_when_opted_in() {
    let id = WidgetId::from_hash("gpu_view_hitbox");
    let surface = UVec2::new(200, 100);
    let mut h = Harness::mono(surface);
    let scene = scene();
    h.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            GpuView::new(&scene)
                .id(id)
                .sense(Sense::CLICK)
                .size((Sizing::fixed(100.0), Sizing::fixed(50.0)))
                .show(ui);
        });
    });
    h.click(Vec2::new(50.0, 25.0));
    let clicked = h.frame_value(|ui| {
        Panel::hstack()
            .auto_id()
            .show(ui, |ui| {
                GpuView::new(&scene)
                    .id(id)
                    .sense(Sense::CLICK)
                    .size((Sizing::fixed(100.0), Sizing::fixed(50.0)))
                    .show(ui)
                    .left
                    .clicked()
            })
            .inner
    });
    assert!(clicked, "GpuView senses clicks when sense is set");
}
