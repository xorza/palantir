// A backdrop filter's pane, drawn through the effect pipeline as palantir's own backdrop effect
// (`effect_pipeline/pane_form.rs`): its result read back, bent through its lens, saturated, grained,
// held within its luminance bounds, tinted and stroked, over the rounded rect a quad would shade,
// turned and windowed as one. `PANE_FORM` pins frost, glass of one index or glass that spreads the
// spectrum per pipeline, each apart as the lens and its loop hold registers, which V3D spilled on
// every pane while one shader drew them all.

// One pane as `Backdrops` writes its slot (`BackdropSlot`), as `effect/backdrop.wgsl` declares
// it for an app's effect: the slot's texel coordinate of the lattice's texel 0, the physical px
// of a texel or 0 for a pane with nothing painted before it, which shows `clear`; the lamp's
// glint and the lens, `(bezel, thickness, refractive_index, dispersion)` in physical px, zero for
// a pane of no glass (`LensOptics`); the luminance it shows within, 65504 and 0 for none; its
// saturation and grain; the rect an app's effect reads; and the tint and stroke over it, straight
// alpha, linear, faded by a paint animation.
struct BackdropSlot {
    offset: vec2<f32>,
    lattice: f32,
    highlight: f32,
    lens: vec4<f32>,
    clear: vec3<f32>,
    max_luminance: f32,
    min_luminance: f32,
    saturation: f32,
    noise: f32,
    stroke_width: f32,
    reads: vec4<f32>,
    tint: vec4<f32>,
    stroke_color: vec4<f32>,
};
@group(2) @binding(0) var backdrop_results: texture_2d<f32>;
@group(2) @binding(1) var backdrop_sampler: sampler;
@group(2) @binding(2) var<storage, read> backdrop_slots: array<BackdropSlot>;

// Whether the target stores 8 bits a channel through the sRGB curve, which a pane dithers its
// smooth result for.
override DITHER_SRGB8: bool = false;

override PANE_FORM: u32;
const PANE_FORM_LENS: u32 = /*{PANE_FORM_LENS}*/;
const PANE_FORM_SPECTRUM: u32 = /*{PANE_FORM_SPECTRUM}*/;

// The instance attributes an effect's pipeline reads (`EffectInstance`): the pane's rect
// `(min, size)` and corner radii in physical px, its clock `(time, opacity, scale, rotation)`,
// its row in the frame's panes and whether it is a window. The parameter word at location 3 is
// an app's effect's alone.
struct PaneIn {
    @location(0) rect: vec4<f32>,
    @location(1) corners: vec4<f32>,
    @location(2) clock: vec4<f32>,
    @location(4) pane: u32,
    @location(5) flags: u32,
};

// `EffectInstance::flags`'s window bit: the pane fills outside its rounded rect.
const PANE_FLAG_WINDOW: u32 = /*{PANE_FLAG_WINDOW}*/;

struct PaneOut {
    @builtin(position) clip: vec4<f32>,
    // The pane's own frame, unturned, from its rect's top-left.
    @location(0) local: vec2<f32>,
    @location(1) @interpolate(flat) size: vec2<f32>,
    @location(2) @interpolate(flat) radius: vec4<f32>,
    @location(3) @interpolate(flat) opacity: f32,
    @location(4) @interpolate(flat) pane: u32,
    @location(5) @interpolate(flat) flags: u32,
    // Nonzero for a turned pane, whose rect edge is no pixel edge.
    @location(6) @interpolate(flat) turned: u32,
};

// The effect pipeline's dual-source output: `color + under · dst`.
struct PaneBlend {
    @location(0) @blend_src(0) color: vec4<f32>,
    @location(0) @blend_src(1) under: vec4<f32>,
};

