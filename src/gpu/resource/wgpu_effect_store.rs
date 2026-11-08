//! The pipelines of the effects a host loaded, built as the registry loads each one.

use std::cell::{Ref, RefCell};

use crate::effect::effect_store::{EffectModule, EffectStore};
use crate::gpu::device::gpu::Gpu;
use crate::gpu::pipeline::effect_pipeline::{self, EffectLayouts};
use crate::gpu::pipeline::stencil_variant::StencilVariant;
use crate::gpu::resource::texture_binding::TextureBinding;
use crate::primitives::identity::effect_id::EffectId;
use crate::renderer::render_buffer::effect_draw_row::PaneForm;

/// The [`EffectStore`] a host's registry loads through and its backend draws from. It keeps each
/// effect's module, so a new swapchain format or a new device builds every effect again, and
/// builds at the load, not at the first draw, which would stutter on a first press: the reason
/// Flutter wrote Impeller.
///
/// Built before any device, as the registry every recorder clones holds it from the start; the
/// host [attaches](Self::attach) each device it draws with.
#[derive(Debug, Default)]
pub(crate) struct WgpuEffectStore {
    state: RefCell<StoreState>,
}

#[derive(Debug, Default)]
struct StoreState {
    /// Each loaded effect's module, at its id.
    modules: Vec<EffectModule>,
    /// `None` until the first attach.
    attached: Option<AttachedEffects>,
    /// Moves with every build, so a reader of [`AttachedEffects::for_format`] knows to copy again.
    revision: u64,
}

/// The store's side of one device: the layouts every effect builds over, and the pipelines of
/// each format it draws to.
#[derive(Debug)]
pub(crate) struct AttachedEffects {
    gpu: Gpu,
    pub(crate) layouts: EffectLayouts,
    /// [`effect_pipeline::PANE_SHADER`]'s module, which every pane form builds from.
    pane_shader: wgpu::ShaderModule,
    /// One for each format, a few in all, in the order each was first drawn to: a frame reads
    /// one format's pipelines whole, and an effect's load adds one to each. Refused: a table of each
    /// effect's pipelines for each format, which a frame reads one entry a row of; one flat table,
    /// which a new format reshapes.
    formats: Vec<FormatEffects>,
}

/// Every pipeline one format draws effects with: the pane forms', and each loaded effect's.
#[derive(Debug)]
pub(crate) struct FormatEffects {
    format: wgpu::TextureFormat,
    /// At each [`PaneForm`].
    pub(crate) panes: [StencilVariant; PaneForm::COUNT],
    /// At each effect's id.
    pub(crate) effects: Vec<StencilVariant>,
}

impl WgpuEffectStore {
    /// Build on `gpu` from now on, binding images through `binding`, the device's: every loaded
    /// effect again, for no format yet, as the backend names each one it draws to
    /// ([`Self::ensure_format`]).
    pub(crate) fn attach(&self, gpu: &Gpu, binding: &TextureBinding) {
        let mut state = self.state.borrow_mut();
        state.attached = Some(AttachedEffects {
            gpu: gpu.clone(),
            layouts: EffectLayouts::new(gpu, binding),
            pane_shader: effect_pipeline::PANE_SHADER.module(gpu),
            formats: Vec::new(),
        });
        state.revision += 1;
    }

    /// Build every loaded effect for `format`, where it is new: before the frame that paints it.
    pub(crate) fn ensure_format(&self, format: wgpu::TextureFormat) {
        let mut state = self.state.borrow_mut();
        let StoreState {
            modules,
            attached,
            revision,
        } = &mut *state;
        let attached = attached.as_mut().expect(ATTACHED);
        if attached.for_format(format).is_some() {
            return;
        }
        let panes = PaneForm::ALL.map(|form| {
            effect_pipeline::build_pane(
                &attached.gpu,
                &attached.layouts,
                &attached.pane_shader,
                form,
                format,
            )
        });
        let effects = modules
            .iter()
            .enumerate()
            .map(|(idx, module)| {
                effect_pipeline::build(&attached.gpu, &attached.layouts, module, format, idx)
            })
            .collect();
        attached.formats.push(FormatEffects {
            format,
            panes,
            effects,
        });
        *revision += 1;
    }

