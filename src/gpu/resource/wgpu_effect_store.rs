//! The pipelines of the effects a host loaded, built where a use becomes known.

use std::cell::{Ref, RefCell};

use fixedbitset::FixedBitSet;

use crate::effect::effect_store::{EffectModule, EffectStore};
use crate::gpu::device::gpu::Gpu;
use crate::gpu::pipeline::effect_pipeline::{self, EffectLayouts};
use crate::gpu::pipeline::stencil_variant::StencilVariant;
use crate::gpu::resource::texture_binding::TextureBinding;
use crate::primitives::identity::effect_id::EffectId;
use crate::renderer::render_buffer::effect_draw_row::{EffectProgram, PaneForm};

/// The [`EffectStore`] a host's registry loads through and its backend draws from. It keeps each
/// effect's module, and builds the pipelines of the effects some use wants at one place, the
/// start of a frame's GPU phase ([`Self::prepare`]), before any pass: an effect a load wants
/// built at once, each effect a window's themes name, and each a frame draws. A theme's effects so
/// build together before a press can reach them, and nothing builds that no use wants, as a
/// pipeline-state cache with a warm-up does (Unity's `ShaderVariantCollection.WarmUp`, Unreal's
/// PSO precaching). Refused: building at the load or the theme set, which a later one replaces
/// for nothing; building at the first press, which stutters, the reason Flutter wrote Impeller;
/// building on a worker, which shows an effect late. A wanted effect stays built, for every
/// format and every device after.
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
    /// The effects some use wanted, at each id: built for every format of every device.
    wanted: FixedBitSet,
    /// The pane forms a frame drew, at each [`PaneForm`]: built as [`Self::wanted`]'s effects.
    wanted_panes: [bool; PaneForm::COUNT],
    /// The wanted effects the attached device builds at the next [`WgpuEffectStore::prepare`]: a
    /// new want, or a want reloaded with a new module.
    pending: Vec<EffectId>,
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
    /// one format's pipelines whole, and a build adds one to each. Refused: a table of each
    /// effect's pipelines for each format, which a frame reads one entry a row of; one flat table,
    /// which a new format reshapes.
    formats: Vec<FormatEffects>,
}

/// Every pipeline one format draws effects with: the pane forms', and each loaded effect's,
/// [`StencilVariant::NONE`] where none is built.
#[derive(Debug)]
pub(crate) struct FormatEffects {
    format: wgpu::TextureFormat,
    /// At each [`PaneForm`].
    pub(crate) panes: [StencilVariant; PaneForm::COUNT],
    /// At each effect's id.
    pub(crate) effects: Vec<StencilVariant>,
}

impl WgpuEffectStore {
    /// Build on `gpu` from now on, binding images through `binding`, the device's: every wanted
    /// effect again, for each format the backend names ([`Self::ensure_format`]).
    pub(crate) fn attach(&self, gpu: &Gpu, binding: &TextureBinding) {
        let mut state = self.state.borrow_mut();
        state.attached = Some(AttachedEffects {
            gpu: gpu.clone(),
            layouts: EffectLayouts::new(gpu, binding),
            pane_shader: effect_pipeline::PANE_SHADER.module(gpu),
            formats: Vec::new(),
        });
        // A new format builds every wanted effect, so nothing is pending on a fresh device.
        state.pending.clear();
        state.revision += 1;
    }

    /// Build every wanted effect and pane form for `format`, where it is new: before the frame
    /// that paints it.
    pub(crate) fn ensure_format(&self, format: wgpu::TextureFormat) {
        let mut state = self.state.borrow_mut();
        let StoreState {
            modules,
            wanted,
            wanted_panes,
            attached,
            revision,
            pending: _,
        } = &mut *state;
        let attached = attached.as_mut().expect(ATTACHED);
        if attached.for_format(format).is_some() {
            return;
        }
        let mut built = FormatEffects {
            format,
            panes: [StencilVariant::NONE; PaneForm::COUNT],
            effects: vec![StencilVariant::NONE; modules.len()],
        };
        for form in PaneForm::ALL
            .into_iter()
            .filter(|form| wanted_panes[form.idx()])
        {
            built.panes[form.idx()] = attached.build_pane(form, format);
        }
        for idx in wanted.ones() {
            built.effects[idx] = attached.build(&modules[idx], format, idx);
        }
        attached.formats.push(built);
        *revision += 1;
    }

