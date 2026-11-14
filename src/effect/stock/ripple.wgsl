// Material's flat ripple, as Compose's `RippleAnimation` draws it: a disc that grows over `enter_seconds`
// from 30 % of the rect's longer side to 10 px past the circle through its corners, along
// FastOutSlowIn, while its center runs linearly from the press to the rect's center. It fades in
// over the first third of `enter_seconds`, is whole from the release, and fades out over `exit_seconds`.

fn effect(input: EffectInput, params: Params) -> EffectOutput {
    let grown = press_through(input.time, params.enter_seconds);
    let start_radius = max(input.size.x, input.size.y) * 0.3;
    let end_radius = length(input.size) * 0.5 + 10.0;
    let radius = mix(start_radius, end_radius, press_fast_out_slow_in(grown));
    let center = mix(params.point, input.size * 0.5, grown);
    let released = params.released_seconds >= 0.0 && input.time >= params.released_seconds;
    let fade_in = select(press_through(input.time, params.enter_seconds / 3.0), 1.0, released);
    let alpha = fade_in * (1.0 - press_fade(params, input.time));
    let disc = saturate((radius - distance(input.pos, center)) * input.scale + 0.5);
    let a = params.color.a * alpha * disc;
    return effect_over(vec4<f32>(params.color.rgb * a, a));
}
