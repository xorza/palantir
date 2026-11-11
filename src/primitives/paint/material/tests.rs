use crate::effect::effect_registry::EffectRegistry;
use crate::internals::panic_probe::assert_panics_with;
use crate::primitives::effect::effect_name::EffectName;
use crate::primitives::identity::effect_id::EffectId;
use crate::primitives::math::lerp::Lerp;
use crate::primitives::math::libm_ext::Libm;
use crate::primitives::paint::brush::Brush;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::srgba_u8::SrgbaU8;
use crate::primitives::paint::material::lens::Lens;
use crate::primitives::paint::material::material_fallback::MaterialFallback;
use crate::primitives::paint::material::material_input::MaterialInput;
use crate::primitives::paint::material::{Material, bend_root, ceiling_bend};
use crate::primitives::paint::paint::Paint;
use crate::primitives::reduction::{Reduction, Reductions};
use crate::shape;
use crate::shape::Shape;
use crate::shape::paint::lowered_material::{LoweredMaterial, MaterialRead};
use crate::shape::record_store::{RecordStore, Resolve};

/// `material` lowered through effect 0, with its input staged in `store`.
fn captured_in(store: &mut RecordStore, material: &Material) -> LoweredMaterial {
    LoweredMaterial::lower(store, material, EffectId(0))
}

/// [`captured_in`] a store of its own.
fn captured(material: &Material) -> LoweredMaterial {
    captured_in(&mut RecordStore::default(), material)
}

/// The constructors name their stock effects and set exactly their own fields, the theme's
/// fallback included, each `with_*` its own, `with_fallback` a brush of the author's in its place.
#[test]
fn the_constructors_and_setters_set_their_own_fields() {
    assert_eq!(
        Material::blur(6.0),
        Material {
            effect: EffectName::BLUR,
            blur: 6.0,
            input: MaterialInput::IDENTITY,
            max_luminance: None,
            min_luminance: None,
            fallback: MaterialFallback::ThemeSurface,
        }
    );
    assert_eq!(
        Material::frosted(12.0),
        Material {
            effect: EffectName::FROSTED,
            blur: 12.0,
            input: MaterialInput {
                saturation: 1.8,
                noise: 0.04,
                lens: Lens::NONE,
            },
            ..Material::blur(0.0)
        }
    );
    let lens = Lens::WINDOW_GLASS;
    assert_eq!(
        Material::glass(lens),
        Material {
            effect: EffectName::GLASS,
            input: MaterialInput {
                lens,
                ..MaterialInput::IDENTITY
            },
            ..Material::blur(0.0)
        }
    );
    let set = Material::blur(0.0)
        .with_blur(2.0)
        .with_saturation(0.5)
        .with_noise(0.1)
        .with_max_luminance(0.2)
        .with_min_luminance(0.1)
        .with_lens(lens)
        .with_fallback(RgbaF32::WHITE);
    assert_eq!(
        set,
        Material {
            effect: EffectName::BLUR,
            blur: 2.0,
            input: MaterialInput {
                saturation: 0.5,
                noise: 0.1,
                lens,
            },
            max_luminance: Some(0.2),
            min_luminance: Some(0.1),
            fallback: MaterialFallback::Brush(Brush::Solid(RgbaF32::WHITE)),
        }
    );
    assert_eq!(
        Material::new(EffectName::GLASS, MaterialInput::IDENTITY),
        Material::glass(Lens::NONE)
    );
}

