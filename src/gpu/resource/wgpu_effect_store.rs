//! The pipelines of the effects a host loaded, built as the registry loads each one.

use std::cell::{Ref, RefCell};

use crate::effect::effect_store::EffectStore;
use crate::gpu::device::gpu::Gpu;
use crate::gpu::pipeline::effect_pipeline::{self, EffectLayouts};
use crate::gpu::pipeline::stencil_variant::StencilVariant;
use crate::primitives::identity::effect_id::EffectId;

/// The [`EffectStore`] a host's registry loads through and its backend draws from. It keeps each
/// effect's module text, so a new swapchain format or a new device builds every effect again, and
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
    /// Each loaded effect's module text, at its id.
    texts: Vec<String>,
    /// `None` until the first attach.
    attached: Option<AttachedEffects>,
    /// Moves with every build, so a reader of [`AttachedEffects::pipelines`] knows to copy again.
    revision: u64,
}

/// The store's side of one device: the layouts every effect builds over, the formats it draws
/// to, and each effect's pipelines for each format.
#[derive(Debug)]
pub(crate) struct AttachedEffects {
    gpu: Gpu,
    pub(crate) layouts: EffectLayouts,
    formats: Vec<wgpu::TextureFormat>,
    /// At each effect's id, then in `formats` order.
    pipelines: Vec<Vec<StencilVariant>>,
}

impl WgpuEffectStore {
    /// Build on `gpu` from now on: every loaded effect again, for no format yet, as the backend
    /// names each one it draws to ([`Self::ensure_format`]).
    pub(crate) fn attach(&self, gpu: &Gpu) {
        let mut state = self.state.borrow_mut();
        let pipelines = state.texts.iter().map(|_| Vec::new()).collect();
        state.attached = Some(AttachedEffects {
            gpu: gpu.clone(),
            layouts: EffectLayouts::new(gpu),
            formats: Vec::new(),
            pipelines,
        });
        state.revision += 1;
    }

    /// Build every loaded effect for `format`, where it is new: before the frame that paints it.
    pub(crate) fn ensure_format(&self, format: wgpu::TextureFormat) {
        let mut state = self.state.borrow_mut();
        let StoreState {
            texts,
            attached,
            revision,
        } = &mut *state;
        let attached = attached.as_mut().expect(ATTACHED);
        if attached.formats.contains(&format) {
            return;
        }
        attached.formats.push(format);
        for (idx, text) in texts.iter().enumerate() {
            let built = effect_pipeline::build(&attached.gpu, &attached.layouts, text, format, idx);
            attached.pipelines[idx].push(built);
        }
        *revision += 1;
    }

    /// The pipelines built now and the revision they are at.
    pub(crate) fn read(&self) -> EffectsRead<'_> {
        let state = self.state.borrow();
        let revision = state.revision;
        EffectsRead {
            attached: Ref::map(state, |state| state.attached.as_ref().expect(ATTACHED)),
            revision,
        }
    }
}

/// [`WgpuEffectStore::read`]'s borrow.
#[derive(Debug)]
pub(crate) struct EffectsRead<'a> {
    pub(crate) attached: Ref<'a, AttachedEffects>,
    pub(crate) revision: u64,
}

impl AttachedEffects {
    /// Every effect's pipelines for `format`, at each id; `None` where the format is not built.
    pub(crate) fn for_format(
        &self,
        format: wgpu::TextureFormat,
    ) -> Option<impl Iterator<Item = &StencilVariant>> {
        let at = self.formats.iter().position(|&built| built == format)?;
        Some(self.pipelines.iter().map(move |formats| &formats[at]))
    }
}

const ATTACHED: &str = "the host attaches a device before it draws or loads an effect";

impl EffectStore for WgpuEffectStore {
    fn load(&self, id: EffectId, wgsl: &str) {
        let mut state = self.state.borrow_mut();
        let StoreState {
            texts,
            attached,
            revision,
        } = &mut *state;
        debug_assert!(
            id.idx() <= texts.len(),
            "the registry hands out ids in order"
        );
        if id.idx() == texts.len() {
            texts.push(wgsl.to_owned());
        } else {
            wgsl.clone_into(&mut texts[id.idx()]);
        }
        if let Some(attached) = attached {
            let built: Vec<StencilVariant> = attached
                .formats
                .iter()
                .map(|&format| {
                    effect_pipeline::build(&attached.gpu, &attached.layouts, wgsl, format, id.idx())
                })
                .collect();
            if id.idx() == attached.pipelines.len() {
                attached.pipelines.push(built);
            } else {
                attached.pipelines[id.idx()] = built;
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
