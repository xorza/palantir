use std::cell::{Cell, RefCell};
use std::rc::Rc;

use aperture::{
    Color, Configure, GpuFrameCtx, GpuInitCtx, GpuPaint, GpuView, Mesh, Panel, PolylineColors,
    Shape, Spinner, Text, Ui, Vec2, WidgetId,
};
use glam::UVec2;

use crate::fixtures::DARK_BG;
use crate::harness::TwoWindowHarness;

#[derive(Debug)]
struct InitCounter {
    count: Rc<Cell<u32>>,
}

impl GpuPaint for InitCounter {
    fn init(&mut self, _ctx: &GpuInitCtx<'_>) {
        self.count.set(self.count.get() + 1);
    }

    fn paint(&mut self, ctx: &mut GpuFrameCtx<'_>) {
        let _pass = ctx.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("visual.multi_window.gpu_view"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: ctx.target,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
    }
}

fn scene(
    ui: &mut Ui,
    mesh: &Mesh,
    points: &[Vec2],
    colors: &[Color],
    label: &str,
    id: &'static str,
) {
    Panel::zstack()
        .id(WidgetId::from_hash(id))
        .size(96.0)
        .show(ui, |ui| {
            ui.add_shape(Shape::mesh(mesh));
            ui.add_shape(Shape::polyline(
                points,
                PolylineColors::PerPoint(colors),
                3.0,
            ));
            let label = ui.intern(label);
            Text::new(label)
                .id(WidgetId::from_hash((id, "text")))
                .position(Vec2::new(18.0, 38.0))
                .show(ui);
            Spinner::new()
                .id(WidgetId::from_hash((id, "spinner")))
                .diameter(92.0)
                .thickness(2.0)
                .show(ui);
        });
}

/// Window B must not replace the variable-length payloads retained by window
/// A's animation-only frame.
#[test]
fn interleaved_window_paint_only_preserves_pixels() {
    let mut harness = TwoWindowHarness::new();
    let size = UVec2::new(112, 112);

    let mesh_a = Mesh::filled_triangle(
        Vec2::new(12.0, 14.0),
        Vec2::new(72.0, 20.0),
        Vec2::new(26.0, 74.0),
        Color::rgb(0.15, 0.65, 0.95),
    );
    let points_a = [
        Vec2::new(8.0, 82.0),
        Vec2::new(28.0, 10.0),
        Vec2::new(68.0, 84.0),
        Vec2::new(88.0, 12.0),
    ];
    let colors_a = [
        Color::rgb(1.0, 0.2, 0.2),
        Color::rgb(1.0, 0.8, 0.2),
        Color::rgb(0.2, 0.9, 0.4),
        Color::rgb(0.3, 0.5, 1.0),
    ];

    let mesh_b = Mesh::filled_polygon(
        &[
            Vec2::new(78.0, 8.0),
            Vec2::new(90.0, 46.0),
            Vec2::new(58.0, 88.0),
            Vec2::new(14.0, 70.0),
            Vec2::new(8.0, 24.0),
        ],
        Color::rgb(0.9, 0.2, 0.65),
    );
    let points_b = [
        Vec2::new(90.0, 88.0),
        Vec2::new(82.0, 18.0),
        Vec2::new(58.0, 64.0),
        Vec2::new(38.0, 14.0),
        Vec2::new(20.0, 76.0),
        Vec2::new(6.0, 32.0),
    ];
    let colors_b = [
        Color::rgb(0.8, 0.1, 0.8),
        Color::rgb(0.2, 0.8, 0.9),
        Color::rgb(0.9, 0.5, 0.1),
        Color::rgb(0.1, 0.9, 0.3),
        Color::rgb(0.9, 0.1, 0.2),
        Color::rgb(0.4, 0.3, 1.0),
    ];

    let first_a = harness.render(0, size, 1.0, DARK_BG, |ui| {
        scene(ui, &mesh_a, &points_a, &colors_a, "retained A", "window-a");
    });
    let _ = harness.render(1, size, 1.0, DARK_BG, |ui| {
        scene(
            ui,
            &mesh_b,
            &points_b,
            &colors_b,
            "window B has a much longer label",
            "window-b",
        );
    });
    let paint_only_a = harness.render(0, size, 1.0, DARK_BG, |ui| {
        scene(ui, &mesh_a, &points_a, &colors_a, "retained A", "window-a");
    });

    assert_eq!(paint_only_a, first_a);
}

#[test]
fn interleaved_windows_retain_their_own_gpu_view_targets() {
    let mut harness = TwoWindowHarness::new();
    let size = UVec2::new(32, 32);
    let a_inits = Rc::new(Cell::new(0));
    let b_inits = Rc::new(Cell::new(0));
    let a: Rc<RefCell<dyn GpuPaint>> = Rc::new(RefCell::new(InitCounter {
        count: a_inits.clone(),
    }));
    let b: Rc<RefCell<dyn GpuPaint>> = Rc::new(RefCell::new(InitCounter {
        count: b_inits.clone(),
    }));

    let _ = harness.render(0, size, 1.0, DARK_BG, |ui| {
        GpuView::new(a.clone())
            .id(WidgetId::from_hash("gpu-a"))
            .show(ui);
    });
    let _ = harness.render(1, size, 1.0, DARK_BG, |ui| {
        GpuView::new(b.clone())
            .id(WidgetId::from_hash("gpu-b"))
            .show(ui);
    });
    let _ = harness.render(0, size, 1.0, DARK_BG, |ui| {
        GpuView::new(a.clone())
            .id(WidgetId::from_hash("gpu-a"))
            .show(ui);
    });

    assert_eq!(a_inits.get(), 1, "window B evicted window A's target");
    assert_eq!(b_inits.get(), 1);
}
