//! The escape hatch to raw `wgpu`: a widget whose rect an app paints
//! itself, into a texture the encoder composites like any other image.

use crate::layout::types::sizing::Sizing;
use crate::renderer::gpu_paint::GpuPaint;
use crate::renderer::gpu_paint::gpu_paint_ref::GpuPaintRef;
use crate::scene::node::Node;
use crate::scene::node::configure::Configure;
use crate::ui::Ui;
use crate::widgets::response::Response;
use std::cell::RefCell;
use std::rc::Rc;

/// A widget that renders raw `wgpu` content into its rect. App code
/// implements [`GpuPaint`] on its own renderer, keeps it in
/// `Rc<RefCell<…>>`, and lends that handle to [`GpuView::new`] each frame.
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
/// self.scene.borrow_mut().camera = self.camera;
/// GpuView::new(&self.scene)
///     .size((Sizing::fill(1.0), Sizing::fill(1.0)))  // Configure::size
///     .show(ui);
/// # }
/// # }
/// ```
///
/// Defaults to filling its parent on both axes (a viewport has no
/// intrinsic size); override sizing / id via [`Configure`]. Doesn't sense
/// by default — opt in with [`Configure::sense`] to drive interaction
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
    ///
    /// Borrowed and generic over the renderer's own type, so the caller
    /// keeps the handle it holds across frames and never writes
    /// `dyn GpuPaint`: the refcount bump and the type erasure both happen
    /// here, on the one line that can spell the target type.
    #[track_caller]
    pub fn new<T: GpuPaint + 'static>(paint: &Rc<RefCell<T>>) -> Self {
        let paint: Rc<RefCell<T>> = Rc::clone(paint);
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
mod tests;
