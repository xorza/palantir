//! Loading effects: the derive's layout, each refusal, a replacing load, a refused reload, and a
//! name used with other types; the time each frame reads, reduced motion and the power-saver
//! alternative; on a device, each blend, a clip, a recovery, a reload, the time and the images.

use crate::shape::effect_paint::EffectPaint;
use std::error;
use std::ptr;
use std::rc::Rc;

use glam::{UVec2, Vec2};
use half::f16;
use std::time::Duration;

use crate::animation::motion_role::MotionRole;
use crate::effect::contract::{self, ContractSpec, ParamsLayout};
use crate::effect::effect_module::EffectModule;
use crate::effect::effect_registry::{EffectRegistry, MaterialFit, ReducedName};
use crate::effect::effect_source::{EffectSource, ReducedEffect};
use crate::effect::error::{EffectError, EntryMismatch, ParamMismatch};
use crate::effect::press_params::PressParams;
use crate::effect::stock;
use crate::gpu::device::gpu::Gpu;
use crate::gpu::test_gpu::headless_test_gpu;
use crate::gpu::test_gpu::isolated_headless_test_gpu;
use crate::host::harness::Harness;
use crate::host::harness::harness_builder::HarnessDepth;
use crate::image_registry::image_handle::ImageHandle;
use crate::internals::paint_capture::PaintCall;
use crate::internals::panic_probe::assert_panics_with;
use crate::internals::srgb_reference;
use crate::primitives::effect::effect_kind::{BackdropEffect, ColorEffect, KindId};
use crate::primitives::effect::effect_name::EffectName;
use crate::primitives::effect::effect_params::{EffectParam, EffectParams, ParamType};
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::effect_id::EffectId;
use crate::primitives::layout::clip_mode::ClipMode;
use crate::primitives::layout::sizing::{SizeSpec, Sizing};
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::srgba_u8::SrgbaU8;
use crate::primitives::paint::image::Image;
use crate::primitives::paint::material::Material;
use crate::primitives::paint::material::backdrop_reach::BackdropReach;
use crate::primitives::paint::material::frosted_params::FrostedParams;
use crate::primitives::paint::material::glass_params::GlassParams;
use crate::primitives::paint::material::lens::Lens;
use crate::primitives::paint::material::material_effect::MaterialEffect;
use crate::primitives::preference::Preference;
use crate::primitives::reduction::Reduction;
use crate::renderer::frontend::payload::draw_closed_payload::closed_effect::{
    EffectCaptureDraw, EffectRead,
};
use crate::shape::Shape;
use crate::shape::paint::capture_params::CaptureReach;
use crate::shape::paint_animation::{PaintAnimation, PaintRepeat};
use crate::theme::Theme;
use crate::theme::error::ThemeEffectError;
use crate::theme::look::press_effect::PressEffect;
use crate::ui::Ui;
use crate::ui::frame_report::FramePaint;
use crate::ui::ui_preferences::UiPreferences;
use crate::widget_core::configure::Configure;
use crate::widgets::panel::Panel;

/// The contract an effect of `kind` over `params` with `image_slots` images is checked against.
const fn spec(kind: KindId, params: ParamsLayout, image_slots: u8) -> ContractSpec {
    ContractSpec {
        kind,
        params,
        image_slots,
    }
}

/// The id of the app's load `n`, from 0, past the stock effects every registry holds first.
fn app(n: u32) -> EffectId {
    let count = stock::effects().press.len() + 2;
    EffectId(u32::try_from(count).unwrap() + n)
}

/// A ring's parameters: a vec4 and four f32s, 32 B in Rust and in WGSL.
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable, palantir_derive::EffectParams)]
#[repr(C)]
struct Ring {
    tint: [f32; 4],
    radius: f32,
    width: f32,
    edge: f32,
    r#speed: f32,
}

const RING: EffectName<Ring> = EffectName::named("ring").expect("the name fits");
const THIN_RING: EffectName<Ring> = EffectName::named("thin ring").expect("the name fits");

const RING_PARAMS: &str = "
struct Params { tint: vec4<f32>, radius: f32, width: f32, edge: f32, speed: f32 }
";

/// A ring's WGSL with `params` as its parameter struct and `entry` as its function.
fn ring_wgsl(params: &str, entry: &str) -> String {
    format!("{params}\n{entry}")
}

const RING_ENTRY: &str = "
fn effect(input: EffectInput, params: Params) -> EffectOutput {
    return EffectOutput(params.tint, vec4<f32>(0.0));
}
";

fn ring(wgsl: impl Into<String>) -> EffectSource<Ring> {
    EffectSource::new(wgsl.into())
}

fn load(wgsl: &str) -> Result<(), EffectError> {
    EffectRegistry::default().load(RING, &ring(wgsl.to_owned()))
}

/// The derive lists each field in order with its `offset_of!` and its WGSL type: the vec4 at 0,
/// then the f32s at 16, 20, 24 and 28. A raw identifier keeps its plain name.
#[test]
fn the_derive_describes_each_field() {
    let field = |name, offset, ty| EffectParam { name, offset, ty };
    assert_eq!(
        Ring::FIELDS,
        [
            field("tint", 0, ParamType::Vec4F32),
            field("radius", 16, ParamType::F32),
            field("width", 20, ParamType::F32),
            field("edge", 24, ParamType::F32),
            field("speed", 28, ParamType::F32),
        ]
    );
    assert_eq!(<() as EffectParams>::FIELDS, []);
}

/// A matching ring loads; one with no parameters loads with a one-argument `effect`, and a
/// backdrop kind loads under the same contract. Through the `Ui` too.
#[test]
fn a_matching_effect_loads() {
    let registry = EffectRegistry::default();
    assert!(!registry.contains(RING));
    registry
        .load(RING, &ring(ring_wgsl(RING_PARAMS, RING_ENTRY)))
        .unwrap();
    assert!(registry.contains(RING));
    assert!(!registry.contains(THIN_RING));

    let plain: EffectName<()> = EffectName::named("plain").unwrap();
    let plain_wgsl = "fn effect(input: EffectInput) -> EffectOutput {
        return EffectOutput(vec4<f32>(1.0), vec4<f32>(0.0));
    }";
    registry
        .load(plain, &EffectSource::new(plain_wgsl))
        .unwrap();
    let water: EffectName<(), BackdropEffect> = EffectName::named("water").unwrap();
    registry
        .load(water, &EffectSource::new(plain_wgsl))
        .unwrap();
    assert!(registry.contains(water));

    let h = Harness::new(UVec2::new(8, 8));
    assert!(!h.ui().has_effect(RING));
    h.ui()
        .load_effect(RING, ring(ring_wgsl(RING_PARAMS, RING_ENTRY)))
        .unwrap();
    assert!(h.ui().has_effect(RING));
}

/// Each refusal its own case: text that does not parse, a module that does not validate, no
/// `effect`, each signature departure, too many image slots, a binding of the app's own.
#[test]
fn each_refusal_is_its_own_case() {
    let parse = load("fn effect(").unwrap_err();
    let EffectError::Parse(wgsl) = &parse else {
        panic!("{parse:?}");
    };
    assert!(
        wgsl.diagnostic().contains("struct EffectInput"),
        "the error at the end of the app's text prints the contract's line"
    );
    assert_eq!(
        wgsl.span(),
        None,
        "it points past the app's text, into the prelude"
    );
    assert_eq!(parse.to_string(), "the effect's WGSL does not parse");
    assert_eq!(
        error::Error::source(&parse).map(ToString::to_string),
        Some(wgsl.to_string()),
        "naga's message is the source"
    );
    assert!(error::Error::source(wgsl).is_none(), "and shows once");

    let stray = "fn effect() { let = 1.0; }";
    let EffectError::Parse(wgsl) = load(stray).unwrap_err() else {
        panic!("a stray `=` does not parse");
    };
    let span = wgsl.span().expect("the error points into the app's text");
    assert_eq!(&stray[span], "=", "{}", wgsl.diagnostic());

    // Only the second pass, over palantir's whole prelude, meets the name again; naga labels the
    // prelude's definition first, and the span is the app's.
    let shadow = format!("{RING_PARAMS}\nfn palantir_shape_point() {{}}\n{RING_ENTRY}");
    let EffectError::Parse(wgsl) = load(&shadow).unwrap_err() else {
        panic!("a name the prelude defines does not parse");
    };
    let span = wgsl.span().expect("the app's definition is labeled");
    assert_eq!(&shadow[span.clone()], "palantir_shape_point");
    assert_eq!(
        span.start,
        RING_PARAMS.len() + "\nfn ".len(),
        "{}",
        wgsl.diagnostic()
    );

    let invalid = format!(
        "{RING_PARAMS}
        @group(0) @binding(0) var<uniform> unsized_values: array<f32>;
        {RING_ENTRY}"
    );
    let validation = load(&invalid).unwrap_err();
    let EffectError::Validation(wgsl) = &validation else {
        panic!("{validation:?}");
    };
    assert!(wgsl.diagnostic().contains("unsized_values"));
    let span = wgsl.span().expect("the error points into the app's text");
    assert!(
        invalid[span].contains("unsized_values"),
        "{}",
        wgsl.diagnostic()
    );
    assert_eq!(
        validation.to_string(),
        "the effect's WGSL does not validate"
    );
    assert!(matches!(
        load(&ring_wgsl(RING_PARAMS, "fn other() {}")),
        Err(EffectError::MissingEntry)
    ));

    let entry = |entry: &str| match load(&ring_wgsl(RING_PARAMS, entry)) {
        Err(EffectError::Entry(mismatch)) => mismatch,
        other => panic!("{entry}: {other:?}"),
    };
    assert_eq!(
        entry("fn effect(input: EffectInput) -> EffectOutput { return EffectOutput(); }"),
        EntryMismatch::ArgumentCount { want: 2, got: 1 }
    );
    assert_eq!(
        entry(
            "fn effect(input: vec4<f32>, params: Params) -> EffectOutput { return EffectOutput(); }"
        ),
        EntryMismatch::Input
    );
    assert_eq!(
        entry("@fragment fn effect() -> @location(0) vec4<f32> { return vec4<f32>(); }"),
        EntryMismatch::Stage
    );
    assert_eq!(
        entry(
            "fn effect(input: EffectInput, params: f32) -> EffectOutput { return EffectOutput(); }"
        ),
        EntryMismatch::Params
    );
    assert_eq!(
        entry("fn effect(input: EffectInput, params: Params) -> vec4<f32> { return vec4<f32>(); }"),
        EntryMismatch::Output
    );

    let slots = EffectRegistry::default().load(
        RING,
        &ring(ring_wgsl(RING_PARAMS, RING_ENTRY)).image_slots(5),
    );
    assert!(matches!(
        slots,
        Err(EffectError::TooManyImageSlots { slots: 5 })
    ));
    assert!(
        EffectRegistry::default()
            .load(
                RING,
                &ring(ring_wgsl(RING_PARAMS, RING_ENTRY)).image_slots(4)
            )
            .is_ok()
    );

    let own = format!(
        "{RING_PARAMS}
        @group(2) @binding(0) var other: texture_2d<f32>;
        {RING_ENTRY}"
    );
    let binding = load(&own).unwrap_err();
    assert!(
        matches!(&binding, EffectError::Binding { name } if name == "other"),
        "{binding:?}"
    );
    assert_eq!(
        binding.to_string(),
        "the effect's WGSL binds `other` itself, where palantir binds every resource"
    );
    // A name like a slot's binds of its own past the slots declared: no slot here, so neither
    // the sampler nor `effect_image0` is the contract's.
    for name in ["effect_image0", "effect_sampler"] {
        let ty = if name == "effect_sampler" {
            "sampler"
        } else {
            "texture_2d<f32>"
        };
        let posing = format!(
            "{RING_PARAMS}
            @group(1) @binding(3) var {name}: {ty};
            {RING_ENTRY}"
        );
        assert!(
            matches!(load(&posing), Err(EffectError::Binding { name: got }) if got == name),
            "{name}"
        );
    }
}

