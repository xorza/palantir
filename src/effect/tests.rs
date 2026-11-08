//! Loading effects: the derive's layout, each refusal, a replacing load, a refused reload, and a
//! name used with other types; the time each frame reads, reduced motion and the power-saver
//! alternative; on a device, each blend, a clip, a recovery, a reload, the time and the images.

use std::error;
use std::rc::Rc;

use glam::{UVec2, Vec2};
use std::time::Duration;

use crate::effect::contract::{self, ParamsLayout};
use crate::effect::effect_kind::{BackdropEffect, ColorEffect, KindId};
use crate::effect::effect_name::EffectName;
use crate::effect::effect_params::{EffectParam, EffectParams, ParamType};
use crate::effect::effect_registry::{EffectRegistry, ReducedName};
use crate::effect::effect_source::{EffectSource, ReducedPaint};
use crate::effect::error::{EffectError, EntryMismatch, ParamMismatch};
use crate::effect::press_input::PressInput;
use crate::effect::stock;
use crate::gpu::device::gpu::Gpu;
use crate::gpu::test_gpu::headless_test_gpu;
use crate::gpu::test_gpu::isolated_headless_test_gpu;
use crate::host::harness::Harness;
use crate::host::harness::harness_builder::Depth;
use crate::image_registry::image_handle::ImageHandle;
use crate::internals::paint_capture::PaintCall;
use crate::internals::panic_probe::assert_panics_with;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::effect_id::EffectId;
use crate::primitives::identity::inline_name::InlineName;
use crate::primitives::layout::clip_mode::ClipMode;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::backdrop_filter::BackdropFilter;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::srgba_u8::SrgbaU8;
use crate::primitives::paint::image::Image;
use crate::shape::Shape;
use crate::shape::paint_animation::{PaintAnimation, PaintMotion, PaintRepeat};
use crate::theme::Theme;
use crate::theme::error::ThemeEffectError;
use crate::theme::look::press_effect::PressEffect;
use crate::ui::Ui;
use crate::ui::frame_report::FramePaint;
use crate::widget_core::configure::Configure;
use crate::widgets::panel::Panel;

