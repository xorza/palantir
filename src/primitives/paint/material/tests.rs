use crate::internals::srgb_reference;
use crate::primitives::effect::effect_kind::BackdropEffect;
use crate::primitives::effect::effect_name::EffectName;
use crate::primitives::math::lerp::Lerp;
use crate::primitives::math::libm_ext::Libm;
use crate::primitives::paint::border::Border;
use crate::primitives::paint::brush::Brush;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::srgba_u8::SrgbaU8;
use crate::primitives::paint::material::Material;
use crate::primitives::paint::material::backdrop_fallback::BackdropFallback;
use crate::primitives::paint::material::frosted_params::FrostedParams;
use crate::primitives::paint::material::glass_params::GlassParams;
use crate::primitives::paint::material::ink_bound::{bend_root, ceiling_bend};
use crate::primitives::paint::material::lens::Lens;
use crate::primitives::paint::material::material_effect::MaterialEffect;
use crate::primitives::paint::material::material_paint::MaterialPaint;
use crate::primitives::paint::paint::Paint;

/// The constructors name their stock effects, lay out each effect's own parameters in order, and
/// set exactly their own fields, the theme's fallback included; each `with_*` sets its own,
/// `with_fallback` a brush of the author's in its place.
#[test]
fn the_constructors_and_setters_set_their_own_fields() {
    assert_eq!(
        Material::blur(6.0),
        Material {
            effect: MaterialEffect::new(
                EffectName::FROSTED,
                FrostedParams {
                    saturation: 1.0,
                    noise: 0.0,
                },
            ),
            backdrop_blur: 6.0,
            max_luminance: None,
            min_luminance: None,
            fallback: BackdropFallback::ThemeSurface,
        }
    );
    let frosted = Material::frosted(12.0);
    assert_eq!(frosted.effect.name(), "palantir.frosted");
    assert_eq!(frosted.effect.params(), [1.8, 0.04]);
    assert_eq!(frosted.backdrop_blur, 12.0);
    let glass = Material::glass(Lens::WINDOW_GLASS);
    assert_eq!(glass.effect.name(), "palantir.glass");
    assert_eq!(
        glass.effect.params(),
        [1.0, 0.0, 24.0, 16.0, 1.5, 0.06, 0.6],
        "saturation, grain, then the lens flat"
    );
    assert_eq!(
        glass,
        Material::new(EffectName::GLASS, GlassParams::new(Lens::WINDOW_GLASS))
    );
    assert_eq!(
        bytemuck::pod_read_unaligned::<GlassParams>(glass.effect.bytes()),
        GlassParams::new(Lens::WINDOW_GLASS)
    );
    let set = Material::blur(0.0)
        .with_backdrop_blur(2.0)
        .with_max_luminance(0.2)
        .with_min_luminance(0.1)
        .with_fallback(RgbaF32::WHITE);
    assert_eq!(
        set,
        Material {
            backdrop_blur: 2.0,
            max_luminance: Some(0.2),
            min_luminance: Some(0.1),
            fallback: BackdropFallback::Brush(Brush::Solid(RgbaF32::WHITE)),
            ..Material::blur(0.0)
        }
    );
}