/// A source of `n` image slots declares the sampler at binding 0 of group 1 and `effect_image0` to
/// `effect_image{n-1}` at bindings 1 to `n`: two slots read `effect_image1`, and the module
/// carries both declarations and the slot count; `effect_image2` is no name with two slots, and
/// `effect_sampler` none with no slot.
#[test]
fn image_slots_declare_their_images() {
    let params = ParamsLayout {
        fields: Ring::FIELDS,
        size: size_of::<Ring>(),
    };
    let sampling = |image: &str| {
        ring_wgsl(
            RING_PARAMS,
            &format!(
                "fn effect(input: EffectInput, params: Params) -> EffectOutput {{
                    return effect_over(textureSample({image}, effect_sampler, input.pos / input.size));
                }}"
            ),
        )
    };
    let module =
        contract::check(&sampling("effect_image1"), spec(KindId::Color, params, 2)).unwrap();
    assert_eq!(module.image_slots, 2);
    for line in [
        "@group(1) @binding(0) var effect_sampler: sampler;",
        "@group(1) @binding(1) var effect_image0: texture_2d<f32>;",
        "@group(1) @binding(2) var effect_image1: texture_2d<f32>;",
    ] {
        assert!(module.text.contains(line), "{line}");
    }
    assert!(!module.text.contains("effect_image2:"));
    assert!(matches!(
        contract::check(&sampling("effect_image2"), spec(KindId::Color, params, 2)),
        Err(EffectError::Parse(_))
    ));
    assert!(matches!(
        contract::check(&sampling("effect_image0"), spec(KindId::Color, params, 0)),
        Err(EffectError::Parse(_))
    ));
}

/// The parameter struct against `Ring`, one departure each: a member short, a member renamed, a
/// member moved by `@align(32)`, a member retyped, and a span grown by `@size(16)` on the last,
/// from 32 B to 48 B.
#[test]
fn each_parameter_departure_names_itself() {
    let layout = |params: &str| match load(&ring_wgsl(params, RING_ENTRY)) {
        Err(EffectError::ParamLayout(mismatch)) => mismatch,
        other => panic!("{params}: {other:?}"),
    };
    assert_eq!(
        layout("struct Params { tint: vec4<f32>, radius: f32, width: f32, edge: f32 }"),
        ParamMismatch::FieldCount { wgsl: 4, rust: 5 }
    );
    assert_eq!(
        layout(
            "struct Params { tint: vec4<f32>, radius: f32, thickness: f32, edge: f32, speed: f32 }"
        ),
        ParamMismatch::Name {
            idx: 2,
            wgsl: "thickness".to_owned(),
            rust: "width",
        }
    );
    assert_eq!(
        layout(
            "struct Params { tint: vec4<f32>, @align(32) radius: f32, width: f32, edge: f32, speed: f32 }"
        ),
        ParamMismatch::Offset {
            name: "radius",
            wgsl: 32,
            rust: 16,
        }
    );
    assert_eq!(
        layout("struct Params { tint: vec4<f32>, radius: u32, width: f32, edge: f32, speed: f32 }"),
        ParamMismatch::Type {
            name: "radius",
            rust: ParamType::F32,
        }
    );
    assert_eq!(
        layout(
            "struct Params { tint: vec4<f32>, radius: f32, width: f32, edge: f32, @size(16) speed: f32 }"
        ),
        ParamMismatch::Size { wgsl: 48, rust: 32 }
    );
    assert_eq!(
        EffectError::ParamLayout(ParamMismatch::Type {
            name: "radius",
            rust: ParamType::F32,
        })
        .to_string(),
        "the parameter struct types `radius` other than the Rust field's `f32`"
    );
}

/// 260 B of parameters, past the 256 B an instance holds, is refused before the WGSL is read.
/// Whether `effect` takes parameters follows `U`'s bytes, not its field list: 16 B listed as no
/// fields want a second argument. The source's `Debug` and the alternative's equality need
/// nothing of `U`.
#[test]
fn parameters_past_the_limit_are_refused() {
    #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
    #[repr(C)]
    struct Huge {
        data: [u32; 65],
    }
    impl EffectParams for Huge {
        const FIELDS: &'static [EffectParam] = &[];
    }
    #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
    #[repr(C)]
    struct Unlisted {
        data: [u32; 4],
    }
    impl EffectParams for Unlisted {
        const FIELDS: &'static [EffectParam] = &[];
    }
    let huge: EffectName<Huge> = EffectName::named("huge").unwrap();
    let refused = EffectRegistry::default().load(huge, &EffectSource::new("not read"));
    assert!(matches!(
        refused,
        Err(EffectError::ParamsTooLarge { size: 260 })
    ));
    let message = refused.unwrap_err().to_string();
    assert_eq!(
        message,
        "the parameters are 260 B, past the 256 B an instance holds"
    );

    let unlisted: EffectName<Unlisted> = EffectName::named("unlisted").unwrap();
    let one_argument = "fn effect(input: EffectInput) -> EffectOutput { return EffectOutput(); }";
    let refused = EffectRegistry::default().load(unlisted, &EffectSource::new(one_argument));
    assert!(
        matches!(
            refused,
            Err(EffectError::Entry(EntryMismatch::ArgumentCount {
                want: 2,
                got: 1
            }))
        ),
        "{refused:?}"
    );

    let source = EffectSource::<Huge>::new("x").power_saver(ReducedEffect::Nothing);
    assert!(format!("{source:?}").contains("Nothing"));
    assert_eq!(
        ReducedEffect::Other(huge),
        ReducedEffect::<Huge>::Other(huge)
    );
    assert_ne!(ReducedEffect::<Huge>::Same, ReducedEffect::Nothing);
}

/// A backdrop effect's reach is its source's, over each paint's own parameters, so two paints of
/// one effect read as far as theirs give: `|dx|` here, 3 and 7. A color effect keeps none.
#[test]
fn a_backdrop_effects_reach_is_its_sources_over_each_paints_parameters() {
    let registry = EffectRegistry::default();
    registry
        .load(
            SHIFT,
            &EffectSource::new(SHIFT_WGSL).reach(|shift: &Shift| BackdropReach::Px(shift.dx.abs())),
        )
        .unwrap();
    let id = registry.id_of_name(SHIFT);
    let reach = |dx: f32| registry.reach_of(id, bytemuck::bytes_of(&Shift { dx }));
    assert_eq!(reach(-3.0), BackdropReach::Px(3.0));
    assert_eq!(reach(7.0), BackdropReach::Px(7.0));
    registry
        .load(RING, &ring(ring_wgsl(RING_PARAMS, RING_ENTRY)))
        .unwrap();
    let ring_id = registry.id_of_name(RING);
    assert_panics_with("a backdrop effect keeps its reach", || {
        registry.reach_of(ring_id, &[0; 32])
    });
}

/// A second load under a name replaces the first, its power-saver alternative with it; a reload
/// that fails returns its error and leaves the effect loaded before.
#[test]
fn a_load_replaces_and_a_refused_reload_keeps_the_last() {
    let registry = EffectRegistry::default();
    let valid = ring_wgsl(RING_PARAMS, RING_ENTRY);
    registry.load(RING, &ring(valid.clone())).unwrap();
    assert_eq!(registry.power_saver_of(RING), Some(ReducedName::Same));
    registry.load(THIN_RING, &ring(valid.clone())).unwrap();
    let thin = ReducedName::Other(registry.id_of_name(THIN_RING));
    registry
        .load(
            RING,
            &ring(valid).power_saver(ReducedEffect::Other(THIN_RING)),
        )
        .unwrap();
    assert_eq!(registry.power_saver_of(RING), Some(thin));

    let refused = registry.load(
        RING,
        &ring("fn effect(").power_saver(ReducedEffect::Nothing),
    );
    assert!(matches!(refused, Err(EffectError::Parse(_))));
    assert!(registry.contains(RING));
    assert_eq!(
        registry.power_saver_of(RING),
        Some(thin),
        "the effect before the refused reload"
    );
}

/// Every registry holds the stock ripples from its start, the sparkle saving power with the flat
/// one, and no app load takes a name in their prefix, valid source or not, so none replaces them.
#[test]
fn the_stock_ripples_hold_their_reserved_names() {
    let registry = EffectRegistry::default();
    assert!(registry.contains(EffectName::RIPPLE));
    assert_eq!(
        registry.power_saver_of(EffectName::RIPPLE),
        Some(ReducedName::Same)
    );
    assert_eq!(
        registry.power_saver_of(EffectName::SPARKLE),
        Some(ReducedName::Other(registry.id_of_name(EffectName::RIPPLE)))
    );
    let reserved = EffectName::<Ring>::named("palantir.ring").unwrap();
    let refused = registry.load(reserved, &ring(ring_wgsl(RING_PARAMS, RING_ENTRY)));
    assert!(
        matches!(refused, Err(EffectError::ReservedName)),
        "{refused:?}"
    );
    assert_eq!(
        refused.unwrap_err().to_string(),
        "the effect's name starts with `palantir.`, which palantir's stock effects hold"
    );
    assert!(!registry.contains(reserved));
    let ripple = EffectName::<PressParams>::named("palantir.ripple").unwrap();
    assert!(matches!(
        registry.load(ripple, &EffectSource::new("fn effect(")),
        Err(EffectError::ReservedName)
    ));
    assert_eq!(registry.epoch(), 4, "the four stock loads alone");
}

/// The stock effects are checked once a process: every call hands back the same effects, and each
/// holds the module checking its source makes, which a registry registers as it would an app's.
#[test]
fn the_stock_effects_are_checked_once_as_an_apps_are() {
    let effects = stock::effects();
    assert!(ptr::eq(effects, stock::effects()), "one check a process");
    let check = |module: &EffectModule, checked: Result<EffectModule, EffectError>| {
        assert_eq!(*module, checked.expect("a stock effect meets the contract"));
    };
    for press in &effects.press {
        check(&press.module, press.source.check());
    }
    check(&effects.frosted.module, effects.frosted.source.check());
    check(&effects.glass.module, effects.glass.source.check());
}

/// An app's press effect: its press color, over.
const RIPPLE_WGSL: &str = concat!(
    include_str!("stock/press.wgsl"),
    "fn effect(input: EffectInput, params: Params) -> EffectOutput {
        return effect_over(params.color);
    }"
);

/// A theme's press slots name only effects over `PressParams` that are loaded, and its chromes'
/// materials only backdrop effects over `MaterialInput`: one nothing loaded and one of other
/// parameters are each refused with the name, and the theme held before stays; a stock ripple, a
/// stock material and an app's press effect are taken.
#[test]
fn a_theme_names_only_loaded_effects() {
    let mut h = Harness::mono(UVec2::new(16, 16));
    h.ui()
        .load_effect(BLEND, EffectSource::new(BLEND_WGSL))
        .unwrap();
    let scheme = h.ui().color_scheme();
    let held = Rc::clone(h.ui().scheme_theme(scheme));
    let naming = |name: &str| {
        let mut theme = Theme::stock(scheme);
        theme.menu.item.defaults.press = Some(PressEffect {
            effect: EffectName::named(name).unwrap(),
            color: RgbaF32::WHITE,
            highlight: RgbaF32::WHITE,
            enter: Duration::ZERO,
            exit: Duration::ZERO,
        });
        theme
    };
    let missing = EffectName::named("missing").unwrap();
    assert_eq!(
        h.ui_mut().set_scheme_theme(scheme, naming("missing")),
        Err(ThemeEffectError::UnloadedPress(missing))
    );
    assert_eq!(
        ThemeEffectError::UnloadedPress(missing).to_string(),
        "the theme's press effect `missing` is not loaded"
    );
    let blend = EffectName::named(BLEND.name()).unwrap();
    assert_eq!(
        h.ui_mut().set_scheme_theme(scheme, naming(BLEND.name())),
        Err(ThemeEffectError::NotPress(blend))
    );
    assert_eq!(
        ThemeEffectError::NotPress(blend).to_string(),
        "the theme's press effect `blend` is not a color effect over `PressParams`"
    );
    // A slot's material is checked as a press slot is: a name nothing loaded, one loaded as a
    // color effect, and parameters of another lane count are refused, in the tooltip's slot and
    // in the modal's scrim alike; the stock glass is a material.
    let wearing = |name: &str| {
        let mut theme = Theme::stock(scheme);
        let material = EffectName::<(), BackdropEffect>::named(name).unwrap();
        theme.tooltip.material = Some(Material::new(material, ()));
        theme
    };
    let missing_material = MaterialEffect::new(
        EffectName::<(), BackdropEffect>::named("missing").unwrap(),
        (),
    );
    assert_eq!(
        h.ui_mut().set_scheme_theme(scheme, wearing("missing")),
        Err(ThemeEffectError::UnloadedMaterial(missing_material))
    );
    assert_eq!(
        ThemeEffectError::UnloadedMaterial(missing_material).to_string(),
        "the theme's material effect `missing` is not loaded"
    );
    let mut scrimmed = Theme::stock(scheme);
    scrimmed.modal.scrim_material = Some(Material::new(
        EffectName::<(), BackdropEffect>::named("missing").unwrap(),
        (),
    ));
    assert_eq!(
        h.ui_mut().set_scheme_theme(scheme, scrimmed),
        Err(ThemeEffectError::UnloadedMaterial(missing_material))
    );
    let blend_material = MaterialEffect::new(
        EffectName::<(), BackdropEffect>::named(BLEND.name()).unwrap(),
        (),
    );
    assert_eq!(
        h.ui_mut().set_scheme_theme(scheme, wearing(BLEND.name())),
        Err(ThemeEffectError::NotMaterial(blend_material))
    );
    assert_eq!(
        ThemeEffectError::NotMaterial(blend_material).to_string(),
        "the theme's material effect `blend` is not a backdrop effect over at most 8 f32 lanes \
         that binds no image"
    );
    let frosted = MaterialEffect::new(
        EffectName::<(), BackdropEffect>::named("palantir.frosted").unwrap(),
        (),
    );
    assert_eq!(
        h.ui_mut()
            .set_scheme_theme(scheme, wearing("palantir.frosted")),
        Err(ThemeEffectError::MaterialParams {
            effect: frosted,
            want: 2,
            got: 0
        })
    );
    assert_eq!(
        ThemeEffectError::MaterialParams {
            effect: frosted,
            want: 2,
            got: 0
        }
        .to_string(),
        "the theme's material effect `palantir.frosted` takes 2 parameter lanes, and its \
         material holds 0"
    );
    assert!(
        Rc::ptr_eq(h.ui().scheme_theme(scheme), &held),
        "the theme before stays"
    );
    let mut glassy = Theme::stock(scheme);
    glassy.tooltip.material = Some(Material::glass(Lens::WINDOW_GLASS));
    h.ui_mut().set_scheme_theme(scheme, glassy).unwrap();

    h.ui_mut()
        .set_scheme_theme(scheme, naming("palantir.sparkle"))
        .unwrap();
    let ours = EffectName::<PressParams>::named("ours").unwrap();
    h.ui()
        .load_effect(ours, EffectSource::new(RIPPLE_WGSL))
        .unwrap();
    h.ui_mut().set_scheme_theme(scheme, naming("ours")).unwrap();
    assert_eq!(
        h.ui()
            .scheme_theme(scheme)
            .menu
            .item
            .defaults
            .press
            .map(|press| press.effect),
        Some(ours)
    );
}