/// The id of the app's load `n`, from 0, past the stock effects every registry holds first.
fn app(n: u32) -> EffectId {
    EffectId(u32::try_from(stock::COUNT).unwrap() + n)
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
    EffectRegistry::default().load(RING, ring(wgsl.to_owned()))
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
        .load(RING, ring(ring_wgsl(RING_PARAMS, RING_ENTRY)))
        .unwrap();
    assert!(registry.contains(RING));
    assert!(!registry.contains(THIN_RING));

    let plain: EffectName<()> = EffectName::named("plain").unwrap();
    let plain_wgsl = "fn effect(input: EffectInput) -> EffectOutput {
        return EffectOutput(vec4<f32>(1.0), vec4<f32>(0.0));
    }";
    registry.load(plain, EffectSource::new(plain_wgsl)).unwrap();
    let water: EffectName<(), BackdropEffect> = EffectName::named("water").unwrap();
    registry.load(water, EffectSource::new(plain_wgsl)).unwrap();
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
    let EffectError::Parse { source, text } = &parse else {
        panic!("{parse:?}");
    };
    assert!(text.starts_with("fn effect(\n"), "the app's text first");
    assert!(text.contains("struct EffectInput"), "then the prelude");
    let printed = source.emit_to_string(text);
    assert!(
        printed.contains("struct EffectInput"),
        "the error at the end of the app's text prints the contract's line"
    );
    assert!(
        error::Error::source(&parse).is_some(),
        "naga's error is the source"
    );
    assert_eq!(parse.to_string(), "the effect's WGSL does not parse");
    let invalid = format!(
        "{RING_PARAMS}
        @group(0) @binding(0) var<uniform> unsized_values: array<f32>;
        {RING_ENTRY}"
    );
    let validation = load(&invalid).unwrap_err();
    let EffectError::Validation { source, text } = &validation else {
        panic!("{validation:?}");
    };
    assert!(
        text.starts_with(&format!("{invalid}\n")),
        "the app's text first"
    );
    assert!(source.emit_to_string(text).contains("unsized_values"));
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
        ring(ring_wgsl(RING_PARAMS, RING_ENTRY)).image_slots(5),
    );
    assert!(matches!(
        slots,
        Err(EffectError::TooManyImageSlots { slots: 5 })
    ));
    assert!(
        EffectRegistry::default()
            .load(
                RING,
                ring(ring_wgsl(RING_PARAMS, RING_ENTRY)).image_slots(4)
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
    let module = contract::check(&sampling("effect_image1"), KindId::Color, params, 2).unwrap();
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
        contract::check(&sampling("effect_image2"), KindId::Color, params, 2),
        Err(EffectError::Parse { .. })
    ));
    assert!(matches!(
        contract::check(&sampling("effect_image0"), KindId::Color, params, 0),
        Err(EffectError::Parse { .. })
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
    let refused = EffectRegistry::default().load(huge, EffectSource::new("not read"));
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
    let refused = EffectRegistry::default().load(unlisted, EffectSource::new(one_argument));
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

    let source = EffectSource::<Huge>::new("x").power_saver(ReducedPaint::Nothing);
    assert!(format!("{source:?}").contains("Nothing"));
    assert_eq!(
        ReducedPaint::Effect(huge),
        ReducedPaint::<Huge>::Effect(huge)
    );
    assert_ne!(ReducedPaint::<Huge>::Same, ReducedPaint::Nothing);
}

/// A second load under a name replaces the first, its power-saver alternative with it; a reload
/// that fails returns its error and leaves the effect loaded before.
#[test]
fn a_load_replaces_and_a_refused_reload_keeps_the_last() {
    let registry = EffectRegistry::default();
    let valid = ring_wgsl(RING_PARAMS, RING_ENTRY);
    registry.load(RING, ring(valid.clone())).unwrap();
    assert_eq!(registry.power_saver_of(RING), Some(ReducedName::Same));
    registry
        .load(
            RING,
            ring(valid).power_saver(ReducedPaint::Effect(THIN_RING)),
        )
        .unwrap();
    assert_eq!(
        registry.power_saver_of(RING),
        Some(ReducedName::Effect(InlineName::new("thin ring").unwrap()))
    );

    let refused = registry.load(RING, ring("fn effect(").power_saver(ReducedPaint::Nothing));
    assert!(matches!(refused, Err(EffectError::Parse { .. })));
    assert!(registry.contains(RING));
    assert_eq!(
        registry.power_saver_of(RING),
        Some(ReducedName::Effect(InlineName::new("thin ring").unwrap())),
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
        Some(ReducedName::Effect(EffectName::RIPPLE.inline()))
    );
    let reserved = EffectName::<Ring>::named("palantir.ring").unwrap();
    let refused = registry.load(reserved, ring(ring_wgsl(RING_PARAMS, RING_ENTRY)));
    assert!(
        matches!(refused, Err(EffectError::ReservedName)),
        "{refused:?}"
    );
    assert_eq!(
        refused.unwrap_err().to_string(),
        "the effect's name starts with `palantir.`, which palantir's stock effects hold"
    );
    assert!(!registry.contains(reserved));
    let ripple = EffectName::<PressInput>::named("palantir.ripple").unwrap();
    assert!(matches!(
        registry.load(ripple, EffectSource::new("fn effect(")),
        Err(EffectError::ReservedName)
    ));
    assert_eq!(registry.epoch(), 2, "the two stock loads alone");
}

/// An app's press effect: its press color, over.
const RIPPLE_WGSL: &str = concat!(
    include_str!("stock/press.wgsl"),
    "fn effect(input: EffectInput, params: Params) -> EffectOutput {
        return effect_over(params.color);
    }"
);

