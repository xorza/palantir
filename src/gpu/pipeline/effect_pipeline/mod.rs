//! [`EffectPipeline`]: the frame's effect instances and parameter words, and the draw of each run
//! of one effect and form through that pair's pipeline. The pipelines live in the
//! [`WgpuEffectStore`], which builds them where a use wants them.

pub(crate) mod backdrop_groups;
pub(crate) mod wgpu_effect_store;

use std::array;
use std::mem::offset_of;

use crate::common::span::Span;
use crate::effect::MAX_IMAGE_SLOTS;
use crate::effect::contract::{
    BACKDROPS_GROUP, HALO_OVERRIDE, IMAGES_GROUP, TRIANGLE_OVERRIDE, WORDS_GROUP,
};
use crate::effect::effect_store::EffectModule;
use crate::gpu::device::gpu::Gpu;
use crate::gpu::pipeline::clip_variant::{ClipVariant, Clipping};
use crate::gpu::pipeline::effect_pipeline::backdrop_groups::BackdropGroups;
use crate::gpu::pipeline::effect_pipeline::wgpu_effect_store::WgpuEffectStore;
use crate::gpu::pipeline::pass_recorder::PassRecorder;
use crate::gpu::pipeline::runs::Runs;
use crate::gpu::pipeline::shape_template::{self, TemplateIndices};
use crate::gpu::resource::dynamic_buffer::DynamicBuffer;
use crate::gpu::resource::effect_image_groups::{EffectImageGroups, EffectImages};
use crate::gpu::resource::gpu_ctx::GpuCtx;
use crate::gpu::resource::pipeline_recipe::{self, Pipeline, PipelineRecipe, vertex_attr};
use crate::gpu::resource::texture_binding::TextureBinding;
use crate::gpu::resource::wgpu_image_store::WgpuImageStore;
use crate::primitives::effect::effect_kind::KindId;
use crate::primitives::identity::texture_id::TextureId;
use crate::renderer::render_buffer::effect_draw_row::{EffectInstance, EffectKey};
use crate::renderer::render_buffer::shape_form::ShapeForm;
use crate::renderer::render_buffer::template_cover::TemplateCover;

/// The device effects build on, and the layouts every effect's pipeline builds over: the frame
/// group, which holds the gradient rows a material's box reads, the images at [`IMAGES_GROUP`]
/// for an effect that binds any, a backdrop effect's backdrop results at [`BACKDROPS_GROUP`], the
/// parameter words at [`WORDS_GROUP`], and the immediates every pipeline declares.
#[derive(Debug)]
pub(crate) struct EffectDevice {
    gpu: Gpu,
    /// The parameter words' group layout.
    pub(crate) group: wgpu::BindGroupLayout,
    /// A color effect's, at each count of image slots from 0.
    color: [wgpu::PipelineLayout; MAX_IMAGE_SLOTS as usize + 1],
    /// A backdrop effect's, at each count of image slots from 0.
    backdrop: [wgpu::PipelineLayout; MAX_IMAGE_SLOTS as usize + 1],
}

impl EffectDevice {
    pub(crate) fn new(gpu: &Gpu, binding: &TextureBinding) -> Self {
        let group = gpu
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("palantir.effect.bgl"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });
        let images = |slots: usize| match u8::try_from(slots).unwrap() {
            0 => None,
            slots => Some(binding.images_layout(slots)),
        };
        let color = array::from_fn(|slots| {
            pipeline_recipe::pipeline_layout(
                gpu,
                "palantir.effect.pl",
                &[Some(binding.frame()), images(slots), None, Some(&group)],
            )
        });
        let backdrop = array::from_fn(|slots| {
            pipeline_recipe::pipeline_layout(
                gpu,
                "palantir.effect.backdrop.pl",
                &[
                    Some(binding.frame()),
                    images(slots),
                    Some(binding.backdrops()),
                    Some(&group),
                ],
            )
        });
        Self {
            gpu: gpu.clone(),
            group,
            color,
            backdrop,
        }
    }

    /// The pipeline layout of an effect of `module`'s kind and image slots.
    fn pipeline(&self, module: &EffectModule) -> &wgpu::PipelineLayout {
        let layouts = match module.kind {
            KindId::Color => &self.color,
            KindId::Backdrop => &self.backdrop,
        };
        &layouts[usize::from(module.image_slots)]
    }
}

/// `src·One + dst·Src1` on every channel: the effect's `color` plus what is under it weighted by
/// its `under`, which gives over, add, multiply and screen through one pipeline, with the coverage
/// applied exactly, as the blend is linear in it. Refused: a pipeline per blend mode (Godot's
/// render modes); source-over alone, which sends multiply and screen through a copy, as wgpu 30
/// has no framebuffer fetch (wgpu#10148).
const BLEND: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::Src1,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::Src1,
        operation: wgpu::BlendOperation::Add,
    },
};