/// The blur, the fallback, an absent floor and the parameters of one effect blend, the floor
/// through 0, the parameters lane by lane, while the effect and the ceiling switch at `t = 0.5`,
/// the parameters with the effect where the ends name two. Halfway from a floor of 0.4 to none is
/// `0.4 · 0.5 = 0.2`; halfway from the frost at saturation 1.8 and grain 0.04 to a plain blur, the
/// frost at 1.0 and 0, is `(1.4, 0.02)`; from frosted at σ 4 to glass at σ 2 is σ 3, the glass
/// from `t = 0.5` with its own lanes, and from a black fallback to a white one the grey two colors
/// blend to, while the theme's fallback and a brush switch at `t = 0.5`. An absent material blends through the other end at rest, its parameters kept: a
/// quarter of the way to the frost at σ 8 is σ 2, at the frost's own lanes. The ends stay exact.
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
    assert_eq!(
        Material::lerp(&Material::frosted(0.0), &Material::blur(0.0), 0.5)
            .effect
            .params(),
        [1.4, 0.02]
    );
    let frosted = Material::frosted(4.0).with_fallback(RgbaF32::BLACK);
    let glass = Material::glass(Lens::WINDOW_GLASS)
        .with_backdrop_blur(2.0)
        .with_fallback(RgbaF32::WHITE);
    let half = Material::lerp(&frosted, &glass, 0.5);
    assert_eq!(
        half.fallback,
        BackdropFallback::Brush(Brush::Solid(RgbaF32::lerp(
            &RgbaF32::BLACK,
            &RgbaF32::WHITE,
            0.5
        )))
    );
    let themed = Material::blur(2.0);
    assert_eq!(
        Material::lerp(&themed, &glass, 0.49).fallback,
        BackdropFallback::ThemeSurface
    );
    assert_eq!(
        Material::lerp(&themed, &glass, 0.5).fallback,
        glass.fallback
    );
    assert_eq!(half.effect, glass.effect, "the glass, with its parameters");
    assert_eq!(
        Material::lerp(&frosted, &glass, 0.49).effect,
        frosted.effect
    );
    assert_eq!(half.backdrop_blur, 3.0);
    let appearing =
        Option::<Material>::lerp(&None, &Some(Material::frosted(8.0)), 0.25).expect("a material");
    assert_eq!(appearing.backdrop_blur, 2.0);
    assert_eq!(appearing.effect.params(), [1.8, 0.04]);
    assert_eq!(
        Option::<Material>::lerp(&None, &Some(frosted.clone()), 1.0),
        Some(frosted)
    );
    assert_eq!(Option::<Material>::lerp(&None, &None, 0.5), None);
    let ceiling = plain.clone().with_max_luminance(0.5);
    assert_eq!(
        Material::lerp(&ceiling, &plain, 0.49).max_luminance,
        Some(0.5)
    );
    assert_eq!(Material::lerp(&ceiling, &plain, 0.5).max_luminance, None);
}

/// A material's parameters are f32 lanes that span their struct: a scalar and a vec3 are 4 lanes,
/// the glass's 7 and none 0; an integer, a struct past 8 lanes, and one whose bytes its fields
/// leave unlisted are no material's.
#[test]
fn a_materials_parameters_are_listed_f32_lanes() {
    use crate::primitives::effect::effect_params::{EffectParam, EffectParams};
    #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable, palantir_derive::EffectParams)]
    #[repr(C)]
    struct Mixed {
        a: f32,
        b: [f32; 3],
    }
    #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable, palantir_derive::EffectParams)]
    #[repr(C)]
    struct Count {
        n: u32,
    }
    #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable, palantir_derive::EffectParams)]
    #[repr(C)]
    struct Wide {
        a: [f32; 4],
        b: [f32; 4],
        c: f32,
    }
    #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
    #[repr(C)]
    struct Unlisted {
        data: [f32; 2],
    }
    impl EffectParams for Unlisted {
        const FIELDS: &'static [EffectParam] = &[];
    }
    assert_eq!(MaterialEffect::lanes_for::<Mixed>(), Some(4));
    assert_eq!(MaterialEffect::lanes_for::<GlassParams>(), Some(7));
    // The lens's fields read flat after the two of the glass's own, a word apart, as the stock
    // glass's WGSL `Params` declares them.
    let glass: Vec<(&str, usize)> = GlassParams::FIELDS
        .iter()
        .map(|field| (field.name, field.offset))
        .collect();
    assert_eq!(
        glass,
        [
            ("saturation", 0),
            ("noise", 4),
            ("bezel", 8),
            ("thickness", 12),
            ("refractive_index", 16),
            ("dispersion", 20),
            ("highlight", 24),
        ]
    );
    assert_eq!(MaterialEffect::lanes_for::<()>(), Some(0));
    assert_eq!(MaterialEffect::lanes_for::<Count>(), None);
    assert_eq!(MaterialEffect::lanes_for::<Wide>(), None);
    assert_eq!(MaterialEffect::lanes_for::<Unlisted>(), None);
    assert_eq!(
        MaterialEffect::new(
            EffectName::<Mixed, BackdropEffect>::named("mixed").unwrap(),
            Mixed {
                a: 1.0,
                b: [2.0, 3.0, 4.0]
            }
        )
        .params(),
        [1.0, 2.0, 3.0, 4.0]
    );
}

