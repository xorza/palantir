//! The pipelines of the effects a host loaded, built where a use becomes known.

use fixedbitset::FixedBitSet;

use crate::effect::effect_module::EffectModule;
use crate::effect::effect_registry::{ChangesSince, EffectChange, EffectRegistry};
use crate::gpu::device::gpu::Gpu;
use crate::gpu::pipeline::clip_variant::{ClipVariant, Clipping};
use crate::gpu::pipeline::effect_pipeline::{EffectDevice, EffectTarget};
use crate::gpu::resource::texture_binding::TextureBinding;
use crate::primitives::effect::effect_kind::KindId;
use crate::primitives::identity::effect_id::EffectId;
use crate::renderer::render_buffer::effect_draw_row::EffectKey;
use crate::renderer::render_buffer::shape_form::ShapeForm;
use crate::renderer::render_buffer::template_cover::TemplateCover;

/// The pipelines a backend draws effects with, on its device. It reads the host's
/// [`EffectRegistry`], which holds each effect's module, and builds the pipelines some use wants
/// at one place, the start of a frame's GPU phase ([`Self::prepare`]), before any pass: the box
/// form of an effect a load wants built at once and of each effect a window's themes name, and the
/// form of each draw a frame holds. A theme's effects so build together before a press can reach
/// them, and nothing builds that no use wants, as a pipeline-state cache with a warm-up does
/// (Unity's `ShaderVariantCollection.WarmUp`, Unreal's PSO precaching). Refused: building at the
/// load or the theme set, which a later one replaces for nothing; at the first press, which
/// stutters, the reason Flutter wrote Impeller; on a worker, which shows an effect late; every
/// form at a want, two pipelines where a press and a material draw a box. Cost: an effect's first
/// triangle builds at the frame that draws it.
///
/// Each wanted form builds the half of its [`ClipVariant`] a frame with no rounded clip binds, and
/// the half that takes them from the first frame that draws one, every wanted form at once, as
/// every other family builds a half on the first frame whose census asks for it. Refused: both
/// halves at the want, which compiles a program an app with no rounded clip never binds, at its
/// load and at each device recovery. Cost: that first frame builds the clipped half of every
/// wanted form.
///
/// A wanted form stays built for every format. The store lives as long as its device: one on a
/// new device reads every effect and every want the registry holds, and builds the forms a frame
/// draws as frames draw them. Refused: a store the registry pushes into, shared through an `Rc`,
/// which held a second copy of every module.
#[derive(Debug)]
pub(crate) struct WgpuEffectStore {
    device: EffectDevice,
    /// One for each format, a few in all, in the order each was first drawn to: a frame reads
    /// one format's pipelines whole, and a build adds one to each. Refused: a table of each
    /// effect's pipelines for each format, which a frame reads one entry a row of; one flat table,
    /// which a new format reshapes.
    formats: Vec<FormatEffects>,
    /// Each loaded effect's kind, at its id, which a draw binds by.
    kinds: Vec<KindId>,
    /// Each loaded effect's compiled text, at its id, once a form of it builds: one parse and
    /// validation an effect for every form and format, which costs about what a pipeline does,
    /// dropped at a reload.
    shaders: Vec<Option<wgpu::ShaderModule>>,
    /// The forms some use wanted, at each key's [`EffectKey::slot`]: built for every format.
    wanted: FixedBitSet,
    /// The wanted forms the next [`Self::prepare`] builds: a new want, a want reloaded with a new
    /// module, or every want when the clipped half is first asked for.
    pending: Vec<EffectKey>,
    /// Whether a frame drew rounded clips, from which on every wanted form builds the clipped half
    /// too.
    clipped: bool,
    /// What the next read of the registry hands over: everything, for a store on a new device.
    since: ChangesSince,
}

/// Every pipeline one format draws effects with: each form of each loaded effect,
/// [`ClipVariant::NONE`] where none is built.
#[derive(Debug)]
pub(crate) struct FormatEffects {
    format: wgpu::TextureFormat,
    /// At each key's [`EffectKey::slot`].
    pub(crate) effects: Vec<ClipVariant>,
}

impl WgpuEffectStore {
    /// A store on `gpu`, binding images through `binding`, the device's.
    pub(crate) fn new(gpu: &Gpu, binding: &TextureBinding) -> Self {
        Self {
            device: EffectDevice::new(gpu, binding),
            formats: Vec::new(),
            kinds: Vec::new(),
            shaders: Vec::new(),
            wanted: FixedBitSet::new(),
            pending: Vec::new(),
            clipped: false,
            since: ChangesSince::Start,
        }
    }