    /// The start of a frame's GPU phase: want each effect and pane form `programs`, the frame's
    /// draws, name, and build every wanted one not built yet for each format, before any pass.
    pub(crate) fn prepare(&self, programs: &[EffectProgram]) {
        let mut state = self.state.borrow_mut();
        for &program in programs {
            match program {
                EffectProgram::App(id) => state.want(id),
                EffectProgram::Pane(form) => state.want_pane(form),
            }
        }
        let StoreState {
            modules,
            attached,
            pending,
            revision,
            ..
        } = &mut *state;
        let Some(attached) = attached else {
            return;
        };
        if pending.is_empty() {
            return;
        }
        let AttachedEffects {
            gpu,
            layouts,
            formats,
            ..
        } = attached;
        for id in pending.drain(..) {
            for built in formats.iter_mut() {
                built.effects[id.idx()] = effect_pipeline::build(
                    gpu,
                    layouts,
                    &modules[id.idx()],
                    built.format,
                    id.idx(),
                );
            }
        }
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

impl StoreState {
    /// Want the effect `id`: the next prepare builds it, where no use wanted it before.
    fn want(&mut self, id: EffectId) {
        if !self.wanted.contains(id.idx()) {
            self.wanted.insert(id.idx());
            self.pending.push(id);
        }
    }

    /// Want the pane form `form`, built at once for each format, as the frame that wants it
    /// draws it.
    fn want_pane(&mut self, form: PaneForm) {
        if self.wanted_panes[form.idx()] {
            return;
        }
        self.wanted_panes[form.idx()] = true;
        if let Some(attached) = &mut self.attached {
            for idx in 0..attached.formats.len() {
                let format = attached.formats[idx].format;
                attached.formats[idx].panes[form.idx()] = attached.build_pane(form, format);
            }
            self.revision += 1;
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

    fn build(
        &self,
        module: &EffectModule,
        format: wgpu::TextureFormat,
        idx: usize,
    ) -> StencilVariant {
        effect_pipeline::build(&self.gpu, &self.layouts, module, format, idx)
    }

    fn build_pane(&self, form: PaneForm, format: wgpu::TextureFormat) -> StencilVariant {
        effect_pipeline::build_pane(&self.gpu, &self.layouts, &self.pane_shader, form, format)
    }
}

const ATTACHED: &str = "the host attaches a device before it draws or loads an effect";

impl EffectStore for WgpuEffectStore {
    fn register(&self, id: EffectId, module: &EffectModule) {
        let mut state = self.state.borrow_mut();
        let StoreState {
            modules,
            wanted,
            pending,
            attached,
            ..
        } = &mut *state;
        debug_assert!(
            id.idx() <= modules.len(),
            "the registry hands out ids in order"
        );
        if id.idx() == modules.len() {
            modules.push(module.clone());
            wanted.grow(modules.len());
            if let Some(attached) = attached {
                for built in &mut attached.formats {
                    built.effects.push(StencilVariant::NONE);
                }
            }
        } else {
            module.clone_into(&mut modules[id.idx()]);
            // A wanted effect reloaded builds its new module at the next prepare.
            if wanted.contains(id.idx()) && !pending.contains(&id) {
                pending.push(id);
            }
        }
    }

    fn want(&self, id: EffectId) {
        self.state.borrow_mut().want(id);
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::gpu::pipeline::stencil_variant::{StencilMode, StencilVariant};
    use crate::gpu::resource::wgpu_effect_store::{ATTACHED, FormatEffects, WgpuEffectStore};
    use crate::renderer::render_buffer::effect_draw_row::PaneForm;

    impl WgpuEffectStore {
        /// Whether each loaded effect, at its id, is built, the same for every format drawn to.
        pub(crate) fn built_effects(&self) -> Vec<bool> {
            let state = self.state.borrow();
            (0..state.modules.len())
                .map(|idx| self.built_alike(|format| &format.effects[idx]))
                .collect()
        }

        /// Whether each [`PaneForm`] is built, the same for every format drawn to.
        pub(crate) fn built_panes(&self) -> [bool; PaneForm::COUNT] {
            PaneForm::ALL.map(|form| self.built_alike(|format| &format.panes[form.idx()]))
        }

        /// Whether the pipeline `of` picks is built, which every format drawn to agrees on.
        fn built_alike(&self, of: impl Fn(&FormatEffects) -> &StencilVariant) -> bool {
            let state = self.state.borrow();
            let formats = &state.attached.as_ref().expect(ATTACHED).formats;
            assert!(!formats.is_empty(), "a frame drew to a format");
            let built = |format| {
                let variant = of(format);
                assert_eq!(
                    variant.is_built(StencilMode::Off),
                    variant.is_built(StencilMode::Test),
                    "a build makes both halves"
                );
                variant.is_built(StencilMode::Off)
            };
            let first = built(&formats[0]);
            assert!(formats.iter().all(|format| built(format) == first));
            first
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use glam::UVec2;

    use crate::animation::animation_spec::AnimationSpec;
    use crate::effect::effect_source::{EffectBuild, EffectSource};
    use crate::gpu::surface::owned_target::OwnedTarget;
    use crate::gpu::surface::render_target::TargetFormat;
    use crate::gpu::test_gpu::headless_test_gpu;
    use crate::host::offscreen::{OffscreenHost, OffscreenTime};
    use crate::primitives::effect::effect_name::EffectName;
    use crate::primitives::paint::backdrop_filter::BackdropFilter;
    use crate::primitives::paint::color::RgbaF32;
    use crate::shape::Shape;
    use crate::text::shaper::TextShaper;
    use crate::theme::Theme;
    use crate::theme::color_scheme::ColorScheme;
    use crate::theme::look::press_effect::PressEffect;
    use crate::ui::Ui;

    const BLUE: EffectName<()> = EffectName::named("blue").expect("the name fits");
    const RED: EffectName<()> = EffectName::named("red").expect("the name fits");

    fn solid(rgb: &str) -> String {
        format!(
            "fn effect(input: EffectInput) -> EffectOutput {{
                return effect_over(vec4<f32>({rgb}, 1.0));
            }}"
        )
    }

    fn host() -> (OffscreenHost, OwnedTarget) {
        let gpu = headless_test_gpu();
        let host = OffscreenHost::builder(gpu.handles())
            .shaper(TextShaper::test_mono())
            .time(OffscreenTime::Manual)
            .build();
        let target = OwnedTarget::window(
            &gpu,
            "palantir.effect.builds",
            UVec2::new(8, 8),
            TargetFormat::new(wgpu::TextureFormat::Rgba8UnormSrgb),
        );
        (host, target)
    }

    /// An effect builds where a use first wants it, and stays built: the stock ripples (ids 0
    /// and 1) and `OnUse` blue (2) build nothing over a frame that draws none of them, `Now` red
    /// (3) builds at that frame; blue builds at the frame that draws it, red staying built. A
    /// reload of blue as green builds again, so the next frame paints green. A pane form builds
    /// at the frame that draws it, frost alone for a blur.
    #[test]
    fn an_effect_builds_where_a_use_wants_it() {
        let (mut host, target) = host();
        host.ui()
            .load_effect(BLUE, EffectSource::new(solid("0.0, 0.0, 1.0")))
            .unwrap();
        host.ui()
            .load_effect(
                RED,
                EffectSource::new(solid("1.0, 0.0, 0.0")).build(EffectBuild::Now),
            )
            .unwrap();
        host.frame(target.as_target(), &mut |ui: &mut Ui| {
            ui.add_shape(Shape::rect().fill(RgbaF32::WHITE));
        });
        let store = |host: &OffscreenHost| host.effect_store().built_effects();
        assert_eq!(store(&host), [false, false, false, true]);
        assert_eq!(host.effect_store().built_panes(), [false; 3]);

        let mut blue = |ui: &mut Ui| ui.add_shape(Shape::effect(BLUE, ()));
        host.frame(target.as_target(), &mut blue);
        assert_eq!(store(&host), [false, false, true, true]);
        assert_eq!(target.read_back().get_pixel(4, 4).0, [0, 0, 255, 255]);

        host.ui()
            .load_effect(BLUE, EffectSource::new(solid("0.0, 1.0, 0.0")))
            .unwrap();
        host.frame(target.as_target(), &mut blue);
        assert_eq!(target.read_back().get_pixel(4, 4).0, [0, 255, 0, 255]);

        host.frame(target.as_target(), &mut |ui: &mut Ui| {
            ui.add_shape(Shape::rect().fill(RgbaF32::WHITE));
            ui.add_shape(Shape::rect().backdrop_filter(BackdropFilter::blur(3.0)));
        });
        assert_eq!(host.effect_store().built_panes(), [true, false, false]);
        assert_eq!(store(&host), [false, false, true, true]);
    }

    /// The press effects of a window's themes build at the next frame that draws, for both
    /// schemes, each with the alternative it names: a sparkle theme for the scheme not shown
    /// builds nothing before that frame, then the sparkle (id 1) and the flat ripple it saves
    /// power with (0). A theme another set replaced before the frame builds nothing.
    #[test]
    fn a_window_theme_builds_its_press_effects_at_the_next_record() {
        let sparkle = || {
            let mut theme = Theme::stock(ColorScheme::Light);
            theme.transition = AnimationSpec::SNAP;
            theme.button.defaults.press = Some(PressEffect {
                effect: EffectName::SPARKLE,
                color: RgbaF32::WHITE,
                highlight: RgbaF32::WHITE,
                enter: Duration::ZERO,
                exit: Duration::ZERO,
            });
            theme
        };
        let mut nothing = |_: &mut Ui| {};
        let mut fill = |ui: &mut Ui| ui.add_shape(Shape::rect().fill(RgbaF32::WHITE));

        let (mut host, target) = host();
        host.ui_mut()
            .set_theme(ColorScheme::Dark, sparkle())
            .unwrap();
        host.ui_mut()
            .set_theme(ColorScheme::Dark, Theme::stock(ColorScheme::Dark))
            .unwrap();
        host.frame(target.as_target(), &mut nothing);
        assert_eq!(host.effect_store().built_effects(), [false, false]);

        let shown = host.ui().color_scheme();
        let hidden = match shown {
            ColorScheme::Light => ColorScheme::Dark,
            ColorScheme::Dark => ColorScheme::Light,
        };
        host.ui_mut().set_theme(hidden, sparkle()).unwrap();
        assert_eq!(host.effect_store().built_effects(), [false, false]);
        host.frame(target.as_target(), &mut fill);
        assert_eq!(host.effect_store().built_effects(), [true, true]);
    }

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