/// A name keeps the parameter type and kind it loaded with: naming it with others, to load or to
/// ask, panics with both, and so does a power-saver alternative loaded with other types. A shape
/// that names an effect nothing loaded panics with the name as it is added.
#[test]
fn a_name_used_with_other_types_panics() {
    let registry = EffectRegistry::default();
    registry
        .load(RING, &ring(ring_wgsl(RING_PARAMS, RING_ENTRY)))
        .unwrap();
    let untyped: EffectName<()> = EffectName::named("ring").unwrap();
    assert_panics_with("effect `ring` was loaded as", || registry.contains(untyped));
    let backdrop: EffectName<Ring, BackdropEffect> = EffectName::named("ring").unwrap();
    assert_panics_with("BackdropEffect", || {
        registry.load(backdrop, &EffectSource::new("not read"))
    });
    let same: EffectName<Ring, ColorEffect> = EffectName::named("ring").unwrap();
    assert!(registry.contains(same));

    let mut h = Harness::new(UVec2::new(8, 8));
    let params = Ring {
        tint: [1.0; 4],
        radius: 0.0,
        width: 0.0,
        edge: 0.0,
        r#speed: 0.0,
    };
    assert_panics_with("no effect is loaded under `ring`", || {
        h.frame(|ui| {
            ui.add_shape(Shape::rect(), EffectPaint::new(RING, params));
        });
    });

    let untyped_thin: EffectName<()> = EffectName::named("thin ring").unwrap();
    let plain = "fn effect(input: EffectInput) -> EffectOutput { return EffectOutput(); }";
    registry
        .load(untyped_thin, &EffectSource::new(plain))
        .unwrap();
    assert_panics_with("effect `thin ring` was loaded as", || {
        registry.load(
            RING,
            &ring(ring_wgsl(RING_PARAMS, RING_ENTRY)).power_saver(ReducedEffect::Other(THIN_RING)),
        )
    });
}

/// A blend picker's parameters: a premultiplied linear color, and which blend `effect` returns.
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable, palantir_derive::EffectParams)]
#[repr(C)]
struct Blend {
    color: [f32; 4],
    mode: u32,
    pad0: u32,
    pad1: u32,
    pad2: u32,
}

const BLEND: EffectName<Blend> = EffectName::named("blend").expect("the name fits");

const BLEND_WGSL: &str = "
struct Params { color: vec4<f32>, mode: u32, pad0: u32, pad1: u32, pad2: u32 }

fn effect(input: EffectInput, params: Params) -> EffectOutput {
    switch params.mode {
        case 0u: { return effect_over(params.color); }
        case 1u: { return effect_add(params.color); }
        case 2u: { return effect_multiply(params.color); }
        case 3u: { return effect_screen(params.color); }
        // A probe of the input: its size over 80 and its first corner, opaque.
        default: { return effect_over(vec4<f32>(input.size / 80.0, input.corners.x, 1.0)); }
    }
}
";

/// The pixel at `(x, y)` of a 160 × 40 window whose ground is linear `(0.2, 0.4, 0.6)` and which
/// `paint` draws over, at scale 1.
fn painted(paint: impl Fn(&mut Ui), at: &[(u32, u32)]) -> Vec<[u8; 4]> {
    let gpu = headless_test_gpu();
    let mut h = Harness::builder(UVec2::new(160, 40))
        .mono()
        .depth(HarnessDepth::gpu(gpu.handles()))
        .build();
    h.ui()
        .load_effect(BLEND, EffectSource::new(BLEND_WGSL))
        .unwrap();
    let ground = RgbaF32::new(0.2, 0.4, 0.6, 1.0);
    h.prime(2, |ui| {
        ui.add_shape(Shape::rect(), ground);
        paint(ui);
    });
    let image = h.read_back();
    at.iter().map(|&(x, y)| image.get_pixel(x, y).0).collect()
}

/// Each blend over the linear ground `g = (0.2, 0.4, 0.6, 1)`, with the premultiplied color
/// `c = (0.4, 0.2, 0.1, 0.5)`, in its own 40 px square; each center's pixel is the blend's exact
/// value, encoded to sRGB:
/// - over, `c + g·(1 − α)`: `(0.5, 0.4, 0.4)`;
/// - add, `c + g`: `(0.6, 0.6, 0.7)`;
/// - multiply, `g·(c + 1 − α)`: `(0.18, 0.28, 0.36)`;
/// - screen, `c + g·(1 − c)`: `(0.52, 0.52, 0.64)`.
///
/// Alpha holds at 1 under each.
#[test]
fn each_blend_paints_its_exact_value() {
    let centers: Vec<(u32, u32)> = (0..4).map(|i| (40 * i + 20, 20)).collect();
    let got = painted(
        |ui| {
            for mode in 0..4 {
                ui.add_shape(
                    Shape::rect().at(Rect::new(40.0 * mode as f32, 0.0, 40.0, 40.0)),
                    EffectPaint::new(
                        BLEND,
                        Blend {
                            color: [0.4, 0.2, 0.1, 0.5],
                            mode,
                            pad0: 0,
                            pad1: 0,
                            pad2: 0,
                        },
                    ),
                );
            }
        },
        &centers,
    );
    let want: Vec<[u8; 4]> = [
        (0.5, 0.4, 0.4),
        (0.6, 0.6, 0.7),
        (0.18, 0.28, 0.36),
        (0.52, 0.52, 0.64),
    ]
    .into_iter()
    .map(|(r, g, b)| bytes(RgbaF32::new(r, g, b, 1.0)))
    .collect();
    for (mode, (got, want)) in got.iter().zip(&want).enumerate() {
        for lane in 0..4 {
            assert!(
                got[lane].abs_diff(want[lane]) <= 1,
                "blend {mode}: got {got:?}, want {want:?}"
            );
        }
    }
}

/// A rounded clip cuts an effect by its coverage `k`, outside the effect's own code: the over blend
/// of `c = (0.4, 0.2, 0.1, 0.5)` over the ground `g = (0.2, 0.4, 0.6)` paints
/// `k·c + g·(1 − k·α) = g + k·(c − α·g)`. A clip whose left edge lies at 10.25 px leaves column
/// 10's center, 10.5, a quarter pixel inside, `k = 0.75`: `(0.2 + 0.75·0.3, 0.4 + 0.75·0,
/// 0.6 + 0.75·(−0.2)) = (0.425, 0.4, 0.45)`. Column 20 takes the whole blend, `(0.5, 0.4, 0.4)`,
/// and column 5, past the clip, the ground.
#[test]
fn a_rounded_clip_cuts_an_effect_by_its_coverage() {
    let gpu = headless_test_gpu();
    let mut h = Harness::builder(UVec2::new(40, 32))
        .mono()
        .depth(HarnessDepth::gpu(gpu.handles()))
        .build();
    h.set_pixel_snap(false);
    h.ui()
        .load_effect(BLEND, EffectSource::new(BLEND_WGSL))
        .unwrap();
    let ground = RgbaF32::new(0.2, 0.4, 0.6, 1.0);
    h.prime(2, |ui| {
        ui.add_shape(Shape::rect(), ground);
        Panel::canvas()
            .id_salt("canvas")
            .size(SizeSpec::FILL)
            .show(ui, |ui| {
                Panel::canvas()
                    .id_salt("clip")
                    .at((10.25, 4.0))
                    .size((Sizing::fixed(20.0), Sizing::fixed(24.0)))
                    .chrome(Chrome::rounded(RgbaF32::TRANSPARENT, Corners::all(4.0)))
                    .clip_rounded()
                    .show(ui, |ui| {
                        ui.add_shape(
                            Shape::rect().at(Rect::new(-20.0, -10.0, 80.0, 60.0)),
                            EffectPaint::new(
                                BLEND,
                                Blend {
                                    color: [0.4, 0.2, 0.1, 0.5],
                                    mode: 0,
                                    pad0: 0,
                                    pad1: 0,
                                    pad2: 0,
                                },
                            ),
                        );
                    });
            });
    });
    let image = h.read_back();
    for (x, want) in [
        (5, (0.2, 0.4, 0.6)),
        (10, (0.425, 0.4, 0.45)),
        (20, (0.5, 0.4, 0.4)),
    ] {
        let got = image.get_pixel(x, 16).0;
        let want = bytes(RgbaF32::new(want.0, want.1, want.2, 1.0));
        assert!(
            got.iter().zip(want).all(|(g, w)| g.abs_diff(w) <= 1),
            "column {x}: got {got:?}, want {want:?}"
        );
    }
}