// Every pixel center within `AA_HALF_WIDTH` of the rect, as a quad's `shaded_bounds` takes it.
@vertex
fn vs(@builtin(vertex_index) vi: u32, pane: PaneIn) -> PaneOut {
    let pos = pane.rect.xy;
    let size = pane.rect.zw;
    let rotation = pane.clock.w;
    var lo = pos;
    var hi = pos + size;
    if (rotation != 0.0) {
        lo -= vec2<f32>(AA_HALF_WIDTH);
        hi += vec2<f32>(AA_HALF_WIDTH);
    } else if ((pane.flags & PANE_FLAG_WINDOW) == 0u) {
        lo = floor(lo - vec2<f32>(AA_HALF_WIDTH - 0.5));
        hi = ceil(hi + vec2<f32>(AA_HALF_WIDTH - 0.5));
    }
    let corner = select(lo, hi, CORNERS[vi] > vec2<f32>(0.5));
    var out: PaneOut;
    out.clip = clip_from_px(turned(corner, pos + size * 0.5, rotation));
    out.local = corner - pos;
    out.size = size;
    out.radius = pane.corners;
    out.opacity = pane.clock.y;
    out.pane = pane.pane;
    out.flags = pane.flags;
    out.turned = u32(rotation != 0.0);
    return out;
}

// The pane's result at screen position `p`: the B-spline over the slot's
// texels through four bilinear fetches (Sigg and Hadwiger), whose weights
// add the same variance at every phase, the texel itself at level 0, or the
// clear the slot holds.
fn backdrop_result(p: vec2<f32>, slot: BackdropSlot) -> vec3<f32> {
    if (slot.lattice == 0.0) {
        return slot.clear;
    }
    let x = p / slot.lattice - vec2<f32>(0.5) + slot.offset;
    if (slot.lattice == 1.0) {
        // A lens reads between pixels, where the bilinear read is the
        // lens's own resampling; else a pixel center lands on a texel's.
        if (slot.lens.x > 0.0) {
            let size = vec2<f32>(textureDimensions(backdrop_results));
            return textureSampleLevel(backdrop_results, backdrop_sampler, (x + vec2<f32>(0.5)) / size, 0.0).rgb;
        }
        return textureLoad(backdrop_results, vec2<i32>(floor(x + vec2<f32>(0.5))), 0).rgb;
    }
    let base = floor(x);
    let f = x - base;
    let g = vec2<f32>(1.0) - f;
    let w0 = g * g * g / 6.0;
    let w1 = (4.0 - 6.0 * f * f + 3.0 * f * f * f) / 6.0;
    let w3 = f * f * f / 6.0;
    let w2 = vec2<f32>(1.0) - w0 - w1 - w3;
    let s0 = w0 + w1;
    let s1 = w2 + w3;
    let size = vec2<f32>(textureDimensions(backdrop_results));
    let lo = (base - vec2<f32>(0.5) + w1 / s0) / size;
    let hi = (base + vec2<f32>(1.5) + w3 / s1) / size;
    return s0.y * (s0.x * textureSampleLevel(backdrop_results, backdrop_sampler, vec2<f32>(lo.x, lo.y), 0.0).rgb
                 + s1.x * textureSampleLevel(backdrop_results, backdrop_sampler, vec2<f32>(hi.x, lo.y), 0.0).rgb)
         + s1.y * (s0.x * textureSampleLevel(backdrop_results, backdrop_sampler, vec2<f32>(lo.x, hi.y), 0.0).rgb
                 + s1.x * textureSampleLevel(backdrop_results, backdrop_sampler, vec2<f32>(hi.x, hi.y), 0.0).rgb);
}

// A grain value in `0..1` for the whole pixel of the pane's own frame at
// `local`, so the grain moves and turns with the pane.
fn grain(local: vec2<f32>) -> f32 {
    let cell = vec2<u32>(vec2<i32>(floor(local)) + vec2<i32>(32768));
    var h = cell.x * 0x9E3779B1u ^ (cell.y * 0x85EBCA77u);
    h ^= h >> 15u;
    h *= 0x2C1B3C6Du;
    h ^= h >> 12u;
    return f32(h >> 8u) / 16777216.0;
}

// How far in, px, a lens sends the sample `inward` px in from its edge for
// light of index `index`: the squircle bezel's tilt, Snell's law in vector
// form for light falling straight down, and the bent ray carried from the
// glass's top to the plane under it. `LensOptics::displacement`'s copy.
fn lens_displacement(lens: vec4<f32>, inward: f32, index: f32) -> f32 {
    let x = clamp(inward / lens.x, 0.0, 1.0);
    let g = 1.0 - x;
    let rise = max(1.0 - g * g * g * g, 1e-30);
    let height = lens.y * pow(rise, 0.25);
    let slope = select(lens.y / lens.x * g * g * g * pow(rise, -0.75), 0.0, x >= 1.0);
    let norm = sqrt(slope * slope + 1.0);
    let nx = slope / norm;
    let nz = 1.0 / norm;
    let eta = 1.0 / index;
    let k = 1.0 - eta * eta * (1.0 - nz * nz);
    let along = eta * nz - sqrt(k);
    return height * (-along * nx) / (eta - along * nz);
}

