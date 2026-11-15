// Android's sparkle ripple, ported from AOSP `RippleShader.java` and the timing of
// `RippleAnimationSession.java` (Copyright (C) 2021 The Android Open Source Project, Apache License
// 2.0): a soft disc of the theme's color under a ring of sparkles of its highlight, both from the
// press toward the rect's center. Its progress runs to 0.5 over `enter_seconds` along FastOutSlowIn, then
// linearly to 1 over `exit_seconds`. It works in physical px, as Android does, and the sparkles' noise
// starts at the press's `seed` in place of Android's clock.

const SPARKLE_PI: f32 = 3.1415926535897932384626;

fn sparkle_triangle_noise(m: vec2<f32>) -> f32 {
    var n = fract(m * vec2<f32>(5.3987, 5.4421));
    n += dot(n.yx, n.xy + vec2<f32>(21.5351, 14.3137));
    let xy = n.x * n.y;
    return fract(xy * 95.4307) + fract(xy * 75.04961) - 1.0;
}

fn sparkle_threshold(v: f32, l: f32, h: f32) -> f32 {
    return step(l, v) * (1.0 - step(h, v));
}

fn sparkle_sparkles(uv: vec2<f32>, t: f32, alpha: f32) -> f32 {
    let n = sparkle_triangle_noise(uv);
    var s = 0.0;
    for (var i = 0; i < 4; i++) {
        let l = f32(i) * 0.1;
        let h = l + 0.05;
        let o = sin(SPARKLE_PI * (t + 0.35 * f32(i)));
        s += sparkle_threshold(n + o, l, h);
    }
    return saturate(s) * alpha;
}

fn sparkle_soft_circle(uv: vec2<f32>, xy: vec2<f32>, radius: f32, blur: f32) -> f32 {
    // A circle of no radius covers nothing; the ratio below would read 0 / 0 at its center.
    if (radius <= 0.0) {
        return 0.0;
    }
    let blur_half = blur * 0.5;
    let d = distance(uv, xy);
    return 1.0 - smoothstep(1.0 - blur_half, 1.0 + blur_half, d / radius);
}

fn sparkle_soft_ring(uv: vec2<f32>, xy: vec2<f32>, radius: f32, progress: f32, blur: f32) -> f32 {
    let thickness = 0.05 * radius;
    let current = radius * progress;
    let outer = sparkle_soft_circle(uv, xy, current + thickness, blur);
    let inner = sparkle_soft_circle(uv, xy, max(current - thickness, 0.0), blur);
    return saturate(outer - inner);
}

fn sparkle_sub_progress(start: f32, end: f32, progress: f32) -> f32 {
    return (clamp(progress, start, end) - start) / (end - start);
}

// GLSL's `mod`, which floors, where WGSL's `%` truncates.
fn sparkle_mod(x: vec2<f32>, y: vec2<f32>) -> vec2<f32> {
    return x - y * floor(x / y);
}

fn sparkle_circle_grid(
    resolution: vec2<f32>,
    coord_in: vec2<f32>,
    center: vec2<f32>,
    rotation: vec2<f32>,
    cell_diameter: f32,
) -> f32 {
    let turn = mat2x2<f32>(vec2<f32>(rotation.x, -rotation.y), vec2<f32>(rotation.y, rotation.x));
    var coord = turn * (center - coord_in) + center;
    coord = sparkle_mod(coord, vec2<f32>(cell_diameter)) / resolution;
    let normal_radius = cell_diameter / resolution.y * 0.5;
    let radius = 0.65 * normal_radius;
    return sparkle_soft_circle(coord, vec2<f32>(normal_radius), radius, radius * 50.0);
}

// Android's turbulence, with the circles and turns `RippleShader.setNoisePhase` sets from the
// phase.
fn sparkle_turbulence(uv_in: vec2<f32>, phase: f32) -> f32 {
    let scale = 1.5;
    let c1 = vec2<f32>(
        scale * 0.5 + phase * 0.01 * cos(scale * 0.55),
        scale * 0.5 + phase * 0.01 * sin(scale * 0.55),
    );
    let c2 = vec2<f32>(
        scale * 0.2 + phase * -0.0066 * cos(scale * 0.45),
        scale * 0.2 + phase * -0.0066 * sin(scale * 0.45),
    );
    let c3 = vec2<f32>(
        scale + phase * -0.0066 * cos(scale * 0.35),
        scale + phase * -0.0066 * sin(scale * 0.35),
    );
    let right = SPARKLE_PI * 0.0078125;
    let a1 = phase * right + 1.7 * SPARKLE_PI;
    let a2 = phase * -right + 2.0 * SPARKLE_PI;
    let a3 = phase * right + 2.75 * SPARKLE_PI;
    let grid = vec2<f32>(0.8);
    let uv = uv_in * grid;
    let g1 = sparkle_circle_grid(grid, uv, c1, vec2<f32>(cos(a1), sin(a1)), 0.17);
    let g2 = sparkle_circle_grid(grid, uv, c2, vec2<f32>(cos(a2), sin(a2)), 0.2);
    let g3 = sparkle_circle_grid(grid, uv, c3, vec2<f32>(cos(a3), sin(a3)), 0.275);
    let v = (g1 * g1 + g2 - g3) * 0.5;
    return saturate(0.45 + 0.8 * v);
}

fn effect(input: EffectInput, params: Params) -> EffectOutput {
    let p = input.pos * input.scale;
    let size = input.size * input.scale;
    let touch = params.point * input.scale;
    let origin = size * 0.5;
    let progress = 0.5 * press_fast_out_slow_in(press_through(input.time, params.enter_seconds))
        + 0.5 * press_fade(params, input.time);
    // Android's noise phase in ms: its start, then 7000 / 214 = 32 more over a 7 s loop.
    let phase = params.seed * 1000.0 + 32.0 * min(input.time, 7.0) / 7.0;
    let max_radius = length(origin) * 2.3;

    let fade_in = sparkle_sub_progress(0.0, 0.13, progress);
    let scale_in = sparkle_sub_progress(0.0, 1.0, progress);
    let fade_out_noise = sparkle_sub_progress(0.4, 0.5, progress);
    let fade_out_ripple = sparkle_sub_progress(0.4, 1.0, progress);
    let center = mix(touch, origin, saturate(progress * 2.0));
    let ring = sparkle_soft_ring(p, center, max_radius, scale_in, 1.0);
    let alpha = min(fade_in, 1.0 - fade_out_noise);
    let uv = p / size;
    let density_uv = uv - sparkle_mod(uv, vec2<f32>(2.1) / size);
    let turb = sparkle_turbulence(uv, phase);
    let sparkles = sparkle_sparkles(density_uv, phase * 0.001, params.highlight.a);
    let sparkle_alpha = sparkles * ring * alpha * turb;
    let fade = min(fade_in, 1.0 - fade_out_ripple);
    let wave_alpha = sparkle_soft_circle(p, center, max_radius * scale_in, 1.0) * fade
        * params.color.a;
    let wave = vec4<f32>(params.color.rgb * wave_alpha, wave_alpha);
    let sparkle = vec4<f32>(params.highlight.rgb * params.highlight.a, params.highlight.a);
    return effect_over(mix(wave, sparkle, sparkle_alpha));
}
