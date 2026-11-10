// A shape's turn, which palantir's stages and an effect's text alike turn a frame by.

// `p` turned `rotation` radians about `center`. Exact at no turn, so an unturned shape draws
// where it would without one.
fn palantir_turned(p: vec2<f32>, center: vec2<f32>, rotation: f32) -> vec2<f32> {
    if (rotation == 0.0) {
        return p;
    }
    let cs = vec2<f32>(cos(rotation), sin(rotation));
    let v = p - center;
    return center + vec2<f32>(cs.x * v.x - cs.y * v.y, cs.y * v.x + cs.x * v.y);
}
