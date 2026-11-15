use crate::effect::effect_registry::{EffectRegistry, ResolvedMaterial};
use crate::effect::effect_source::{EffectSource, ReducedEffect};
use crate::internals::panic_probe::assert_panics_with;
use crate::primitives::effect::effect_kind::BackdropEffect;
use crate::primitives::effect::effect_name::EffectName;
use crate::primitives::identity::effect_id::EffectId;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::material::Material;
use crate::primitives::paint::material::backdrop_reach::BackdropReach;
use crate::primitives::paint::material::frosted_params::FrostedParams;
use crate::primitives::paint::material::lens::Lens;
use crate::primitives::reduction::{Reduction, Reductions};
use crate::shape;
use crate::shape::Shape;
use crate::shape::paint::capture_params::CaptureReach;
use crate::shape::paint::lowered_material::{LoweredMaterial, MaterialRead};
use crate::shape::record_store::{RecordStore, Resolve};
use glam::Vec2;

/// `material` lowered through effect 0 under the box alone, with its parameters staged in `store`.
fn captured_in(store: &mut RecordStore, material: &Material) -> LoweredMaterial {
    let params = store.stage_effect_params(material.effect.bytes());
    LoweredMaterial::lower(
        material,
        ResolvedMaterial {
            id: EffectId(0),
            reach: BackdropReach::NONE,
        },
        params,
    )
}

/// [`captured_in`] a store of its own.
fn captured(material: &Material) -> LoweredMaterial {
    captured_in(&mut RecordStore::default(), material)
}

/// A rect, a circle and a triangle take the same checks: each case panics with its kind's rule.
#[test]
fn a_material_out_of_its_kinds_panics_where_it_enters() {
    let base = Material::blur(1.0);
    let nan = Material::new(
        EffectName::FROSTED,
        FrostedParams {
            saturation: f32::NAN,
            noise: 0.0,
        },
    );
    for (material, rule) in [
        (Material::blur(-1.0), "a length must"),
        (base.clone().with_max_luminance(-0.1), "a factor must"),
        (
            base.clone().with_min_luminance(f32::INFINITY),
            "a factor must",
        ),
        (
            base.with_fallback(RgbaF32::new(f32::NAN, 0.0, 0.0, 1.0)),
            "a color must",
        ),
        (nan, "a material's parameters must be finite"),
    ] {
        for entry in ["rect", "circle", "triangle"] {
            let panic = std::panic::catch_unwind(|| match entry {
                "rect" => _ = shape::internals::check(&Shape::rect(), material.clone()),
                "circle" => _ = shape::internals::check(&Shape::circle(), material.clone()),
                _ => {
                    let triangle = Shape::triangle(Vec2::ZERO, Vec2::X, Vec2::Y);
                    _ = shape::internals::check(&triangle, material.clone());
                }
            })
            .expect_err(entry);
            let message = panic
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| panic.downcast_ref::<&str>().copied())
                .unwrap_or_default();
            assert!(message.contains(rule), "{entry} {material:?}: {message}");
        }
    }
}