/// The effect's header, then its clock `(time, opacity, scale)`, its parameters' first word, its
/// backdrop's row, its flags, a material's border width and a halo's reach.
const INSTANCE_ATTRS: [wgpu::VertexAttribute; 12] = {
    let h = shape_template::HEADER_ATTRS;
    let at = shape_template::HEADER_LOCATIONS;
    [
        h[0],
        h[1],
        h[2],
        h[3],
        h[4],
        h[5],
        vertex_attr(
            at,
            wgpu::VertexFormat::Float32x3,
            offset_of!(EffectInstance, time),
        ),
        vertex_attr(
            at + 1,
            wgpu::VertexFormat::Uint32,
            offset_of!(EffectInstance, params),
        ),
        vertex_attr(
            at + 2,
            wgpu::VertexFormat::Uint32,
            offset_of!(EffectInstance, backdrop_idx),
        ),
        vertex_attr(
            at + 3,
            wgpu::VertexFormat::Uint32,
            offset_of!(EffectInstance, flags),
        ),
        vertex_attr(
            at + 4,
            wgpu::VertexFormat::Float32,
            offset_of!(EffectInstance, border_width),
        ),
        vertex_attr(
            at + 5,
            wgpu::VertexFormat::Float32,
            offset_of!(EffectInstance, reach),
        ),
    ]
};

const _: () = {
    assert!(offset_of!(EffectInstance, header) == 0);
    assert!(
        offset_of!(EffectInstance, opacity) == offset_of!(EffectInstance, time) + size_of::<f32>()
    );
    assert!(
        offset_of!(EffectInstance, scale) == offset_of!(EffectInstance, opacity) + size_of::<f32>()
    );
};

const INSTANCE_LAYOUT: wgpu::VertexBufferLayout<'static> = pipeline_recipe::vertex_layout(
    size_of::<EffectInstance>(),
    wgpu::VertexStepMode::Instance,
    &INSTANCE_ATTRS,
);

/// What one of an effect's pipelines is built for: the effect and form `key` names, from `module`,
/// against `format`, the halves of its [`ClipVariant`] `halves` names.
#[derive(Clone, Copy, Debug)]
pub(crate) struct EffectTarget<'a> {
    pub(crate) module: &'a EffectModule,
    pub(crate) key: EffectKey,
    pub(crate) format: wgpu::TextureFormat,
    pub(crate) halves: &'a [Clipping],
}

impl EffectDevice {
    /// Build the halves of `variant` that `target` names and it lacks, from one shader module,
    /// over the layout of its kind and image slots: its one stage pinned to its form, which for a
    /// backdrop effect also draws a material's box and dithers that result before an 8-bit sRGB
    /// target stores it.
    ///
    /// # Panics
    ///
    /// For a [`ShapeForm::Rect`], as an effect draws its shape's template.
    pub(crate) fn ensure(&self, variant: &mut ClipVariant, target: EffectTarget<'_>) {
        let EffectTarget {
            module,
            key:
                EffectKey {
                    effect,
                    form,
                    cover,
                },
            format,
            halves,
        } = target;
        assert!(
            form != ShapeForm::Rect,
            "an effect draws its shape's template"
        );
        assert!(
            module.kind == KindId::Color || cover == TemplateCover::Whole,
            "a halo is a color effect's"
        );
        if halves.iter().all(|&clipping| variant.is_built(clipping)) {
            return;
        }
        let pinned = |on: bool| f64::from(u8::from(on));
        let shape = [
            (TRIANGLE_OVERRIDE, pinned(form == ShapeForm::Triangle)),
            (HALO_OVERRIDE, pinned(cover == TemplateCover::Rim)),
        ];
        let backdrop = [
            shape[0],
            shape[1],
            (
                "PALANTIR_DITHER_SRGB8",
                pinned(format.is_srgb() && format.target_pixel_byte_cost() == Some(4)),
            ),
        ];
        // Only the material path, which a backdrop effect alone holds, declares the dither.
        let constants: &[(&str, f64)] = match module.kind {
            KindId::Color => &shape,
            KindId::Backdrop => &backdrop,
        };
        let label = format!("palantir.effect.{}.{form:?}.{cover:?}", effect.0);
        let shader = self
            .gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(&label),
                source: wgpu::ShaderSource::Wgsl(module.text.as_str().into()),
            });
        let recipe = PipelineRecipe {
            vertex_entry: "palantir_vs",
            fragment_entry: "palantir_fs",
            vertex_buffers: &[Some(INSTANCE_LAYOUT)],
            topology: wgpu::PrimitiveTopology::TriangleList,
            blend: Some(BLEND),
            constants,
            ..PipelineRecipe::color(&label, &shader, self.pipeline(module), format)
        };
        for &clipping in halves {
            variant.ensure(&self.gpu, clipping, &recipe);
        }
    }
}

