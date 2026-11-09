
// Palantir's material stage, around a backdrop effect over `MaterialInput` that a `Material`
// names: what the effect shows over what it reads, held within the material's luminance bounds,
// under the box's fill, dithered where an 8-bit sRGB target stores it, and under the box's border,
// over the rounded rect a quad of the box would shade, turned and windowed as one. The pane's slot
// holds the bounds, the fill and the border, faded by a paint animation.

// Whether the target stores 8 bits a channel through the sRGB curve, which a material dithers its
// smooth result for.
override PALANTIR_DITHER_SRGB8: bool = false;

// `EffectInstance::flags`'s window bit: the box fills outside its rounded rect.
const PALANTIR_WINDOW: u32 = /*{WINDOW}*/;

// Rec. 709 luminance weights, over linear light.
const PALANTIR_LUMA: vec3<f32> = vec3<f32>(0.2126, 0.7152, 0.0722);

struct PalantirMaterialInstance {
    @location(0) rect: vec4<f32>,
    @location(1) corners: vec4<f32>,
    @location(2) clock: vec4<f32>,
    @location(3) params: u32,
    @location(4) pane: u32,
    @location(5) flags: u32,
}

struct PalantirMaterialVarying {
    @builtin(position) clip: vec4<f32>,
    @location(0) @interpolate(flat) rect: vec4<f32>,
    @location(1) @interpolate(flat) corners: vec4<f32>,
    @location(2) @interpolate(flat) clock: vec4<f32>,
    @location(3) @interpolate(flat) params: u32,
    @location(4) @interpolate(flat) pane: u32,
    @location(5) @interpolate(flat) flags: u32,
}

// Every pixel center within the antialiasing reach of the rect, as a quad's `shaded_bounds` takes
// it; a window's rect is its edge exactly, as it lies over content of its own extent.
@vertex
fn palantir_material_vs(@builtin(vertex_index) vi: u32, instance: PalantirMaterialInstance) -> PalantirMaterialVarying {
    let pos = instance.rect.xy;
    let size = instance.rect.zw;
    let rotation = instance.clock.w;
    var lo = pos;
    var hi = pos + size;
    if (rotation != 0.0) {
        lo -= vec2<f32>(PALANTIR_AA_HALF_WIDTH);
        hi += vec2<f32>(PALANTIR_AA_HALF_WIDTH);
    } else if ((instance.flags & PALANTIR_WINDOW) == 0u) {
        lo = floor(lo - vec2<f32>(PALANTIR_AA_HALF_WIDTH - 0.5));
        hi = ceil(hi + vec2<f32>(PALANTIR_AA_HALF_WIDTH - 0.5));
    }
    let corner = select(lo, hi, PALANTIR_CORNERS[vi] > vec2<f32>(0.5));
    let px = palantir_turned(corner, pos + size * 0.5, rotation);
    let ndc = px * (vec2<f32>(2.0, -2.0) / palantir_imm.viewport_size) + vec2<f32>(-1.0, 1.0);
    return PalantirMaterialVarying(
        vec4<f32>(ndc, 0.0, 1.0),
        instance.rect,
        instance.corners,
        instance.clock,
        instance.params,
        instance.pane,
        instance.flags,
    );
}

@fragment
fn palantir_material_fs(v: PalantirMaterialVarying) -> PalantirOut {
    let scale = v.clock.z;
    let local = palantir_local(v.clip, v.rect, v.clock.w);
    let input = EffectInput(local / scale, v.rect.zw / scale, v.corners / scale, v.clock.x, scale);
    palantir_pane = v.pane;
    palantir_rect = v.rect;
    palantir_rotation = v.clock.w;
    palantir_scale = scale;
    let out = effect(input, palantir_params(v.params));
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
    let tint = select(0.0, pane.tint.a / opacity, opacity > 0.0);
    c = mix(c, pane.tint.rgb, clamp(tint, 0.0, 1.0));
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

// The rounded rect of `fill` under the slot's border, as a quad's `shade_rect` shades it. A point
// past the largest radius, and past the border and the antialiasing reach, inside the shape is the
// fill, without the distance.
fn palantir_material_shade(v: PalantirMaterialVarying, local: vec2<f32>, pane: PalantirPane, fill: vec4<f32>) -> vec4<f32> {
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

// The prelude's `composite`: the fill inside the border, the border's annulus, their coverages
// partitioning each pixel so they sum with no seam.
fn palantir_composite(d: f32, fill: vec4<f32>, stroke_color: vec4<f32>, stroke_width: f32) -> vec4<f32> {
    let outer_aa = palantir_edge_coverage(d);
    if (stroke_width > 0.0) {
        let inner_aa = palantir_edge_coverage(d + stroke_width);
        let stroke_a = (outer_aa - inner_aa) * stroke_color.a;
        let fill_a = inner_aa * fill.a;
        return vec4<f32>(stroke_color.rgb * stroke_a, stroke_a) + vec4<f32>(fill.rgb * fill_a, fill_a);
    }
    let a = fill.a * outer_aa;
    return vec4<f32>(fill.rgb * a, a);
}

// The prelude's `composite_window`: the fill outside the rounded shape, the border's annulus, and
// a transparent interior.
fn palantir_composite_window(d: f32, fill: vec4<f32>, stroke_color: vec4<f32>, stroke_width: f32) -> vec4<f32> {
    let outer_aa = palantir_edge_coverage(d);
    let fill_a = (1.0 - outer_aa) * fill.a;
    if (stroke_width > 0.0) {
        let inner_aa = palantir_edge_coverage(d + stroke_width);
        let stroke_a = (outer_aa - inner_aa) * stroke_color.a;
        return vec4<f32>(stroke_color.rgb * stroke_a, stroke_a) + vec4<f32>(fill.rgb * fill_a, fill_a);
    }
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