/// What a material reads follows the reductions: its effect over what is under it while
/// transparency runs whole, its effect resolved through the registry, where the frost a blur wears
/// is effect 2, past the two ripples, and the glass effect 3, reading as far as its lens bends; and
/// its fallback while transparency is reduced, with the theme's surface at hand for one that names
/// none. A material that cannot paint panics in every mode: over a name nothing loaded, over a
/// color effect, and with another count of lanes than its effect takes.
#[test]
fn a_material_reads_its_fallback_while_transparency_is_reduced() {
    let effects = EffectRegistry::default();
    let theme = RgbaF32::new(0.1, 0.2, 0.3, 0.75);
    let resolve = |transparency| Resolve {
        effects: &effects,
        reductions: Reductions {
            transparency,
            ..Reductions::NONE
        },
        material_fallback: theme,
    };
    let blur = Material::blur(4.0);
    assert_eq!(
        MaterialRead::resolve(&blur, resolve(Reduction::Unreduced)),
        Some(MaterialRead::Capture(ResolvedMaterial {
            id: EffectId(2),
            reach: BackdropReach::NONE,
        }))
    );
    assert_eq!(
        MaterialRead::resolve(&blur, resolve(Reduction::Reduced)),
        Some(MaterialRead::Fallback { theme })
    );
    assert_eq!(
        MaterialRead::resolve(
            &Material::glass(Lens::WINDOW_GLASS),
            resolve(Reduction::Unreduced)
        ),
        Some(MaterialRead::Capture(ResolvedMaterial {
            id: EffectId(3),
            reach: BackdropReach::Lens(Lens::WINDOW_GLASS),
        }))
    );
    let gone = Material::new(EffectName::<(), BackdropEffect>::named("gone").unwrap(), ());
    assert_panics_with("no effect is loaded under `gone`", || {
        MaterialRead::resolve(&gone, resolve(Reduction::Reduced))
    });
    let ripple = Material::new(
        EffectName::<(), BackdropEffect>::named("palantir.ripple").unwrap(),
        (),
    );
    assert_panics_with(
        "a material over `palantir.ripple` paints effect 0, which is no material's",
        || MaterialRead::resolve(&ripple, resolve(Reduction::Unreduced)),
    );
    let short = Material::new(
        EffectName::<(), BackdropEffect>::named("palantir.frosted").unwrap(),
        (),
    );
    assert_panics_with(
        "effect `palantir.frosted` takes 2 parameter lanes, and its material holds 0",
        || MaterialRead::resolve(&short, resolve(Reduction::Unreduced)),
    );
}

/// An effect's source declares how far a material over it reads from its parameters: an app's
/// backdrop effect over one lane reads that lane's px past its box, so a material of 6 reads 6
/// and one of 2 reads 2; with no declaration a material reads under its box alone. An alternative
/// paints at its own id and reach.
#[test]
fn an_effects_source_declares_its_materials_reach() {
    #[derive(
        Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable, palantir_derive::EffectParams,
    )]
    #[repr(C)]
    struct Shift {
        dx: f32,
    }
    let wgsl = "struct Params { dx: f32 }
    fn effect(input: EffectInput, params: Params) -> EffectOutput {
        return effect_over(effect_backdrop(input.pos + vec2<f32>(params.dx, 0.0)));
    }";
    let effects = EffectRegistry::default();
    let reaching = EffectName::<Shift, BackdropEffect>::named("reaching").unwrap();
    let plain = EffectName::<Shift, BackdropEffect>::named("plain").unwrap();
    effects
        .load(
            reaching,
            &EffectSource::new(wgsl).reach(|shift| BackdropReach::Px(shift.dx)),
        )
        .unwrap();
    effects.load(plain, &EffectSource::new(wgsl)).unwrap();
    let reach = |effect, dx| {
        effects
            .resolve_material(&Material::new(effect, Shift { dx }), Reductions::NONE)
            .map(|resolved| resolved.reach)
    };
    assert_eq!(reach(reaching, 6.0), Some(BackdropReach::Px(6.0)));
    assert_eq!(reach(reaching, 2.0), Some(BackdropReach::Px(2.0)));
    assert_eq!(reach(plain, 6.0), Some(BackdropReach::NONE));

    // While power is reduced, a material paints its effect's alternative, at that alternative's
    // id and reach: ids follow load order past the four stock effects, so `reaching` is 4 and
    // `saving`, which gives way to it, 6; `vanishing` gives way to nothing.
    let saving = EffectName::<Shift, BackdropEffect>::named("saving").unwrap();
    let vanishing = EffectName::<Shift, BackdropEffect>::named("vanishing").unwrap();
    effects
        .load(
            saving,
            &EffectSource::new(wgsl).power_saver(ReducedEffect::Other(reaching)),
        )
        .unwrap();
    effects
        .load(
            vanishing,
            &EffectSource::new(wgsl).power_saver(ReducedEffect::Nothing),
        )
        .unwrap();
    let resolve = |effect, power| {
        let reductions = Reductions {
            power,
            ..Reductions::NONE
        };
        effects.resolve_material(&Material::new(effect, Shift { dx: 6.0 }), reductions)
    };
    assert_eq!(
        resolve(saving, Reduction::Unreduced),
        Some(ResolvedMaterial {
            id: EffectId(6),
            reach: BackdropReach::NONE,
        })
    );
    assert_eq!(
        resolve(saving, Reduction::Reduced),
        Some(ResolvedMaterial {
            id: EffectId(4),
            reach: BackdropReach::Px(6.0),
        })
    );
    assert!(resolve(vanishing, Reduction::Unreduced).is_some());
    assert_eq!(resolve(vanishing, Reduction::Reduced), None);
}

