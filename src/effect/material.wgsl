// Palantir's material path, for a backdrop effect drawn as a `Material`'s box: what the effect
// shows over what it reads, held within the material's luminance bounds, under the box's fill,
// dithered where an 8-bit sRGB target stores it, over the whole box with the box's border laid
// over it, as CSS's background and `backdrop-filter` cover the border box; over the rounded rect
// a quad of the box would shade, turned as one. The backdrop's slot holds the bounds,
// the fill and the border, faded by a paint animation.

// Whether the target stores 8 bits a channel through the sRGB curve, which a material dithers its
// smooth result for.
override PALANTIR_DITHER_SRGB8: bool = false;

// The solid fill's tag (`FillKind`), the one that reads no gradient.
const PALANTIR_FILL_SOLID: u32 = /*{FILL_SOLID}*/;

// The material's box at `local`, around `out`, what the effect shows there.
fn palantir_material(v: PalantirVarying, local: vec2<f32>, input: EffectInput, out: EffectOutput) -> PalantirOut {
    let backdrop = palantir_backdrops[v.backdrop_idx];
    var c = out.color.rgb;
    if (any(out.under.rgb != vec3<f32>(0.0))) {
        c += out.under.rgb * effect_backdrop(input.pos).rgb;
    }
    // Scaled rather than clamped per channel, so the hue and the chroma hold.
    let luminance = dot(c, PALANTIR_LUMA);
    if (luminance > backdrop.max_luminance) {
        c *= backdrop.max_luminance / luminance;
    }
    // Lifted by a neutral, whose luminance is its level as the weights sum to 1, so black reaches
    // the floor too; after the ceiling, so the floor wins where the two cross.
    let lifted = dot(c, PALANTIR_LUMA);
    if (lifted < backdrop.min_luminance) {
        c += vec3<f32>(backdrop.min_luminance - lifted);
    }
    // A paint animation's fade scaled the opacity and the fill's alpha alike, so the fill's own
    // share is its alpha over the opacity.
    let opacity = v.clock.y;
    var fill = backdrop.fill;
    if ((backdrop.fill_kind & PALANTIR_FILL_TAG_MASK) != PALANTIR_FILL_SOLID) {
        let local01 = local / max(v.rect.zw, vec2<f32>(PALANTIR_ZERO_EPS));
        fill = palantir_gradient_fill(
            local01,
            backdrop.fill_kind,
            backdrop.fill_axis,
            backdrop.fill_lut_row,
            backdrop.fill,
        );
    }
    let share = select(0.0, fill.a / opacity, opacity > 0.0);
    c = mix(c, fill.rgb, clamp(share, 0.0, 1.0));
    if (PALANTIR_DITHER_SRGB8) {
        // Interleaved gradient noise (Jimenez) of one encoded step, so a smooth blur stores no
        // band.
        let p = palantir_frag_px(v.clip);
        let ign = fract(52.9829189 * fract(dot(floor(p), vec2<f32>(0.06711056, 0.00583715))));
        c = palantir_srgb_to_linear(max(palantir_linear_to_srgb(c) + (ign - 0.5) / 255.0, vec3<f32>(0.0)));
    }
    // The box's shape under the slot's border, as a quad's `fs_template` shades it; premultiplied,
    // so the clip scales the shade and leaves what is under as over composites it.
    let shaded = palantir_template_fill(
        v.region,
        v.geo,
        v.ring_r,
        vec4<f32>(c, opacity),
        backdrop.border_color,
        backdrop.border_width,
    ) * palantir_clip_coverage(palantir_frag_px(v.clip), v.chain);
    return PalantirOut(shaded, vec4<f32>(1.0 - shaded.a));
}