/// A material springs its blur and each parameter lane, the rest taken from the target: from a
/// frost at σ 4 of saturation 1.5 and grain 0.25 to one at σ 2 of 1 and none is a step of σ 2 and
/// lanes `(0.5, 0.25)`, whose squared magnitude is `4 + 0.25 + 0.0625 = 4.3125`; half of it is σ 1
/// and `(0.25, 0.125)`, and the target plus the step lands on the first frost again.
#[test]
fn a_material_springs_its_blur_and_lanes() {
    use crate::primitives::math::animatable::Animatable;
    let frost = |sigma, saturation, noise| {
        Some(
            Material::new(EffectName::FROSTED, FrostedParams { saturation, noise })
                .with_backdrop_blur(sigma),
        )
    };
    let from = frost(4.0, 1.5, 0.25);
    let to = frost(2.0, 1.0, 0.0);
    let step = from.sub(&to);
    let lanes = |m: &Option<Material>| {
        let m = m.as_ref().expect("a material");
        (m.backdrop_blur, m.effect.params().to_vec())
    };
    assert_eq!(lanes(&step), (2.0, vec![0.5, 0.25]));
    assert_eq!(step.magnitude_squared(), 4.3125);
    assert_eq!(lanes(&step.scale(0.5)), (1.0, vec![0.25, 0.125]));
    assert_eq!(to.add(&step), from);
}

/// A spring retargeted from a frost onto a glass snaps the effect, as a tween does, and springs
/// the blur on: the value and the velocity take the glass's effect, the velocity's lanes from
/// rest, so the spring's offset holds no frost lanes and the target plus it paints the glass's
/// own parameters. The blur's offset, `4 − 2 = 2`, and its velocity stay.
#[test]
fn a_spring_onto_another_effect_snaps_the_effect() {
    use crate::primitives::math::animatable::Animatable;
    let frost = Some(Material::frosted(4.0));
    let glass = Some(Material::glass(Lens::WINDOW_GLASS).with_backdrop_blur(2.0));
    let mut value = frost.clone();
    let mut velocity = frost.scale(0.5);
    value.normalize_for_spring(&glass, &mut velocity);

    let (value, velocity, target) = (
        value.expect("a value"),
        velocity.expect("a velocity"),
        glass.clone().expect("a target"),
    );
    assert_eq!(value.effect, target.effect);
    assert!(velocity.effect.blends_with(&target.effect));
    assert!(velocity.effect.params().iter().all(|&lane| lane == 0.0));
    assert_eq!(velocity.backdrop_blur, 2.0, "the blur's velocity, 4 × 0.5");

    let offset = Some(value).sub(&glass);
    let landed = glass.add(&offset).expect("a material");
    assert_eq!(landed.effect, target.effect);
    assert_eq!(landed.backdrop_blur, 4.0);

    // Over one effect nothing snaps: a frost onto another frost keeps its lanes.
    let mut value = frost.clone();
    let mut velocity = frost.scale(0.5);
    let other_frost = Some(Material::frosted(1.0));
    value.normalize_for_spring(&other_frost, &mut velocity);
    assert_eq!(value, frost);
    assert_eq!(velocity, frost.scale(0.5));
}

/// `Paint::with_material` keeps the paint whole over the material, and a material alone paints
/// as one over [`Paint::NONE`].
#[test]
fn a_paint_with_a_material_keeps_both() {
    let base = Paint::from(RgbaF32::WHITE).with_border(Border::new(RgbaF32::BLACK, 2.0));
    let material = Material::frosted(8.0);
    assert_eq!(
        base.clone().with_material(material.clone()),
        MaterialPaint {
            paint: base,
            material: material.clone(),
        }
    );
    assert_eq!(
        MaterialPaint::from(material.clone()),
        MaterialPaint {
            paint: Paint::NONE,
            material,
        }
    );
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
    use crate::primitives::paint::material::ink_bound::InkBound;
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
    use crate::primitives::paint::material::ink_bound::fallback_alpha;
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
    use crate::primitives::paint::material::ink_bound::grid_alpha_at_or_above;
    assert_eq!(grid_alpha_at_or_above(0.3372), 87.0 / 255.0);
    assert_eq!(grid_alpha_at_or_above(0.3371), 86.0 / 255.0);
    assert_eq!(grid_alpha_at_or_above(0.0), 0.0);
    assert_eq!(grid_alpha_at_or_above(1.0), 1.0);
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
        luminance(mixed.map(|c| srgb_reference::decode(srgb_reference::encode(c) + step)))
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
