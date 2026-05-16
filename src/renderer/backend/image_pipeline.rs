//! GPU side of user images. Mirrors [`super::mesh_pipeline::MeshPipeline`]
//! but draws textured quads — per-instance rect + tint, plus a
//! per-image bind group selected at draw time. The CPU texture bytes
//! live in [`crate::ImageRegistry`]; this module drains its pending
//! list each frame and uploads to GPU, then caches the resulting
//! `GpuImage` by [`ImageHandle`] across frames.

use crate::primitives::image::{Image, ImageHandle, ImageRegistry};
use crate::renderer::render_buffer::ImageInstance;
use rustc_hash::FxHashMap;
use std::rc::Rc;

pub(crate) struct ImagePipeline {
    pipeline: wgpu::RenderPipeline,
    /// Group 0: viewport uniform. Group 1 (texture+sampler) is built
    /// per-image inside [`GpuImage`].
    viewport_bg: wgpu::BindGroup,
    instance_buffer: wgpu::Buffer,
    instance_capacity: usize,
    /// Stencil-test variant — lazy-built when the first rounded-clip
    /// frame uses an image.
    stencil_test: Option<wgpu::RenderPipeline>,
    /// Cached creation inputs needed to lazy-build `stencil_test`.
    shader: wgpu::ShaderModule,
    color_format: wgpu::TextureFormat,
    viewport_bgl: wgpu::BindGroupLayout,
    image_bgl: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// `ImageHandle → GpuImage`. Keyed across frames; the entry is
    /// reused until the user calls `ImageRegistry::unregister` (which
    /// just frees CPU bytes — GPU eviction is a slice-2 concern).
    cache: FxHashMap<ImageHandle, GpuImage>,
}

/// Per-image GPU state. `bind_group` is what the draw call binds to
/// group 1; `view` and `texture` are kept alive alongside it so wgpu
/// validation doesn't reject the bind group on drop.
struct GpuImage {
    #[allow(dead_code)] // owns the GPU texture; bind_group references it
    texture: wgpu::Texture,
    #[allow(dead_code)] // owns the view; bind_group references it
    view: wgpu::TextureView,
    bind_group: wgpu::BindGroup,
}

impl ImagePipeline {
    pub(crate) fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        viewport_buffer: &wgpu::Buffer,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("palantir.image.shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("image.wgsl").into()),
        });

        let viewport_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("palantir.image.viewport.bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let image_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("palantir.image.tex.bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let viewport_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("palantir.image.viewport.bg"),
            layout: &viewport_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: viewport_buffer.as_entire_binding(),
            }],
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("palantir.image.sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("palantir.image.pl"),
            bind_group_layouts: &[Some(&viewport_bgl), Some(&image_bgl)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("palantir.image.pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[instance_layout()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let instance_capacity = 16;
        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("palantir.image.instances"),
            size: (instance_capacity * std::mem::size_of::<ImageInstance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            pipeline,
            viewport_bg,
            instance_buffer,
            instance_capacity,
            stencil_test: None,
            shader,
            color_format: format,
            viewport_bgl,
            image_bgl,
            sampler,
            cache: FxHashMap::default(),
        }
    }

    /// Lazy-build the stencil-test variant for rounded-clip frames.
    /// Idempotent.
    #[profiling::function]
    pub(crate) fn ensure_stencil(&mut self, device: &wgpu::Device) {
        if self.stencil_test.is_some() {
            return;
        }
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("palantir.image.pl.stencil"),
            bind_group_layouts: &[Some(&self.viewport_bgl), Some(&self.image_bgl)],
            immediate_size: 0,
        });
        let pipe = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("palantir.image.pipeline.stencil_test"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &self.shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[instance_layout()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &self.shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: self.color_format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: Some(super::stencil_test_state()),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        self.stencil_test = Some(pipe);
    }

    /// Drain pending images from the registry and upload them to GPU.
    /// Called once per frame from `WgpuBackend::submit` before the
    /// render pass starts. After this returns, every handle the
    /// composer routed into a `DrawImage` is guaranteed to have a
    /// `GpuImage` in the cache (or be missing from the registry, in
    /// which case the draw is silently skipped).
    #[profiling::function]
    pub(crate) fn drain_registry(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        images: &ImageRegistry,
    ) {
        for (handle, image) in images.drain_pending() {
            let gpu = self.upload(device, queue, &image);
            self.cache.insert(handle, gpu);
        }
    }

    fn upload(&self, device: &wgpu::Device, queue: &wgpu::Queue, image: &Rc<Image>) -> GpuImage {
        let size = wgpu::Extent3d {
            width: image.width,
            height: image.height,
            depth_or_array_layers: 1,
        };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("palantir.image.tex"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            // sRGB format: sampler decodes to linear automatically.
            // CPU bytes are sRGB-encoded straight-alpha RGBA8.
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &image.pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(image.width * 4),
                rows_per_image: Some(image.height),
            },
            size,
        );
        let view = texture.create_view(&Default::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("palantir.image.tex.bg"),
            layout: &self.image_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        GpuImage {
            texture,
            view,
            bind_group,
        }
    }

    /// Sync the per-instance buffer. Single contiguous upload — the
    /// schedule slices by batch at draw time.
    #[profiling::function]
    pub(crate) fn upload_instances(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        instances: &[ImageInstance],
    ) {
        if instances.is_empty() {
            return;
        }
        if instances.len() > self.instance_capacity {
            self.instance_capacity = instances.len().next_power_of_two().max(16);
            self.instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("palantir.image.instances"),
                size: (self.instance_capacity * std::mem::size_of::<ImageInstance>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        queue.write_buffer(&self.instance_buffer, 0, bytemuck::cast_slice(instances));
    }

    /// Bind once per pass before iterating image draws.
    #[allow(dead_code)] // wired by schedule (slice 1 Phase 5)
    pub(crate) fn bind<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, stencil: bool) {
        if stencil {
            let p = self.stencil_test.as_ref().expect("ensure_stencil first");
            pass.set_pipeline(p);
        } else {
            pass.set_pipeline(&self.pipeline);
        }
        pass.set_bind_group(0, &self.viewport_bg, &[]);
        pass.set_vertex_buffer(0, self.instance_buffer.slice(..));
    }

    /// Issue one image draw. `instance` indexes into the per-frame
    /// instance buffer. `handle` selects the bind group; missing
    /// entries are silently skipped (the registry lookup raced an
    /// `unregister`).
    #[allow(dead_code)] // wired by schedule (slice 1 Phase 5)
    pub(crate) fn draw<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        handle: ImageHandle,
        instance: u32,
    ) {
        let Some(g) = self.cache.get(&handle) else {
            return;
        };
        pass.set_bind_group(1, &g.bind_group, &[]);
        pass.draw(0..4, instance..instance + 1);
    }
}

const IMAGE_INSTANCE_ATTRS: [wgpu::VertexAttribute; 3] = wgpu::vertex_attr_array![
    0 => Float32x2, // rect.min
    1 => Float32x2, // rect.size
    // `Unorm8x4` normalizes `u8/255 → 0..1`. Tint is linear straight-alpha
    // on the CPU; shader multiplies by the sampled texel and premultiplies
    // at write.
    2 => Unorm8x4,  // tint
];

fn instance_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<ImageInstance>() as u64,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &IMAGE_INSTANCE_ATTRS,
    }
}
