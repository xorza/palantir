struct Viewport {
    size: vec2<f32>,
};

@group(0) @binding(0) var<uniform> viewport: Viewport;

struct VertexOut {
    @builtin(position) clip:         vec4<f32>,
    @location(0)       local:        vec2<f32>,
    @location(1)       size:         vec2<f32>,
    @location(2)       fill:         vec4<f32>,
    @location(3)       radius:       vec4<f32>,
    @location(4)       stroke_color: vec4<f32>,
    @location(5)       stroke_width: f32,
};

@vertex
fn vs(
    @builtin(vertex_index) vi: u32,
    @location(0) pos:          vec2<f32>,
    @location(1) size:         vec2<f32>,
    @location(2) fill:         vec4<f32>,
    @location(3) radius:       vec4<f32>,
    @location(4) stroke_color: vec4<f32>,
    @location(5) stroke_width: f32,
) -> VertexOut {
    var corners = array<vec2<f32>, 4>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 1.0),
    );
    let c = corners[vi];
    let pixel = pos + c * size;
    let clip = vec2<f32>(
        pixel.x / viewport.size.x * 2.0 - 1.0,
        1.0 - pixel.y / viewport.size.y * 2.0,
    );

    var out: VertexOut;
    out.clip         = vec4<f32>(clip, 0.0, 1.0);
    out.local        = c * size;
    out.size         = size;
    out.fill         = fill;
    out.radius       = radius;
    out.stroke_color = stroke_color;
    out.stroke_width = stroke_width;
    return out;
}

// Per-corner SDF rounded rect. radius = (tl, tr, br, bl).
fn sdf_rounded_rect(p: vec2<f32>, size: vec2<f32>, radius: vec4<f32>) -> f32 {
    let half = size * 0.5;
    var r = radius.x;
    if (p.x > half.x) {
        if (p.y > half.y) { r = radius.z; } else { r = radius.y; }
    } else if (p.y > half.y) {
        r = radius.w;
    }
    let q = abs(p - half) - (half - vec2<f32>(r));
    return min(max(q.x, q.y), 0.0) + length(max(q, vec2<f32>(0.0))) - r;
}

@fragment
fn fs(in: VertexOut) -> @location(0) vec4<f32> {
    let d = sdf_rounded_rect(in.local, in.size, in.radius);
    let outer_aa = clamp(0.5 - d, 0.0, 1.0);

    if (in.stroke_width > 0.0) {
        // Stroke sits on the inner edge: fill region is everything inside the rect
        // shifted inward by stroke_width.
        let inner_d  = d + in.stroke_width;
        let inner_aa = clamp(0.5 - inner_d, 0.0, 1.0);
        let stroke_a = (outer_aa - inner_aa) * in.stroke_color.a;
        let fill_a   = inner_aa * in.fill.a;
        let rgb = in.fill.rgb * fill_a + in.stroke_color.rgb * stroke_a;
        let a   = fill_a + stroke_a - fill_a * stroke_a;
        return vec4<f32>(rgb, a);
    }
    let a = in.fill.a * outer_aa;
    return vec4<f32>(in.fill.rgb * a, a);
}