/// A luminance floor lowers to the f16 at or above it, so a material never shows darker than its
/// author allowed: 0.3's nearest f16, 0.300048828125 = 1229/4096, is above it and stays; 0.1's
/// nearest, 0.0999755859375 = 819/8192, is below it, so it lowers one step up, to
/// 0.10003662109375 = 1639/16384; 0.5 is exact. A material with no floor lowers to 0.
#[test]
fn a_luminance_floor_rounds_up_where_it_lowers() {
    for (floor, want) in [
        (Some(0.3), 1229.0 / 4096.0),
        (Some(0.1), 1639.0 / 16384.0),
        (Some(0.5), 0.5),
        (None, 0.0),
    ] {
        let material = Material {
            min_luminance: floor,
            ..Material::blur(1.0)
        };
        assert_eq!(captured(&material).min_luminance(), want, "{floor:?}");
    }
}

/// A luminance ceiling lowers to the f16 at or below it, so a material never shows brighter than
/// its author allowed: 0.3's nearest f16 is 0.300048828125 = 1229/4096, above it, so it lowers
/// one step down, to 0.2998046875 = 307/1024; 0.1's nearest, 0.0999755859375 = 819/8192, is
/// below it and stays; 0.5 is exact. A material with no ceiling lowers to 65504, the lane's top,
/// and 1e6, which a tween may carry past the lane, saturates there.
#[test]
fn a_luminance_ceiling_rounds_down_where_it_lowers() {
    for (ceiling, want) in [
        (Some(0.3), 307.0 / 1024.0),
        (Some(0.1), 819.0 / 8192.0),
        (Some(0.5), 0.5),
        (Some(0.0), 0.0),
        (Some(1.0e6), 65504.0),
        (None, 65504.0),
    ] {
        let material = Material {
            max_luminance: ceiling,
            ..Material::blur(1.0)
        };
        assert_eq!(captured(&material).max_luminance(), want, "{ceiling:?}");
    }
}

/// A tween that overshoots a fade to none, as `OutBack` does, carries a material past its kinds,
/// and it coerces where it paints: a negative blur and floor paint as 0, and a lens's reach as
/// its kinds: a negative bezel and thickness as 0, an index under 1 as 1, a dispersion of 2 as 1.
/// The parameters are staged as they are, its effect's to read: the frost's two lanes, word for
/// word.
#[test]
fn a_material_coerces_into_its_kinds_where_it_paints() {
    let past = Material {
        backdrop_blur: -0.4,
        min_luminance: Some(-0.04),
        ..Material::new(
            EffectName::FROSTED,
            FrostedParams {
                saturation: -0.1,
                noise: 0.02,
            },
        )
    };
    let mut store = RecordStore::default();
    let lens = Lens {
        bezel: -2.4,
        thickness: -1.6,
        refractive_index: 0.95,
        dispersion: 2.0,
        highlight: -1.0,
    };
    let params = store.stage_effect_params(past.effect.bytes());
    let lowered = LoweredMaterial::lower(
        &past,
        ResolvedMaterial {
            id: EffectId(0),
            reach: BackdropReach::Lens(lens),
        },
        params,
    );
    assert_eq!(lowered.capture.sigma, 0.0);
    assert_eq!(lowered.min_luminance(), 0.0);
    let CaptureReach::Lens(lanes) = lowered.capture.reach else {
        panic!("a lens's reach");
    };
    assert_eq!(lanes.lanes(), [0.0, 0.0, 1.0, 1.0]);
    let start = lowered.params.start as usize;
    assert_eq!(lowered.params.len, 2);
    let words: Vec<f32> = store.effect_params[start..start + 2]
        .iter()
        .map(|&w| f32::from_bits(w))
        .collect();
    assert_eq!(words, [-0.1, 0.02]);
}
