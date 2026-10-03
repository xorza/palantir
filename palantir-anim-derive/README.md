# palantir-anim-derive

`#[derive(Animatable)]` for [Palantir](https://github.com/xorza/palantir), an
immediate-mode GUI library for Rust.

This crate is an implementation detail of `palantir` and is not meant to be
depended on directly — `palantir` re-exports the derive next to the trait, so
`use palantir::Animatable;` pulls in both:

```rust,ignore
use palantir::Animatable;

#[derive(Clone, Debug, Default, PartialEq, Animatable)]
struct Look {
    color: Color,
    // Non-animated: lerp jumps to the target, spring math noops on it,
    // and it contributes nothing to the magnitude.
    #[animate(snap)]
    font_size_px: f32,
}
```

The derive walks each named field of a struct: animated fields call into their
own `Animatable` impl, and fields marked `#[animate(snap)]` are excluded from
the arithmetic. It only applies to structs with named fields.

## License

Apache-2.0 or MIT, at your option — see
[the Palantir README](https://github.com/xorza/palantir#license).
