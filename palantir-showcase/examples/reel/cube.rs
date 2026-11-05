//! Slide 8's cube: the showcase's renderer, turned from the reel's time.

use std::cell::RefCell;
use std::rc::Rc;

use palantir::glam::Mat4;
use palantir::{GpuFrameCtx, GpuInitCtx, GpuPaint};
use palantir_showcase::cube_renderer::CubeRenderer;

/// The cube; its turn is set from the reel's time, never the frame's own clock, so a render repeats.
#[derive(Debug)]
pub(crate) struct Cube {
    renderer: CubeRenderer,
    /// Radians about the vertical axis.
    pub(crate) angle: f32,
}

/// The cube as `GpuView` holds it.
pub(crate) type CubeHandle = Rc<RefCell<Cube>>;

impl Cube {
    pub(crate) fn handle() -> CubeHandle {
        Rc::new(RefCell::new(Self {
            renderer: CubeRenderer::default(),
            angle: 0.0,
        }))
    }
}

impl GpuPaint for Cube {
    fn init(&mut self, ctx: &GpuInitCtx<'_>) {
        self.renderer.init(ctx);
    }

    fn paint(&mut self, ctx: &mut GpuFrameCtx<'_>) {
        let model = Mat4::from_rotation_y(self.angle + 0.6) * Mat4::from_rotation_x(0.5);
        self.renderer.paint(ctx, model);
    }
}
