// Palantir's material path, for a backdrop effect drawn as a `Material`'s box: what the effect
// shows over what it reads, held within the material's luminance bounds, under the box's fill,
// dithered where an 8-bit sRGB target stores it, over the whole box with the box's border laid
// over it, as CSS's background and `backdrop-filter` cover the border box; over the rounded rect
// a quad of the box would shade, turned and windowed as one. The pane's slot holds the bounds,
// the fill and the border, faded by a paint animation.

// Whether the target stores 8 bits a channel through the sRGB curve, which a material dithers its
// smooth result for.
override PALANTIR_DITHER_SRGB8: bool = false;

// Rec. 709 luminance weights, over linear light.
const PALANTIR_LUMA: vec3<f32> = vec3<f32>(0.2126, 0.7152, 0.0722);

// `fill_kind`'s fields and tags, as the quad shader reads them.
const PALANTIR_FILL_TAG_MASK: u32 = /*{FILL_TAG_MASK}*/;
const PALANTIR_SPREAD_SHIFT: u32 = /*{SPREAD_SHIFT}*/;
const PALANTIR_SPREAD_MASK: u32 = /*{SPREAD_MASK}*/;
const PALANTIR_FILL_SOLID: u32 = /*{FILL_SOLID}*/;
const PALANTIR_FILL_LINEAR: u32 = /*{FILL_LINEAR}*/;
const PALANTIR_FILL_RADIAL: u32 = /*{FILL_RADIAL}*/;
const PALANTIR_SPREAD_REPEAT: u32 = /*{SPREAD_REPEAT}*/;
const PALANTIR_SPREAD_REFLECT: u32 = /*{SPREAD_REFLECT}*/;
const PALANTIR_ZERO_EPS: f32 = 1e-6;
const PALANTIR_TAU: f32 = 6.2831853;

// The gradient LUT atlas the quads read, so a gradient fill shades in this stage.
@group(3) @binding(0) var palantir_gradient_tex: texture_2d<f32>;
@group(3) @binding(1) var palantir_gradient_sampler: sampler;

// The material's box at `local`, around `out`, what the effect shows there.
fn palantir_material(v: PalantirVarying, local: vec2<f32>, input: EffectInput, out: EffectOutput) -> PalantirOut {
    let pane = palantir_panes[v.pane];
    var c = out.color.rgb;
    if (any(out.under.rgb != vec3<f32>(0.0))) {
        c += out.under.rgb * effect_backdrop(input.pos).rgb;
    }
    // Scaled rather than clamped per channel, so the hue and the chroma hold.
    let luminance = dot(c, PALANTIR_LUMA);
    if (luminance > pane.max_luminance) {
        c *= pane.max_luminance / luminance;
    }
    // Lifted by a neutral, whose luminance is its level as the weights sum to 1, so black reaches
    // the floor too; after the ceiling, so the floor wins where the two cross.
    let lifted = dot(c, PALANTIR_LUMA);
    if (lifted < pane.min_luminance) {
        c += vec3<f32>(pane.min_luminance - lifted);
    }
    // A paint animation's fade scaled the opacity and the fill's alpha alike, so the fill's own
    // share is its alpha over the opacity.
    let opacity = v.clock.y;
    let fill = palantir_fill(pane, local / max(v.rect.zw, vec2<f32>(PALANTIR_ZERO_EPS)));
    let share = select(0.0, fill.a / opacity, opacity > 0.0);
    c = mix(c, fill.rgb, clamp(share, 0.0, 1.0));
    if (PALANTIR_DITHER_SRGB8) {
        // Interleaved gradient noise (Jimenez) of one encoded step, so a smooth blur stores no
        // band.
        let p = v.clip.xy + palantir_imm.frag_origin;
        let ign = fract(52.9829189 * fract(dot(floor(p), vec2<f32>(0.06711056, 0.00583715))));
        c = palantir_srgb_to_linear(max(palantir_linear_to_srgb(c) + (ign - 0.5) / 255.0, vec3<f32>(0.0)));
    }
    let shaded = palantir_material_shade(v, local, pane, vec4<f32>(c, opacity));
    return PalantirOut(shaded, vec4<f32>(1.0 - shaded.a));
}

// The slot's fill at `local01`, the box's own 0..1 frame, straight: its color, or its gradient
// times the color, as the quad shader's `eval_fill` evaluates it.
fn palantir_fill(pane: PalantirPane, local01: vec2<f32>) -> vec4<f32> {
    let kind = pane.fill_kind & PALANTIR_FILL_TAG_MASK;
    if (kind == PALANTIR_FILL_SOLID) {
        return pane.fill;
    }
    let spread = (pane.fill_kind >> PALANTIR_SPREAD_SHIFT) & PALANTIR_SPREAD_MASK;
    var t01: f32 = 0.0;
    if (kind == PALANTIR_FILL_LINEAR) {
        let span = pane.fill_axis.w - pane.fill_axis.z;
        let span_safe = select(1.0, span, abs(span) > PALANTIR_ZERO_EPS);
        t01 = (dot(local01, pane.fill_axis.xy) - pane.fill_axis.z) / span_safe;
    } else if (kind == PALANTIR_FILL_RADIAL) {
        // `t = 1` on the ellipse the radius vector spans.
        let radius = pane.fill_axis.zw;
        let rx = select(1.0, radius.x, abs(radius.x) > PALANTIR_ZERO_EPS);
        let ry = select(1.0, radius.y, abs(radius.y) > PALANTIR_ZERO_EPS);
        t01 = length((local01 - pane.fill_axis.xy) / vec2<f32>(rx, ry));
    } else {
        // Conic, the remaining kind: clockwise on screen, as y points down. `+ 1.0` lifts
        // atan2's negative half before `fract`.
        let p = local01 - pane.fill_axis.xy;
        t01 = fract((atan2(p.y, p.x) - pane.fill_axis.z) / PALANTIR_TAU + 1.0);
    }
    var t = clamp(t01, 0.0, 1.0);
    if (spread == PALANTIR_SPREAD_REPEAT) {
        t = fract(t01);
    } else if (spread == PALANTIR_SPREAD_REFLECT) {
        t = abs(fract(t01 * 0.5) - 0.5) * 2.0;
    }
    return palantir_gradient_lut(t, pane.fill_lut_row) * pane.fill;
}