// Fraunhofer's D, F and C lines, µm: a lens's index is its index at D, and
// its dispersion the step from C to F, `n_F − n_C` (`lens_optics`).
const LINE_D: f32 = /*{LINE_D}*/;
const LINE_F: f32 = /*{LINE_F}*/;
const LINE_C: f32 = /*{LINE_C}*/;

// The ends of the spectrum a lens spreads, µm.
const SPECTRUM_RED: f32 = /*{SPECTRUM_RED}*/;
const SPECTRUM_VIOLET: f32 = /*{SPECTRUM_VIOLET}*/;

// The spectrum's weight in XYZ over the share `t` of the way along it, red
// end first, held flat across each of `SPECTRUM_SAMPLES` cells: each cell's
// integral, and the running integrals of the weight and of `t` times it at
// the cell's start (`spectrum_tables::SpectrumTables`). A buffer, as V3D runs
// a dynamically indexed constant array as a ladder of selects over every
// entry, and spilled.
const SPECTRUM_SAMPLES: u32 = /*{SPECTRUM_SAMPLES}*/;
struct SpectrumCell {
    weight: vec3<f32>,
    cumulative: vec3<f32>,
    moment: vec3<f32>,
}
@group(2) @binding(3) var<storage, read> spectrum: array<SpectrumCell, SPECTRUM_SAMPLES>;

// The most whole steps a bezel pixel reads across the spectrum: it bounds a
// pixel's cost at 33 reads, 22–27 % over the default dispersion's frame on
// an Apple laptop (`.notes/research/2026-10-28-backdrops.md`, "Measured:
// dispersion").
const SPECTRUM_STEPS: f32 = 32.0;

// The spectrum's running integrals from its red end to `t`: of the weight,
// and of `t` times it.
struct SpectrumIntegrals {
    area: vec3<f32>,
    moment: vec3<f32>,
}

fn spectrum_integrals(t: f32) -> SpectrumIntegrals {
    let n = f32(SPECTRUM_SAMPLES);
    let cell = min(u32(t * n), SPECTRUM_SAMPLES - 1u);
    let start = f32(cell) / n;
    let at = spectrum[cell];
    return SpectrumIntegrals(
        at.cumulative + at.weight * (t * n - f32(cell)),
        at.moment + at.weight * (0.5 * n * (t * t - start * start)),
    );
}

// Linear sRGB to CIE XYZ under D65, and back (IEC 61966-2-1), column-major.
const RGB_TO_XYZ: mat3x3<f32> = mat3x3<f32>(
    vec3<f32>(0.4124564, 0.2126729, 0.0193339),
    vec3<f32>(0.3575761, 0.7151522, 0.1191920),
    vec3<f32>(0.1804375, 0.0721750, 0.9503041),
);
const XYZ_TO_RGB: mat3x3<f32> = mat3x3<f32>(
    vec3<f32>(3.2404542, -0.9692660, 0.0556434),
    vec3<f32>(-1.5371385, 1.8760108, -0.2040259),
    vec3<f32>(-0.4985314, 0.0415560, 1.0572252),
);

// The index of a lens for light the share `t` of the way from the
// spectrum's red end to its violet end: Cauchy's `n(λ) = A + B / λ²`
// through the index at D with `n_F − n_C` its dispersion, affine in
// `u = 1 / λ²`, along which `t` runs. At least 1: the red end of a weak,
// dispersive lens falls below it, where Snell's law has no ray at a steep
// bezel and a ray would bend outward, past the reach.
fn lens_index(lens: vec4<f32>, t: f32) -> f32 {
    let u = mix(1.0 / (SPECTRUM_RED * SPECTRUM_RED), 1.0 / (SPECTRUM_VIOLET * SPECTRUM_VIOLET), t);
    let per_u = lens.w / (1.0 / (LINE_F * LINE_F) - 1.0 / (LINE_C * LINE_C));
    return max(lens.z + per_u * (u - 1.0 / (LINE_D * LINE_D)), 1.0);
}