    /// Build every wanted form for `format` on `gpu`, this store's device, where it is new: before
    /// the frame that paints it.
    pub(crate) fn ensure_format(
        &mut self,
        gpu: &Gpu,
        effects: &EffectRegistry,
        format: wgpu::TextureFormat,
    ) {
        self.read(effects);
        if self.for_format(format).is_some() {
            return;
        }
        let mut built = FormatEffects {
            format,
            effects: vec![ClipVariant::NONE; self.kinds.len() * EffectKey::FORMS],
        };
        let modules = effects.modules();
        for slot in self.wanted.ones() {
            let key = EffectKey::of_slot(slot);
            let module = modules.get(key.effect);
            self.device.ensure(
                gpu,
                &mut built.effects[slot],
                EffectTarget {
                    module,
                    shader: shader(&mut self.shaders, gpu, key.effect, module),
                    key,
                    format,
                    halves: halves(self.clipped),
                },
            );
        }
        self.formats.push(built);
    }

    /// The start of a frame's GPU phase on `gpu`, this store's device: read what `effects`
    /// changed, want each key of `keys`, the frame's draws, and build every wanted form's halves
    /// not built yet for each format, before any pass; a frame of `clipping` [`Clipping::On`]
    /// first asks for the clipped half of every wanted form.
    pub(crate) fn prepare(
        &mut self,
        gpu: &Gpu,
        effects: &EffectRegistry,
        keys: &[EffectKey],
        clipping: Clipping,
    ) {
        self.read(effects);
        for &key in keys {
            self.want(key);
        }
        if clipping == Clipping::On && !self.clipped {
            self.clipped = true;
            self.pending.clear();
            self.pending
                .extend(self.wanted.ones().map(EffectKey::of_slot));
        }
        if self.pending.is_empty() {
            return;
        }
        let modules = effects.modules();
        for key in self.pending.drain(..) {
            let module = modules.get(key.effect);
            let shader = shader(&mut self.shaders, gpu, key.effect, module);
            for built in &mut self.formats {
                self.device.ensure(
                    gpu,
                    &mut built.effects[key.slot()],
                    EffectTarget {
                        module,
                        shader,
                        key,
                        format: built.format,
                        halves: halves(self.clipped),
                    },
                );
            }
        }
    }

    /// Every pipeline `format` draws effects with; `None` where the format is not built.
    pub(crate) fn for_format(&self, format: wgpu::TextureFormat) -> Option<&FormatEffects> {
        self.formats.iter().find(|built| built.format == format)
    }

    /// Each loaded effect's kind, at its id.
    pub(crate) fn kinds(&self) -> &[KindId] {
        &self.kinds
    }

    /// The layouts every effect's pipeline builds over.
    pub(crate) const fn device(&self) -> &EffectDevice {
        &self.device
    }

    /// Take what `effects` changed since this store last read it: a new effect grows every
    /// format's table, a reloaded one drops its pipelines and builds its wanted forms again, and a
    /// want of a box builds at the next prepare.
    fn read(&mut self, effects: &EffectRegistry) {
        let since = self.since;
        effects.drain_changes(since, |change| match change {
            EffectChange::Loaded { id, module } => self.load(id, module.kind),
            EffectChange::Wanted(id) => self.want(EffectKey {
                effect: id,
                form: ShapeForm::Box,
                cover: TemplateCover::Whole,
            }),
        });
        self.since = ChangesSince::LastRead;
    }

    /// Hold `id`, of `kind`, new or in place of the one before.
    fn load(&mut self, id: EffectId, kind: KindId) {
        debug_assert!(
            id.idx() <= self.kinds.len(),
            "the registry hands out ids in order"
        );
        if id.idx() == self.kinds.len() {
            self.kinds.push(kind);
            self.shaders.push(None);
            let slots = self.kinds.len() * EffectKey::FORMS;
            self.wanted.grow(slots);
            for built in &mut self.formats {
                built.effects.resize(slots, ClipVariant::NONE);
            }
            return;
        }
        self.kinds[id.idx()] = kind;
        self.shaders[id.idx()] = None;
        // Each wanted form of an effect reloaded drops its pipelines, and builds its new module
        // at the next prepare.
        let first = EffectKey::FORMS * id.idx();
        for key in (first..first + EffectKey::FORMS).map(EffectKey::of_slot) {
            for built in &mut self.formats {
                built.effects[key.slot()] = ClipVariant::NONE;
            }
            if self.wanted.contains(key.slot()) && !self.pending.contains(&key) {
                self.pending.push(key);
            }
        }
    }