// The prelude's `gradient_lut`: LUT row `row` at ramp parameter `t` in 0..1, straight.
fn palantir_gradient_lut(t: f32, row: u32) -> vec4<f32> {
    let dims = vec2<f32>(textureDimensions(palantir_gradient_tex));
    let u = (t * (dims.x - 1.0) + 0.5) / dims.x;
    let v = (f32(row) + 0.5) / dims.y;
    let c = textureSampleLevel(palantir_gradient_tex, palantir_gradient_sampler, vec2<f32>(u, v), 0.0);
    return vec4<f32>(select(vec3<f32>(0.0), c.rgb / c.a, c.a > 0.0), c.a);
}

// The rounded rect of `fill` under the slot's border, as a quad's `shade_rect` shades it. A point
// past the largest radius, and past the border and the antialiasing reach, inside the shape is the
// fill, without the distance.
fn palantir_material_shade(v: PalantirVarying, local: vec2<f32>, pane: PalantirPane, fill: vec4<f32>) -> vec4<f32> {
    let windowed = (v.flags & PALANTIR_WINDOW) != 0u;
    let size = v.rect.zw;
    if (!windowed) {
        let inset = max(
            max(max(v.corners.x, v.corners.y), max(v.corners.z, v.corners.w)),
            pane.stroke_width + PALANTIR_AA_HALF_WIDTH,
        );
        if (all(local > vec2<f32>(inset)) && all(local < size - vec2<f32>(inset))) {
            return vec4<f32>(fill.rgb * fill.a, fill.a);
        }
    }
    let d = palantir_sdf(local, size, v.corners);
    if (windowed) {
        var window = palantir_composite_window(d, fill, pane.stroke_color, pane.stroke_width);
        if (v.clock.w != 0.0) {
            // Turned, the rect edge no longer lies on content of its own extent, so it is a
            // visible edge and fades as any edge does.
            let e = abs(local - size * 0.5) - size * 0.5;
            window *= palantir_edge_coverage(e.x) * palantir_edge_coverage(e.y);
        }
        return window;
    }
    return palantir_composite(d, fill, pane.stroke_color, pane.stroke_width);
}

// The prelude's `edge_coverage`: coverage, at a pixel centered `d` outside an edge, of the side
// the edge bounds.
fn palantir_edge_coverage(d: f32) -> f32 {
    return clamp((PALANTIR_AA_HALF_WIDTH - d) / (2.0 * PALANTIR_AA_HALF_WIDTH), 0.0, 1.0);
}

// The fill over the whole box, its border band included, and the border over it there: CSS's
// border-box background, which shows no sharp content through a translucent border.
fn palantir_composite(d: f32, fill: vec4<f32>, stroke_color: vec4<f32>, stroke_width: f32) -> vec4<f32> {
    let outer_aa = palantir_edge_coverage(d);
    let fill_a = fill.a * outer_aa;
    let under = vec4<f32>(fill.rgb * fill_a, fill_a);
    if (stroke_width > 0.0) {
        let inner_aa = palantir_edge_coverage(d + stroke_width);
        let stroke_a = (outer_aa - inner_aa) * stroke_color.a;
        return vec4<f32>(stroke_color.rgb * stroke_a, stroke_a) + under * (1.0 - stroke_a);
    }
    return under;
}

// The window's twin of `palantir_composite`: the fill outside the border's inner edge, the border
// over it in its band, and a transparent interior.
fn palantir_composite_window(d: f32, fill: vec4<f32>, stroke_color: vec4<f32>, stroke_width: f32) -> vec4<f32> {
    let outer_aa = palantir_edge_coverage(d);
    if (stroke_width > 0.0) {
        let inner_aa = palantir_edge_coverage(d + stroke_width);
        let fill_a = (1.0 - inner_aa) * fill.a;
        let stroke_a = (outer_aa - inner_aa) * stroke_color.a;
        return vec4<f32>(stroke_color.rgb * stroke_a, stroke_a) + vec4<f32>(fill.rgb * fill_a, fill_a) * (1.0 - stroke_a);
    }
    let fill_a = (1.0 - outer_aa) * fill.a;
    return vec4<f32>(fill.rgb * fill_a, fill_a);
}

// The prelude's `srgb_to_linear`, IEC 61966-2-1's decode.
fn palantir_srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let low = c / 12.92;
    let high = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(high, low, c <= vec3<f32>(0.04045));
}

// The prelude's `linear_to_srgb`, IEC 61966-2-1's encode.
fn palantir_linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    let low = c * 12.92;
    let high = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(high, low, c <= vec3<f32>(0.0031308));
}
