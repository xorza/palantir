// A box's fill and border over its edge, a quad's and a material's alike: CSS's border-box
// background (`background-clip: border-box`, the CSS default), weighted by area. With `o` and `i`
// the coverage of the border's outer and inner edge, the band `o − i` takes the border over the
// fill and the rest of the covered side the fill alone, so each part of a pixel counts once, and
// an opaque fill sums to `o` under any border, with no seam at the inner edge. Colors are straight
// in and premultiplied out. Refused: the fill inside the border, which showed what is behind a
// plain box through a translucent border and the fill on a material, one `Paint` two ways; the
// fill at `o` with the border composited over it, which counted the band's share of the fill
// twice: an opaque box and border half over a pixel read 0.75 where they cover 0.5.

// What the band shows: the border over `f`, the premultiplied fill.
fn palantir_border_over(f: vec4<f32>, border_color: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(border_color.rgb * border_color.a, border_color.a) + f * (1.0 - border_color.a);
}

// The box at `d` from its edge: the band, then the fill inside it.
fn palantir_composite(d: f32, fill: vec4<f32>, border_color: vec4<f32>, border_width: f32) -> vec4<f32> {
    let o = palantir_edge_coverage(d);
    let f = vec4<f32>(fill.rgb * fill.a, fill.a);
    if (border_width <= 0.0) {
        return f * o;
    }
    let i = palantir_edge_coverage(d + border_width);
    return palantir_border_over(f, border_color) * (o - i) + f * i;
}

// The outside twin of `palantir_composite`: the band, then the fill outside the box, `1 − o`, and
// a transparent interior.
fn palantir_composite_outside(d: f32, fill: vec4<f32>, border_color: vec4<f32>, border_width: f32) -> vec4<f32> {
    let o = palantir_edge_coverage(d);
    let f = vec4<f32>(fill.rgb * fill.a, fill.a);
    if (border_width <= 0.0) {
        return f * (1.0 - o);
    }
    let i = palantir_edge_coverage(d + border_width);
    return palantir_border_over(f, border_color) * (o - i) + f * (1.0 - o);
}
