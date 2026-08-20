use crate::layout::types::sizing::Sizing;
use crate::renderer::gpu_paint::GpuPaint;
use crate::renderer::gpu_paint::gpu_paint_ref::GpuPaintRef;
use crate::scene::node::{Configure, Node};
use crate::ui::Ui;
use crate::widgets::response::Response;
use std::cell::RefCell;
use std::rc::Rc;

/// A widget that renders raw `wgpu` content into its rect. App code
/// implements [`GpuPaint`] on its own renderer, wraps it in
/// `Rc<RefCell<…>>`, and hands a clone to [`GpuView::new`] each frame.
/// The framework owns an off-screen texture sized to the widget's composed
/// physical rect (uniformly downsampled at the device texture cap), runs the
/// callback into it during submit, and composites the result through the image
/// pipeline — so the view clips, rounds, and z-orders like any other widget.
///
/// Both the renderer callback and the framework-owned off-screen texture
/// persist for as long as the widget keeps being recorded — skipping paints
/// does not cost the texture, so [`GpuPaint::init`] runs once per view.
/// Per-frame parameters are natural: mutate your own `Rc` before constructing
/// the widget.
///
/// ```
/// # use std::{cell::RefCell, rc::Rc};
/// # use palantir::{Configure, GpuFrameCtx, GpuPaint, GpuView, Sizing, Ui};
/// # struct MyScene { camera: [f32; 3] }
/// # impl GpuPaint for MyScene {
/// #     fn paint(&mut self, _ctx: &mut GpuFrameCtx<'_>) {}
/// # }
/// # struct App { scene: Rc<RefCell<MyScene>>, camera: [f32; 3] }
/// # impl App {
/// # fn demo(&mut self, ui: &mut Ui) {
/// let scene = self.scene.clone();          // Rc<RefCell<MyScene>>
/// scene.borrow_mut().camera = self.camera;
/// GpuView::new(scene)
///     .size((Sizing::fill(1.0), Sizing::fill(1.0)))  // Configure::size
///     .show(ui);
/// # }
/// # }
/// ```
///
/// Defaults to filling its parent on both axes (a viewport has no
/// intrinsic size); override sizing / id via [`Configure`](crate::Configure). Doesn't sense
/// by default — opt in with [`Configure::sense`](crate::Configure::sense) to drive interaction
/// (drag / click) from the returned [`Response`].
#[derive(Debug)]
pub struct GpuView {
    node: Node,
    /// Wrapped at construction rather than carried raw: [`GpuPaintRef`]
    /// exists so a struct holding a `dyn GpuPaint` can still derive
    /// `Debug`, and holding the `Rc` directly meant hand-writing the one
    /// impl the wrapper was introduced to remove.
    paint: GpuPaintRef,
    repaint: bool,
}

impl GpuView {
    /// New view backed by `paint` (the app's renderer). The framework calls
    /// [`GpuPaint::init`] once (when the device is first available) and
    /// [`GpuPaint::paint`] each painted frame, into an off-screen target
    /// sized to this widget's effective raster resolution.
    #[track_caller]
    pub fn new(paint: Rc<RefCell<dyn GpuPaint>>) -> Self {
        Self {
            node: Node::leaf().size((Sizing::fill(1.0), Sizing::fill(1.0))),
            paint: GpuPaintRef(paint),
            repaint: true,
        }
    }

    /// Whether the view's content changed this frame (default `true` —
    /// repaint every frame). Pass `false` when your scene is unchanged: the
    /// widget is then treated as **undamaged**, so a frame forced by other
    /// widgets leaves its already-presented surface pixels untouched and skips
    /// `GpuPaint::paint`.
    ///
    /// Purely a saving. The view keeps its off-screen texture — retention
    /// follows what the frame *recorded*, not what it painted — so sitting a
    /// frame out costs nothing and does not re-run [`GpuPaint::init`]. Drive
    /// the dirty signal from your own change tracking (camera moved, sim
    /// ticked); target retention is implemented in
    /// `src/renderer/backend/image_pipeline/render_target.rs`.
    pub fn repaint(mut self, repaint: bool) -> Self {
        self.repaint = repaint;
        self
    }

    /// Record the view. With [`Self::repaint`] left at its `true` default it
    /// re-renders on every painted frame, so call [`Ui::request_repaint`] each
    /// frame to animate.
    pub fn show(self, ui: &mut Ui) -> Response<'_> {
        let Self {
            node,
            paint,
            repaint,
        } = self;
        let widget = ui.widget(node);
        let response = widget.response(ui);
        let id = widget.id();
        widget.record(ui, None, |ui| {
            ui.gpu_view(id, paint, repaint);
        });
        Response::eager(id, ui, response)
    }
}

impl_configure!(GpuView);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::harness::UiHarness;

    use crate::input::sense::Sense;
    use crate::layout::types::align::{Align, HAlign, VAlign};
    use crate::layout::types::sizing::Sizing;
    use crate::primitives::rect::Rect;
    use crate::primitives::widget_id::WidgetId;
    use crate::renderer::frontend::Frontend;
    use crate::renderer::gpu_paint::gpu_frame_ctx::GpuFrameCtx;
    use crate::renderer::render_plan::{RenderKind, RenderPlan};
    use crate::scene::damage::region::DamageRegion;
    use crate::scene::layer::Layer;
    use crate::scene::node::Configure;
    use crate::scene::shapes::paint::ImageSource;
    use crate::scene::shapes::record::ShapeRecord;
    use crate::widgets::panel::Panel;
    use glam::{UVec2, Vec2};

    #[derive(Debug)]
    struct NoopPaint;
    impl GpuPaint for NoopPaint {
        fn paint(&mut self, _ctx: &mut GpuFrameCtx<'_>) {}
    }

    fn scene() -> Rc<RefCell<dyn GpuPaint>> {
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
                    GpuView::new(scene())
                        .size((Sizing::fixed(150.0), Sizing::fixed(90.0)))
                        .show(ui)
                        .node()
                })
                .inner
        });
        let tree = &h.ui.forest.trees[Layer::Main];
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
        let r = h.ui.layout[Layer::Main].rect[node.idx()];
        assert_eq!((r.size.w, r.size.h), (150.0, 90.0));
    }

    /// Default sizing fills the parent — a viewport has no intrinsic size.
    #[test]
    fn default_fills_parent() {
        let mut h = UiHarness::new(UVec2::new(160, 100));
        let node = h.frame_value(|ui| GpuView::new(scene()).show(ui).node());
        let r = h.ui.layout[Layer::Main].rect[node.idx()];
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
            GpuView::new(scene())
                .id(WidgetId::from_hash("gpu_view_retained"))
                .size((Sizing::fixed(100.0), Sizing::fixed(100.0)))
                .align(Align::new(HAlign::Right, VAlign::Bottom))
                .show(ui);
        });

        let mut frontend = Frontend::for_test();
        let plan = |kind| RenderPlan {
            clear: h.ui.theme().window_clear,
            kind,
        };

        frontend.build(h.ui.frame_scene(), plan(RenderKind::Full));
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
        let elsewhere = DamageRegion::from(Rect::new(0.0, 0.0, 20.0, 20.0));
        frontend.build(
            h.ui.frame_scene(),
            plan(RenderKind::Partial { region: elsewhere }),
        );
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
                GpuView::new(scene())
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
                clicked |= GpuView::new(scene())
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
}