    /// Want the form `key` names: the next prepare builds it, where no use wanted it before.
    fn want(&mut self, key: EffectKey) {
        if !self.wanted.put(key.slot()) {
            self.pending.push(key);
        }
    }
}

/// The compiled text of `effect`, `module`, from `shaders`, compiling it on `gpu` at its first use.
fn shader<'s>(
    shaders: &'s mut [Option<wgpu::ShaderModule>],
    gpu: &Gpu,
    effect: EffectId,
    module: &EffectModule,
) -> &'s wgpu::ShaderModule {
    shaders[effect.idx()].get_or_insert_with(|| EffectDevice::shader(gpu, effect, module))
}

/// The halves every wanted form builds: the clipless one, and the clipped one once a frame drew
/// rounded clips.
const fn halves(clipped: bool) -> &'static [Clipping] {
    if clipped {
        &[Clipping::Off, Clipping::On]
    } else {
        &[Clipping::Off]
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::gpu::pipeline::clip_variant::{ClipVariant, Clipping};
    use crate::gpu::pipeline::effect_pipeline::wgpu_effect_store::{
        FormatEffects, WgpuEffectStore,
    };
    use crate::primitives::identity::effect_id::EffectId;
    use crate::renderer::render_buffer::effect_draw_row::EffectKey;
    use crate::renderer::render_buffer::shape_form::ShapeForm;
    use crate::renderer::render_buffer::template_cover::TemplateCover;

    impl WgpuEffectStore {
        /// Whether each loaded effect's pipeline of `form`, at its id, has the half `clipping`
        /// binds built, the same for every format drawn to.
        pub(crate) fn built_effects(&self, form: ShapeForm, clipping: Clipping) -> Vec<bool> {
            (0..self.kinds.len())
                .map(|idx| {
                    let key = EffectKey {
                        effect: EffectId::from_idx(idx),
                        form,
                        cover: TemplateCover::Whole,
                    };
                    self.built_alike(|format| &format.effects[key.slot()], clipping)
                })
                .collect()
        }

        /// Whether the half `clipping` binds of the pipeline `of` picks is built, which every
        /// format drawn to agrees on.
        fn built_alike(
            &self,
            of: impl Fn(&FormatEffects) -> &ClipVariant,
            clipping: Clipping,
        ) -> bool {
            let formats = &self.formats;
            assert!(!formats.is_empty(), "a frame drew to a format");
            let built = |format| of(format).is_built(clipping);
            let first = built(&formats[0]);
            assert!(formats.iter().all(|format| built(format) == first));
            first
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use glam::{UVec2, Vec2};

    use crate::animation::animation_spec::AnimationSpec;
    use crate::effect::effect_source::EffectSource;
    use crate::gpu::device::gpu::Gpu;
    use crate::gpu::pipeline::clip_variant::Clipping;
    use crate::gpu::surface::owned_target::OwnedTarget;
    use crate::gpu::test_gpu::{headless_test_gpu, isolated_headless_test_gpu};
    use crate::gpu::texel::TargetFormat;
    use crate::host::offscreen::{OffscreenHost, OffscreenTime};
    use crate::primitives::effect::effect_name::EffectName;
    use crate::primitives::geometry::corners::Corners;
    use crate::primitives::layout::clip_mode::ClipMode;
    use crate::primitives::paint::chrome::Chrome;
    use crate::primitives::paint::color::RgbaF32;
    use crate::primitives::paint::material::Material;
    use crate::primitives::paint::material::lens::Lens;
    use crate::renderer::render_buffer::shape_form::ShapeForm;
    use crate::shape::Shape;
    use crate::shape::effect_paint::EffectPaint;
    use crate::text::shaper::TextShaper;
    use crate::theme::Theme;
    use crate::theme::color_scheme::ColorScheme;
    use crate::theme::look::press_effect::PressEffect;
    use crate::ui::Ui;
    use crate::widget_core::configure::Configure;
    use crate::widgets::panel::Panel;

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
    /// and 1), the glass (3) and `OnUse` blue (4) build nothing over a frame that draws none of
    /// them, `Now` red (5) builds at that frame, and so does the frost (2), which the stock themes'
    /// floats wear; blue builds at the frame that draws it, red staying built. A reload of blue as
    /// green builds again, so the next frame paints green. A material builds at the frame that
    /// draws it, through the one stage its effect draws every use with: the glass for a glass box,
    /// while a blurred box draws through the frost, built already. A triangle of blue builds
    /// blue's triangle form alone, at the frame that draws it, beside its box.
    #[test]
    fn an_effect_builds_where_a_use_wants_it() {
        let (mut host, target) = host();
        host.ui()
            .load_effect(BLUE, EffectSource::new(solid("0.0, 0.0, 1.0")))
            .unwrap();
        host.ui()
            .load_effect(
                RED,
                EffectSource::new(solid("1.0, 0.0, 0.0")).prebuild(true),
            )
            .unwrap();
        host.frame(target.as_target(), &mut |ui: &mut Ui| {
            ui.add_shape(Shape::rect(), RgbaF32::WHITE);
        });
        let store = |host: &OffscreenHost| {
            host.effect_store()
                .built_effects(ShapeForm::Box, Clipping::Off)
        };
        assert_eq!(store(&host), [false, false, true, false, false, true]);

        let mut blue = |ui: &mut Ui| {
            ui.add_shape(Shape::rect(), EffectPaint::new(BLUE, ()));
        };
        host.frame(target.as_target(), &mut blue);
        assert_eq!(store(&host), [false, false, true, false, true, true]);
        assert_eq!(target.read_back().get_pixel(4, 4).0, [0, 0, 255, 255]);

        host.ui()
            .load_effect(BLUE, EffectSource::new(solid("0.0, 1.0, 0.0")))
            .unwrap();
        host.frame(target.as_target(), &mut blue);
        assert_eq!(target.read_back().get_pixel(4, 4).0, [0, 255, 0, 255]);

        host.frame(target.as_target(), &mut |ui: &mut Ui| {
            ui.add_shape(Shape::rect(), RgbaF32::WHITE);
            ui.add_shape(Shape::rect(), Material::blur(3.0));
        });
        assert_eq!(store(&host), [false, false, true, false, true, true]);
        host.frame(target.as_target(), &mut |ui: &mut Ui| {
            ui.add_shape(Shape::rect(), RgbaF32::WHITE);
            ui.add_shape(Shape::rect(), Material::glass(Lens::WINDOW_GLASS));
        });
        assert_eq!(store(&host), [false, false, true, true, true, true]);

        let triangles = |host: &OffscreenHost| {
            host.effect_store()
                .built_effects(ShapeForm::Triangle, Clipping::Off)
        };
        assert_eq!(triangles(&host), [false; 6]);
        host.frame(target.as_target(), &mut |ui: &mut Ui| {
            ui.add_shape(
                Shape::triangle(Vec2::ZERO, Vec2::new(8.0, 0.0), Vec2::new(0.0, 8.0)),
                EffectPaint::new(BLUE, ()),
            );
        });
        assert_eq!(triangles(&host), [false, false, false, false, true, false]);
        assert_eq!(store(&host), [false, false, true, true, true, true]);
        assert_eq!(target.read_back().get_pixel(1, 1).0, [0, 255, 0, 255]);
    }

    /// A want builds a form's clipless half alone; the clipped half waits for the first frame
    /// whose draws name a rounded clip, then builds for every wanted form at once: blue (id 4),
    /// which that frame draws inside the clip and paints, and the frost (2) and `Now` red (5), which
    /// a theme and a load wanted and no frame draws.
    #[test]
    fn the_clipped_half_waits_for_a_frame_with_a_rounded_clip() {
        let (mut host, target) = host();
        host.ui()
            .load_effect(BLUE, EffectSource::new(solid("0.0, 0.0, 1.0")))
            .unwrap();
        host.ui()
            .load_effect(
                RED,
                EffectSource::new(solid("1.0, 0.0, 0.0")).prebuild(true),
            )
            .unwrap();
        let built = |host: &OffscreenHost, clipping| {
            host.effect_store().built_effects(ShapeForm::Box, clipping)
        };
        let wanted = [false, false, true, false, true, true];
        host.frame(target.as_target(), &mut |ui: &mut Ui| {
            ui.add_shape(Shape::rect(), EffectPaint::new(BLUE, ()));
        });
        assert_eq!(built(&host, Clipping::Off), wanted);
        assert_eq!(built(&host, Clipping::On), [false; 6]);

        host.frame(target.as_target(), &mut |ui: &mut Ui| {
            Panel::canvas()
                .id_salt("rounded")
                .size(8.0)
                .chrome(Chrome::rounded(RgbaF32::WHITE, Corners::all(3.0)))
                .clip(ClipMode::Rounded)
                .show(ui, |ui| {
                    ui.add_shape(Shape::rect(), EffectPaint::new(BLUE, ()));
                });
        });
        assert_eq!(built(&host, Clipping::On), wanted);
        assert_eq!(target.read_back().get_pixel(4, 4).0, [0, 0, 255, 255]);
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
        let mut fill = |ui: &mut Ui| {
            ui.add_shape(Shape::rect(), RgbaF32::WHITE);
        };

        let (mut host, target) = host();
        host.ui_mut()
            .set_scheme_theme(ColorScheme::Dark, sparkle())
            .unwrap();
        host.ui_mut()
            .set_scheme_theme(ColorScheme::Dark, Theme::stock(ColorScheme::Dark))
            .unwrap();
        let ripples = |host: &OffscreenHost| {
            host.effect_store()
                .built_effects(ShapeForm::Box, Clipping::Off)[..2]
                .to_vec()
        };
        host.frame(target.as_target(), &mut nothing);
        assert_eq!(ripples(&host), [false, false]);

        let shown = host.ui().color_scheme();
        let hidden = match shown {
            ColorScheme::Light => ColorScheme::Dark,
            ColorScheme::Dark => ColorScheme::Light,
        };
        host.ui_mut().set_scheme_theme(hidden, sparkle()).unwrap();
        assert_eq!(ripples(&host), [false, false]);
        host.frame(target.as_target(), &mut fill);
        assert_eq!(ripples(&host), [true, true]);
    }

    /// A store lives as long as its device: a load, a loss and a recovery onto another device, in
    /// that order, build again what the registry wants before the recovered host's first frame
    /// draws, the frost (2) its theme names and `Now` red (5), with blue (4), which that frame
    /// draws; a reload of blue after the recovery paints its new module.
    #[test]
    fn a_recovery_builds_again_what_the_registry_wants() {
        let lost = isolated_headless_test_gpu();
        let mut host = OffscreenHost::builder(lost.handles())
            .shaper(TextShaper::test_mono())
            .time(OffscreenTime::Manual)
            .build();
        host.ui()
            .load_effect(BLUE, EffectSource::new(solid("0.0, 0.0, 1.0")))
            .unwrap();
        host.ui()
            .load_effect(
                RED,
                EffectSource::new(solid("1.0, 0.0, 0.0")).prebuild(true),
            )
            .unwrap();
        let mut blue = |ui: &mut Ui| {
            ui.add_shape(Shape::rect(), EffectPaint::new(BLUE, ()));
        };
        let window = |gpu: &Gpu| {
            OwnedTarget::window(
                gpu,
                "palantir.effect.recovery",
                UVec2::new(8, 8),
                TargetFormat::new(wgpu::TextureFormat::Rgba8UnormSrgb),
            )
        };
        host.frame(window(&lost).as_target(), &mut blue);
        lost.lose();

        let gpu = headless_test_gpu();
        host.recover(gpu.handles());
        let target = window(&gpu);
        host.frame(target.as_target(), &mut blue);
        assert_eq!(
            host.effect_store()
                .built_effects(ShapeForm::Box, Clipping::Off),
            [false, false, true, false, true, true]
        );
        assert_eq!(target.read_back().get_pixel(4, 4).0, [0, 0, 255, 255]);

        host.ui()
            .load_effect(BLUE, EffectSource::new(solid("0.0, 1.0, 0.0")))
            .unwrap();
        host.frame(target.as_target(), &mut blue);
        assert_eq!(target.read_back().get_pixel(4, 4).0, [0, 255, 0, 255]);
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
            ui.add_shape(Shape::rect(), RgbaF32::new(0.2, 0.4, 0.6, 1.0));
            ui.add_shape(Shape::rect(), EffectPaint::new(BLUE, ()));
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
