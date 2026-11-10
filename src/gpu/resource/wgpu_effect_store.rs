//! The pipelines of the effects a host loaded, built where a use becomes known.

use std::cell::{Ref, RefCell};

use fixedbitset::FixedBitSet;

use crate::effect::effect_store::{EffectModule, EffectStore};
use crate::gpu::device::gpu::Gpu;
use crate::gpu::pipeline::clip_variant::{ClipVariant, Clipping};
use crate::gpu::pipeline::effect_pipeline::{EffectDevice, EffectTarget};
use crate::gpu::resource::texture_binding::TextureBinding;
use crate::primitives::identity::effect_id::EffectId;
use crate::renderer::render_buffer::effect_draw_row::EffectKey;
use crate::renderer::render_buffer::shape_form::ShapeForm;

/// The [`EffectStore`] a host's registry loads through and its backend draws from. It keeps each
/// effect's module, and builds the pipelines some use wants at one place, the start of a frame's
/// GPU phase ([`Self::prepare`]), before any pass: the box form of an effect a load wants built at
/// once and of each effect a window's themes name, and the form of each draw a frame holds. A
/// theme's effects so build together before a press can reach them, and nothing builds that no use
/// wants, as a pipeline-state cache with a warm-up does (Unity's
/// `ShaderVariantCollection.WarmUp`, Unreal's PSO precaching). Refused: building at the load or the
/// theme set, which a later one replaces for nothing; building at the first press, which stutters,
/// the reason Flutter wrote Impeller; building on a worker, which shows an effect late; every form
/// at a want, two pipelines where a press and a material draw a box. Cost: an effect's first
/// triangle builds at the frame that draws it.
///
/// Each wanted form builds the half of its [`ClipVariant`] a frame with no rounded clip binds, and
/// the half that takes them from the first frame that draws one, every wanted form at once, as
/// every other family builds a half on the first frame whose census asks for it. Refused: both
/// halves at the want, which compiles a program an app with no rounded clip never binds, at its
/// load and at each device recovery. Cost: that first frame builds the clipped half of every
/// wanted form.
///
/// A wanted form stays built, for every format and every device after.
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
    /// The forms some use wanted, at each key's [`EffectKey::slot`]: built for every format of
    /// every device.
    wanted: FixedBitSet,
    /// The wanted forms the attached device builds at the next [`WgpuEffectStore::prepare`]: a new
    /// want, a want reloaded with a new module, or every want when the clipped half is first
    /// asked for.
    pending: Vec<EffectKey>,
    /// Whether a frame drew rounded clips, from which on every wanted form builds the clipped half
    /// too.
    clipped: bool,
    /// `None` until the first attach.
    attached: Option<AttachedEffects>,
    /// Moves with every build, so a reader of [`AttachedEffects::for_format`] knows to copy again.
    revision: u64,
}

/// The store's side of one device: the device every effect builds on, and the pipelines of each
/// format it draws to.
#[derive(Debug)]
pub(crate) struct AttachedEffects {
    pub(crate) device: EffectDevice,
    /// One for each format, a few in all, in the order each was first drawn to: a frame reads
    /// one format's pipelines whole, and a build adds one to each. Refused: a table of each
    /// effect's pipelines for each format, which a frame reads one entry a row of; one flat table,
    /// which a new format reshapes.
    formats: Vec<FormatEffects>,
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
    /// Build on `gpu` from now on, binding images through `binding`, the device's: every wanted
    /// effect again, for each format the backend names ([`Self::ensure_format`]).
    pub(crate) fn attach(&self, gpu: &Gpu, binding: &TextureBinding) {
        let mut state = self.state.borrow_mut();
        state.attached = Some(AttachedEffects {
            device: EffectDevice::new(gpu, binding),
            formats: Vec::new(),
        });
        // A new format builds every wanted effect, so nothing is pending on a fresh device.
        state.pending.clear();
        state.revision += 1;
    }

    /// Build every wanted form for `format`, where it is new: before the frame that paints it.
    pub(crate) fn ensure_format(&self, format: wgpu::TextureFormat) {
        let mut state = self.state.borrow_mut();
        let StoreState {
            modules,
            wanted,
            clipped,
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
            effects: vec![ClipVariant::NONE; modules.len() * ShapeForm::ALL.len()],
        };
        for slot in wanted.ones() {
            let key = EffectKey::of_slot(slot);
            attached.device.ensure(
                &mut built.effects[slot],
                EffectTarget {
                    module: &modules[key.effect.idx()],
                    key,
                    format,
                    halves: halves(*clipped),
                },
            );
        }
        attached.formats.push(built);
        *revision += 1;
    }