// The pane's result in XYZ where light the share `t` along the spectrum
// lands, refracted at its own index.
fn lens_ray(p: vec2<f32>, outward: vec2<f32>, inward: f32, slot: BackdropSlot, t: f32) -> vec3<f32> {
    let shift = lens_displacement(slot.lens, inward, lens_index(slot.lens, t));
    return RGB_TO_XYZ * backdrop_result(p - outward * shift, slot);
}

// What the bezel shows `inward` px in at `p`, bent along `outward`: every
// wavelength refracted at its own index and the spectrum summed back to
// color in XYZ (`gpu::backdrop`, "Dispersion spreads the whole spectrum").
//
// The reads step a lattice texel apart, the finest the result holds, so no
// read shows as a ghost of the next; past `SPECTRUM_STEPS` steps they spread
// further. Between two reads the result is linear, as the bilinear read is,
// and each stretch's integral against the weight comes in closed form from
// the running integrals. Its integral against `(t − a) / (b − a)` divides a
// difference of moments by the stretch's length, which `f32` cannot hold
// below a thousandth; there the weight is flat enough to take half the
// stretch's area. The last stretch is a part one, so no contour marks where
// the step count grows with the spread. A spread under a thousandth of a px
// shows nothing, so the pane keeps one read at D.
fn lens_spectrum(p: vec2<f32>, outward: vec2<f32>, inward: f32, slot: BackdropSlot) -> vec3<f32> {
    let lens = slot.lens;
    let spread = abs(
        lens_displacement(lens, inward, lens_index(lens, 1.0))
            - lens_displacement(lens, inward, lens_index(lens, 0.0)),
    );
    if (slot.lattice == 0.0 || spread < 1e-3) {
        return backdrop_result(p - outward * lens_displacement(lens, inward, lens.z), slot);
    }
    let dt = max(slot.lattice, spread / SPECTRUM_STEPS) / spread;
    var a = 0.0;
    var at_a = lens_ray(p, outward, inward, slot, 0.0);
    var integrals_a = SpectrumIntegrals(vec3<f32>(0.0), vec3<f32>(0.0));
    var step = 0u;
    var xyz = vec3<f32>(0.0);
    loop {
        step++;
        let b = min(f32(step) * dt, 1.0);
        let at_b = lens_ray(p, outward, inward, slot, b);
        let integrals_b = spectrum_integrals(b);
        let area = integrals_b.area - integrals_a.area;
        let length = b - a;
        let toward_b = select(
            (integrals_b.moment - integrals_a.moment - a * area) / length,
            0.5 * area,
            length < 1e-3,
        );
        xyz += at_a * (area - toward_b) + at_b * toward_b;
        if (b >= 1.0) {
            break;
        }
        a = b;
        at_a = at_b;
        integrals_a = integrals_b;
    }
    return XYZ_TO_RGB * xyz;
}

// The glint a lamp low at the top left leaves on the bezel `inward` px in,
// its outward normal `outward` on the screen: Schlick's Fresnel at the
// surface's tilt `θ`, times how squarely the surface's normal faces the
// lamp, `sin θ` of it along `outward`. A flat face takes none, so the glint
// fades to 0 where the bezel meets the flat top (`gpu::backdrop`, "The glint
// fades into the flat top").
//
// The slope `k · g³ · r^(−¾)` is vertical at the edge, so the normal comes
// from `(k · g³, r^¾)`, the slope times `r^¾`, which no edge overflows:
// `f32` holds no square of the slope there.
fn lens_glint(lens: vec4<f32>, inward: f32, outward: vec2<f32>) -> f32 {
    let g = 1.0 - clamp(inward / lens.x, 0.0, 1.0);
    let rise = 1.0 - g * g * g * g;
    let across = lens.y / lens.x * g * g * g;
    let up = sqrt(rise * sqrt(rise));
    let norm = sqrt(across * across + up * up);
    let cos_in = up / norm;
    let sin_in = across / norm;
    let f0 = pow((lens.z - 1.0) / (lens.z + 1.0), 2.0);
    let fresnel = f0 + (1.0 - f0) * pow(1.0 - cos_in, 5.0);
    let facing = max(dot(outward, vec2<f32>(-0.70710678, -0.70710678)), 0.0);
    return fresnel * sin_in * facing;
}

