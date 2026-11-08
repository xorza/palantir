//! Loading effects: the derive's layout, each refusal, a replacing load, a refused reload, and a
//! name used with other types.

use std::error;

use glam::UVec2;

use crate::effect::effect_kind::{BackdropEffect, ColorEffect};
use crate::effect::effect_name::EffectName;
use crate::effect::effect_params::{EffectParam, EffectParams, ParamType};
use crate::effect::effect_registry::{EffectRegistry, PowerSaverName};
use crate::effect::effect_source::{EffectSource, PowerSaverPaint};
use crate::effect::error::{EffectError, EntryMismatch, ParamMismatch};
use crate::host::harness::Harness;
use crate::internals::panic_probe::assert_panics_with;
use crate::primitives::identity::inline_name::InlineName;

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
/// `effect`, each signature departure, too many image slots.
#[test]
fn each_refusal_is_its_own_case() {
    let parse = load("fn effect(").unwrap_err();
    let EffectError::Parse { source, text } = &parse else {
        panic!("{parse:?}");
    };
    assert!(text.starts_with("fn effect(\n"), "the app's text first");
    assert!(text.contains("struct EffectInput"), "then the prelude");
    let printed = source.emit_to_string(text);
    assert!(printed.contains("wgsl:3:"), "{printed}");
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

    let source = EffectSource::<Huge>::new("x").power_saver(PowerSaverPaint::Nothing);
    assert!(format!("{source:?}").contains("Nothing"));
    assert_eq!(
        PowerSaverPaint::Effect(huge),
        PowerSaverPaint::<Huge>::Effect(huge)
    );
    assert_ne!(PowerSaverPaint::<Huge>::Same, PowerSaverPaint::Nothing);
}

/// A second load under a name replaces the first, its power-saver alternative with it; a reload
/// that fails returns its error and leaves the effect loaded before.
#[test]
fn a_load_replaces_and_a_refused_reload_keeps_the_last() {
    let registry = EffectRegistry::default();
    let valid = ring_wgsl(RING_PARAMS, RING_ENTRY);
    registry.load(RING, ring(valid.clone())).unwrap();
    assert_eq!(registry.power_saver_of(RING), Some(PowerSaverName::Same));
    registry
        .load(
            RING,
            ring(valid).power_saver(PowerSaverPaint::Effect(THIN_RING)),
        )
        .unwrap();
    assert_eq!(
        registry.power_saver_of(RING),
        Some(PowerSaverName::Effect(
            InlineName::new("thin ring").unwrap()
        ))
    );

    let refused = registry.load(
        RING,
        ring("fn effect(").power_saver(PowerSaverPaint::Nothing),
    );
    assert!(matches!(refused, Err(EffectError::Parse { .. })));
    assert!(registry.contains(RING));
    assert_eq!(
        registry.power_saver_of(RING),
        Some(PowerSaverName::Effect(
            InlineName::new("thin ring").unwrap()
        )),
        "the effect before the refused reload"
    );
}

/// A name keeps the parameter type and kind it loaded with: naming it with others, to load or to
/// ask, panics with both, and so does a power-saver alternative loaded with other types.
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

    let untyped_thin: EffectName<()> = EffectName::named("thin ring").unwrap();
    let plain = "fn effect(input: EffectInput) -> EffectOutput { return EffectOutput(); }";
    registry
        .load(untyped_thin, EffectSource::new(plain))
        .unwrap();
    assert_panics_with("effect `thin ring` was loaded as", || {
        registry.load(
            RING,
            ring(ring_wgsl(RING_PARAMS, RING_ENTRY))
                .power_saver(PowerSaverPaint::Effect(THIN_RING)),
        )
    });
}
