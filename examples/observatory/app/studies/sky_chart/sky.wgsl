// The night sky under the chart's stars: a dark ground and the band of the example's Milky Way,
// for the part of the chart in view. One full-screen triangle; every fragment turns its chart
// point back into RA and Dec and reads its galactic latitude.

struct Sky {
    // The visible part of the chart, in chart pixels: its corner and its size.
    view_min: vec2<f32>,
    view_size: vec2<f32>,
    // The whole chart, in chart pixels.
    chart: vec2<f32>,
    // 1 for night vision, which turns the sky red; 0 otherwise.
    night: f32,
    _pad: f32,
};

@group(0) @binding(0) var<uniform> sky: Sky;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs(@builtin(vertex_index) idx: u32) -> VsOut {
    let x = f32((idx << 1u) & 2u);
    let y = f32(idx & 2u);
    var out: VsOut;
    out.clip = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    out.uv = vec2<f32>(x, y);
    return out;
}

const PI: f32 = 3.14159265;
// The galactic north pole (RA 192.85948°, Dec 27.12825°) and center (RA 266.405°,
// Dec −28.936°) as unit vectors, as `catalog::galactic_to_equatorial` places them.
const POLE: vec3<f32> = vec3<f32>(-0.86766615, -0.19807637, 0.45598378);
const CENTER: vec3<f32> = vec3<f32>(-0.05487557, -0.87343856, -0.48383236);
// The band's spread in galactic latitude, radians: 8°, as the generator draws it.
const SIGMA: f32 = 0.13962634;

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let at = sky.view_min + in.uv * sky.view_size;
    let ra = (1.0 - at.x / sky.chart.x) * 2.0 * PI;
    let dec = 0.5 * PI - at.y / sky.chart.y * PI;
    let v = vec3<f32>(cos(dec) * cos(ra), cos(dec) * sin(ra), sin(dec));
    let b = asin(clamp(dot(v, POLE), -1.0, 1.0));
    let band = exp(-0.5 * (b / SIGMA) * (b / SIGMA));
    // The band brightens toward the galactic center, as the real one does.
    let core = 0.6 + 0.4 * max(dot(v, CENTER), 0.0);
    let ground = vec3<f32>(0.004, 0.006, 0.014);
    let glow = vec3<f32>(0.020, 0.024, 0.036) * band * core;
    let color = ground + glow;
    let red = vec3<f32>(dot(color, vec3<f32>(0.2126, 0.7152, 0.0722)) * 1.6, 0.0, 0.0);
    return vec4<f32>(mix(color, red, sky.night), 1.0);
}