// The pane: its result, bent where it is glass, saturated, grained, held within its luminance
// bounds and tinted, at the pane's opacity, over the rounded rect under its stroke. A paint
// animation's fade scaled the opacity and the tint's alpha alike, so the tint's own share is its
// alpha over the opacity.
//
// A lens bends along the distance's screen gradient, the edge's outward normal on the screen,
// turned with the pane. Only the glass pipelines measure the distance, so the gradient is taken
// in uniform control flow.
@fragment
fn fs(in: PaneOut) -> PaneBlend {
    let slot = backdrop_slots[in.pane];
    let lens = slot.lens;
    let p = frag_px(in.clip);
    var c: vec3<f32>;
    if (PANE_FORM == PANE_FORM_LENS || PANE_FORM == PANE_FORM_SPECTRUM) {
        let d = sdf_rounded_rect(in.local, in.size, in.radius);
        let gradient = vec2<f32>(dpdx(d), dpdy(d));
        let inward = -d;
        if (inward > 0.0 && inward < lens.x && dot(gradient, gradient) > 0.0) {
            let outward = normalize(gradient);
            if (PANE_FORM == PANE_FORM_SPECTRUM) {
                c = lens_spectrum(p, outward, inward, slot);
            } else {
                c = backdrop_result(p - outward * lens_displacement(lens, inward, lens.z), slot);
            }
            c += vec3<f32>(slot.highlight * lens_glint(lens, inward, outward));
        } else {
            c = backdrop_result(p, slot);
        }
    } else {
        c = backdrop_result(p, slot);
    }
    let opacity = in.opacity;
    c = max(mix(vec3<f32>(dot(c, LUMA)), c, slot.saturation), vec3<f32>(0.0));
    if (slot.noise > 0.0) {
        c = mix(c, vec3<f32>(grain(in.local)), slot.noise);
    }
    // Scaled rather than clamped per channel, so the hue and the chroma hold.
    let luminance = dot(c, LUMA);
    if (luminance > slot.max_luminance) {
        c *= slot.max_luminance / luminance;
    }
    // Lifted by a neutral, whose luminance is its level as LUMA sums to 1, so black reaches the
    // floor too; after the ceiling, so the floor wins where the two cross.
    let lifted = dot(c, LUMA);
    if (lifted < slot.min_luminance) {
        c += vec3<f32>(slot.min_luminance - lifted);
    }
    let tint = select(0.0, slot.tint.a / opacity, opacity > 0.0);
    c = mix(c, slot.tint.rgb, clamp(tint, 0.0, 1.0));
    if (DITHER_SRGB8) {
        // Interleaved gradient noise (Jimenez) of one encoded step, so a smooth blur stores no
        // band.
        let ign = fract(52.9829189 * fract(dot(floor(p), vec2<f32>(0.06711056, 0.00583715))));
        c = srgb_to_linear(max(linear_to_srgb(c) + (ign - 0.5) / 255.0, vec3<f32>(0.0)));
    }
    let shaded = shade_pane(in, slot, vec4<f32>(c, opacity));
    return PaneBlend(shaded, vec4<f32>(1.0 - shaded.a));
}

// The rounded rect of `fill` under the slot's stroke, as a quad's `shade_rect` shades it. A point
// past the largest radius, and past the stroke and `AA_HALF_WIDTH`, inside the shape is the fill,
// without the distance.
fn shade_pane(in: PaneOut, slot: BackdropSlot, fill: vec4<f32>) -> vec4<f32> {
    let windowed = (in.flags & PANE_FLAG_WINDOW) != 0u;
    if (!windowed) {
        let inset = max(
            max(max(in.radius.x, in.radius.y), max(in.radius.z, in.radius.w)),
            slot.stroke_width + AA_HALF_WIDTH,
        );
        if (all(in.local > vec2<f32>(inset)) && all(in.local < in.size - vec2<f32>(inset))) {
            return premultiply(fill.rgb, fill.a);
        }
    }
    let d = sdf_rounded_rect(in.local, in.size, in.radius);
    if (windowed) {
        var window = composite_window(d, fill, slot.stroke_color, slot.stroke_width);
        if (in.turned != 0u) {
            // Turned, the rect edge no longer lies on content of its own extent, so it is a
            // visible edge and fades as any edge does.
            let e = abs(in.local - in.size * 0.5) - in.size * 0.5;
            window *= edge_coverage(e.x) * edge_coverage(e.y);
        }
        return window;
    }
    return composite(d, fill, slot.stroke_color, slot.stroke_width);
}
