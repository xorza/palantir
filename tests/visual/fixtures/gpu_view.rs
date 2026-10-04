//! `GpuView` fixture: proves the end-to-end pipe — an app `GpuPaint`
//! callback renders into the framework-owned off-screen target, which is
//! then composited into the UI through the image pipeline.

#![expect(
    clippy::cast_sign_loss,
    reason = "test fixtures cast non-negative sizes, coordinates, indices and colour channels"
)]
#![expect(
    clippy::disallowed_types,
    reason = "an outside consumer of the published surface, where naming a wgpu type is the point"
)]

use std::cell::RefCell;
use std::rc::Rc;

use glam::UVec2;
use palantir::{Configure, GpuFrameCtx, GpuPaint, GpuView, Panel, Sizing, TranslateScale};

use crate::fixtures::{SRGB_ROUND_TRIP, assert_px};
use crate::harness::Harness;

/// Clears the off-screen target to opaque red via the app's own render
/// pass on the framework-supplied encoder + target.
#[derive(Debug)]
struct RedClear;

impl GpuPaint for RedClear {
    fn paint(&mut self, ctx: &mut GpuFrameCtx<'_>) {
        let _pass = ctx.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("visual.gpu_view.red_clear"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: ctx.target,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 1.0,
                        g: 0.0,
                        b: 0.0,
                        a: 1.0,
                    }),
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

/// A full-surface `GpuView` whose renderer clears to red must land red on
/// screen. Pure red is gamma-invariant (sRGB encode/decode fixes 0 and 1),
/// so the texture→composite→backbuffer chain round-trips it exactly —
/// no golden needed, the value is hand-known.
#[test]
fn gpu_view_clear_red_reaches_screen() {
    let mut h = Harness::new();
    let size = UVec2::new(64, 64);
    let paint = Rc::new(RefCell::new(RedClear));
    let p = Rc::clone(&paint);
    let img = h
        .size(size)
        .frame(|ui| {
            // Default sizing fills the surface; the whole frame is the view.
            GpuView::new(&p).show(ui);
        })
        .image;

    // Interior samples (skip the 1px edge to dodge boundary AA).
    for y in [16u32, 32, 48] {
        for x in [16u32, 32, 48] {
            assert_px(
                img.get_pixel(x, y).0,
                [255, 0, 0, 255],
                SRGB_ROUND_TRIP,
                format_args!("pixel ({x},{y}) is the GpuView's red"),
            );
        }
    }
}

/// A `GpuPaint` that builds a real render pipeline + depth attachment and
/// draws a fullscreen green triangle — the same GPU surface the `cube`
/// showcase exercises (pipeline, vertex buffer, depth-stencil state,
/// `draw`), minus the matrices. Guards against wgpu-validation regressions
/// in that path, which the clear-only fixture above can't reach.
#[derive(Debug)]
struct DepthTriangle {
    pipeline: Option<wgpu::RenderPipeline>,
    depth: Option<wgpu::TextureView>,
    depth_size: UVec2,
    logical_square: Option<f32>,
    last_size: UVec2,
    last_display_scale: f32,
    last_raster_scale: f32,
}

const TRI_SHADER: &str = r"
@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    // Oversized triangle covering the whole clip space.
    var p = array<vec2<f32>, 3>(vec2(-1.0, -3.0), vec2(-1.0, 1.0), vec2(3.0, 1.0));
    return vec4<f32>(p[i], 0.5, 1.0);
}
@fragment
fn fs() -> @location(0) vec4<f32> {
    return vec4<f32>(0.0, 1.0, 0.0, 1.0);
}
";

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

