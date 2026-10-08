//! `GpuView` fixture: an app `GpuPaint` callback renders into the framework-owned target, composited through the image pipeline.

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
use palantir::{Configure, GpuFrameContext, GpuPaint, GpuView, Panel, Sizing, TranslateScale};

use crate::fixtures::{SRGB_ROUND_TRIP, assert_px};
use crate::harness::Harness;

/// A `GpuPaint` with a real pipeline, depth attachment and draw (as the `cube` showcase), so wgpu validation sees what an app's paint does.
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

impl DepthTriangle {
    /// Draws over a viewport of `logical_square` logical px when set, else the whole target.
    const fn new(logical_square: Option<f32>) -> Self {
        Self {
            pipeline: None,
            depth: None,
            depth_size: UVec2::ZERO,
            logical_square,
            last_size: UVec2::ZERO,
            last_display_scale: 0.0,
            last_raster_scale: 0.0,
        }
    }
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
    fn init(&mut self, ctx: &palantir::GpuInitContext<'_>) {
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

    fn paint(&mut self, ctx: &mut GpuFrameContext<'_>) {
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
                    // Clear the whole capacity target to BLUE; slack outside `physical_size` must not show in the composite.
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

/// A full-surface `GpuView` reaches the screen, and the √2 capacity ladder's UV crop holds. A 64×64 view gets a 67×67 texture, so 3px of BLUE slack the renderer never touches; the composite must sample `used/capacity` and read green throughout.
#[test]
fn gpu_view_pipeline_depth_and_capacity_crop() {
    let paint = Rc::new(RefCell::new(DepthTriangle::new(None)));
    let img = Harness::new()
        .size(UVec2::new(64, 64))
        .frame(|ui| {
            // Default sizing fills the surface.
            GpuView::new(&paint).show(ui);
        })
        .image;
    // (63,63) discriminates: a full [0,1] UV would sample blue slack at texel ≈66.
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
    let paint = Rc::new(RefCell::new(DepthTriangle::new(Some(16.0))));
    let img = Harness::new()
        .size(UVec2::new(96, 96))
        .scale(2.0)
        .frame(|ui| {
            Panel::zstack()
                .auto_id()
                .size((Sizing::fixed(32.0), Sizing::fixed(32.0)))
                .transform(TranslateScale::from_scale(1.5))
                .show(ui, |ui| {
                    GpuView::new(&paint).show(ui);
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