/// A triangle clips its effect: the triangle (0, 0), (40, 40), (0, 40) under the over blend of
/// `c = (0.4, 0.2, 0.1, 0.5)` paints `(0.5, 0.4, 0.4)` at (8, 32), inside it, and leaves the ground
/// `(0.2, 0.4, 0.6)` at (32, 8), inside its covering rect but past its edge. Its effect reads its
/// covering rect and no corners: the probe over the triangle (40, 0), (80, 40), (40, 40) rounded
/// by 1 paints `(w / 80, w / 80, 0)` at (48, 32), where a rect of corners 1 would read 1 in blue.
/// Its corners' centers lie 1 inside both edges: (41, 1 + √2) at the 45° corner, (79 − √2, 39)
/// at the other, (41, 39) at the right angle, so the rounded shape covers `w = 40 − √2` a side.
/// A rect of the same effect after them, (80, 0, 40 × 40), runs apart, as it draws four corners'
/// vertices, and paints the over blend at (112, 8), where a triangle's would leave the ground.
/// A circle in (110, 0, 50 × 40) reads its square, (115, 0, 40 × 40), and its corners, 20 each:
/// the probe paints `(40 / 80, 40 / 80, 1)` at its center (135, 20), its blue clamped from 20,
/// and leaves the ground at (153, 2), inside the square, 25.5 px from the center, past its edge.
#[test]
fn a_shape_clips_its_effect() {
    let got = painted(
        |ui| {
            ui.add_shape(
                Shape::triangle(Vec2::ZERO, Vec2::new(40.0, 40.0), Vec2::new(0.0, 40.0)),
                EffectPaint::new(
                    BLEND,
                    Blend {
                        color: [0.4, 0.2, 0.1, 0.5],
                        mode: 0,
                        pad0: 0,
                        pad1: 0,
                        pad2: 0,
                    },
                ),
            );
            ui.add_shape(
                Shape::triangle(
                    Vec2::new(40.0, 0.0),
                    Vec2::new(80.0, 40.0),
                    Vec2::new(40.0, 40.0),
                )
                .radius(1.0_f32),
                EffectPaint::new(
                    BLEND,
                    Blend {
                        color: [0.0; 4],
                        mode: 4,
                        pad0: 0,
                        pad1: 0,
                        pad2: 0,
                    },
                ),
            );
            ui.add_shape(
                Shape::rect().at(Rect::new(80.0, 0.0, 40.0, 40.0)),
                EffectPaint::new(
                    BLEND,
                    Blend {
                        color: [0.4, 0.2, 0.1, 0.5],
                        mode: 0,
                        pad0: 0,
                        pad1: 0,
                        pad2: 0,
                    },
                ),
            );
            ui.add_shape(
                Shape::circle().at(Rect::new(110.0, 0.0, 50.0, 40.0)),
                EffectPaint::new(
                    BLEND,
                    Blend {
                        color: [0.0; 4],
                        mode: 4,
                        pad0: 0,
                        pad1: 0,
                        pad2: 0,
                    },
                ),
            );
        },
        &[(8, 32), (32, 8), (48, 32), (112, 8), (135, 20), (153, 2)],
    );
    let want = [
        bytes(RgbaF32::new(0.5, 0.4, 0.4, 1.0)),
        bytes(RgbaF32::new(0.2, 0.4, 0.6, 1.0)),
        {
            let side = (40.0 - std::f32::consts::SQRT_2) / 80.0;
            bytes(RgbaF32::new(side, side, 0.0, 1.0))
        },
        bytes(RgbaF32::new(0.5, 0.4, 0.4, 1.0)),
        bytes(RgbaF32::new(0.5, 0.5, 1.0, 1.0)),
        bytes(RgbaF32::new(0.2, 0.4, 0.6, 1.0)),
    ];
    assert_eq!(got.len(), want.len());
    for (got, want) in got.iter().zip(&want) {
        for lane in 0..4 {
            assert!(
                got[lane].abs_diff(want[lane]) <= 1,
                "got {got:?}, want {want:?}"
            );
        }
    }
}

/// A halo paints outside its shape alone, out to its reach, the shape's edge cut by its outside
/// coverage, over the linear ground `g = (0.2, 0.4, 0.6)`. With the over blend of
/// `c = (0.4, 0.2, 0.1, 0.5)`, row 20 across a box from x 40 to 80 reaching 6 px:
/// - columns 60 and 40, inside the box, column 40's center 0.5 inside its edge, where the
///   outside coverage is `0.5 − 0.5 = 0`: the ground;
/// - columns 39 and 34, past the edge and inside the reach, its outer side at 34: over,
///   `c + g · 0.5 = (0.5, 0.4, 0.4)`; column 33, past the reach: the ground.
///
/// A circle of radius 16 about (116, 20) with a 4 px halo hugs its arc: the pixel centered at
/// (103.5, 7.5), 17.68 px from the center, is over, and the one at (107.5, 11.5), 12.02 px from
/// it, is inside the circle, the ground. The one at (104.5, 8.5), `11.5 · √2 = 16.263` px from
/// it, 0.263 outside, takes the coverage 0.763: `0.763 · c + g · (1 − 0.763 · 0.5)`, `(0.4289,
/// 0.4, 0.4474)`. A box of 20 × 20 at (130, 10) with a 6 px halo reads its
/// own frame: the probe paints `(20 / 80, 20 / 80, 0)` at column 127, 3 px outside its left edge,
/// where the grown rect would read 32 / 80.
#[test]
fn a_halo_paints_outside_its_shape_alone() {
    let over = Blend {
        color: [0.4, 0.2, 0.1, 0.5],
        mode: 0,
        pad0: 0,
        pad1: 0,
        pad2: 0,
    };
    let probe = Blend {
        color: [0.0; 4],
        mode: 4,
        ..over
    };
    let got = painted(
        |ui| {
            ui.add_shape(
                Shape::rect().at(Rect::new(40.0, 10.0, 40.0, 20.0)),
                EffectPaint::new(BLEND, over).outside(6.0),
            );
            ui.add_shape(
                Shape::circle().at(Rect::new(100.0, 4.0, 32.0, 32.0)),
                EffectPaint::new(BLEND, over).outside(4.0),
            );
            ui.add_shape(
                Shape::rect().at(Rect::new(130.0, 10.0, 20.0, 20.0)),
                EffectPaint::new(BLEND, probe).outside(6.0),
            );
        },
        &[
            (60, 20),
            (40, 20),
            (39, 20),
            (34, 20),
            (33, 20),
            (103, 7),
            (107, 11),
            (104, 8),
            (127, 20),
        ],
    );
    let ground = bytes(RgbaF32::new(0.2, 0.4, 0.6, 1.0));
    let full = bytes(RgbaF32::new(0.5, 0.4, 0.4, 1.0));
    let want = [
        ground,
        ground,
        full,
        full,
        ground,
        full,
        ground,
        bytes(RgbaF32::new(0.4289, 0.4, 0.4474, 1.0)),
        bytes(RgbaF32::new(0.25, 0.25, 0.0, 1.0)),
    ];
    assert_eq!(got.len(), want.len());
    for (i, (got, want)) in got.iter().zip(&want).enumerate() {
        for lane in 0..4 {
            assert!(
                got[lane].abs_diff(want[lane]) <= 1,
                "pixel {i}: got {got:?}, want {want:?}"
            );
        }
    }
}

/// A triangle's effect reads no corners, so it has no halo.
#[test]
fn a_triangles_halo_panics() {
    let mut h = Harness::new(UVec2::new(8, 8));
    h.ui()
        .load_effect(BLEND, EffectSource::new(BLEND_WGSL))
        .unwrap();
    let blend = Blend {
        color: [0.0; 4],
        mode: 0,
        pad0: 0,
        pad1: 0,
        pad2: 0,
    };
    assert_panics_with("a triangle's effect has no halo", || {
        h.frame(|ui| {
            ui.add_shape(
                Shape::triangle(Vec2::ZERO, Vec2::new(4.0, 4.0), Vec2::new(0.0, 4.0)),
                EffectPaint::new(BLEND, blend).outside(2.0),
            );
        });
    });
}

/// The flat ripple paints Compose's disc over the linear ground `g = (0.2, 0.4, 0.6)` of a 160 ×
/// 40 rect, its color white at α 0.5, pressed at (20, 20) with Material's 0.225 s enter and 0.15 s
/// exit, at each time a held pass sets:
/// - at 0 it has not faded in: the ground;
/// - at 0.075 s, a third of the enter, it has faded in whole, and its center has run a third of
///   the way to the rect's, to (40, 20); its radius is 48 + (92.46 − 48) · 0.45725 = 68.33,
///   where 48 is 0.3 of the longer side, 92.46 the half diagonal plus 10, and 0.45725
///   FastOutSlowIn at 1/3, so the pixel centered 65.5 px right of its center is over,
///   `0.5 + g · 0.5 = (0.6, 0.7, 0.8)`, and the one 71.5 px right is the ground;
/// - at 0.225 s, grown, it covers the rect to its corners, 82.46 px from the center;
/// - released at 0.1 s and read at 0.3 s, it is half way through the fade that starts at the
///   enter's end, 0.225 s, so its α is 0.25: `0.25 + g · 0.75 = (0.4, 0.55, 0.7)`.
///
/// The sparkle ripple at its enter's end, progress 0.5, has faded its sparkles out and shows its
/// wave alone, centered on the rect, whole at its center and faded by `1 − (0.5 − 0.4) / 0.6`
/// = 5/6: α `0.5 · 5/6 = 5/12`, so `5/12 + g · 7/12 = (0.5333, 0.65, 0.7667)`.
#[test]
fn the_ripples_paint_their_exact_values() {
    let press = |effect, time_ms: u64, released: f32| {
        move |ui: &mut Ui| {
            if let Some(shape) = ui.add_shape(
                Shape::rect().at(Rect::new(0.0, 0.0, 160.0, 40.0)),
                EffectPaint::new(
                    effect,
                    PressParams {
                        color: RgbaF32::new(1.0, 1.0, 1.0, 0.5),
                        highlight: RgbaF32::new(1.0, 1.0, 1.0, 141.0 / 255.0),
                        point: Vec2::new(20.0, 20.0),
                        released_seconds: released,
                        enter_seconds: 0.225,
                        exit_seconds: 0.15,
                        seed: 0.0,
                        pad0: 0.0,
                        pad1: 0.0,
                    },
                ),
            ) {
                shape.animate(
                    PaintAnimation::clock()
                        .with_repeat(PaintRepeat::Held)
                        .with_period(Duration::from_millis(time_ms)),
                );
            }
        }
    };
    let ground = bytes(RgbaF32::new(0.2, 0.4, 0.6, 1.0));
    let over = bytes(RgbaF32::new(0.6, 0.7, 0.8, 1.0));
    let fading = bytes(RgbaF32::new(0.4, 0.55, 0.7, 1.0));
    let ripple = EffectName::RIPPLE;
    let cases = [
        ("unfaded", press(ripple, 0, -1.0), [ground, ground, ground]),
        (
            "a third in",
            press(ripple, 75, -1.0),
            [over, ground, ground],
        ),
        ("grown", press(ripple, 225, -1.0), [over, over, over]),
        ("fading", press(ripple, 300, 0.1), [fading, fading, fading]),
    ];
    let wave = bytes(RgbaF32::new(
        5.0 / 12.0 + 0.2 * 7.0 / 12.0,
        5.0 / 12.0 + 0.4 * 7.0 / 12.0,
        5.0 / 12.0 + 0.6 * 7.0 / 12.0,
        1.0,
    ));
    let sparkle = painted(press(EffectName::SPARKLE, 225, -1.0), &[(80, 20)]);
    for lane in 0..4 {
        assert!(
            sparkle[0][lane].abs_diff(wave[lane]) <= 1,
            "sparkle: got {sparkle:?}, want {wave:?}"
        );
    }
    for (case, paint, want) in cases {
        let got = painted(paint, &[(105, 20), (111, 20), (159, 0)]);
        for (got, want) in got.iter().zip(&want) {
            for lane in 0..4 {
                assert!(
                    got[lane].abs_diff(want[lane]) <= 1,
                    "{case}: got {got:?}, want {want:?}"
                );
            }
        }
    }
}

/// The sparkle ripple draws its sparkles in the press's highlight: 0.18 s into a held press of
/// Android's 0.45 s enter, its ring of sparkles shows, and a highlight of opaque black darkens
/// some pixels against a transparent one, which draws none, and lightens none.
#[test]
fn the_sparkles_take_the_highlight() {
    let sparkle = |highlight: RgbaF32| {
        move |ui: &mut Ui| {
            if let Some(shape) = ui.add_shape(
                Shape::rect().at(Rect::new(0.0, 0.0, 160.0, 40.0)),
                EffectPaint::new(
                    EffectName::SPARKLE,
                    PressParams {
                        color: RgbaF32::new(1.0, 1.0, 1.0, 0.25),
                        highlight,
                        point: Vec2::new(40.0, 20.0),
                        released_seconds: -1.0,
                        enter_seconds: 0.45,
                        exit_seconds: 0.375,
                        seed: 0.0,
                        pad0: 0.0,
                        pad1: 0.0,
                    },
                ),
            ) {
                shape.animate(
                    PaintAnimation::clock()
                        .with_repeat(PaintRepeat::Held)
                        .with_period(Duration::from_millis(180)),
                );
            }
        }
    };
    let every: Vec<(u32, u32)> = (0..40)
        .flat_map(|y| (0..160).map(move |x| (x, y)))
        .collect();
    let none = painted(sparkle(RgbaF32::new(0.0, 0.0, 0.0, 0.0)), &every);
    let black = painted(sparkle(RgbaF32::new(0.0, 0.0, 0.0, 1.0)), &every);
    let darker = |a: &[u8; 4], b: &[u8; 4]| (0..3).all(|lane| a[lane] < b[lane]);
    let darkened = black
        .iter()
        .zip(&none)
        .filter(|(b, n)| darker(b, n))
        .count();
    let lightened = black
        .iter()
        .zip(&none)
        .filter(|(b, n)| darker(n, b))
        .count();
    assert!(darkened > 0, "the sparkles show in the highlight");
    assert_eq!(lightened, 0, "a black highlight lightens nothing");
}

