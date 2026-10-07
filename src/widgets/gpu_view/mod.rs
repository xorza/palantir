//! The escape hatch to raw `wgpu`: a widget whose rect an app paints itself,
//! into a texture the encoder composites like any image.

use crate::primitives::layout::sizing::Sizing;
use crate::renderer::gpu_paint::GpuPaint;
use crate::renderer::gpu_paint::gpu_paint_ref::GpuPaintRef;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widget_core::configure::ConfigureWidget;
use crate::widget_core::response::Response;
use crate::widget_core::widget::Widget;
use std::cell::RefCell;
use std::rc::Rc;

/// A widget that renders raw `wgpu` content into its rect. App code implements
/// [`GpuPaint`] on its own renderer, keeps it in `Rc<RefCell<...>>`, and lends
/// that handle to [`GpuView::new`] each frame. The framework owns an off-screen
/// texture sized to the composed physical rect (uniformly downsampled at the
/// device texture cap), runs the callback into it during submit, and composites
/// it through the image pipeline, so the view clips, rounds and z-orders like
/// any widget.
///
/// The callback and texture persist while the widget keeps being recorded, so
/// [`GpuPaint::init`] runs once per view. Mutate your own `Rc` before
/// constructing the widget for per-frame parameters.
///
/// ```
/// # use std::{cell::RefCell, rc::Rc};
/// # use palantir::{Configure, GpuFrameContext, GpuPaint, GpuView, Sizing, Ui};
/// # struct MyScene { camera: [f32; 3] }
/// # impl GpuPaint for MyScene {
/// #     fn paint(&mut self, _context: &mut GpuFrameContext<'_>) {}
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
/// Fills its parent on both axes by default (no intrinsic size); override via
/// [`Configure`]. Doesn't sense by default; opt in with [`Configure::sense`] to
/// drive interaction from the returned [`Response`].
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct GpuView {
    widget: Widget,
    /// Wrapped so a struct holding a `dyn GpuPaint` can still derive `Debug`:
    /// [`GpuPaintRef`] exists to avoid hand-writing that impl.
    paint: GpuPaintRef,
    repaint: bool,
}

impl GpuView {
    /// New view backed by `paint` (the app's renderer). The framework calls
    /// [`GpuPaint::init`] once when the device is first available and
    /// [`GpuPaint::paint`] each painted frame, into an off-screen target at this
    /// widget's effective raster resolution.
    ///
    /// Borrowed and generic over the renderer's type, so the caller never writes
    /// `dyn GpuPaint`: the refcount bump and type erasure happen here.
    #[track_caller]
    pub fn new<T: GpuPaint + 'static>(paint: &Rc<RefCell<T>>) -> Self {
        let paint: Rc<RefCell<T>> = Rc::clone(paint);
        Self {
            widget: Widget::leaf().size((Sizing::fill(1.0), Sizing::fill(1.0))),
            paint: GpuPaintRef(paint),
            repaint: true,
        }
    }

    /// Whether the view's content changed this frame (default `true`: repaint every
    /// frame). With `false` the widget is **undamaged**: a frame forced by other
    /// widgets leaves its surface pixels alone and skips `GpuPaint::paint`; damage
    /// crossing the view recomposites its off-screen texture, still without
    /// `paint`, unless size or scale changed.
    ///
    /// Purely a saving: the view keeps its texture (retention follows what the
    /// frame *recorded*), so sitting a frame out doesn't re-run [`GpuPaint::init`].
    /// Drive it from your own change tracking.
    pub const fn repaint(mut self, repaint: bool) -> Self {
        self.repaint = repaint;
        self
    }

    /// Record the view. With [`Self::repaint`] at its `true` default it re-renders on
    /// every painted frame; call [`Ui::request_repaint`] each frame to animate.
    pub fn show(self, ui: &mut Ui) -> Response<'_> {
        let Self {
            mut widget,
            paint,
            repaint,
        } = self;
        let id = widget.resolve(ui);
        widget
            .show(ui, None, |ui| ui.gpu_view(id, paint, repaint))
            .response
    }
}

impl Configure for GpuView {
    #[inline]
    fn configure(&mut self) -> ConfigureWidget<'_> {
        self.widget.configure()
    }
}

#[cfg(test)]
mod tests;
