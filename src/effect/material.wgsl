// Palantir's material path, for a backdrop effect drawn as a `Material`'s box: what the effect
// shows over what it reads, held within the material's luminance bounds, under the box's fill,
// dithered where an 8-bit sRGB target stores it, over the whole box with the box's border laid
// over it, as CSS's background and `backdrop-filter` cover the border box; over the rounded rect
// a quad of the box would shade, turned and outside as one. The backdrop's slot holds the bounds,
// the fill and the border, faded by a paint animation.

// Whether the target stores 8 bits a channel through the sRGB curve, which a material dithers its
// smooth result for.
override PALANTIR_DITHER_SRGB8: bool = false;

// Rec. 709 luminance weights, over linear light.
const PALANTIR_LUMA: vec3<f32> = vec3<f32>(0.2126, 0.7152, 0.0722);

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
        let p = v.clip.xy + palantir_imm.frag_origin;
        let ign = fract(52.9829189 * fract(dot(floor(p), vec2<f32>(0.06711056, 0.00583715))));
        c = palantir_srgb_to_linear(max(palantir_linear_to_srgb(c) + (ign - 0.5) / 255.0, vec3<f32>(0.0)));
    }
    let shaded = palantir_material_shade(v, local, backdrop, vec4<f32>(c, opacity));
    return PalantirOut(shaded, vec4<f32>(1.0 - shaded.a));
}

// The shape of `fill` under the slot's border, as a quad's `fs_template` shades it: the template's
// interior, past the border and the antialiasing reach, is the fill without the distance.
fn palantir_material_shade(v: PalantirVarying, local: vec2<f32>, backdrop: PalantirBackdrop, fill: vec4<f32>) -> vec4<f32> {
    if (v.region == PALANTIR_REGION_INTERIOR) {
        return vec4<f32>(fill.rgb * fill.a, fill.a);
    }
    let size = v.rect.zw;
    let d = palantir_template_distance(v.region, v.geo, v.ring_r);
    if ((v.flags & PALANTIR_OUTSIDE) != 0u) {
        var cover = palantir_composite_outside(
            d,
            fill,
            backdrop.border_color,
            backdrop.border_width,
        );
        if (v.clock.w != 0.0) {
            // Turned, the rect edge no longer lies on content of its own extent, so it is a
            // visible edge and fades as any edge does.
            let e = abs(local - size * 0.5) - size * 0.5;
            cover *= palantir_edge_coverage(e.x) * palantir_edge_coverage(e.y);
        }
        return cover;
    }
    return palantir_composite(d, fill, backdrop.border_color, backdrop.border_width);
}

// The fill over the whole box, its border band included, and the border over it there: CSS's
// border-box background, which shows no sharp content through a translucent border.
fn palantir_composite(d: f32, fill: vec4<f32>, border_color: vec4<f32>, border_width: f32) -> vec4<f32> {
    let outer_aa = palantir_edge_coverage(d);
    let fill_a = fill.a * outer_aa;
    let under = vec4<f32>(fill.rgb * fill_a, fill_a);
    if (border_width > 0.0) {
        let inner_aa = palantir_edge_coverage(d + border_width);
        let border_a = (outer_aa - inner_aa) * border_color.a;
        return vec4<f32>(border_color.rgb * border_a, border_a) + under * (1.0 - border_a);
    }
    return under;
}

// The outside twin of `palantir_composite`: the fill outside the border's inner edge, the border
// over it in its band, and a transparent interior.
fn palantir_composite_outside(d: f32, fill: vec4<f32>, border_color: vec4<f32>, border_width: f32) -> vec4<f32> {
    let outer_aa = palantir_edge_coverage(d);
    if (border_width > 0.0) {
        let inner_aa = palantir_edge_coverage(d + border_width);
        let fill_a = (1.0 - inner_aa) * fill.a;
        let border_a = (outer_aa - inner_aa) * border_color.a;
        return vec4<f32>(border_color.rgb * border_a, border_a) + vec4<f32>(fill.rgb * fill_a, fill_a) * (1.0 - border_a);
    }
    let fill_a = (1.0 - outer_aa) * fill.a;
    return vec4<f32>(fill.rgb * fill_a, fill_a);
}