/// An effect reaches past its node, as a ring does, and a clipping ancestor cuts it. In a 160 × 40
/// window, an 80 × 40 panel clips; inside it, a 20 × 20 node paints an opaque blue effect over
/// `(-30, -10, 140, 40)` of its own frame, so `(-30, -10)..(110, 30)` on screen. Inside the clip
/// and past the node, `(50, 10)` shows blue; past the clip, `(100, 10)`, and below the rect,
/// `(50, 35)`, show the ground. Under a rounded clip of radius 12 the rounded-clip twin draws: the
/// middle shows blue and the corner pixel `(1, 1)` the ground.
#[test]
fn an_effect_past_its_node_meets_its_ancestors_clip() {
    let ring = |clip: ClipMode| {
        move |ui: &mut Ui| {
            Panel::vstack()
                .size((Sizing::fixed(80.0), Sizing::fixed(40.0)))
                .chrome(
                    Chrome::NONE
                        .with_corners(Corners::all(12.0))
                        .with_fill(RgbaF32::TRANSPARENT),
                )
                .clip(clip)
                .show(ui, |ui| {
                    Panel::vstack()
                        .size((Sizing::fixed(20.0), Sizing::fixed(20.0)))
                        .show(ui, |ui| {
                            ui.add_shape(
                                Shape::rect().at(Rect::new(-30.0, -10.0, 140.0, 40.0)),
                                EffectPaint::new(
                                    BLEND,
                                    Blend {
                                        color: [0.0, 0.0, 1.0, 1.0],
                                        mode: 0,
                                        pad0: 0,
                                        pad1: 0,
                                        pad2: 0,
                                    },
                                ),
                            );
                        });
                });
        }
    };
    let blue = [0, 0, 255, 255];
    let ground = RgbaF32::new(0.2, 0.4, 0.6, 1.0);
    // Either neighbor of `123.56, 169.62, 203.42`, and no byte past them.
    assert!(stores([124, 170, 203, 255], ground));
    assert!(stores([123, 169, 204, 255], ground));
    assert!(!stores([125, 170, 203, 255], ground));
    assert!(!stores([124, 168, 203, 255], ground));
    let [inside, past, below] = painted(ring(ClipMode::Rect), &[(50, 10), (100, 10), (50, 35)])
        .try_into()
        .unwrap();
    assert_eq!(inside, blue);
    assert!(stores(past, ground), "past the clip: {past:?}");
    assert!(stores(below, ground), "below the rect: {below:?}");
    let [middle, corner] = painted(ring(ClipMode::Rounded), &[(40, 15), (1, 1)])
        .try_into()
        .unwrap();
    assert_eq!(middle, blue);
    assert!(
        stores(corner, ground),
        "the rounded clip's corner cuts the effect: {corner:?}"
    );
}

/// A new device builds every loaded effect again: after a loss and a recovery onto a fresh
/// device, the same frame paints the effect's blue, as before the loss.
#[test]
fn a_recovered_device_paints_the_loaded_effects() {
    let gpu = isolated_headless_test_gpu();
    let mut h = Harness::builder(UVec2::new(16, 16))
        .mono()
        .depth(HarnessDepth::gpu(gpu.handles()))
        .build();
    h.ui()
        .load_effect(BLEND, EffectSource::new(BLEND_WGSL))
        .unwrap();
    let paint = |ui: &mut Ui| {
        ui.add_shape(Shape::rect(), RgbaF32::new(0.2, 0.4, 0.6, 1.0));
        ui.add_shape(
            Shape::rect(),
            EffectPaint::new(
                BLEND,
                Blend {
                    color: [0.0, 0.0, 1.0, 1.0],
                    mode: 0,
                    pad0: 0,
                    pad1: 0,
                    pad2: 0,
                },
            ),
        );
    };
    h.prime(2, paint);
    assert_eq!(h.read_back().get_pixel(8, 8).0, [0, 0, 255, 255]);
    gpu.handles().lose();
    let fresh = isolated_headless_test_gpu();
    h.recover(fresh.handles());
    h.prime(2, paint);
    assert_eq!(h.read_back().get_pixel(8, 8).0, [0, 0, 255, 255]);
}

/// A reload under the same name repaints whole, though no shape's hash moves: an opaque blue
/// effect over a 16 × 16 window, then a reload that paints red, and the next frame of the same
/// record is a full repaint whose pixels are red.
#[test]
fn a_reload_repaints_the_unchanged_shapes() {
    let gpu = headless_test_gpu();
    let mut h = Harness::builder(UVec2::new(16, 16))
        .mono()
        .depth(HarnessDepth::gpu(gpu.handles()))
        .build();
    h.ui()
        .load_effect(BLEND, EffectSource::new(BLEND_WGSL))
        .unwrap();
    let paint = |ui: &mut Ui| {
        ui.add_shape(
            Shape::rect(),
            EffectPaint::new(
                BLEND,
                Blend {
                    color: [0.0, 0.0, 1.0, 1.0],
                    mode: 0,
                    pad0: 0,
                    pad1: 0,
                    pad2: 0,
                },
            ),
        );
    };
    h.prime(2, paint);
    assert_eq!(h.read_back().get_pixel(8, 8).0, [0, 0, 255, 255]);
    let red = "
struct Params { color: vec4<f32>, mode: u32, pad0: u32, pad1: u32, pad2: u32 }

fn effect(input: EffectInput, params: Params) -> EffectOutput {
    return effect_over(vec4<f32>(1.0, 0.0, 0.0, 1.0));
}
";
    h.ui().load_effect(BLEND, EffectSource::new(red)).unwrap();
    assert_eq!(h.frame(paint).paint(), FramePaint::Full);
    assert_eq!(h.read_back().get_pixel(8, 8).0, [255, 0, 0, 255]);
}

const BLUE: Blend = Blend {
    color: [0.0, 0.0, 1.0, 1.0],
    mode: 0,
    pad0: 0,
    pad1: 0,
    pad2: 0,
};

/// A 16 × 16 deviceless window with [`BLEND`] loaded.
fn blend_window() -> Harness {
    let h = Harness::mono(UVec2::new(16, 16));
    h.ui()
        .load_effect(BLEND, EffectSource::new(BLEND_WGSL))
        .unwrap();
    h
}

/// Each effect draw of the frame `h` shows: its effect and its time.
fn effect_draws(h: &Harness) -> Vec<(EffectId, f32)> {
    h.encode_paint()
        .calls
        .iter()
        .filter_map(|call| match call {
            PaintCall::Effect(p) => Some((p.paint.effect, p.paint.time)),
            _ => None,
        })
        .collect()
}

/// Each frame encodes the time into the pass at its own instant, paint-only frames included: a 2 s
/// loop from 1 s reads 0 at 0.5 s, before its start, and asks for a frame at the start; then
/// 0.5 s at 1.5 s, 0 at 3 s, the wrap, and 0.75 s at 3.75 s, each frame asking for the next at
/// once. A still effect beside it reads 0 throughout, and alone it asks for no frame.
#[test]
fn an_effect_reads_its_time_each_frame_and_wakes_while_it_moves() {
    let ms = Duration::from_millis;
    let looping = PaintAnimation::clock()
        .with_started_at(ms(1000))
        .with_period(ms(2000))
        .with_repeat(PaintRepeat::Forever);
    let record = |ui: &mut Ui| {
        ui.add_shape(Shape::rect(), EffectPaint::new(BLEND, BLUE));
        if let Some(shape) = ui.add_shape(Shape::rect(), EffectPaint::new(BLEND, BLUE)) {
            shape.animate(looping);
        }
    };
    let mut h = blend_window();
    for (at, time, wake) in [
        (ms(500), 0.0, ms(1000)),
        (ms(1500), 0.5, ms(1500)),
        (ms(3000), 0.0, ms(3000)),
        (ms(3750), 0.75, ms(3750)),
    ] {
        let report = h.at(at).frame(record);
        assert_eq!(report.repaint_at, Some(wake), "at {at:?}");
        assert_eq!(
            effect_draws(&h),
            [(app(0), 0.0), (app(0), time)],
            "at {at:?}"
        );
    }

    let mut still = blend_window();
    let report = still.frame(|ui| {
        ui.add_shape(Shape::rect(), EffectPaint::new(BLEND, BLUE));
    });
    assert_eq!(report.repaint_at, None, "a still effect asks for no frame");
}

/// Under reduced motion each effect paints as any paint animation does, with no wake: a
/// decorative loop reads 0, as unanimated; a decorative single pass of 2 s reads its end, 2 s,
/// from the first frame; an essential loop runs on, 0.5 s into its 2 s pass at 0.5 s. Reduced
/// power stops no paint motion: under it alone, with motion following the system, both decorative
/// animations run, 0.5 s into their pass, and the loop asks for its next frame.
#[test]
fn reduced_motion_holds_a_decorative_effect() {
    let ms = Duration::from_millis;
    let looping = PaintAnimation::clock()
        .with_period(ms(2000))
        .with_repeat(PaintRepeat::Forever);
    let once = PaintAnimation::clock().with_period(ms(2000));
    let progress = looping.with_motion(MotionRole::Essential);
    let mut h = blend_window();
    h.set_system_motion_reduced(true);
    let record = |ui: &mut Ui| {
        if let Some(shape) = ui.add_shape(Shape::rect(), EffectPaint::new(BLEND, BLUE)) {
            shape.animate(looping);
        }
        if let Some(shape) = ui.add_shape(Shape::rect(), EffectPaint::new(BLEND, BLUE)) {
            shape.animate(once);
        }
    };
    let report = h.at(ms(500)).frame(record);
    assert_eq!(report.repaint_at, None);
    let times: Vec<f32> = effect_draws(&h).iter().map(|&(_, time)| time).collect();
    assert_eq!(times, [0.0, 2.0]);

    let report = h.at(ms(500)).frame(|ui| {
        if let Some(shape) = ui.add_shape(Shape::rect(), EffectPaint::new(BLEND, BLUE)) {
            shape.animate(progress);
        }
    });
    assert_eq!(report.repaint_at, Some(ms(500)));
    assert_eq!(effect_draws(&h), [(app(0), 0.5)]);

    let mut saver = blend_window();
    saver.set_system_power_reduced(true);
    let report = saver.at(ms(500)).frame(record);
    assert_eq!(report.repaint_at, Some(ms(500)), "the loop runs on");
    let times: Vec<f32> = effect_draws(&saver).iter().map(|&(_, time)| time).collect();
    assert_eq!(times, [0.5, 0.5]);
}

/// While power is reduced each effect paints what its source named: `blend` paints `lite`,
/// `gone` paints nothing, and `lite` itself, `Same`, paints as itself. With full power each
/// paints as itself: ids 1, 2 and 0 in load order `lite`, `blend`, `gone`.
#[test]
fn reduced_power_paints_each_alternative() {
    let lite: EffectName<Blend> = EffectName::named("lite").unwrap();
    let gone: EffectName<Blend> = EffectName::named("gone").unwrap();
    let mut h = Harness::mono(UVec2::new(16, 16));
    h.ui()
        .load_effect(lite, EffectSource::new(BLEND_WGSL))
        .unwrap();
    h.ui()
        .load_effect(
            BLEND,
            EffectSource::new(BLEND_WGSL).power_saver(ReducedEffect::Other(lite)),
        )
        .unwrap();
    h.ui()
        .load_effect(
            gone,
            EffectSource::new(BLEND_WGSL).power_saver(ReducedEffect::Nothing),
        )
        .unwrap();
    let record = |ui: &mut Ui| {
        for name in [BLEND, gone, lite] {
            ui.add_shape(Shape::rect(), EffectPaint::new(name, BLUE));
        }
    };
    h.frame(record);
    let ids =
        |h: &Harness| -> Vec<EffectId> { effect_draws(h).iter().map(|&(id, _)| id).collect() };
    assert_eq!(ids(&h), [app(1), app(2), app(0)]);
    h.set_system_power_reduced(true);
    h.frame(record);
    assert_eq!(ids(&h), [app(0), app(0)]);
    h.set_system_power_reduced(false);
    h.frame(record);
    assert_eq!(ids(&h), [app(1), app(2), app(0)], "back at full power");
}

