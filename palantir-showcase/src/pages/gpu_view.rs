//! Raw-wgpu demo: a slowly auto-rotating colored cube rendered through a [`GpuView`]; drag to orbit. [`CubeRenderer`] holds the wgpu, and this page its turn and continuous repaint.

use std::cell::RefCell;
use std::rc::Rc;

use crate::cube_renderer::CubeRenderer;
use crate::scaffold::api;
use crate::support;
use crate::support::{Kit, Section};
use palantir::glam::Mat4;
use palantir::{
    Configure, GpuFrameCtx, GpuInitCtx, GpuPaint, GpuView, Sense, SizeSpec, Switch, Ui, WidgetId,
};

/// The app's persistent renderer; `'static` so it can live behind the `Rc<RefCell<…>>` the framework holds.
#[derive(Debug)]
struct Cube {
    renderer: CubeRenderer,
    /// Whether the cube turns on its own; off at first, so a view at rest asks for no frame.
    spinning: bool,
    spin: f32,
    yaw: f32,
    pitch: f32,
}

impl Cube {
    fn new() -> Self {
        Self {
            renderer: CubeRenderer::default(),
            spinning: false,
            spin: 0.0,
            yaw: 0.6,
            pitch: 0.5,
        }
    }

    fn orbit(&mut self, dx: f32, dy: f32) {
        self.yaw += dx * 0.01;
        self.pitch = (self.pitch + dy * 0.01).clamp(-1.5, 1.5);
    }
}

impl GpuPaint for Cube {
    fn init(&mut self, ctx: &GpuInitCtx<'_>) {
        self.renderer.init(ctx);
    }

    fn paint(&mut self, ctx: &mut GpuFrameCtx<'_>) {
        // Slow auto-rotation (~34°/s), framerate-independent via real `dt` (ZERO on the first paint).
        if self.spinning {
            self.spin += 0.6 * ctx.dt.as_secs_f32();
        }
        let model = Mat4::from_rotation_y(self.spin + self.yaw) * Mat4::from_rotation_x(self.pitch);
        self.renderer.paint(ctx, model);
    }
}

/// Showcase page. The cube lives in a state row under an id no node records, so it lasts as long as the `Ui`.
pub(crate) fn build(ui: &mut Ui, k: Kit) {
    let id = WidgetId::from_hash("showcase::gpu_view::cube");
    ui.with_state_or_insert_with(
        id,
        |_| Rc::new(RefCell::new(Cube::new())),
        |ui, cube| view(ui, k, cube),
    );
}

fn view(ui: &mut Ui, k: Kit, cube: &Rc<RefCell<Cube>>) {
    Section::new("Cube")
        .apis(&[api!(type GpuView), api!(type GpuPaint)])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "Drawn with raw wgpu into an off-screen texture the framework owns, then \
             composited as an ordinary image — so clipping, rounded corners, z-order and \
             partial repaint all come for free. It clears to transparent, so the page shows \
             around the cube. Drag inside the view to orbit it.",
            );
            let mut spinning = cube.borrow().spinning;
            Switch::new(&mut spinning).label("Spin").show(ui);
            cube.borrow_mut().spinning = spinning;
        });
    if cube.borrow().spinning {
        // A GpuView re-renders on every painted frame, so keep frames coming to animate the spin.
        ui.request_repaint();
    }

    // GpuView doesn't sense by default: opt into drag so `Response` reports the orbit delta.
    let resp = GpuView::new(cube)
        .sense(Sense::DRAG)
        .size(SizeSpec::FILL)
        .show(ui);
    if let Some(delta) = resp.left.drag.delta() {
        cube.borrow_mut().orbit(delta.x * 0.05, delta.y * 0.05);
    }
}