/// One frame's effects: their instances, their parameter words, the group the words bind
/// through, and each draw's images; their pipelines are the store's, which a frame reaches
/// through `PassPipelines`.
#[derive(Debug)]
pub(crate) struct EffectPipeline {
    instances: DynamicBuffer<EffectInstance>,
    words: DynamicBuffer<u32>,
    /// Each draw's images this frame, at its index; scratch kept across frames.
    images: Vec<EffectImages>,
    /// Bound over `words`; made again when the buffer grows or shrinks.
    group: Option<wgpu::BindGroup>,
    group_capacity: usize,
    /// The shape template's triangles, which every effect draws over its shape.
    indices: TemplateIndices,
}

/// One batch of effect draws: the frame's per-draw key and instance columns, this batch's slice of
/// them, each effect's module at its id, the groups the draws bind their images through, and the
/// groups a backdrop effect reads its backdrop's result through, `None` before the first frame
/// with a backdrop.
#[derive(Clone, Copy, Debug)]
pub(crate) struct EffectBatch<'a> {
    pub(crate) keys: &'a [EffectKey],
    pub(crate) instances: &'a [EffectInstance],
    pub(crate) modules: &'a [EffectModule],
    pub(crate) items: Span,
    pub(crate) groups: &'a EffectImageGroups,
    pub(crate) backdrops: Option<BackdropGroups<'a>>,
}

/// What one frame uploads: each draw's instance and the span of its images in `textures`, the
/// record store's image column, and the parameter words.
#[derive(Clone, Copy, Debug)]
pub(crate) struct EffectUpload<'a> {
    pub(crate) instances: &'a [EffectInstance],
    pub(crate) images: &'a [Span],
    pub(crate) textures: &'a [TextureId],
    pub(crate) words: &'a [u32],
}

impl EffectPipeline {
    pub(crate) fn new(gpu: &Gpu) -> Self {
        Self {
            instances: DynamicBuffer::vertex(gpu, "palantir.effect.instances", 4),
            words: DynamicBuffer::storage(gpu, "palantir.effect.words", 64),
            images: Vec::new(),
            group: None,
            group_capacity: 0,
            indices: TemplateIndices::new(gpu),
        }
    }

    /// Upload the frame's instances and words, and note each draw's images with the image store,
    /// which makes the group of a set first drawn.
    pub(crate) fn upload(
        &mut self,
        ctx: &mut GpuCtx<'_>,
        stores: EffectStores<'_>,
        upload: EffectUpload<'_>,
    ) {
        self.instances.upload_instances(ctx, upload.instances);
        self.words.upload_instances(ctx, upload.words);
        self.images.clear();
        self.images.extend(
            upload
                .images
                .iter()
                .map(|span| EffectImages::new(&upload.textures[span.range()])),
        );
        // Neighbors share a set as a run of draws does, so each run notes its set once.
        stores.images.ensure_effect_groups(
            self.images
                .chunk_by(|a, b| a == b)
                .map(|run| run[0])
                .filter(|set| !set.ids().is_empty()),
            ctx.frame,
        );
        if self.group.is_none() || self.words.capacity() != self.group_capacity {
            let read = stores.effects.read();
            self.group = Some(
                ctx.gpu
                    .device
                    .create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("palantir.effect.bg"),
                        layout: &read.attached.device.group,
                        entries: &[wgpu::BindGroupEntry {
                            binding: 0,
                            resource: self.words.buffer.as_entire_binding(),
                        }],
                    }),
            );
            self.group_capacity = self.words.capacity();
        }
    }

    /// Draw one batch, one `draw` per run of instances of one key over one set of images, each
    /// through its key's pipeline with its form's vertices, a halo's rim alone. A run whose image
    /// is not on the device is skipped, as an image draw is.
    pub(crate) fn draw_batch<'a>(
        &'a self,
        rec: &mut PassRecorder<'a>,
        pipelines: impl Fn(EffectKey) -> &'a Pipeline,
        EffectBatch {
            keys,
            instances,
            modules,
            items,
            groups,
            backdrops,
        }: EffectBatch<'_>,
    ) {
        let group = self
            .group
            .as_ref()
            .expect("a frame uploads before it draws");
        rec.vertex(0, &self.instances.buffer);
        // A backdrop's composite reads the texture its result lies in through a group over it
        // alone, so a draw splits where that texture changes.
        let key = |i: u32| {
            let key = keys[i as usize];
            let read = (modules[key.effect.idx()].kind == KindId::Backdrop)
                .then(|| backdrops?.read_of(instances[i as usize].backdrop_idx))
                .flatten();
            (key, self.images[i as usize], read)
        };
        for run in Runs::new(items, key) {
            let (key, images, read) = run.key;
            let images_group = if images.ids().is_empty() {
                None
            } else {
                let Some(images_group) = groups.get(&images) else {
                    continue;
                };
                Some(images_group)
            };
            let kind = modules[key.effect.idx()].kind;
            rec.pipeline(pipelines(key));
            rec.group(WORDS_GROUP, group);
            if let Some(images_group) = images_group {
                rec.group_once(IMAGES_GROUP, images_group);
            }
            if kind == KindId::Backdrop {
                let backdrops =
                    backdrops.expect("a frame with a backdrop effect prepares its backdrops");
                rec.group_once(BACKDROPS_GROUP, backdrops.group(read));
            }
            self.indices
                .draw(rec, key.form, key.cover, run.instances.u32_range());
        }
    }
}