/// An alternative is held at the load, on every machine, not where it paints, which only one
/// that reduces power would reach: one nothing loaded yet panics there naming the preference, and
/// so does one loaded with other types. A clock-only animation on a shape that reads no time
/// panics as it is added.
#[test]
fn an_effect_that_cannot_paint_panics() {
    let missing: EffectName<Blend> = EffectName::named("missing").unwrap();
    let h = Harness::mono(UVec2::new(16, 16));
    assert_panics_with(
        "no effect is loaded under `missing`, the power-saver alternative of `blend`: load it \
         first",
        || {
            h.ui().load_effect(
                BLEND,
                EffectSource::new(BLEND_WGSL).power_saver(ReducedEffect::Other(missing)),
            )
        },
    );
    let untyped: EffectName<()> = EffectName::named("missing").unwrap();
    let plain = "fn effect(input: EffectInput) -> EffectOutput { return EffectOutput(); }";
    h.ui()
        .load_effect(untyped, EffectSource::new(plain))
        .unwrap();
    assert_panics_with("effect `missing` was loaded as", || {
        h.ui().load_effect(
            BLEND,
            EffectSource::new(BLEND_WGSL).power_saver(ReducedEffect::Other(missing)),
        )
    });

    let mut h = blend_window();
    assert_panics_with("drives no channel", || {
        h.frame(|ui| {
            if let Some(shape) = ui.add_shape(Shape::rect(), RgbaF32::WHITE) {
                shape.animate(PaintAnimation::clock());
            }
        })
    });
}

/// The time reaches the shader: an effect that paints `time / 2` in red over a 2 s loop, at
/// 1.5 s, paints red 0.75, encoded to sRGB.
#[test]
fn the_time_reaches_the_effect() {
    let gpu = headless_test_gpu();
    let mut h = Harness::builder(UVec2::new(16, 16))
        .mono()
        .depth(HarnessDepth::gpu(gpu.handles()))
        .build();
    let timed = "
struct Params { color: vec4<f32>, mode: u32, pad0: u32, pad1: u32, pad2: u32 }

fn effect(input: EffectInput, params: Params) -> EffectOutput {
    return effect_over(vec4<f32>(input.time / 2.0, 0.0, 0.0, 1.0));
}
";
    h.ui().load_effect(BLEND, EffectSource::new(timed)).unwrap();
    let looping = PaintAnimation::clock()
        .with_period(Duration::from_secs(2))
        .with_repeat(PaintRepeat::Forever);
    h.at(Duration::from_millis(1500)).prime(2, |ui| {
        if let Some(shape) = ui.add_shape(Shape::rect(), EffectPaint::new(BLEND, BLUE)) {
            shape.animate(looping);
        }
    });
    let got = h.read_back().get_pixel(8, 8).0;
    assert!(stores(got, RgbaF32::new(0.75, 0.0, 0.0, 1.0)), "{got:?}");
}

/// A moving effect repaints its own rect each paint-only frame, past its node and with its
/// antialiasing reach, and nothing else: a loop over `(2, 2, 6, 6)` beside a still effect over
/// `(10, 10, 4, 4)`, at scale 1, damages `(1.5, 1.5, 7, 7)` at 16 ms, the rect grown by the
/// 0.5 px reach.
#[test]
fn a_moving_effect_damages_its_rect_alone() {
    let looping = PaintAnimation::clock()
        .with_period(Duration::from_secs(2))
        .with_repeat(PaintRepeat::Forever);
    let record = |ui: &mut Ui| {
        if let Some(shape) = ui.add_shape(
            Shape::rect().at(Rect::new(2.0, 2.0, 6.0, 6.0)),
            EffectPaint::new(BLEND, BLUE),
        ) {
            shape.animate(looping);
        }
        ui.add_shape(
            Shape::rect().at(Rect::new(10.0, 10.0, 4.0, 4.0)),
            EffectPaint::new(BLEND, BLUE),
        );
    };
    let mut h = blend_window();
    h.frame(record);
    let report = h.at(Duration::from_millis(16)).frame(record);
    assert_eq!(report.paint(), FramePaint::Partial);
    let rects: Vec<Rect> = h.damage_region().iter_rects().collect();
    assert_eq!(rects, [Rect::new(1.5, 1.5, 7.0, 7.0)]);
}

/// A moving halo repaints its shape grown by its reach and the antialiasing reach: a loop over
/// `(6, 6, 4, 4)` reaching 3 px, at scale 1, damages `(2.5, 2.5, 11, 11)` at 16 ms, `6 − 3 − 0.5`
/// and `4 + 2 · 3 + 2 · 0.5`.
#[test]
fn a_moving_halo_damages_its_reach() {
    let looping = PaintAnimation::clock()
        .with_period(Duration::from_secs(2))
        .with_repeat(PaintRepeat::Forever);
    let record = |ui: &mut Ui| {
        if let Some(shape) = ui.add_shape(
            Shape::rect().at(Rect::new(6.0, 6.0, 4.0, 4.0)),
            EffectPaint::new(BLEND, BLUE).outside(3.0),
        ) {
            shape.animate(looping);
        }
    };
    let mut h = blend_window();
    h.frame(record);
    let report = h.at(Duration::from_millis(16)).frame(record);
    assert_eq!(report.paint(), FramePaint::Partial);
    let rects: Vec<Rect> = h.damage_region().iter_rects().collect();
    assert_eq!(rects, [Rect::new(2.5, 2.5, 11.0, 11.0)]);
}

/// A halo's reach is part of its shape's content: a still halo over `(6, 6, 4, 4)` that grows its
/// reach from 1 to 2 px repaints the larger rect, `(3.5, 3.5, 9, 9)`, which holds the last one;
/// were the reach not hashed, the tree's rollups would hold and keep the last rect.
#[test]
fn a_halos_reach_is_its_content() {
    let record = |reach: f32| {
        move |ui: &mut Ui| {
            ui.add_shape(
                Shape::rect().at(Rect::new(6.0, 6.0, 4.0, 4.0)),
                EffectPaint::new(BLEND, BLUE).outside(reach),
            );
        }
    };
    let mut h = blend_window();
    h.frame(record(1.0));
    let report = h.frame(record(2.0));
    assert_eq!(report.paint(), FramePaint::Partial);
    let rects: Vec<Rect> = h.damage_region().iter_rects().collect();
    assert_eq!(rects, [Rect::new(3.5, 3.5, 9.0, 9.0)]);
}

const PAIR: EffectName<()> = EffectName::named("pair").expect("the name fits");

/// Two images, the first over the rect's left half and the second over its right; both sampled,
/// as a sample needs uniform control flow.
const PAIR_WGSL: &str = "
fn effect(input: EffectInput) -> EffectOutput {
    let uv = vec2<f32>(0.5);
    let left = textureSample(effect_image0, effect_sampler, uv);
    let right = textureSample(effect_image1, effect_sampler, uv);
    return effect_over(select(right, left, input.pos.x < input.size.x * 0.5));
}
";

/// A 2 × 2 opaque image of sRGB `hex`.
fn solid(hex: u32) -> Image {
    let mut image = Image::blank(UVec2::splat(2));
    image.texels_mut().fill(SrgbaU8::hex(hex));
    image
}

/// A 32 × 16 window on a device with [`PAIR`] loaded.
fn pair_window(gpu: &Gpu) -> Harness {
    let h = Harness::builder(UVec2::new(32, 16))
        .mono()
        .depth(HarnessDepth::gpu(gpu.clone()))
        .build();
    h.ui()
        .load_effect(PAIR, EffectSource::new(PAIR_WGSL).image_slots(2))
        .unwrap();
    h
}

/// Each shape binds its own images, in slot order, and neighbors that bind other images split
/// into their own draws: `(0, 0, 16, 16)` binds red then blue, `(16, 0, 16, 16)` green then red,
/// so the four half centers read red, blue, green and red. Two sets keep two groups, and a third
/// shape over the first set adds none. Dropping the green image's handle drops the group that
/// binds it.
#[test]
fn each_shape_binds_its_images_in_slot_order() {
    let gpu = headless_test_gpu();
    let mut h = pair_window(&gpu.handles());
    let red = h.ui().load_image(&solid(0xff0000)).unwrap();
    let blue = h.ui().load_image(&solid(0x0000ff)).unwrap();
    let green = h.ui().load_image(&solid(0x00ff00)).unwrap();
    let shapes = |ui: &mut Ui, sets: &[(f32, [&ImageHandle; 2])]| {
        for &(x, [first, second]) in sets {
            ui.add_shape(
                Shape::rect().at(Rect::new(x, 0.0, 16.0, 16.0)),
                EffectPaint::new(PAIR, ()).images([first.clone(), second.clone()]),
            );
        }
    };
    h.prime(2, |ui| {
        shapes(
            ui,
            &[
                (0.0, [&red, &blue]),
                (16.0, [&green, &red]),
                (0.0, [&red, &blue]),
            ],
        );
    });
    let image = h.read_back();
    let at = |x| image.get_pixel(x, 8).0;
    assert_eq!(
        [at(4), at(12), at(20), at(28)],
        [
            [255, 0, 0, 255],
            [0, 0, 255, 255],
            [0, 255, 0, 255],
            [255, 0, 0, 255]
        ]
    );
    assert_eq!(h.effect_image_groups(), 2);

    h.prime(1, |ui| shapes(ui, &[(0.0, [&red, &blue])]));
    drop(green);
    assert_eq!(
        h.effect_image_groups(),
        1,
        "the group that bound green goes with it"
    );
}

/// An image not on the device paints nothing and stops nothing: after a loss and a recovery onto
/// a fresh device, the effect over the ground shows the ground until the app writes its image
/// again, and then the image.
#[test]
fn an_image_lost_with_its_device_paints_nothing_until_written_again() {
    let gpu = isolated_headless_test_gpu();
    let mut h = pair_window(&gpu.handles());
    let red = h.ui().load_image(&solid(0xff0000)).unwrap();
    let ground = RgbaF32::new(0.2, 0.4, 0.6, 1.0);
    let paint = |ui: &mut Ui| {
        ui.add_shape(Shape::rect(), ground);
        ui.add_shape(
            Shape::rect(),
            EffectPaint::new(PAIR, ()).images([red.clone(), red.clone()]),
        );
    };
    h.prime(2, paint);
    assert_eq!(h.read_back().get_pixel(8, 8).0, [255, 0, 0, 255]);

    gpu.handles().lose();
    let fresh = isolated_headless_test_gpu();
    h.recover(fresh.handles());
    assert!(!red.is_resident());
    h.prime(2, paint);
    let lost = h.read_back().get_pixel(8, 8).0;
    assert!(stores(lost, ground), "the ground: {lost:?}");

    red.update(&solid(0xff0000)).unwrap();
    h.prime(2, paint);
    assert_eq!(h.read_back().get_pixel(8, 8).0, [255, 0, 0, 255]);
}

/// An effect paint binds as many images as the effect it paints declares: one image for two slots
/// panics with both counts as it is added; so does a power-saver alternative of other slots, at
/// its namer's load, and a reload of the alternative to other slots, at that reload.
#[test]
fn an_effect_paint_binds_as_many_images_as_its_effect_declares() {
    let mut h = Harness::mono(UVec2::new(8, 8));
    h.ui()
        .load_effect(PAIR, EffectSource::new(PAIR_WGSL).image_slots(2))
        .unwrap();
    let red = h.ui().load_image(&solid(0xff0000)).unwrap();
    assert_panics_with(
        "`pair` paints an effect that binds 2 images, and the paint binds 1",
        || {
            h.frame(|ui| {
                ui.add_shape(
                    Shape::rect(),
                    EffectPaint::new(PAIR, ()).images([red.clone()]),
                );
            })
        },
    );

    let single: EffectName<()> = EffectName::named("single").unwrap();
    let one_image = "fn effect(input: EffectInput) -> EffectOutput {
        return effect_over(textureSample(effect_image0, effect_sampler, vec2<f32>(0.5)));
    }";
    h.ui()
        .load_effect(single, EffectSource::new(one_image).image_slots(1))
        .unwrap();
    assert_panics_with(
        "effect `single` binds 1 images, and `pair`, which paints in its place, 2",
        || {
            h.ui().load_effect(
                PAIR,
                EffectSource::new(PAIR_WGSL)
                    .image_slots(2)
                    .power_saver(ReducedEffect::Other(single)),
            )
        },
    );
    let twin: EffectName<()> = EffectName::named("twin").unwrap();
    h.ui()
        .load_effect(twin, EffectSource::new(PAIR_WGSL).image_slots(2))
        .unwrap();
    h.ui()
        .load_effect(
            PAIR,
            EffectSource::new(PAIR_WGSL)
                .image_slots(2)
                .power_saver(ReducedEffect::Other(twin)),
        )
        .unwrap();
    assert_panics_with(
        "effect `pair` binds 2 images, and `twin`, which paints in its place, 1",
        || {
            h.ui()
                .load_effect(twin, EffectSource::new(one_image).image_slots(1))
        },
    );
}