    /// The start of a frame's GPU phase: want each key of `keys`, the frame's draws, and build
    /// every wanted form's halves not built yet for each format, before any pass; a frame of
    /// `clipping` [`Clipping::On`] first asks for the clipped half of every wanted form.
    pub(crate) fn prepare(&self, keys: &[EffectKey], clipping: Clipping) {
        let mut state = self.state.borrow_mut();
        for &key in keys {
            state.want(key);
        }
        let StoreState {
            modules,
            wanted,
            clipped,
            attached,
            pending,
            revision,
        } = &mut *state;
        if clipping == Clipping::On && !*clipped {
            *clipped = true;
            pending.clear();
            pending.extend(wanted.ones().map(EffectKey::of_slot));
        }
        let Some(attached) = attached else {
            return;
        };
        if pending.is_empty() {
            return;
        }
        let AttachedEffects { device, formats } = attached;
        for key in pending.drain(..) {
            for built in formats.iter_mut() {
                device.ensure(
                    &mut built.effects[key.slot()],
                    EffectTarget {
                        module: &modules[key.effect.idx()],
                        key,
                        format: built.format,
                        halves: halves(*clipped),
                    },
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

/// The halves every wanted form builds: the clipless one, and the clipped one once a frame drew
/// rounded clips.
const fn halves(clipped: bool) -> &'static [Clipping] {
    if clipped {
        &[Clipping::Off, Clipping::On]
    } else {
        &[Clipping::Off]
    }
}

impl StoreState {
    /// Want the form `key` names: the next prepare builds it, where no use wanted it before.
    fn want(&mut self, key: EffectKey) {
        if !self.wanted.contains(key.slot()) {
            self.wanted.insert(key.slot());
            self.pending.push(key);
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
    fn register(&self, id: EffectId, module: &EffectModule) {
        let mut state = self.state.borrow_mut();
        let StoreState {
            modules,
            wanted,
            pending,
            attached,
            revision,
            ..
        } = &mut *state;
        debug_assert!(
            id.idx() <= modules.len(),
            "the registry hands out ids in order"
        );
        let slots = (modules.len() + 1) * ShapeForm::ALL.len();
        if id.idx() == modules.len() {
            modules.push(module.clone());
            wanted.grow(slots);
            if let Some(attached) = attached {
                for built in &mut attached.formats {
                    built.effects.resize(slots, ClipVariant::NONE);
                }
            }
        } else {
            module.clone_into(&mut modules[id.idx()]);
            // Each wanted form of an effect reloaded drops its pipelines, and builds its new
            // module at the next prepare.
            for form in ShapeForm::ALL {
                let key = EffectKey { effect: id, form };
                if let Some(attached) = attached {
                    for built in &mut attached.formats {
                        built.effects[key.slot()] = ClipVariant::NONE;
                    }
                }
                if wanted.contains(key.slot()) && !pending.contains(&key) {
                    pending.push(key);
                }
            }
            *revision += 1;
        }
    }

    fn want(&self, id: EffectId) {
        self.state.borrow_mut().want(EffectKey {
            effect: id,
            form: ShapeForm::Box,
        });
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::gpu::pipeline::clip_variant::{ClipVariant, Clipping};
    use crate::gpu::resource::wgpu_effect_store::{ATTACHED, FormatEffects, WgpuEffectStore};
    use crate::primitives::identity::effect_id::EffectId;
    use crate::renderer::render_buffer::effect_draw_row::EffectKey;
    use crate::renderer::render_buffer::shape_form::ShapeForm;

    impl WgpuEffectStore {
        /// Whether each loaded effect's pipeline of `form`, at its id, has the half `clipping`
        /// binds built, the same for every format drawn to.
        pub(crate) fn built_effects(&self, form: ShapeForm, clipping: Clipping) -> Vec<bool> {
            let state = self.state.borrow();
            (0..state.modules.len())
                .map(|idx| {
                    let key = EffectKey {
                        effect: EffectId(u32::try_from(idx).unwrap()),
                        form,
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
            let state = self.state.borrow();
            let formats = &state.attached.as_ref().expect(ATTACHED).formats;
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
    use crate::effect::effect_source::{EffectBuild, EffectSource};
    use crate::gpu::pipeline::clip_variant::Clipping;
    use crate::gpu::surface::owned_target::OwnedTarget;
    use crate::gpu::surface::render_target::TargetFormat;
    use crate::gpu::test_gpu::headless_test_gpu;
    use crate::host::offscreen::{OffscreenHost, OffscreenTime};
    use crate::primitives::effect::effect_name::EffectName;
    use crate::primitives::geometry::corners::Corners;
    use crate::primitives::layout::clip_mode::ClipMode;
    use crate::primitives::paint::chrome::Chrome;
    use crate::primitives::paint::color::RgbaF32;
    use crate::primitives::paint::material::Material;
    use crate::primitives::paint::paint::Paint;
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
    /// and 1), the blur and the glass (2 and 4) and `OnUse` blue (5) build nothing over a frame
    /// that draws none of them, `Now` red (6) builds at that frame, and so does the frost (3), which
    /// the stock themes' floats wear; blue builds at the frame that draws it, red staying built. A
    /// reload of blue as green builds again, so the next frame paints green. A material builds at
    /// the frame that draws it, through the one stage its effect draws every use with: the blur
    /// for a blurred box. A triangle of blue builds blue's triangle form alone, at the frame that
    /// draws it, beside its box.
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
            ui.add_shape(Shape::rect(), RgbaF32::WHITE);
        });
        let store = |host: &OffscreenHost| {
            host.effect_store()
                .built_effects(ShapeForm::Box, Clipping::Off)
        };
        assert_eq!(
            store(&host),
            [false, false, false, true, false, false, true]
        );

        let mut blue = |ui: &mut Ui| {
            ui.add_shape(Shape::rect(), EffectPaint::new(BLUE, ()));
        };
        host.frame(target.as_target(), &mut blue);
        assert_eq!(store(&host), [false, false, false, true, false, true, true]);
        assert_eq!(target.read_back().get_pixel(4, 4).0, [0, 0, 255, 255]);

        host.ui()
            .load_effect(BLUE, EffectSource::new(solid("0.0, 1.0, 0.0")))
            .unwrap();
        host.frame(target.as_target(), &mut blue);
        assert_eq!(target.read_back().get_pixel(4, 4).0, [0, 255, 0, 255]);

        host.frame(target.as_target(), &mut |ui: &mut Ui| {
            ui.add_shape(Shape::rect(), RgbaF32::WHITE);
            ui.add_shape(
                Shape::rect(),
                Paint::NONE.with_material(Material::blur(3.0)),
            );
        });
        assert_eq!(store(&host), [false, false, true, true, false, true, true]);

        let triangles = |host: &OffscreenHost| {
            host.effect_store()
                .built_effects(ShapeForm::Triangle, Clipping::Off)
        };
        assert_eq!(triangles(&host), [false; 7]);
        host.frame(target.as_target(), &mut |ui: &mut Ui| {
            ui.add_shape(
                Shape::triangle(Vec2::ZERO, Vec2::new(8.0, 0.0), Vec2::new(0.0, 8.0)),
                EffectPaint::new(BLUE, ()),
            );
        });
        assert_eq!(
            triangles(&host),
            [false, false, false, false, false, true, false]
        );
        assert_eq!(store(&host), [false, false, true, true, false, true, true]);
        assert_eq!(target.read_back().get_pixel(1, 1).0, [0, 255, 0, 255]);
    }

    /// A want builds a form's clipless half alone; the clipped half waits for the first frame
    /// whose draws name a rounded clip, then builds for every wanted form at once: blue (id 5),
    /// which that frame draws inside the clip and paints, and the frost (3) and `Now` red (6), which
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
                EffectSource::new(solid("1.0, 0.0, 0.0")).build(EffectBuild::Now),
            )
            .unwrap();
        let built = |host: &OffscreenHost, clipping| {
            host.effect_store().built_effects(ShapeForm::Box, clipping)
        };
        let wanted = [false, false, false, true, false, true, true];
        host.frame(target.as_target(), &mut |ui: &mut Ui| {
            ui.add_shape(Shape::rect(), EffectPaint::new(BLUE, ()));
        });
        assert_eq!(built(&host, Clipping::Off), wanted);
        assert_eq!(built(&host, Clipping::On), [false; 7]);

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
            .set_theme(ColorScheme::Dark, sparkle())
            .unwrap();
        host.ui_mut()
            .set_theme(ColorScheme::Dark, Theme::stock(ColorScheme::Dark))
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
        host.ui_mut().set_theme(hidden, sparkle()).unwrap();
        assert_eq!(ripples(&host), [false, false]);
        host.frame(target.as_target(), &mut fill);
        assert_eq!(ripples(&host), [true, true]);
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
