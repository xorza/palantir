//! Loading effects: the derive's layout, each refusal, a replacing load, a refused reload, and a
//! name used with other types.

use crate::effect::tests::*;

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
    include_str!("../stock/press.wgsl"),
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
                include_str!("../stock/material.wgsl"),
                include_str!("../stock/frosted.wgsl")
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