/// The two stores an effect upload reads: the effects' pipelines and the images' textures.
#[derive(Clone, Copy, Debug)]
pub(crate) struct EffectStores<'a> {
    pub(crate) effects: &'a WgpuEffectStore,
    pub(crate) images: &'a WgpuImageStore,
}

#[cfg(feature = "internals")]
pub(crate) mod bench;

#[cfg(test)]
mod tests {
    use crate::effect::contract::{self, ContractSpec, ParamsLayout};
    use crate::gpu::pipeline::clip_variant::{ClipVariant, Clipping};
    use crate::gpu::pipeline::effect_pipeline::{EffectDevice, EffectTarget};
    use crate::gpu::resource::texture_binding::TextureBinding;
    use crate::gpu::test_gpu::headless_test_gpu;
    use crate::internals::panic_probe;
    use crate::primitives::effect::effect_kind::KindId;
    use crate::primitives::identity::effect_id::EffectId;
    use crate::renderer::render_buffer::effect_draw_row::EffectKey;
    use crate::renderer::render_buffer::shape_form::ShapeForm;
    use crate::renderer::render_buffer::template_cover::TemplateCover;

    /// A color and a backdrop effect each build the halves of a box and of a triangle a target
    /// names, the clipless half first and the clipped one after it; a rect's one quad is refused.
    /// A color effect builds a box's halo; a backdrop effect's halo is refused.
    #[test]
    fn an_effect_builds_the_half_its_target_names() {
        let gpu = headless_test_gpu();
        let device = EffectDevice::new(&gpu, &TextureBinding::new(&gpu));
        let spec = |kind| ContractSpec {
            kind,
            params: ParamsLayout {
                fields: &[],
                size: 0,
            },
            image_slots: 0,
        };
        for kind in [KindId::Color, KindId::Backdrop] {
            let module = contract::check(
                "fn effect(input: EffectInput) -> EffectOutput {
                    return effect_over(vec4<f32>(1.0));
                }",
                spec(kind),
            )
            .unwrap();
            let covered = |form, cover, halves| EffectTarget {
                module: &module,
                key: EffectKey {
                    effect: EffectId(0),
                    form,
                    cover,
                },
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                halves,
            };
            let target = |form, halves| covered(form, TemplateCover::Whole, halves);
            for form in [ShapeForm::Box, ShapeForm::Triangle] {
                let mut variant = ClipVariant::NONE;
                device.ensure(&mut variant, target(form, &[Clipping::Off]));
                assert!(variant.is_built(Clipping::Off), "{kind:?} {form:?}");
                assert!(!variant.is_built(Clipping::On), "{kind:?} {form:?}");
                device.ensure(&mut variant, target(form, &[Clipping::Off, Clipping::On]));
                assert!(variant.is_built(Clipping::On), "{kind:?} {form:?}");
            }
            panic_probe::assert_panics_with("an effect draws its shape's template", || {
                let mut rect = ClipVariant::NONE;
                device.ensure(&mut rect, target(ShapeForm::Rect, &[Clipping::Off]));
            });
            let mut halo = ClipVariant::NONE;
            let rim = covered(ShapeForm::Box, TemplateCover::Rim, &[Clipping::Off]);
            match kind {
                KindId::Color => {
                    device.ensure(&mut halo, rim);
                    assert!(halo.is_built(Clipping::Off), "a color effect's halo");
                }
                KindId::Backdrop => {
                    panic_probe::assert_panics_with("a halo is a color effect's", || {
                        device.ensure(&mut halo, rim);
                    });
                }
            }
        }
    }
}