/// A rect and a chrome take the same checks: each case panics with its kind's rule.
#[test]
fn a_material_out_of_its_kinds_panics_where_it_enters() {
    let lens = Lens::WINDOW_GLASS;
    let base = Material::blur(1.0);
    for (material, rule) in [
        (Material::blur(-1.0), "a length must"),
        (base.clone().with_saturation(f32::NAN), "a factor must"),
        (base.clone().with_noise(-0.5), "a factor must"),
        (base.clone().with_max_luminance(-0.1), "a factor must"),
        (
            base.clone().with_min_luminance(f32::INFINITY),
            "a factor must",
        ),
        (
            base.with_fallback(RgbaF32::new(f32::NAN, 0.0, 0.0, 1.0)),
            "a color must",
        ),
        (
            Material::glass(Lens {
                thickness: f32::INFINITY,
                ..lens
            }),
            "a length must",
        ),
        (
            Material::glass(Lens {
                refractive_index: 0.9,
                ..lens
            }),
            "a refractive index must",
        ),
    ] {
        for entry in ["rect", "chrome"] {
            let panic = std::panic::catch_unwind(|| match entry {
                "rect" => _ = shape::internals::check(&Shape::rect(), material.clone()),
                _ => Chrome::NONE.with_material(material.clone()).validate(),
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
/// transparency runs whole, its effect resolved through the registry, where the stock blur is
/// effect 2, past the two ripples; and its fallback while transparency is reduced, with the
/// theme's surface at hand for one that names none.
#[test]
fn a_material_reads_its_fallback_while_transparency_is_reduced() {
    let effects = EffectRegistry::default();
    let theme = RgbaF32::new(0.1, 0.2, 0.3, 0.75);
    let resolve = |transparency| Resolve {
        effects: &effects,
        reductions: Reductions {
            transparency,
            ..Reductions::FULL
        },
        material_fallback: theme,
    };
    let blur_id = effects
        .resolve(EffectName::BLUR.typed(), Reductions::FULL)
        .expect("the stock blur paints")
        .id;
    assert_eq!(blur_id.0, 2);
    let blur = Material::blur(4.0);
    assert_eq!(
        MaterialRead::resolve(&blur, resolve(Reduction::Full)),
        Some(MaterialRead::Capture { effect: blur_id })
    );
    assert_eq!(
        MaterialRead::resolve(&blur, resolve(Reduction::Reduced)),
        Some(MaterialRead::Fallback { theme })
    );
    // A name nothing loaded panics in every mode.
    let gone = Material::new(EffectName::named("gone").unwrap(), MaterialInput::IDENTITY);
    assert_panics_with("no effect is loaded under `gone`", || {
        MaterialRead::resolve(&gone, resolve(Reduction::Reduced))
    });
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
/// and it coerces where it paints: a negative blur, floor, saturation, noise, bezel and thickness
/// paint as 0, an index under 1 as 1, and a dispersion and highlight clamp into `0..=1`, 2 as 1,
/// −1 as 0. The input staged for the effect holds the coerced values.
#[test]
fn a_material_coerces_into_its_kinds_where_it_paints() {
    let past = Material {
        effect: EffectName::GLASS,
        blur: -0.4,
        input: MaterialInput {
            saturation: -0.1,
            noise: -0.01,
            lens: Lens {
                bezel: -2.4,
                thickness: -1.6,
                refractive_index: 0.95,
                dispersion: 2.0,
                highlight: -1.0,
            },
        },
        max_luminance: None,
        min_luminance: Some(-0.04),
        fallback: MaterialFallback::ThemeSurface,
    };
    let mut store = RecordStore::default();
    let lowered = captured_in(&mut store, &past);
    assert_eq!(lowered.capture.sigma, 0.0);
    assert_eq!(lowered.min_luminance(), 0.0);
    let start = lowered.params.start as usize;
    assert_eq!(lowered.params.len, 7);
    let words: Vec<f32> = store.effect_params[start..start + 7]
        .iter()
        .map(|&w| f32::from_bits(w))
        .collect();
    assert_eq!(words, [0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0]);
}

/// The blur, the input, the fallback and an absent floor blend, the floor through 0, while the
/// effect and the ceiling switch at `t = 0.5`. Halfway from a floor of 0.4 to none is
/// `0.4 · 0.5 = 0.2`; halfway from frosted at σ 4 to blur at σ 2 is σ 3, saturation
/// `(1.8 + 1) / 2 = 1.4` and grain `0.04 / 2 = 0.02`, and from a black fallback to a white one
/// the grey two colors blend to, while the theme's fallback and a brush switch at `t = 0.5`. An absent material blends through the other
/// end at rest: a quarter of the way to the default glass is a bezel of `24 · ¼ = 6`, a thickness
/// of `16 · ¼ = 4`, an index of `1 + 0.5 · ¼ = 1.125`, a dispersion of `0.06 · ¼ = 0.015` and a
/// highlight of `0.6 · ¼ = 0.15`. The ends stay exact.
#[test]
fn a_material_blends_its_numbers_and_snaps_its_effect() {
    let plain = Material::blur(4.0);
    let floored = plain.clone().with_min_luminance(0.4);
    assert_eq!(
        Material::lerp(&floored, &plain, 0.5).min_luminance,
        Some(0.2)
    );
    assert_eq!(Material::lerp(&floored, &plain, 0.0), floored);
    assert_eq!(Material::lerp(&floored, &plain, 1.0), plain);
    let frosted = Material::frosted(4.0).with_fallback(RgbaF32::BLACK);
    let blur = Material::blur(2.0).with_fallback(RgbaF32::WHITE);
    let half = Material::lerp(&frosted, &blur, 0.5);
    assert_eq!(
        half.fallback,
        MaterialFallback::Brush(Brush::Solid(RgbaF32::lerp(
            &RgbaF32::BLACK,
            &RgbaF32::WHITE,
            0.5
        )))
    );
    let themed = Material::blur(2.0);
    assert_eq!(
        Material::lerp(&themed, &blur, 0.49).fallback,
        MaterialFallback::ThemeSurface
    );
    assert_eq!(Material::lerp(&themed, &blur, 0.5).fallback, blur.fallback);
    assert_eq!(half.effect, EffectName::BLUR);
    assert_eq!(
        Material::lerp(&frosted, &blur, 0.49).effect,
        EffectName::FROSTED
    );
    assert_eq!(half.blur, 3.0);
    assert_eq!(half.input.saturation, 1.4);
    assert_eq!(half.input.noise, 0.02);
    let glass = Material::glass(Lens::WINDOW_GLASS);
    assert_eq!(
        Option::<Material>::lerp(&None, &Some(glass.clone()), 0.25).map(|m| m.input.lens),
        Some(Lens {
            bezel: 6.0,
            thickness: 4.0,
            refractive_index: 1.125,
            dispersion: 0.015,
            highlight: 0.15,
        })
    );
    assert_eq!(
        Option::<Material>::lerp(&None, &Some(glass.clone()), 1.0),
        Some(glass)
    );
    assert_eq!(Option::<Material>::lerp(&None, &None, 0.5), None);
    let ceiling = plain.clone().with_max_luminance(0.5);
    assert_eq!(
        Material::lerp(&ceiling, &plain, 0.49).max_luminance,
        Some(0.5)
    );
    assert_eq!(Material::lerp(&ceiling, &plain, 0.5).max_luminance, None);
}

/// `with_material` sets the material alone, and a chrome whose only paint is a material is not a
/// no-op: it makes a backdrop.
#[test]
fn a_chrome_with_only_a_material_paints() {
    let base = Chrome::new(RgbaF32::WHITE);
    let material = Material::frosted(8.0);
    let with = base.clone().with_material(material.clone());
    assert_eq!(
        with,
        Chrome {
            paint: Paint {
                material: Some(material.clone()),
                ..base.paint
            },
            ..base
        }
    );
    assert!(!Chrome::NONE.with_material(material).is_noop());
}

/// The ceiling holds the muted ink at WCAG's 4.5:1 over the brightest backdrop as an 8-bit target
/// stores it. `#a4a4a4` is `Y = 0.371238`, so the brightest ground is
/// `(0.371238 + 0.05) / 4.5 − 0.05 = 0.043608`, which encodes to byte 58.91, so byte 58 is the
/// highest at or under it, and the composite may reach byte 57, `0.04091520`. Under a `#383838`
/// fill at 0.72, `Y = 0.039546`, the ceiling is `(0.04091520 − 0.72 · 0.039546) / 0.28 =
/// 0.04443538`, and under `#1f1f1f`, `Y = 0.013702`, `(0.04091520 − 0.72 · 0.013702) / 0.28 =
/// 0.11089178`. A ground at either ceiling stores byte 57. The setter sets the ceiling alone.
#[test]
fn a_ceiling_for_an_ink_holds_it_at_the_text_contrast() {
    let ink = RgbaF32::hex(0xa4a4a4);
    let level = |byte: u8| RgbaF32::from_srgba8(SrgbaU8::rgb(byte, byte, byte));
    assert!(ink.contrast_ratio(level(58)) >= 4.5);
    assert!(
        ink.contrast_ratio(level(59)) < 4.5,
        "byte 58 is the highest"
    );
    for (fill, want) in [(0x383838, 0.04443538), (0x1f1f1f, 0.11089178)] {
        let fill = RgbaF32::hex(fill).with_alpha(0.72);
        let material = Material::frosted(20.0).with_max_luminance_for(ink, fill);
        let ceiling = material.max_luminance.expect("a ceiling");
        assert_eq!(
            material,
            Material::frosted(20.0).with_max_luminance(ceiling)
        );
        // The hand value's eighth digit, and the derivation's f32 rounding, a few ulps of 1e-8.
        assert!((ceiling - want).abs() < 1e-7, "{ceiling}");
        let ground = 0.28 * ceiling + 0.72 * fill.luminance();
        assert!((ground - level(57).r).abs() < 2e-8, "{ground}");
    }
}

/// The floor holds the dark ink at WCAG's 4.5:1 over the darkest backdrop as an 8-bit target
/// stores it. `#3d4248` is `Y = 0.053564`, so the darkest ground is
/// `4.5 · (0.053564 + 0.05) − 0.05 = 0.416038`; byte 173 is the lowest at or over it, and the
/// composite must reach byte 174, `0.42326767`. Under a `#e4e6e8` fill, `Y = 0.78913804`, at 0.3
/// the floor is `(0.42326767 − 0.3 · 0.78913804) / 0.7 = 0.26646608`; at 0.72 the fill alone
/// clears it, so the floor is 0. A ground at the floor stores byte 174. A fill of no alpha covers
/// nothing, whatever its color, so the floor is the ground itself, 0.42326767.
#[test]
fn a_floor_for_an_ink_holds_it_at_the_text_contrast() {
    let ink = RgbaF32::hex(0x3d4248);
    let level = |byte: u8| RgbaF32::from_srgba8(SrgbaU8::rgb(byte, byte, byte));
    assert!(ink.contrast_ratio(level(173)) >= 4.5);
    assert!(
        ink.contrast_ratio(level(172)) < 4.5,
        "byte 173 is the lowest"
    );
    let fill = RgbaF32::hex(0xe4e6e8).with_alpha(0.3);
    let material = Material::frosted(20.0).with_min_luminance_for(ink, fill);
    let floor = material.min_luminance.expect("a floor");
    assert_eq!(material, Material::frosted(20.0).with_min_luminance(floor));
    assert!((floor - 0.266_466_08).abs() < 1e-7, "{floor}");
    let ground = 0.7 * floor + 0.3 * fill.luminance();
    assert!((ground - level(174).r).abs() < 2e-8, "{ground}");
    let thick = RgbaF32::hex(0xe4e6e8).with_alpha(0.72);
    let clear = Material::frosted(20.0).with_min_luminance_for(ink, thick);
    assert_eq!(clear.min_luminance, Some(0.0));
    let bare = Material::frosted(20.0).with_min_luminance_for(ink, RgbaF32::TRANSPARENT);
    let floor = bare.min_luminance.expect("a floor");
    assert!((floor - level(174).r).abs() < 2e-8, "{floor}");
}

/// Every refusal of a bound comes back as its case, none as a panic. A floor for `#808080`
/// (`Y ≈ 0.2159`) needs a ground of `4.5 · 0.2659 − 0.05 ≈ 1.147`, past white, so no fill holds
/// it; one for black over a black fill needs about 0.175, which the fill's 0 misses. A ceiling for
/// `#202020` (`Y ≈ 0.0144`) needs `0.0644 / 4.5 − 0.05 < 0`, where no level lies; one for white
/// needs about 0.183, which a white fill's 1 exceeds.
#[test]
fn each_refused_bound_returns_its_case() {
    use crate::primitives::paint::material::InkBound;
    let material = Material::frosted(20.0);
    let half = |hex: u32| RgbaF32::hex(hex).with_alpha(0.5);
    let floors = [
        (0x808080, half(0xffffff), InkBound::InkFailsOnFill),
        (0x000000, half(0x000000), InkBound::InkFailsOnFill),
        (
            0x000000,
            RgbaF32::hex(0xffffff),
            InkBound::FillNotTranslucent { alpha: 1.0 },
        ),
    ];
    for (ink, fill, want) in floors {
        assert_eq!(
            material.min_luminance_for(RgbaF32::hex(ink), fill),
            Err(want),
            "floor for {ink:06x}"
        );
    }
    assert_eq!(
        material.max_luminance_for(RgbaF32::hex(0xffffff), half(0xffffff)),
        Err(InkBound::InkFailsOnFill),
        "ceiling for white over white"
    );
    assert!(
        matches!(
            material.max_luminance_for(RgbaF32::hex(0x202020), half(0x000000)),
            Err(InkBound::InkTooDark { .. })
        ),
        "ceiling for a dark grey"
    );
}

/// A fallback's alpha holds the ink over the worst page. Light `#a4a4a4` ink grounds at byte 57,
/// `0.04091520` (above); over a white page the `#383838` surface at `a` shows
/// `a · 0.0395462 + (1 − a)`, so `a ≥ (1 − 0.0409152) / (1 − 0.0395462) = 0.9985747`, 254.64
/// steps, so 255/255. Dark `#3d4248` ink grounds at byte 174, `0.42326767`; over a black page the
/// `#e4e6e8` surface shows `a · 0.7891380`, so `a ≥ 0.5363671`, 136.77 steps, so 137/255. Where
/// no alpha holds the ink, the surface fails it even opaque, the fallback is opaque: white ink on
/// a white surface, and dark ink on a black one.
#[test]
fn a_fallback_takes_the_least_alpha_that_holds_its_ink() {
    use crate::primitives::paint::material::fallback_alpha;
    assert_eq!(
        fallback_alpha(RgbaF32::hex(0xa4a4a4), RgbaF32::hex(0x383838)),
        1.0
    );
    assert_eq!(
        fallback_alpha(RgbaF32::hex(0x3d4248), RgbaF32::hex(0xe4e6e8)),
        137.0 / 255.0
    );
    assert_eq!(fallback_alpha(RgbaF32::WHITE, RgbaF32::WHITE), 1.0);
    assert_eq!(fallback_alpha(RgbaF32::BLACK, RgbaF32::BLACK), 1.0);
}

/// An alpha rounds up to the 8-bit grid, and up again where that step's f16 reads lower: 86/255
/// is 0.3372549, whose f16 is 0.3371582, so 0.3372 takes 87/255 and 0.3371 takes 86/255. 0 and 1
/// stay.
#[test]
fn a_fallback_alpha_rounds_up_to_the_grid_as_f16_reads_it() {
    use crate::primitives::paint::material::grid_alpha_at_or_above;
    assert_eq!(grid_alpha_at_or_above(0.3372), 87.0 / 255.0);
    assert_eq!(grid_alpha_at_or_above(0.3371), 86.0 / 255.0);
    assert_eq!(grid_alpha_at_or_above(0.0), 0.0);
    assert_eq!(grid_alpha_at_or_above(1.0), 1.0);
}

/// IEC 61966-2-1 in `f64`, written out here as a reference apart from palantir's own.
fn srgb_encode(c: f64) -> f64 {
    if c <= 0.003_130_8 {
        12.92 * c
    } else {
        1.055 * c.libm_powf(1.0 / 2.4) - 0.055
    }
}

fn srgb_decode(e: f64) -> f64 {
    let e = e.clamp(0.0, 1.0);
    if e <= 0.040_45 {
        e / 12.92
    } else {
        ((e + 0.055) / 1.055).libm_powf(2.4)
    }
}

/// A saturated fill, whose zero channels sit in the sRGB curve's linear toe, takes a ceiling and
/// a floor, and each holds the ink at 4.5:1: a backdrop of each primary, secondary and white,
/// scaled to the bound, mixed under the fill and stored a whole encoded step off on every
/// channel toward the ink, as the dither and the rounding may move it, still stores a ground the
/// ink reads 4.5:1 on.
#[test]
fn a_saturated_fill_holds_the_ink_at_the_text_contrast() {
    const WEIGHTS: [f64; 3] = [0.2126, 0.7152, 0.0722];
    const HUES: [[f64; 3]; 7] = [
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.0, 1.0, 1.0],
        [1.0, 0.0, 1.0],
        [1.0, 1.0, 0.0],
        [1.0, 1.0, 1.0],
    ];
    // The bends, against the tangency the hull of each curve finds when sampled: the ceiling's at
    // 0.012923650, the floor's at 0.013640802, the floor's under the 0.175 a floor's ground is.
    assert!(
        (ceiling_bend() - 0.012_923_65).abs() < 1e-7,
        "{}",
        ceiling_bend()
    );
    let floor_bend = bend_root().libm_powf(2.4);
    assert!((floor_bend - 0.013_640_8).abs() < 1e-7, "{floor_bend}");
    assert!(floor_bend < 4.5 * 0.05 - 0.05);
    let channels = |c: RgbaF32| [f64::from(c.r), f64::from(c.g), f64::from(c.b)];
    let luminance = |c: [f64; 3]| c.iter().zip(WEIGHTS).map(|(c, w)| c * w).sum::<f64>();
    // Each hue at `bound`, where its brightest channel can reach it.
    let backdrops = |bound: f64| {
        HUES.into_iter().filter_map(move |hue| {
            let scale = bound / luminance(hue);
            (scale <= 1.0).then(|| hue.map(|c| c * scale))
        })
    };
    let stored = |backdrop: [f64; 3], fill: RgbaF32, step: f64| {
        let alpha = f64::from(fill.a);
        let mixed: [f64; 3] =
            std::array::from_fn(|i| (1.0 - alpha) * backdrop[i] + alpha * channels(fill)[i]);
        luminance(mixed.map(|c| srgb_decode(srgb_encode(c) + step)))
    };

    let light = RgbaF32::hex(0xebebeb);
    let light_y = f64::from(light.luminance());
    for fill in [0x800000, 0x005000, 0x000080] {
        let fill = RgbaF32::hex(fill).with_alpha(0.6);
        let ceiling = Material::frosted(20.0)
            .with_max_luminance_for(light, fill)
            .max_luminance
            .expect("a ceiling");
        let mut seen = 0;
        for backdrop in backdrops(f64::from(ceiling)) {
            let ground = stored(backdrop, fill, 1.0 / 255.0);
            assert!(
                (light_y + 0.05) / (ground + 0.05) >= 4.5,
                "{fill:?} {backdrop:?}"
            );
            seen += 1;
        }
        assert!(seen > 0, "{fill:?}: some hue reaches the ceiling");
    }

    let dark = RgbaF32::hex(0x3d4248);
    let dark_y = f64::from(dark.luminance());
    for fill in [0xffff00, 0x00ffff, 0x00ff00] {
        let fill = RgbaF32::hex(fill).with_alpha(0.3);
        let floor = Material::frosted(20.0)
            .with_min_luminance_for(dark, fill)
            .min_luminance
            .expect("a floor");
        let mut seen = 0;
        for backdrop in backdrops(f64::from(floor)) {
            let ground = stored(backdrop, fill, -1.0 / 255.0);
            assert!(
                (ground + 0.05) / (dark_y + 0.05) >= 4.5,
                "{fill:?} {backdrop:?}"
            );
            seen += 1;
        }
        assert!(seen > 0, "{fill:?}: some hue reaches the floor");
    }
}

/// An ink too dark for its ground to store where the bound is exact takes no ceiling: `#868686`,
/// the darkest grey that does, takes one; `#858585`, a step under it, panics. So does every ink
/// whose ground lies under black's first step, where no level stores a step under it: `#747474`,
/// `Y = 0.174647`, the lightest grey under `Y = 0.175`, grounds at
/// `(0.174647 + 0.05) / 4.5 − 0.05 = −0.000078`, and `#000000` at `0.05 / 4.5 − 0.05 = −0.0389`.
#[test]
fn a_ceiling_refuses_an_ink_too_dark_for_an_exact_bound() {
    let fill = RgbaF32::hex(0x000000).with_alpha(0.5);
    let ceiling = |ink| Material::frosted(20.0).with_max_luminance_for(RgbaF32::hex(ink), fill);
    assert!(ceiling(0x868686).max_luminance.is_some());
    assert!(RgbaF32::hex(0x747474).luminance() < 0.175);
    assert!(RgbaF32::hex(0x757575).luminance() > 0.175);
    for ink in [0x858585, 0x747474, 0x000000] {
        let panic = std::panic::catch_unwind(|| ceiling(ink)).expect_err("too dark");
        let message = panic.downcast_ref::<String>().map_or("", String::as_str);
        assert!(
            message.starts_with("the ink is too dark for a ceiling"),
            "#{ink:06x}: {message}"
        );
    }
}