/// A theme's press slots name only effects over `PressInput` that are loaded: one nothing loaded
/// and one of other parameters are each refused with the name, and the theme held before stays;
/// a stock ripple and an app's press effect are taken.
#[test]
fn a_theme_names_only_loaded_press_effects() {
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
        h.ui_mut().set_theme(scheme, naming("missing")),
        Err(ThemeEffectError::Unloaded(missing))
    );
    assert_eq!(
        ThemeEffectError::Unloaded(missing).to_string(),
        "the theme's press effect `missing` is not loaded"
    );
    let blend = EffectName::named(BLEND.name()).unwrap();
    assert_eq!(
        h.ui_mut().set_theme(scheme, naming(BLEND.name())),
        Err(ThemeEffectError::NotPress(blend))
    );
    assert_eq!(
        ThemeEffectError::NotPress(blend).to_string(),
        "the theme's press effect `blend` is not a color effect over `PressInput`"
    );
    assert!(
        Rc::ptr_eq(h.ui().scheme_theme(scheme), &held),
        "the theme before stays"
    );

    h.ui_mut()
        .set_theme(scheme, naming("palantir.sparkle"))
        .unwrap();
    let ours = EffectName::<PressInput>::named("ours").unwrap();
    h.ui()
        .load_effect(ours, EffectSource::new(RIPPLE_WGSL))
        .unwrap();
    h.ui_mut().set_theme(scheme, naming("ours")).unwrap();
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
        .load(RING, ring(ring_wgsl(RING_PARAMS, RING_ENTRY)))
        .unwrap();
    let untyped: EffectName<()> = EffectName::named("ring").unwrap();
    assert_panics_with("effect `ring` was loaded as", || registry.contains(untyped));
    let backdrop: EffectName<Ring, BackdropEffect> = EffectName::named("ring").unwrap();
    assert_panics_with("BackdropEffect", || {
        registry.load(backdrop, EffectSource::new("not read"))
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
        h.frame(|ui| ui.add_shape(Shape::effect(RING, params)));
    });

    let untyped_thin: EffectName<()> = EffectName::named("thin ring").unwrap();
    let plain = "fn effect(input: EffectInput) -> EffectOutput { return EffectOutput(); }";
    registry
        .load(untyped_thin, EffectSource::new(plain))
        .unwrap();
    assert_panics_with("effect `thin ring` was loaded as", || {
        registry.load(
            RING,
            ring(ring_wgsl(RING_PARAMS, RING_ENTRY)).power_saver(ReducedPaint::Effect(THIN_RING)),
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
        default: { return effect_screen(params.color); }
    }
}
";

/// The pixel at `(x, y)` of a 160 × 40 window whose ground is linear `(0.2, 0.4, 0.6)` and which
/// `paint` draws over, at scale 1.
fn painted(paint: impl Fn(&mut Ui), at: &[(u32, u32)]) -> Vec<[u8; 4]> {
    let gpu = headless_test_gpu();
    let mut h = Harness::builder(UVec2::new(160, 40))
        .mono()
        .depth(Depth::gpu(gpu.handles()))
        .build();
    h.ui()
        .load_effect(BLEND, EffectSource::new(BLEND_WGSL))
        .unwrap();
    let ground = RgbaF32::new(0.2, 0.4, 0.6, 1.0);
    h.prime(2, |ui| {
        ui.add_shape(Shape::rect().fill(ground));
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
                    Shape::effect(
                        BLEND,
                        Blend {
                            color: [0.4, 0.2, 0.1, 0.5],
                            mode,
                            pad0: 0,
                            pad1: 0,
                            pad2: 0,
                        },
                    )
                    .at(Rect::new(40.0 * mode as f32, 0.0, 40.0, 40.0)),
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
            ui.add_shape_animated(
                Shape::effect(
                    effect,
                    PressInput {
                        color: RgbaF32::new(1.0, 1.0, 1.0, 0.5),
                        highlight: RgbaF32::new(1.0, 1.0, 1.0, 141.0 / 255.0),
                        point: Vec2::new(20.0, 20.0),
                        released,
                        enter: 0.225,
                        exit: 0.15,
                        seed: 0.0,
                        pad0: 0.0,
                        pad1: 0.0,
                    },
                )
                .at(Rect::new(0.0, 0.0, 160.0, 40.0)),
                PaintAnimation::clock()
                    .with_repeat(PaintRepeat::Held)
                    .with_period(Duration::from_millis(time_ms)),
            );
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
            ui.add_shape_animated(
                Shape::effect(
                    EffectName::SPARKLE,
                    PressInput {
                        color: RgbaF32::new(1.0, 1.0, 1.0, 0.25),
                        highlight,
                        point: Vec2::new(40.0, 20.0),
                        released: -1.0,
                        enter: 0.45,
                        exit: 0.375,
                        seed: 0.0,
                        pad0: 0.0,
                        pad1: 0.0,
                    },
                )
                .at(Rect::new(0.0, 0.0, 160.0, 40.0)),
                PaintAnimation::clock()
                    .with_repeat(PaintRepeat::Held)
                    .with_period(Duration::from_millis(180)),
            );
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
                .chrome(Chrome {
                    fill: RgbaF32::TRANSPARENT.into(),
                    corners: Corners::all(12.0),
                    ..Chrome::default()
                })
                .clip(clip)
                .show(ui, |ui| {
                    Panel::vstack()
                        .size((Sizing::fixed(20.0), Sizing::fixed(20.0)))
                        .show(ui, |ui| {
                            ui.add_shape(
                                Shape::effect(
                                    BLEND,
                                    Blend {
                                        color: [0.0, 0.0, 1.0, 1.0],
                                        mode: 0,
                                        pad0: 0,
                                        pad1: 0,
                                        pad2: 0,
                                    },
                                )
                                .at(Rect::new(-30.0, -10.0, 140.0, 40.0)),
                            );
                        });
                });
        }
    };
    let blue = [0, 0, 255, 255];
    let ground = bytes(RgbaF32::new(0.2, 0.4, 0.6, 1.0));
    assert_eq!(
        painted(ring(ClipMode::Rect), &[(50, 10), (100, 10), (50, 35)]),
        [blue, ground, ground]
    );
    assert_eq!(
        painted(ring(ClipMode::Rounded), &[(40, 15), (1, 1)]),
        [blue, ground],
        "the rounded clip's corner cuts the effect"
    );
}

/// A new device builds every loaded effect again: after a loss and a recovery onto a fresh
/// device, the same frame paints the effect's blue, as before the loss.
#[test]
fn a_recovered_device_paints_the_loaded_effects() {
    let gpu = isolated_headless_test_gpu();
    let mut h = Harness::builder(UVec2::new(16, 16))
        .mono()
        .depth(Depth::gpu(gpu.handles()))
        .build();
    h.ui()
        .load_effect(BLEND, EffectSource::new(BLEND_WGSL))
        .unwrap();
    let paint = |ui: &mut Ui| {
        ui.add_shape(Shape::rect().fill(RgbaF32::new(0.2, 0.4, 0.6, 1.0)));
        ui.add_shape(Shape::effect(
            BLEND,
            Blend {
                color: [0.0, 0.0, 1.0, 1.0],
                mode: 0,
                pad0: 0,
                pad1: 0,
                pad2: 0,
            },
        ));
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
        .depth(Depth::gpu(gpu.handles()))
        .build();
    h.ui()
        .load_effect(BLEND, EffectSource::new(BLEND_WGSL))
        .unwrap();
    let paint = |ui: &mut Ui| {
        ui.add_shape(Shape::effect(
            BLEND,
            Blend {
                color: [0.0, 0.0, 1.0, 1.0],
                mode: 0,
                pad0: 0,
                pad1: 0,
                pad2: 0,
            },
        ));
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
            PaintCall::Effect(p) => Some((p.effect, p.time)),
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
        ui.add_shape(Shape::effect(BLEND, BLUE));
        ui.add_shape_animated(Shape::effect(BLEND, BLUE), looping);
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
    let report = still.frame(|ui| ui.add_shape(Shape::effect(BLEND, BLUE)));
    assert_eq!(report.repaint_at, None, "a still effect asks for no frame");
}

/// Under reduced motion each effect paints as any paint animation does, with no wake: a
/// decorative loop reads 0, as unanimated; a decorative single pass of 2 s reads its end, 2 s,
/// from the first frame; a progress loop runs on, 0.5 s into its 2 s pass at 0.5 s.
#[test]
fn reduced_motion_holds_a_decorative_effect() {
    let ms = Duration::from_millis;
    let looping = PaintAnimation::clock()
        .with_period(ms(2000))
        .with_repeat(PaintRepeat::Forever);
    let once = PaintAnimation::clock().with_period(ms(2000));
    let progress = looping.with_motion(PaintMotion::Progress);
    let mut h = blend_window();
    h.set_system_motion_reduced(true);
    let record = |ui: &mut Ui| {
        ui.add_shape_animated(Shape::effect(BLEND, BLUE), looping);
        ui.add_shape_animated(Shape::effect(BLEND, BLUE), once);
    };
    let report = h.at(ms(500)).frame(record);
    assert_eq!(report.repaint_at, None);
    let times: Vec<f32> = effect_draws(&h).iter().map(|&(_, time)| time).collect();
    assert_eq!(times, [0.0, 2.0]);

    let report = h
        .at(ms(500))
        .frame(|ui| ui.add_shape_animated(Shape::effect(BLEND, BLUE), progress));
    assert_eq!(report.repaint_at, Some(ms(500)));
    assert_eq!(effect_draws(&h), [(app(0), 0.5)]);
}

/// While power is reduced each effect paints what its source named: `blend` paints `lite`,
/// `gone` paints nothing, and `lite` itself, `Same`, paints as itself. With full power each
/// paints as itself: ids 0, 2 and 1 in load order `blend`, `lite`, `gone`.
#[test]
fn reduced_power_paints_each_alternative() {
    let lite: EffectName<Blend> = EffectName::named("lite").unwrap();
    let gone: EffectName<Blend> = EffectName::named("gone").unwrap();
    let mut h = Harness::mono(UVec2::new(16, 16));
    h.ui()
        .load_effect(
            BLEND,
            EffectSource::new(BLEND_WGSL).power_saver(ReducedPaint::Effect(lite)),
        )
        .unwrap();
    h.ui()
        .load_effect(lite, EffectSource::new(BLEND_WGSL))
        .unwrap();
    h.ui()
        .load_effect(
            gone,
            EffectSource::new(BLEND_WGSL).power_saver(ReducedPaint::Nothing),
        )
        .unwrap();
    let record = |ui: &mut Ui| {
        for name in [BLEND, gone, lite] {
            ui.add_shape(Shape::effect(name, BLUE));
        }
    };
    h.frame(record);
    let ids =
        |h: &Harness| -> Vec<EffectId> { effect_draws(h).iter().map(|&(id, _)| id).collect() };
    assert_eq!(ids(&h), [app(0), app(2), app(1)]);
    h.set_system_power_reduced(true);
    h.frame(record);
    assert_eq!(ids(&h), [app(1), app(1)]);
    h.set_system_power_reduced(false);
    h.frame(record);
    assert_eq!(ids(&h), [app(0), app(2), app(1)], "back at full power");
}

/// What an effect's paint cannot be panics as the shape is added: an alternative nothing loaded,
/// or one loaded later with other types, while power is reduced; a clock-only animation on a
/// shape that reads no time.
#[test]
fn an_effect_that_cannot_paint_panics() {
    let missing: EffectName<Blend> = EffectName::named("missing").unwrap();
    let mut h = Harness::mono(UVec2::new(16, 16));
    h.ui()
        .load_effect(
            BLEND,
            EffectSource::new(BLEND_WGSL).power_saver(ReducedPaint::Effect(missing)),
        )
        .unwrap();
    h.set_system_power_reduced(true);
    assert_panics_with(
        "no effect is loaded under `missing`, the power-saver alternative of `blend`",
        || h.frame(|ui| ui.add_shape(Shape::effect(BLEND, BLUE))),
    );
    let untyped: EffectName<()> = EffectName::named("missing").unwrap();
    let plain = "fn effect(input: EffectInput) -> EffectOutput { return EffectOutput(); }";
    h.ui()
        .load_effect(untyped, EffectSource::new(plain))
        .unwrap();
    assert_panics_with("effect `missing` was loaded as", || {
        h.frame(|ui| ui.add_shape(Shape::effect(BLEND, BLUE)))
    });

    let mut h = blend_window();
    assert_panics_with("drives no channel", || {
        h.frame(|ui| {
            ui.add_shape_animated(Shape::rect().fill(RgbaF32::WHITE), PaintAnimation::clock());
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
        .depth(Depth::gpu(gpu.handles()))
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
        ui.add_shape_animated(Shape::effect(BLEND, BLUE), looping);
    });
    assert_eq!(
        h.read_back().get_pixel(8, 8).0,
        bytes(RgbaF32::new(0.75, 0.0, 0.0, 1.0))
    );
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
        ui.add_shape_animated(
            Shape::effect(BLEND, BLUE).at(Rect::new(2.0, 2.0, 6.0, 6.0)),
            looping,
        );
        ui.add_shape(Shape::effect(BLEND, BLUE).at(Rect::new(10.0, 10.0, 4.0, 4.0)));
    };
    let mut h = blend_window();
    h.frame(record);
    let report = h.at(Duration::from_millis(16)).frame(record);
    assert_eq!(report.paint(), FramePaint::Partial);
    let rects: Vec<Rect> = h.damage_region().iter_rects().collect();
    assert_eq!(rects, [Rect::new(1.5, 1.5, 7.0, 7.0)]);
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
        .depth(Depth::gpu(gpu.clone()))
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
                Shape::effect(PAIR, ())
                    .at(Rect::new(x, 0.0, 16.0, 16.0))
                    .images([first.clone(), second.clone()]),
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
        ui.add_shape(Shape::rect().fill(ground));
        ui.add_shape(Shape::effect(PAIR, ()).images([red.clone(), red.clone()]));
    };
    h.prime(2, paint);
    assert_eq!(h.read_back().get_pixel(8, 8).0, [255, 0, 0, 255]);

    gpu.handles().lose();
    let fresh = isolated_headless_test_gpu();
    h.recover(fresh.handles());
    assert!(!red.is_resident());
    h.prime(2, paint);
    assert_eq!(h.read_back().get_pixel(8, 8).0, bytes(ground));

    red.update(&solid(0xff0000)).unwrap();
    h.prime(2, paint);
    assert_eq!(h.read_back().get_pixel(8, 8).0, [255, 0, 0, 255]);
}

/// A shape binds as many images as the effect it paints declares: one image for two slots panics
/// with both counts as it is added; so does a power-saver alternative of other slots, at the load
/// when it is loaded already, and at the shape while power is reduced when it loads later.
#[test]
fn a_shape_binds_as_many_images_as_its_effect_declares() {
    let mut h = Harness::mono(UVec2::new(8, 8));
    h.ui()
        .load_effect(PAIR, EffectSource::new(PAIR_WGSL).image_slots(2))
        .unwrap();
    let red = h.ui().load_image(&solid(0xff0000)).unwrap();
    assert_panics_with(
        "`pair` paints an effect that binds 2 images, and the shape binds 1",
        || h.frame(|ui| ui.add_shape(Shape::effect(PAIR, ()).images([red.clone()]))),
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
                    .power_saver(ReducedPaint::Effect(single)),
            )
        },
    );
    let later: EffectName<()> = EffectName::named("later").unwrap();
    h.ui()
        .load_effect(
            PAIR,
            EffectSource::new(PAIR_WGSL)
                .image_slots(2)
                .power_saver(ReducedPaint::Effect(later)),
        )
        .unwrap();
    h.ui()
        .load_effect(later, EffectSource::new(one_image).image_slots(1))
        .unwrap();
    h.set_system_power_reduced(true);
    assert_panics_with(
        "`pair` paints an effect that binds 1 images, and the shape binds 2",
        || h.frame(|ui| ui.add_shape(Shape::effect(PAIR, ()).images([red.clone(), red.clone()]))),
    );
}

/// An image update repaints each effect that binds it, as it repaints an image shape: the effect
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
            Shape::effect(PAIR, ())
                .at(Rect::new(2.0, 2.0, 6.0, 6.0))
                .images([changing.clone(), still.clone()]),
        );
        ui.add_shape(
            Shape::effect(PAIR, ())
                .at(Rect::new(10.0, 10.0, 4.0, 4.0))
                .images([still.clone(), still.clone()]),
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
        .depth(Depth::gpu(gpu.handles()))
        .build();
    h.ui()
        .load_effect(SHIFT, EffectSource::new(SHIFT_WGSL))
        .unwrap();
    let red = RgbaF32::new(1.0, 0.0, 0.0, 1.0);
    let blue = RgbaF32::new(0.0, 0.0, 1.0, 1.0);
    let green = RgbaF32::new(0.0, 1.0, 0.0, 1.0);
    let paint = |middle: RgbaF32, dx: f32| {
        move |ui: &mut Ui| {
            ui.add_shape(Shape::rect().at(Rect::new(0.0, 0.0, 16.0, 16.0)).fill(red));
            ui.add_shape(
                Shape::rect()
                    .at(Rect::new(16.0, 0.0, 10.0, 16.0))
                    .fill(middle),
            );
            ui.add_shape(
                Shape::rect()
                    .at(Rect::new(26.0, 0.0, 6.0, 16.0))
                    .fill(green),
            );
            ui.add_shape(
                Shape::backdrop_effect(SHIFT, Shift { dx })
                    .at(Rect::new(4.0, 0.0, 12.0, 16.0))
                    .reach(8.0),
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

/// A backdrop effect blurs what it reads by its σ through the capture and blur a backdrop filter
/// takes, and reads the result as a filter's pane does: over a black and white edge at x 16 in a
/// 32 × 16 window, an effect that shows what it reads, blurred by σ 3, paints row 8 exactly as a
/// rect of `BackdropFilter::blur(3.0)` over the same edge, and unlike the sharp edge.
#[test]
fn a_backdrop_effect_blurs_as_a_backdrop_filter() {
    let gpu = headless_test_gpu();
    let row = |pane: &dyn Fn(&mut Ui)| {
        let mut h = Harness::builder(UVec2::new(32, 16))
            .mono()
            .depth(Depth::gpu(gpu.handles()))
            .build();
        h.ui()
            .load_effect(SHIFT, EffectSource::new(SHIFT_WGSL))
            .unwrap();
        h.prime(2, |ui| {
            ui.add_shape(
                Shape::rect()
                    .at(Rect::new(0.0, 0.0, 16.0, 16.0))
                    .fill(RgbaF32::BLACK),
            );
            ui.add_shape(
                Shape::rect()
                    .at(Rect::new(16.0, 0.0, 16.0, 16.0))
                    .fill(RgbaF32::WHITE),
            );
            pane(ui);
        });
        let image = h.read_back();
        (0..32).map(|x| image.get_pixel(x, 8).0).collect::<Vec<_>>()
    };
    let effect = row(&|ui| {
        ui.add_shape(Shape::backdrop_effect(SHIFT, SHARP).blur(3.0));
    });
    let filter = row(&|ui| {
        ui.add_shape(Shape::rect().backdrop_filter(BackdropFilter::blur(3.0)));
    });
    let sharp = row(&|_| {});
    assert_eq!(effect, filter);
    assert_ne!(effect, sharp, "σ 3 blurs the edge");
}

/// Each preference picks its own alternative, transparency's over motion's over power's: an
/// effect whose source names one for each paints itself with nothing reduced, power's with power
/// reduced, motion's with motion and power, and transparency's with all three. An alternative
/// nothing loaded panics with the preference that named it.
#[test]
fn each_reduction_paints_its_alternative_in_order() {
    let names = ["base", "clear", "still", "saver"]
        .map(|name| -> EffectName<Blend> { EffectName::named(name).unwrap() });
    let [base, clear, still, saver] = names;
    let mut h = Harness::mono(UVec2::new(16, 16));
    h.ui()
        .load_effect(
            base,
            EffectSource::new(BLEND_WGSL)
                .reduced_transparency(ReducedPaint::Effect(clear))
                .reduced_motion(ReducedPaint::Effect(still))
                .power_saver(ReducedPaint::Effect(saver)),
        )
        .unwrap();
    for name in [clear, still, saver] {
        h.ui()
            .load_effect(name, EffectSource::new(BLEND_WGSL))
            .unwrap();
    }
    let painted = |h: &mut Harness| {
        h.frame(|ui| ui.add_shape(Shape::effect(base, BLUE)));
        effect_draws(h)[0].0
    };
    assert_eq!(painted(&mut h), app(0));
    h.set_system_power_reduced(true);
    assert_eq!(painted(&mut h), app(3), "power");
    h.set_system_motion_reduced(true);
    assert_eq!(painted(&mut h), app(2), "motion over power");
    h.set_system_transparency_reduced(true);
    assert_eq!(painted(&mut h), app(1), "transparency over both");

    let gone: EffectName<Blend> = EffectName::named("gone").unwrap();
    h.ui()
        .load_effect(
            base,
            EffectSource::new(BLEND_WGSL).reduced_motion(ReducedPaint::Effect(gone)),
        )
        .unwrap();
    assert_panics_with(
        "no effect is loaded under `gone`, the reduced-motion alternative of `base`",
        || h.frame(|ui| ui.add_shape(Shape::effect(base, BLUE))),
    );
}

/// `effect_backdrop` is a backdrop effect's alone: the same text loads as a backdrop effect and
/// fails to parse as a color effect, where the name is not declared.
#[test]
fn only_a_backdrop_effect_reads_what_is_under_it() {
    let params = ParamsLayout {
        fields: Shift::FIELDS,
        size: size_of::<Shift>(),
    };
    let module = contract::check(SHIFT_WGSL, KindId::Backdrop, params, 0).unwrap();
    assert_eq!(module.kind, KindId::Backdrop);
    assert!(matches!(
        contract::check(SHIFT_WGSL, KindId::Color, params, 0),
        Err(EffectError::Parse { .. })
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
        .depth(Depth::gpu(gpu.handles()))
        .build();
    h.ui()
        .load_effect(SHIFT, EffectSource::new(SHIFT_WGSL))
        .unwrap();
    let green = RgbaF32::new(0.0, 1.0, 0.0, 1.0);
    h.prime(2, |ui: &mut Ui| {
        ui.add_shape(
            Shape::rect()
                .at(Rect::new(0.0, 0.0, 26.0, 16.0))
                .fill(RgbaF32::new(1.0, 0.0, 0.0, 1.0)),
        );
        ui.add_shape(
            Shape::rect()
                .at(Rect::new(26.0, 0.0, 6.0, 16.0))
                .fill(green),
        );
        ui.add_shape(
            Shape::backdrop_effect(SHIFT, Shift { dx: 12.0 })
                .at(Rect::new(16.0, 0.0, 16.0, 16.0))
                .reach(8.0),
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
        .depth(Depth::gpu(gpu.handles()))
        .build();
    h.ui()
        .load_effect(BLEND, EffectSource::new(BLEND_WGSL))
        .unwrap();
    let ground = RgbaF32::new(0.2, 0.4, 0.6, 1.0);
    h.prime(2, |ui: &mut Ui| {
        ui.add_shape(Shape::rect().fill(ground));
        ui.add_shape_animated(
            Shape::effect(BLEND, BLUE).at(Rect::new(0.0, 14.0, 32.0, 4.0)),
            PaintAnimation::turn(0.25, 0.25),
        );
    });
    let image = h.read_back();
    assert_eq!(
        [image.get_pixel(16, 2).0, image.get_pixel(2, 16).0],
        [bytes(RgbaF32::new(0.0, 0.0, 1.0, 1.0)), bytes(ground)]
    );
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
        .depth(Depth::gpu(gpu.handles()))
        .build();
    h.ui()
        .load_effect(SHIFT, EffectSource::new(SHIFT_WGSL))
        .unwrap();
    let blue = RgbaF32::new(0.0, 0.0, 1.0, 1.0);
    h.prime(2, |ui: &mut Ui| {
        ui.add_shape(
            Shape::rect()
                .at(Rect::new(0.0, 0.0, 16.0, 16.0))
                .fill(RgbaF32::new(1.0, 0.0, 0.0, 1.0)),
        );
        ui.add_shape(Shape::rect().at(Rect::new(16.0, 0.0, 8.0, 16.0)).fill(blue));
        ui.add_shape(
            Shape::rect()
                .at(Rect::new(24.0, 0.0, 8.0, 16.0))
                .fill(RgbaF32::new(0.0, 1.0, 0.0, 1.0)),
        );
        ui.add_shape(
            Shape::backdrop_effect(SHIFT, Shift { dx: 12.0 })
                .at(Rect::new(4.0, 0.0, 11.5, 16.0))
                .reach(8.0),
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
        .depth(Depth::gpu(gpu.handles()))
        .build();
    h.ui()
        .load_effect(SHIFT, EffectSource::new(SHIFT_WGSL))
        .unwrap();
    let red = RgbaF32::new(1.0, 0.0, 0.0, 1.0);
    h.prime(2, |ui: &mut Ui| {
        ui.add_shape(Shape::rect().at(Rect::new(0.0, 0.0, 32.0, 16.0)).fill(red));
        ui.add_shape(
            Shape::rect()
                .at(Rect::new(0.0, 16.0, 32.0, 16.0))
                .fill(RgbaF32::new(0.0, 0.0, 1.0, 1.0)),
        );
        ui.add_shape_animated(
            Shape::backdrop_effect(SHIFT, SHARP).at(Rect::new(0.0, 14.0, 32.0, 4.0)),
            PaintAnimation::turn(0.25, 0.25),
        );
    });
    assert_eq!(
        h.read_back().get_pixel(16, 28).0,
        bytes(RgbaF32::new(0.0, 0.0, 1.0, 1.0))
    );
}