/// An image update repaints each effect that binds it, as it repaints an image's paint: the effect
/// over `(2, 2, 6, 6)` binds the image that changes, and the one over `(10, 10, 4, 4)` another,
/// so the update damages `(1.5, 1.5, 7, 7)`, the first rect grown by the 0.5 px reach, alone.
#[test]
fn an_image_update_repaints_the_effects_that_bind_it() {
    let mut h = Harness::mono(UVec2::new(16, 16));
    h.ui()
        .load_effect(PAIR, EffectSource::new(PAIR_WGSL).image_slots(2))
        .unwrap();
    let changing = h.ui().load_image(&solid(0xff0000)).unwrap();
    let still = h.ui().load_image(&solid(0x0000ff)).unwrap();
    let record = |ui: &mut Ui| {
        ui.add_shape(
            Shape::rect().at(Rect::new(2.0, 2.0, 6.0, 6.0)),
            EffectPaint::new(PAIR, ()).images([changing.clone(), still.clone()]),
        );
        ui.add_shape(
            Shape::rect().at(Rect::new(10.0, 10.0, 4.0, 4.0)),
            EffectPaint::new(PAIR, ()).images([still.clone(), still.clone()]),
        );
    };
    h.frame(record);
    changing.update(&solid(0x00ff00)).unwrap();
    h.ui_mut().request_repaint();
    let report = h.frame(record);
    assert_eq!(report.paint(), FramePaint::Partial);
    let rects: Vec<Rect> = h.damage_region().iter_rects().collect();
    assert_eq!(rects, [Rect::new(1.5, 1.5, 7.0, 7.0)]);
}

/// A shift's parameters: how far right a backdrop effect reads, in logical px.
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable, palantir_derive::EffectParams)]
#[repr(C)]
struct Shift {
    dx: f32,
}

const SHIFT: EffectName<Shift, BackdropEffect> = EffectName::named("shift").expect("the name fits");

/// [`SHIFT`]'s source, which reads 8 px past its shape, as its `dx` reaches.
fn reaching_shift() -> EffectSource<Shift, BackdropEffect> {
    EffectSource::new(SHIFT_WGSL).reach(|_| BackdropReach::Px(8.0))
}

/// What is painted `dx` px to the right, opaque.
const SHIFT_WGSL: &str = "
struct Params { dx: f32 }

fn effect(input: EffectInput, params: Params) -> EffectOutput {
    return effect_over(effect_backdrop(input.pos + vec2<f32>(params.dx, 0.0)));
}
";

/// sRGB bytes of an opaque linear color.
fn bytes(color: RgbaF32) -> [u8; 4] {
    let srgb = color.to_srgba8();
    [srgb.r, srgb.g, srgb.b, srgb.a]
}

/// Whether `got` is a byte a target may store for `color`: each channel one of the two bytes
/// nearest its exact encoding, as Vulkan lets a conversion to a normalized format return either
/// (`convertFloatToUint`, "Conversion From Floating-Point to Normalized Fixed-Point"). Mesa's anv
/// was seen to round down where palantir's encoding rounds to nearest: `(0.2, 0.4, 0.6)` encodes
/// to `123.56, 169.62, 203.42`, read back `[123, 169, 203]` there and `[124, 170, 203]` elsewhere.
/// For a color the target receives as written, its encoding clear of a whole byte; a value a
/// shader computes, off by its arithmetic, takes a byte either side of its expected one.
fn stores(got: [u8; 4], color: RgbaF32) -> bool {
    let exact = [
        srgb_reference::encode(f64::from(color.r)),
        srgb_reference::encode(f64::from(color.g)),
        srgb_reference::encode(f64::from(color.b)),
        f64::from(color.a),
    ];
    got.iter()
        .zip(exact)
        .all(|(&byte, value)| (f64::from(byte) - value * 255.0).abs() < 1.0)
}

/// A backdrop effect reads what is painted under it and past its rect within its reach, and a
/// read past the reach reads the reach's edge. In a 32 × 16 window, red fills `x < 16`, blue
/// `16..26`, green from 26; the effect over `(4, 0, 12, 16)` reaches 8 px, so it reads up to
/// x 24. Shifted by 6, pixel 6 reads x 12.5, red, and pixel 12 reads 18.5, blue; shifted by 14,
/// pixel 14 would read 28.5, green, and reads the edge, 24, blue. When the blue under the reach
/// turns green, the effect, whose own rect did not change, reads green: damage under its reach
/// captures it again.
#[test]
fn a_backdrop_effect_reads_under_its_reach_and_clamps_past_it() {
    let gpu = headless_test_gpu();
    let mut h = Harness::builder(UVec2::new(32, 16))
        .mono()
        .depth(HarnessDepth::gpu(gpu.handles()))
        .build();
    h.ui().load_effect(SHIFT, reaching_shift()).unwrap();
    let red = RgbaF32::new(1.0, 0.0, 0.0, 1.0);
    let blue = RgbaF32::new(0.0, 0.0, 1.0, 1.0);
    let green = RgbaF32::new(0.0, 1.0, 0.0, 1.0);
    let paint = |middle: RgbaF32, dx: f32| {
        move |ui: &mut Ui| {
            ui.add_shape(Shape::rect().at(Rect::new(0.0, 0.0, 16.0, 16.0)), red);
            ui.add_shape(Shape::rect().at(Rect::new(16.0, 0.0, 10.0, 16.0)), middle);
            ui.add_shape(Shape::rect().at(Rect::new(26.0, 0.0, 6.0, 16.0)), green);
            ui.add_shape(
                Shape::rect().at(Rect::new(4.0, 0.0, 12.0, 16.0)),
                EffectPaint::new(SHIFT, Shift { dx }),
            );
        }
    };
    h.prime(2, paint(blue, 6.0));
    let image = h.read_back();
    assert_eq!(
        [image.get_pixel(6, 8).0, image.get_pixel(12, 8).0],
        [bytes(red), bytes(blue)]
    );
    h.prime(2, paint(blue, 14.0));
    assert_eq!(
        h.read_back().get_pixel(14, 8).0,
        bytes(blue),
        "the reach's edge"
    );

    h.prime(1, paint(blue, 6.0));
    h.prime(2, paint(green, 6.0));
    assert_eq!(
        h.read_back().get_pixel(12, 8).0,
        bytes(green),
        "a change under the reach"
    );
}

const SHARP: Shift = Shift { dx: 0.0 };

/// While transparency is reduced a backdrop effect paints its fallback over its shape as a plain
/// fill, which runs no effect, takes no ticket and lists no backdrop: the theme's material
/// fallback where it names none, its own where it names one, and nothing for one that paints
/// nothing. At full transparency it captures what is under it.
#[test]
fn a_backdrop_effect_paints_its_fallback_while_transparency_is_reduced() {
    let mut h = Harness::mono(UVec2::new(16, 16));
    h.ui()
        .load_effect(SHIFT, EffectSource::new(SHIFT_WGSL))
        .unwrap();
    let red = RgbaF32::new(1.0, 0.0, 0.0, 0.5);
    let calls = |h: &Harness| -> Vec<&'static str> {
        h.encode_paint()
            .calls
            .iter()
            .map(PaintCall::kind)
            .filter(|kind| *kind == "Fill" || *kind == "Effect")
            .collect()
    };
    let paint = |fallback: RgbaF32| {
        EffectPaint::new(SHIFT, SHARP)
            .backdrop_blur(2.0)
            .fallback(fallback)
    };
    h.frame(|ui| {
        ui.add_shape(Shape::rect(), paint(red));
    });
    let reads: Vec<_> = h
        .encode_paint()
        .calls
        .iter()
        .filter_map(|call| match call {
            PaintCall::Effect(p) => Some(match p.paint.read {
                EffectRead::Backdrop { capture } | EffectRead::Material { capture, .. } => {
                    Some(capture)
                }
                EffectRead::Color { .. } => None,
            }),
            _ => None,
        })
        .collect();
    assert!(matches!(
        reads.as_slice(),
        [Some(EffectCaptureDraw {
            reach: CaptureReach::Px(0.0),
            ..
        })]
    ));
    h.set_system_transparency_reduced(true);
    h.frame(|ui| {
        ui.add_shape(Shape::rect(), paint(red));
    });
    let fills = |h: &Harness| -> Vec<_> {
        h.encode_paint()
            .calls
            .iter()
            .filter_map(|call| match call {
                PaintCall::Fill(p) => Some(p.paint.fill.color),
                _ => None,
            })
            .collect()
    };
    assert_eq!(fills(&h), [red.to_f16()], "the fallback, as a plain fill");
    assert_eq!(calls(&h), ["Fill"], "and no effect");
    h.frame(|ui| {
        ui.add_shape(Shape::rect(), EffectPaint::new(SHIFT, SHARP));
    });
    let theme = h.ui().theme().material_fallback;
    assert_eq!(
        fills(&h),
        [theme.to_f16()],
        "the theme's where it names none"
    );
    h.frame(|ui| {
        ui.add_shape(Shape::rect(), paint(RgbaF32::TRANSPARENT));
    });
    assert_eq!(
        calls(&h),
        Vec::<&str>::new(),
        "a fallback of nothing paints nothing"
    );
    // A name nothing loaded is a logic error in every mode, a use that paints nothing included.
    let gone = EffectName::<Shift, BackdropEffect>::named("gone").unwrap();
    assert_panics_with("no effect is loaded under `gone`", || {
        h.frame(|ui| {
            ui.add_shape(
                Shape::rect(),
                EffectPaint::new(gone, SHARP).fallback(RgbaF32::TRANSPARENT),
            );
        });
    });
}

/// A material's effect is a backdrop effect that binds no image, over f32 lanes that fit a
/// material: one that binds an image loads, as an `EffectPaint` may paint it, and no material
/// wears it; the stock ones and an app's over two lanes take materials of their lane counts.
#[test]
fn a_material_effect_is_a_backdrop_effect_over_lanes() {
    let registry = EffectRegistry::default();
    let pictured = EffectName::<(), BackdropEffect>::named("pictured").unwrap();
    registry
        .load(
            pictured,
            &EffectSource::new(
                "fn effect(input: EffectInput) -> EffectOutput {
                    return effect_over(effect_backdrop(input.pos));
                }",
            )
            .image_slots(1),
        )
        .unwrap();
    let ours = EffectName::<FrostedParams, BackdropEffect>::named("ours").unwrap();
    registry
        .load(
            ours,
            &EffectSource::new(concat!(
                include_str!("stock/material.wgsl"),
                include_str!("stock/frosted.wgsl")
            )),
        )
        .unwrap();
    for (effect, want) in [
        (MaterialEffect::new(pictured, ()), MaterialFit::NotMaterial),
        (
            MaterialEffect::new(
                EffectName::<(), BackdropEffect>::named("unloaded").unwrap(),
                (),
            ),
            MaterialFit::Unloaded,
        ),
        (
            MaterialEffect::new(EffectName::FROSTED, FrostedParams::DEFAULT),
            MaterialFit::Lanes(2),
        ),
        (
            MaterialEffect::new(EffectName::GLASS, GlassParams::new(Lens::WINDOW_GLASS)),
            MaterialFit::Lanes(7),
        ),
        (
            MaterialEffect::new(ours, FrostedParams::DEFAULT),
            MaterialFit::Lanes(2),
        ),
    ] {
        assert_eq!(registry.material_fit(effect), want, "{effect:?}");
    }
}

