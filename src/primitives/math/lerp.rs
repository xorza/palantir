//! [`Lerp`]: a value that blends between two of its kind.

/// A value that blends from `a` to `b`: what a theme's transition and a tween need. A spring needs
/// the arithmetic [`Animatable`](crate::widget::Animatable) adds on top.
///
/// `#[derive(Lerp)]` blends a struct field by field; a type with no blend of its own switches
/// halfway, as `#[animate(snap)]` makes a field do.
pub trait Lerp: Sized {
    /// Interpolate from `a` to `b` at phase `t`, normally `0.0..=1.0`, returning `a` at 0 and
    /// `b` at 1 exactly, so a tween ends on its target. A curve may overshoot, so
    /// implementations must not clamp.
    fn lerp(a: Self, b: Self, t: f32) -> Self;
}
