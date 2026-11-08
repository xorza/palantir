//! [`PressInput`]: what a press effect reads of the press it draws.

use glam::Vec2;

use crate::primitives::paint::color::RgbaF32;

/// The parameters of a press effect, an `EffectName<PressInput>` a theme's press slot names
/// ([`PressEffect`](crate::PressEffect)), which palantir fills for each press of a widget. Its
/// effect's time is the seconds since the press: it runs to `enter` and holds there while the
/// press is held, and after the release runs on to `max(released, enter) + exit`, so a press
/// always grows in whole before it fades, as Compose's `RippleAnimation` and Android's
/// `RippleAnimationSession` wait for the enter before the exit. The WGSL declares, field for field:
///
/// ```wgsl
/// struct Params {
///     color: vec4<f32>,
///     highlight: vec4<f32>,
///     point: vec2<f32>,
///     released: f32,
///     enter: f32,
///     exit: f32,
///     seed: f32,
///     pad0: f32,
///     pad1: f32,
/// }
/// ```
///
/// The pads hold the struct to WGSL's 16-byte stride, which its `vec4` sets.
#[derive(
    Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable, palantir_derive::EffectParams,
)]
#[repr(C)]
pub struct PressInput {
    /// The theme's color, linear and straight.
    pub color: RgbaF32,
    /// The theme's second color, linear and straight
    /// ([`PressEffect::highlight`](crate::PressEffect::highlight)).
    pub highlight: RgbaF32,
    /// Where the press landed, logical px from the widget's top-left; its center for a press of a
    /// key.
    pub point: Vec2,
    /// Seconds from the press to its release; −1 while it is held.
    pub released: f32,
    /// Seconds the press takes to grow in.
    pub enter: f32,
    /// Seconds it takes to fade out after the release.
    pub exit: f32,
    /// A number of the press's own, which differs between presses, for an effect's noise: the
    /// seconds of the press on the window's clock, modulo 64.
    pub seed: f32,
    /// Padding to WGSL's 16-byte stride; palantir writes 0.
    pub pad0: f32,
    /// Padding to WGSL's 16-byte stride; palantir writes 0.
    pub pad1: f32,
}
