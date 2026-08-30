//! The shared handle to an app's `GpuPaint` callback.

use crate::renderer::gpu_paint::GpuPaint;
use std::cell::RefCell;
use std::rc::Rc;

/// The app's `GpuPaint` callback, flowing [`Ui::gpu_views`](crate::ui::Ui) →
/// command-buffer side-list → `RenderBuffer.frame_targets` → backend (the shape
/// itself carries only an epoch). A thin wrapper so the structs that carry it
/// ([`GpuViewEntry`](crate::renderer::gpu_paint::gpu_views::GpuViewEntry),
/// `RenderTargetDraw`) keep their `derive(Debug)` despite
/// `dyn GpuPaint` not being `Debug`. Clone is an `Rc` refcount bump.
#[derive(Clone)]
pub(crate) struct GpuPaintRef(pub(crate) Rc<RefCell<dyn GpuPaint>>);

impl std::fmt::Debug for GpuPaintRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GpuPaint")
    }
}

/// Handle identity, not callback behaviour: two refs are equal when they
/// point at the same `GpuPaint`. `dyn GpuPaint` has no equality of its
/// own, and identity is the only comparison a view's consumers want.
impl PartialEq for GpuPaintRef {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}