impl GpuPaint for DepthTriangle {
    fn init(&mut self, ctx: &palantir::GpuInitCtx<'_>) {
        let device = ctx.device;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("visual.gpu_view.tri.shader"),
            source: wgpu::ShaderSource::Wgsl(TRI_SHADER.into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("visual.gpu_view.tri.pl"),
            bind_group_layouts: &[],
            immediate_size: 0,
        });
        self.pipeline = Some(
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("visual.gpu_view.tri.pipeline"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: ctx.target_format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::Less),
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            }),
        );
    }

    fn paint(&mut self, ctx: &mut GpuFrameCtx<'_>) {
        self.last_size = ctx.physical_size;
        self.last_display_scale = ctx.display_scale;
        self.last_raster_scale = ctx.raster_scale;
        // Depth matches the target size (`physical_size`), like the cube.
        if self.depth.is_none() || self.depth_size != ctx.physical_size {
            let tex = ctx.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("visual.gpu_view.tri.depth"),
                size: wgpu::Extent3d {
                    width: ctx.physical_size.x.max(1),
                    height: ctx.physical_size.y.max(1),
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: DEPTH_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            self.depth = Some(tex.create_view(&wgpu::TextureViewDescriptor::default()));
            self.depth_size = ctx.physical_size;
        }
        let mut pass = ctx.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("visual.gpu_view.tri.pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: ctx.target,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    // Clear the whole (capacity) target to BLUE — the slack
                    // outside `physical_size` must NOT show up in the composite.
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLUE),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: self.depth.as_ref().unwrap(),
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        let viewport = self
            .logical_square
            .map_or(ctx.physical_size, |logical_side| {
                UVec2::splat((logical_side * ctx.raster_scale).round().max(1.0) as u32)
                    .min(ctx.physical_size)
            });
        let (w, h) = (viewport.x.max(1), viewport.y.max(1));
        pass.set_viewport(0.0, 0.0, w as f32, h as f32, 0.0, 1.0);
        pass.set_scissor_rect(0, 0, w, h);
        pass.set_pipeline(self.pipeline.as_ref().unwrap());
        pass.draw(0..3, 0..1);
    }
}

/// The pipeline + depth + draw path (what the cube uses), through a
/// GpuView, **and** the √2 capacity ladder's UV crop. A 64×64 view
/// allocates a 67×67 capacity texture (16,23,33,47,67 rungs), so the
/// bottom/right 3px are BLUE slack the renderer never touches. The green
/// triangle fills only the `physical_size` sub-rect; the composite must sample
/// `used/capacity` so the whole 64×64 widget reads green — including the
/// far corner, which would sample blue slack if the crop were wrong.
#[test]
fn gpu_view_pipeline_depth_and_capacity_crop() {
    let mut h = Harness::new();
    let size = UVec2::new(64, 64);
    let paint = Rc::new(RefCell::new(DepthTriangle {
        pipeline: None,
        depth: None,
        depth_size: UVec2::ZERO,
        logical_square: None,
        last_size: UVec2::ZERO,
        last_display_scale: 0.0,
        last_raster_scale: 0.0,
    }));
    let p = Rc::clone(&paint);
    let img = h
        .size(size)
        .frame(|ui| {
            GpuView::new(&p).show(ui);
        })
        .image;
    // (63,63) is the discriminating pixel: with the correct `used/capacity`
    // crop it samples inside the green sub-rect; with a full-[0,1] UV it
    // would sample the blue slack at texel ≈66.
    for &(x, y) in &[(32u32, 32u32), (63, 63), (0, 63), (63, 0)] {
        assert_px(
            img.get_pixel(x, y).0,
            [0, 255, 0, 255],
            SRGB_ROUND_TRIP,
            format_args!("pixel ({x},{y}) is green, not the capacity slack"),
        );
    }
}

#[test]
fn gpu_view_callback_receives_composed_raster_scale() {
    let mut h = Harness::new();
    let size = UVec2::new(96, 96);
    let paint = Rc::new(RefCell::new(DepthTriangle {
        pipeline: None,
        depth: None,
        depth_size: UVec2::ZERO,
        logical_square: Some(16.0),
        last_size: UVec2::ZERO,
        last_display_scale: 0.0,
        last_raster_scale: 0.0,
    }));
    let p = Rc::clone(&paint);
    let img = h
        .size(size)
        .scale(2.0)
        .frame(|ui| {
            Panel::zstack()
                .auto_id()
                .size((Sizing::fixed(32.0), Sizing::fixed(32.0)))
                .transform(TranslateScale::from_scale(1.5))
                .show(ui, |ui| {
                    GpuView::new(&p).show(ui);
                });
        })
        .image;

    assert_eq!(paint.borrow().last_size, UVec2::new(96, 96));
    assert_eq!(paint.borrow().last_display_scale, 2.0);
    assert_eq!(paint.borrow().last_raster_scale, 3.0);

    for &(x, y, expected) in &[(36, 36, [0, 255, 0, 255]), (60, 60, [0, 0, 255, 255])] {
        assert_px(
            img.get_pixel(x, y).0,
            expected,
            SRGB_ROUND_TRIP,
            format_args!("pixel ({x},{y})"),
        );
    }
}
