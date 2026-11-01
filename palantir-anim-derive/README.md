# palantir-anim-derive

`#[derive(Animatable)]` for [Palantir](https://github.com/xorza/palantir), an
immediate-mode GUI library for Rust.

This crate is an implementation detail of `palantir` and is not meant to be
depended on directly — `palantir` re-exports the derive next to the trait, so
`use palantir::widget::Animatable;` pulls in both:

```rust,ignore
use palantir::widget::Animatable;

#[derive(Clone, Debug, Default, PartialEq, Animatable)]
struct Look {
    color: Color,
    // Blends in a lerp; motion takes the target at once.
    #[animate(lerp_only)]
    font_size: f32,
    // No blend: a lerp switches at t = 0.5; motion takes the target at once.
    #[animate(snap)]
    italic: bool,
}
```

The derive walks each named field of a struct: animated fields call into their
own `Animatable` impl, and fields marked `#[animate(snap)]` or
`#[animate(lerp_only)]` are excluded from the motion arithmetic. It only
applies to structs with named fields.

## License

Apache-2.0 or MIT, at your option — see
[the Palantir README](https://github.com/xorza/palantir#license).
