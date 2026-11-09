// The stock glass: what the material reads, bent through a lens across a rounded bezel at its
// edge, every wavelength at its own index where the lens disperses, glinting, then saturated and
// grained as frosted glass is. Lengths are the material's own logical px; a displacement is
// equivariant in scale, so the bend lands where the physical one would.

// Fraunhofer's D, F and C lines, µm: a lens's index is its index at D, and its dispersion the
// step from C to F, `n_F − n_C` (`lens_optics`).
const GLASS_LINE_D: f32 = /*{LINE_D}*/;
const GLASS_LINE_F: f32 = /*{LINE_F}*/;
const GLASS_LINE_C: f32 = /*{LINE_C}*/;

// The ends of the spectrum a lens spreads, µm.
const GLASS_SPECTRUM_RED: f32 = /*{SPECTRUM_RED}*/;
const GLASS_SPECTRUM_VIOLET: f32 = /*{SPECTRUM_VIOLET}*/;

// The cells of `palantir_spectrum`, the spectrum's weight in XYZ over the share `t` of the way
// along it, red end first, held flat across each cell (`spectrum_tables::SpectrumTables`).
const GLASS_SPECTRUM_SAMPLES: u32 = /*{SPECTRUM_SAMPLES}*/;

// The most whole steps a bezel pixel reads across the spectrum: it bounds a pixel's cost at 33
// reads, 22–27 % over the default dispersion's frame on an Apple laptop
// (`.notes/research/2026-10-28-backdrops.md`, "Measured: dispersion").
const GLASS_SPECTRUM_STEPS: f32 = 32.0;

// Linear sRGB to CIE XYZ under D65, and back (IEC 61966-2-1), column-major.
const GLASS_RGB_TO_XYZ: mat3x3<f32> = mat3x3<f32>(
    vec3<f32>(0.4124564, 0.2126729, 0.0193339),
    vec3<f32>(0.3575761, 0.7151522, 0.1191920),
    vec3<f32>(0.1804375, 0.0721750, 0.9503041),
);
const GLASS_XYZ_TO_RGB: mat3x3<f32> = mat3x3<f32>(
    vec3<f32>(3.2404542, -0.9692660, 0.0556434),
    vec3<f32>(-1.5371385, 1.8760108, -0.2040259),
    vec3<f32>(-0.4985314, 0.0415560, 1.0572252),
);

// The lens as one vector: `(bezel, thickness, refractive_index, dispersion)`.
fn glass_lens(params: Params) -> vec4<f32> {
    return vec4<f32>(params.bezel, params.thickness, params.refractive_index, params.dispersion);
}

// The rounded rect's signed distance at `p` from its top-left, and its outward unit normal there,
// both in its own frame: the gradient of the distance in closed form, along the axis nearer the
// edge inside the straight sides and from the corner's center within a rounded corner.
struct GlassEdge {
    distance: f32,
    outward: vec2<f32>,
}

fn glass_edge(p: vec2<f32>, size: vec2<f32>, radius: vec4<f32>) -> GlassEdge {
    let half = size * 0.5;
    let c = p - half;
    let right = step(0.0, c.x);
    let bottom = step(0.0, c.y);
    let r = mix(mix(radius.x, radius.y, right), mix(radius.w, radius.z, right), bottom);
    let q = abs(c) - (half - vec2<f32>(r));
    let distance = min(max(q.x, q.y), 0.0) + length(max(q, vec2<f32>(0.0))) - r;
    let side = select(vec2<f32>(-1.0), vec2<f32>(1.0), c >= vec2<f32>(0.0));
    var outward: vec2<f32>;
    if (q.x > 0.0 && q.y > 0.0) {
        outward = side * normalize(q);
    } else if (q.x > q.y) {
        outward = vec2<f32>(side.x, 0.0);
    } else {
        outward = vec2<f32>(0.0, side.y);
    }
    return GlassEdge(distance, outward);
}