/// A backdrop effect blurs what it reads by its σ through the capture and blur a material
/// takes, and reads the result as `Material::blur` does: over a black and white edge at x 16 in a
/// 32 × 16 half-float window, with a strip at x 4 to 12 a color effect paints red −0.25, an effect
/// that shows what it reads, blurred by σ 3, paints row 8 to the bit as a rect of
/// `Material::blur(3.0)` over the same ground, but for the strip's negative red, which the
/// material floors at +0, and unlike the sharp ground.
#[test]
fn a_backdrop_effect_blurs_as_a_material() {
    const NEGATIVE: EffectName<()> = EffectName::named("negative").expect("the name fits");
    let gpu = headless_test_gpu();
    let row = |backdrop: &dyn Fn(&mut Ui)| {
        let mut h = Harness::builder(UVec2::new(32, 16))
            .mono()
            .depth(HarnessDepth::gpu_float(gpu.handles()))
            .build();
        h.ui()
            .load_effect(SHIFT, EffectSource::new(SHIFT_WGSL))
            .unwrap();
        h.ui()
            .load_effect(
                NEGATIVE,
                EffectSource::new(
                    "fn effect(input: EffectInput) -> EffectOutput {
                        return effect_over(vec4<f32>(-0.25, 0.5, 1.0, 1.0));
                    }",
                ),
            )
            .unwrap();
        let mut record = |ui: &mut Ui| {
            ui.add_shape(
                Shape::rect().at(Rect::new(0.0, 0.0, 16.0, 16.0)),
                RgbaF32::BLACK,
            );
            ui.add_shape(
                Shape::rect().at(Rect::new(16.0, 0.0, 16.0, 16.0)),
                RgbaF32::WHITE,
            );
            ui.add_shape(
                Shape::rect().at(Rect::new(4.0, 0.0, 8.0, 16.0)),
                EffectPaint::new(NEGATIVE, ()),
            );
            backdrop(ui);
        };
        h.prime(2, &mut record);
        let mut pixels = Vec::new();
        h.capture(&mut record, &mut pixels).unwrap();
        pixels[8 * 32 * 8..9 * 32 * 8]
            .as_chunks::<8>()
            .0
            .iter()
            .map(|&texel| bytemuck::cast::<[u8; 8], [u16; 4]>(texel))
            .collect::<Vec<_>>()
    };
    let effect = row(&|ui| {
        ui.add_shape(
            Shape::rect(),
            EffectPaint::new(SHIFT, SHARP).backdrop_blur(3.0),
        );
    });
    let filter = row(&|ui| {
        ui.add_shape(Shape::rect(), Material::blur(3.0));
    });
    let sharp = row(&|_| {});
    let floored: Vec<[u16; 4]> = effect
        .iter()
        .map(|&[red, green, blue, alpha]| {
            let red = if f16::from_bits(red).to_f32() < 0.0 {
                0
            } else {
                red
            };
            [red, green, blue, alpha]
        })
        .collect();
    assert_ne!(floored, effect, "the strip's negative red reaches the blur");
    assert_eq!(filter, floored);
    assert_ne!(effect, sharp, "σ 3 blurs the ground");
}

/// Each preference picks its own alternative, motion's over power's: an effect whose source names
/// one for each paints itself with nothing reduced, power's with power reduced, and motion's with
/// motion and power. Transparency names none: a color effect paints itself while it is reduced. An
/// alternative nothing loaded panics at the load with the preference that named it.
#[test]
fn each_reduction_paints_its_alternative_in_order() {
    let names = ["base", "still", "saver"]
        .map(|name| -> EffectName<Blend> { EffectName::named(name).unwrap() });
    let [base, still, saver] = names;
    let mut h = Harness::mono(UVec2::new(16, 16));
    for name in [still, saver] {
        h.ui()
            .load_effect(name, EffectSource::new(BLEND_WGSL))
            .unwrap();
    }
    h.ui()
        .load_effect(
            base,
            EffectSource::new(BLEND_WGSL)
                .reduced_motion(ReducedEffect::Other(still))
                .power_saver(ReducedEffect::Other(saver)),
        )
        .unwrap();
    let painted = |h: &mut Harness| {
        h.frame(|ui| {
            ui.add_shape(Shape::rect(), EffectPaint::new(base, BLUE));
        });
        effect_draws(h)[0].0
    };
    assert_eq!(painted(&mut h), app(2));
    h.ui_mut().set_preferences(UiPreferences {
        transparency: Preference::Fixed(Reduction::Reduced),
        ..UiPreferences::default()
    });
    assert_eq!(painted(&mut h), app(2), "transparency names none");
    h.ui_mut().set_preferences(UiPreferences::default());
    h.set_system_power_reduced(true);
    assert_eq!(painted(&mut h), app(1), "power");
    h.set_system_motion_reduced(true);
    assert_eq!(painted(&mut h), app(0), "motion over power");

    let gone: EffectName<Blend> = EffectName::named("gone").unwrap();
    assert_panics_with(
        "no effect is loaded under `gone`, the reduced-motion alternative of `base`",
        || {
            h.ui().load_effect(
                base,
                EffectSource::new(BLEND_WGSL).reduced_motion(ReducedEffect::Other(gone)),
            )
        },
    );
}

/// An effect may name itself as an alternative, on its first load too, and paints itself where it
/// does: under reduced motion here, which wins over power's `Nothing`; a reload keeps its id.
#[test]
fn an_effect_that_names_itself_paints_itself() {
    let base: EffectName<Blend> = EffectName::named("selfish").unwrap();
    let mut h = Harness::mono(UVec2::new(16, 16));
    let source = || {
        EffectSource::new(BLEND_WGSL)
            .reduced_motion(ReducedEffect::Other(base))
            .power_saver(ReducedEffect::Nothing)
    };
    h.ui().load_effect(base, source()).unwrap();
    h.set_system_power_reduced(true);
    h.set_system_motion_reduced(true);
    let painted = |h: &mut Harness| {
        h.frame(|ui| {
            ui.add_shape(Shape::rect(), EffectPaint::new(base, BLUE));
        });
        effect_draws(h).first().map(|&(id, _)| id)
    };
    assert_eq!(
        painted(&mut h),
        Some(app(0)),
        "motion's own name wins over power's nothing"
    );
    h.ui().load_effect(base, source()).unwrap();
    assert_eq!(painted(&mut h), Some(app(0)), "reloaded at the same id");
}

/// `effect_backdrop` is a backdrop effect's alone: the same text loads as a backdrop effect and
/// fails to parse as a color effect, where the name is not declared.
#[test]
fn only_a_backdrop_effect_reads_what_is_under_it() {
    let params = ParamsLayout {
        fields: Shift::FIELDS,
        size: size_of::<Shift>(),
    };
    let module = contract::check(SHIFT_WGSL, spec(KindId::Backdrop, params, 0)).unwrap();
    assert_eq!(module.kind, KindId::Backdrop);
    assert!(matches!(
        contract::check(SHIFT_WGSL, spec(KindId::Color, params, 0)),
        Err(EffectError::Parse(_))
    ));
}

/// A reach past the window reads the window's edge, the last pixel the capture holds: in a
/// 32 × 16 window, red fills `x < 26` and green from 26, and the effect over `(16, 0, 16, 16)`
/// reaches 8 px, to x 40 past the edge at 32. Shifted by 12, pixel 28 reads x 40.5, which holds
/// nothing, and reads the edge pixel, green.
#[test]
fn a_reach_past_the_window_reads_its_edge() {
    let gpu = headless_test_gpu();
    let mut h = Harness::builder(UVec2::new(32, 16))
        .mono()
        .depth(HarnessDepth::gpu(gpu.handles()))
        .build();
    h.ui().load_effect(SHIFT, reaching_shift()).unwrap();
    let green = RgbaF32::new(0.0, 1.0, 0.0, 1.0);
    h.prime(2, |ui: &mut Ui| {
        ui.add_shape(
            Shape::rect().at(Rect::new(0.0, 0.0, 26.0, 16.0)),
            RgbaF32::new(1.0, 0.0, 0.0, 1.0),
        );
        ui.add_shape(Shape::rect().at(Rect::new(26.0, 0.0, 6.0, 16.0)), green);
        ui.add_shape(
            Shape::rect().at(Rect::new(16.0, 0.0, 16.0, 16.0)),
            EffectPaint::new(SHIFT, Shift { dx: 12.0 }),
        );
    });
    assert_eq!(h.read_back().get_pixel(28, 8).0, bytes(green));
}

/// A paint animation turns an effect about its owner's center, as it turns a quad: an opaque blue
/// effect over a 4 px band across the middle of a 32 × 32 window, a quarter turn, paints the
/// band upright through the center, `(16, 2)` blue, and leaves where it lay, `(2, 16)`, the ground.
#[test]
fn a_turn_turns_an_effect_as_a_quad() {
    let gpu = headless_test_gpu();
    let mut h = Harness::builder(UVec2::new(32, 32))
        .mono()
        .depth(HarnessDepth::gpu(gpu.handles()))
        .build();
    h.ui()
        .load_effect(BLEND, EffectSource::new(BLEND_WGSL))
        .unwrap();
    let ground = RgbaF32::new(0.2, 0.4, 0.6, 1.0);
    h.prime(2, |ui: &mut Ui| {
        ui.add_shape(Shape::rect(), ground);
        if let Some(shape) = ui.add_shape(
            Shape::rect().at(Rect::new(0.0, 14.0, 32.0, 4.0)),
            EffectPaint::new(BLEND, BLUE),
        ) {
            shape.animate(PaintAnimation::turn(0.25, 0.25));
        }
    });
    let image = h.read_back();
    assert_eq!(image.get_pixel(16, 2).0, [0, 0, 255, 255]);
    let left = image.get_pixel(2, 16).0;
    assert!(stores(left, ground), "where the band lay: {left:?}");
}

/// The reach is measured from the effect's rect, not from the pixels it rounds out to: with no
/// pixel snap, the effect over `(4, 0, 11.5, 16)` reaches 8 px, to x 23.5, with blue under
/// `16..24` and green from 24. Shifted by 12, pixel 14 would read x 26.5, and reads the reach's
/// edge, 23.5, the center of the last blue pixel, pure blue; measured from the rounded rect it
/// would read 24, half green.
#[test]
fn the_reach_runs_from_the_exact_rect() {
    let gpu = headless_test_gpu();
    let mut h = Harness::builder(UVec2::new(32, 16))
        .mono()
        .pixel_snap(false)
        .depth(HarnessDepth::gpu(gpu.handles()))
        .build();
    h.ui().load_effect(SHIFT, reaching_shift()).unwrap();
    let blue = RgbaF32::new(0.0, 0.0, 1.0, 1.0);
    h.prime(2, |ui: &mut Ui| {
        ui.add_shape(
            Shape::rect().at(Rect::new(0.0, 0.0, 16.0, 16.0)),
            RgbaF32::new(1.0, 0.0, 0.0, 1.0),
        );
        ui.add_shape(Shape::rect().at(Rect::new(16.0, 0.0, 8.0, 16.0)), blue);
        ui.add_shape(
            Shape::rect().at(Rect::new(24.0, 0.0, 8.0, 16.0)),
            RgbaF32::new(0.0, 1.0, 0.0, 1.0),
        );
        ui.add_shape(
            Shape::rect().at(Rect::new(4.0, 0.0, 11.5, 16.0)),
            EffectPaint::new(SHIFT, Shift { dx: 12.0 }),
        );
    });
    assert_eq!(h.read_back().get_pixel(14, 8).0, bytes(blue));
}

/// A turned backdrop effect reads what lies under each of its pixels, through its turn: in a
/// 32 × 32 window, red above y 16 and blue below, a band across the middle turned a quarter
/// stands upright, and its pixel `(16, 28)` reads the blue under it. Read in the unturned frame,
/// its center `(16.5, 28.5)` would land at `(28.5, 15.5)`, red.
#[test]
fn a_turned_backdrop_effect_reads_through_its_turn() {
    let gpu = headless_test_gpu();
    let mut h = Harness::builder(UVec2::new(32, 32))
        .mono()
        .depth(HarnessDepth::gpu(gpu.handles()))
        .build();
    h.ui()
        .load_effect(SHIFT, EffectSource::new(SHIFT_WGSL))
        .unwrap();
    let red = RgbaF32::new(1.0, 0.0, 0.0, 1.0);
    h.prime(2, |ui: &mut Ui| {
        ui.add_shape(Shape::rect().at(Rect::new(0.0, 0.0, 32.0, 16.0)), red);
        ui.add_shape(
            Shape::rect().at(Rect::new(0.0, 16.0, 32.0, 16.0)),
            RgbaF32::new(0.0, 0.0, 1.0, 1.0),
        );
        if let Some(shape) = ui.add_shape(
            Shape::rect().at(Rect::new(0.0, 14.0, 32.0, 4.0)),
            EffectPaint::new(SHIFT, SHARP),
        ) {
            shape.animate(PaintAnimation::turn(0.25, 0.25));
        }
    });
    assert_eq!(
        h.read_back().get_pixel(16, 28).0,
        bytes(RgbaF32::new(0.0, 0.0, 1.0, 1.0))
    );
}