    /// The pipelines built now, each effect's module, and the revision they are at.
    pub(crate) fn read(&self) -> EffectsRead<'_> {
        let state = self.state.borrow();
        let revision = state.revision;
        let (attached, modules) = Ref::map_split(state, |state| {
            (
                state.attached.as_ref().expect(ATTACHED),
                state.modules.as_slice(),
            )
        });
        EffectsRead {
            attached,
            modules,
            revision,
        }
    }
}

/// [`WgpuEffectStore::read`]'s borrow.
#[derive(Debug)]
pub(crate) struct EffectsRead<'a> {
    pub(crate) attached: Ref<'a, AttachedEffects>,
    /// At each effect's id.
    pub(crate) modules: Ref<'a, [EffectModule]>,
    pub(crate) revision: u64,
}

impl AttachedEffects {
    /// Every pipeline `format` draws effects with; `None` where the format is not built.
    pub(crate) fn for_format(&self, format: wgpu::TextureFormat) -> Option<&FormatEffects> {
        self.formats.iter().find(|built| built.format == format)
    }
}

const ATTACHED: &str = "the host attaches a device before it draws or loads an effect";

impl EffectStore for WgpuEffectStore {
    fn load(&self, id: EffectId, module: &EffectModule) {
        let mut state = self.state.borrow_mut();
        let StoreState {
            modules,
            attached,
            revision,
        } = &mut *state;
        debug_assert!(
            id.idx() <= modules.len(),
            "the registry hands out ids in order"
        );
        if id.idx() == modules.len() {
            modules.push(module.clone());
        } else {
            module.clone_into(&mut modules[id.idx()]);
        }
        if let Some(attached) = attached {
            let AttachedEffects {
                gpu,
                layouts,
                pane_shader: _,
                formats,
            } = attached;
            for built in formats {
                let pipeline = effect_pipeline::build(gpu, layouts, module, built.format, id.idx());
                if id.idx() == built.effects.len() {
                    built.effects.push(pipeline);
                } else {
                    built.effects[id.idx()] = pipeline;
                }
                debug_assert_eq!(
                    built.effects.len(),
                    modules.len(),
                    "each format holds a pipeline for every loaded effect"
                );
            }
        }
        *revision += 1;
    }
}

#[cfg(test)]
mod tests {
    use glam::UVec2;

    use crate::effect::effect_name::EffectName;
    use crate::effect::effect_source::EffectSource;
    use crate::gpu::surface::owned_target::OwnedTarget;
    use crate::gpu::surface::render_target::TargetFormat;
    use crate::gpu::test_gpu::headless_test_gpu;
    use crate::host::offscreen::{OffscreenHost, OffscreenTime};
    use crate::primitives::paint::color::RgbaF32;
    use crate::shape::Shape;
    use crate::text::shaper::TextShaper;
    use crate::ui::Ui;

    const BLUE: EffectName<()> = EffectName::named("blue").expect("the name fits");

    /// A target of a new format builds every loaded effect for it before its frame: the same
    /// effect paints opaque blue into an RGBA target, then into a BGRA one, each read back as RGBA.
    #[test]
    fn a_new_format_builds_the_loaded_effects() {
        let gpu = headless_test_gpu();
        let mut host = OffscreenHost::builder(gpu.handles())
            .shaper(TextShaper::test_mono())
            .time(OffscreenTime::Manual)
            .build();
        let wgsl = "fn effect(input: EffectInput) -> EffectOutput {
            return effect_over(vec4<f32>(0.0, 0.0, 1.0, 1.0));
        }";
        host.ui()
            .load_effect(BLUE, EffectSource::new(wgsl))
            .unwrap();
        let mut app = |ui: &mut Ui| {
            ui.add_shape(Shape::rect().fill(RgbaF32::new(0.2, 0.4, 0.6, 1.0)));
            ui.add_shape(Shape::effect(BLUE, ()));
        };
        for format in [
            wgpu::TextureFormat::Rgba8UnormSrgb,
            wgpu::TextureFormat::Bgra8UnormSrgb,
        ] {
            let target = OwnedTarget::window(
                &gpu,
                "palantir.effect.format",
                UVec2::new(8, 8),
                TargetFormat::new(format),
            );
            host.frame(target.as_target(), &mut app);
            assert_eq!(
                target.read_back().get_pixel(4, 4).0,
                [0, 0, 255, 255],
                "{format:?}"
            );
        }
    }
}