// How far in a lens sends the sample `inward` in from its edge for light of index `index`: the
// squircle bezel's tilt, Snell's law in vector form for light falling straight down, and the bent
// ray carried from the glass's top to the plane under it. `LensOptics::displacement`'s copy.
fn glass_displacement(lens: vec4<f32>, inward: f32, index: f32) -> f32 {
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

// The index of a lens for light the share `t` of the way from the spectrum's red end to its violet
// end: Cauchy's `n(λ) = A + B / λ²` through the index at D with `n_F − n_C` its dispersion, affine
// in `u = 1 / λ²`, along which `t` runs. At least 1: the red end of a weak, dispersive lens falls
// below it, where Snell's law has no ray at a steep bezel and a ray would bend outward.
fn glass_index(lens: vec4<f32>, t: f32) -> f32 {
    let u = mix(
        1.0 / (GLASS_SPECTRUM_RED * GLASS_SPECTRUM_RED),
        1.0 / (GLASS_SPECTRUM_VIOLET * GLASS_SPECTRUM_VIOLET),
        t,
    );
    let per_u = lens.w / (1.0 / (GLASS_LINE_F * GLASS_LINE_F) - 1.0 / (GLASS_LINE_C * GLASS_LINE_C));
    return max(lens.z + per_u * (u - 1.0 / (GLASS_LINE_D * GLASS_LINE_D)), 1.0);
}

// The spectrum's running integrals from its red end to `t`: of the weight, and of `t` times it.
struct GlassIntegrals {
    area: vec3<f32>,
    moment: vec3<f32>,
}

fn glass_integrals(t: f32) -> GlassIntegrals {
    let n = f32(GLASS_SPECTRUM_SAMPLES);
    let cell = min(u32(t * n), GLASS_SPECTRUM_SAMPLES - 1u);
    let start = f32(cell) / n;
    let at = palantir_spectrum[cell];
    return GlassIntegrals(
        at.cumulative + at.weight * (t * n - f32(cell)),
        at.moment + at.weight * (0.5 * n * (t * t - start * start)),
    );
}

// What the material reads in XYZ where light the share `t` along the spectrum lands, refracted at
// its own index.
fn glass_ray(pos: vec2<f32>, outward: vec2<f32>, inward: f32, lens: vec4<f32>, t: f32) -> vec3<f32> {
    let shift = glass_displacement(lens, inward, glass_index(lens, t));
    return GLASS_RGB_TO_XYZ * effect_backdrop(pos - outward * shift).rgb;
}

// What the bezel shows `inward` in at `pos`, bent along `outward`: every wavelength refracted at
// its own index and the spectrum summed back to color in XYZ (`gpu::backdrop`, "Dispersion
// spreads the whole spectrum").
//
// The reads step a texel of the capture apart, the finest it holds, so no read shows as a ghost of
// the next; past `GLASS_SPECTRUM_STEPS` steps they spread further. Between two reads the result is
// linear, as the bilinear read is, and each stretch's integral against the weight comes in closed
// form from the running integrals. Its integral against `(t − a) / (b − a)` divides a difference
// of moments by the stretch's length, which `f32` cannot hold below a thousandth; there the weight
// is flat enough to take half the stretch's area. The last stretch is a part one, so no contour
// marks where the step count grows with the spread. A spread under a thousandth of a physical px
// shows nothing, so the material keeps one read at D, as it does over the ground.
fn glass_spectrum(pos: vec2<f32>, outward: vec2<f32>, inward: f32, lens: vec4<f32>, scale: f32) -> vec3<f32> {
    let spread = abs(
        glass_displacement(lens, inward, glass_index(lens, 1.0))
            - glass_displacement(lens, inward, glass_index(lens, 0.0)),
    );
    let lattice = palantir_backdrops[palantir_backdrop_idx].lattice;
    if (lattice == 0.0 || spread * scale < 1e-3) {
        return effect_backdrop(pos - outward * glass_displacement(lens, inward, lens.z)).rgb;
    }
    let dt = max(lattice / scale, spread / GLASS_SPECTRUM_STEPS) / spread;
    var a = 0.0;
    var at_a = glass_ray(pos, outward, inward, lens, 0.0);
    var integrals_a = GlassIntegrals(vec3<f32>(0.0), vec3<f32>(0.0));
    var step = 0u;
    var xyz = vec3<f32>(0.0);
    loop {
        step++;
        let b = min(f32(step) * dt, 1.0);
        let at_b = glass_ray(pos, outward, inward, lens, b);
        let integrals_b = glass_integrals(b);
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
    return GLASS_XYZ_TO_RGB * xyz;
}

// The glint a lamp low at the top left of the screen leaves on the bezel `inward` in, its outward
// normal `outward` on the screen: Schlick's Fresnel at the surface's tilt `θ`, times how squarely
// the surface's normal faces the lamp, `sin θ` of it along `outward`. A flat face takes none, so
// the glint fades to 0 where the bezel meets the flat top (`gpu::backdrop`, "The glint fades into
// the flat top").
//
// The slope `k · g³ · r^(−¾)` is vertical at the edge, so the normal comes from `(k · g³, r^¾)`,
// the slope times `r^¾`, which no edge overflows: `f32` holds no square of the slope there.
fn glass_glint(lens: vec4<f32>, inward: f32, outward: vec2<f32>) -> f32 {
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

fn effect(input: EffectInput, params: Params) -> EffectOutput {
    let lens = glass_lens(params);
    let edge = glass_edge(input.pos, input.size, input.corners);
    let inward = -edge.distance;
    var c: vec3<f32>;
    if (inward > 0.0 && inward < lens.x) {
        if (lens.w > 0.0) {
            c = glass_spectrum(input.pos, edge.outward, inward, lens, input.scale);
        } else {
            c = effect_backdrop(input.pos - edge.outward * glass_displacement(lens, inward, lens.z)).rgb;
        }
        let on_screen = palantir_turned(edge.outward, vec2<f32>(0.0), palantir_rotation);
        c += vec3<f32>(params.highlight * glass_glint(lens, inward, on_screen));
    } else {
        c = effect_backdrop(input.pos).rgb;
    }
    return effect_over(vec4<f32>(material_frost(c, input, params), 1.0));
}
