use crate::forest::element::{Configure, Element};
use crate::layout::types::sizing::Sizing;
use crate::renderer::gpu_view::GpuPaint;
use crate::ui::Ui;
use crate::widgets::{Response, enter_widget};
use std::cell::RefCell;
use std::rc::Rc;

/// A widget that renders raw `wgpu` content into its rect. App code
/// implements [`GpuPaint`] on its own renderer, wraps it in
/// `Rc<RefCell<…>>`, and hands a clone to [`GpuView::new`] each frame.
/// The framework owns an off-screen texture sized to the widget's rect,
/// runs the callback into it during submit, and composites the result
/// through the image pipeline — so the view clips, rounds, and z-orders
/// like any other widget.
///
/// The renderer persists across frames in the widget's per-`WidgetId`
/// state (the off-screen texture frees automatically when the widget
/// disappears). Per-frame parameters are natural: mutate your own `Rc`
/// before constructing the widget.
///
/// ```ignore
/// let scene = self.scene.clone();          // Rc<RefCell<MyScene>>
/// scene.borrow_mut().camera = self.camera;
/// GpuView::new(scene)
///     .size((Sizing::fill(1.0), Sizing::fill(1.0)))  // Configure::size
///     .show(ui);
/// ```
///
/// Defaults to filling its parent on both axes (a viewport has no
/// intrinsic size); override sizing / id via [`Configure`]. Doesn't sense
/// by default — opt in with [`Configure::sense`] to drive interaction
/// (drag / click) from the returned [`Response`].
pub struct GpuView {
    element: Element,
    paint: Rc<RefCell<dyn GpuPaint>>,
    repaint: bool,
}

impl std::fmt::Debug for GpuView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GpuView")
            .field("element", &self.element)
            .field("repaint", &self.repaint)
            .finish_non_exhaustive()
    }
}

impl GpuView {
    /// New view backed by `paint` (the app's renderer). The framework calls
    /// [`GpuPaint::init`] once (when the device is first available) and
    /// [`GpuPaint::paint`] each painted frame, into an off-screen target
    /// sized to this widget's physical rect.
    #[track_caller]
    pub fn new(paint: Rc<RefCell<dyn GpuPaint>>) -> Self {
        let mut element = Element::leaf();
        element.size = (Sizing::fill(1.0), Sizing::fill(1.0)).into();
        Self {
            element,
            paint,
            repaint: true,
        }
    }

    /// Whether the view's content changed this frame (default `true` —
    /// repaint every frame). Pass `false` when your scene is unchanged: the
    /// widget is then treated as **undamaged**, so a frame forced by other
    /// widgets culls this view and skips its `GpuPaint::paint`, reusing last
    /// frame's pixels. The off-screen texture is kept alive for as long as the
    /// widget is recorded, so a later repaint doesn't re-`init`. Drive the
    /// dirty signal from your own change tracking (camera moved, sim ticked).
    pub fn repaint(mut self, repaint: bool) -> Self {
        self.repaint = repaint;
        self
    }

    /// Record the view. With [`Self::repaint`] left at its `true` default it
    /// re-renders on every painted frame, so call [`Ui::request_repaint`] each
    /// frame to animate.
    pub fn show(self, ui: &mut Ui) -> Response<'_> {
        let Self {
            element,
            paint,
            repaint,
        } = self;
        let entry = enter_widget(ui, &element);
        let id = entry.id;
        ui.node(id, element, None, |ui| {
            ui.gpu_view(id, paint, repaint);
        });
        Response::eager(id, ui, entry.raw)
    }
}

impl Configure for GpuView {
    fn element_mut(&mut self) -> &mut Element {
        &mut self.element
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Ui;
    use crate::forest::element::Configure;
    use crate::forest::layer::Layer;
    use crate::forest::shapes::record::ShapeRecord;
    use crate::input::sense::Sense;
    use crate::layout::types::sizing::Sizing;
    use crate::primitives::widget_id::WidgetId;
    use crate::renderer::gpu_view::GpuFrameCtx;
    use crate::widgets::panel::Panel;
    use glam::{UVec2, Vec2};

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
        let mut ui = Ui::for_test();
        let mut node = None;
        ui.run_at(UVec2::new(200, 120), |ui| {
            Panel::hstack().auto_id().show(ui, |ui| {
                node = Some(
                    GpuView::new(scene())
                        .size((Sizing::fixed(150.0), Sizing::fixed(90.0)))
                        .show(ui)
                        .node(),
                );
            });
        });
        let node = node.unwrap();
        let tree = &ui.forest.trees[Layer::Main];
        let mut shapes = tree.shapes_of(node);
        assert!(
            matches!(shapes.next(), Some(ShapeRecord::GpuView { .. })),
            "records exactly one GpuView shape",
        );
        assert!(shapes.next().is_none());
        let r = ui.layout[Layer::Main].rect[node.idx()];
        assert_eq!((r.size.w, r.size.h), (150.0, 90.0));
    }

    /// Default sizing fills the parent — a viewport has no intrinsic size.
    #[test]
    fn default_fills_parent() {
        let mut ui = Ui::for_test();
        let mut node = None;
        ui.run_at(UVec2::new(160, 100), |ui| {
            node = Some(GpuView::new(scene()).show(ui).node());
        });
        let r = ui.layout[Layer::Main].rect[node.unwrap().idx()];
        assert_eq!((r.size.w, r.size.h), (160.0, 100.0));
    }

    /// Doesn't sense by default, but a caller can opt in via
    /// `Configure::sense` and read clicks off the returned `Response`.
    #[test]
    fn senses_click_when_opted_in() {
        let id = WidgetId::from_hash("gpu_view_hitbox");
        let surface = UVec2::new(200, 100);
        let mut ui = Ui::for_test();
        ui.run_at_acked(surface, |ui| {
            Panel::hstack().auto_id().show(ui, |ui| {
                GpuView::new(scene())
                    .id(id)
                    .sense(Sense::CLICK)
                    .size((Sizing::fixed(100.0), Sizing::fixed(50.0)))
                    .show(ui);
            });
        });
        ui.click_at(Vec2::new(50.0, 25.0));
        let mut clicked = false;
        ui.run_at(surface, |ui| {
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
