# palantir-derive

Derive macros for [Palantir](https://github.com/xorza/palantir), an immediate-mode GUI library
for Rust: `#[derive(Lerp)]` and `#[derive(Animatable)]` for themes and motion, and
`#[derive(EffectParams)]` for an effect's parameter struct.

This crate is an implementation detail of `palantir` and is not meant to be
depended on directly — `palantir` re-exports each derive next to its trait, so
`use palantir::widget::{Animatable, Lerp};` pulls in all four:

```rust,ignore
use palantir::widget::{Animatable, Lerp};

#[derive(Clone, Debug, Default, PartialEq, Lerp, Animatable)]
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

`Lerp` blends each named field of a struct through its own `Lerp`, and a field
marked `#[animate(snap)]` switches halfway. `Animatable` adds the spring
arithmetic: animated fields call into their own `Animatable` impl, and fields
marked `#[animate(snap)]` or `#[animate(lerp_only)]` are excluded from it. Both
apply to structs with named fields only.

`EffectParams` describes a `#[repr(C)]` struct field by field, with each field's name, offset
and WGSL type, which `Ui::load_effect` checks against the effect's WGSL parameter struct.

`#[derive(TextSlots)]` is palantir's own: a theme part's text walk, which
visits every field through the field type's own walk.

## License

Apache-2.0 or MIT, at your option — see
[the Palantir README](https://github.com/xorza/palantir#license).
