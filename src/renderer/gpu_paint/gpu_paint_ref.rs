//! The shared handle to an app's `GpuPaint` callback.

use crate::renderer::gpu_paint::GpuPaint;
use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

/// The app's `GpuPaint` callback on its way to the backend. A wrapper so structs carrying it keep `derive(Debug)`; clone is an `Rc` bump.
#[derive(Clone)]
pub(crate) struct GpuPaintRef(pub(crate) Rc<RefCell<dyn GpuPaint>>);

impl fmt::Debug for GpuPaintRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("GpuPaint")
    }
}

/// Identity equality: `dyn GpuPaint` has no equality of its own.
impl PartialEq for GpuPaintRef {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::gpu::device::gpu_frame_context::GpuFrameContext;
    use crate::renderer::gpu_paint::GpuPaint;
    use crate::renderer::gpu_paint::gpu_paint_ref::GpuPaintRef;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// A paint that draws nothing.
    #[derive(Debug)]
    pub(crate) struct NoopPaint;

    impl GpuPaint for NoopPaint {
        fn paint(&mut self, _ctx: &mut GpuFrameContext<'_>) {}
    }

    impl GpuPaintRef {
        pub(crate) fn noop() -> Self {
            Self(Rc::new(RefCell::new(NoopPaint)))
        }
    }
}
