//! App-written effects: an effect is a WGSL function palantir wraps in its own fragment stage, so
//! it stays correct under partial repaint and clips. This module holds its name, its parameters,
//! its source, the registry that loads it and palantir's stock effects. Facts and sources:
//! `.notes/research/2026-11-08-custom-shaders.md`.
//!
//! # Decisions
//!
//! - **Two kinds, each with its own signature.** A color effect reads its position, size, corners,
//!   time, parameters and images; a backdrop effect also reads what is painted under it, through
//!   the capture a [`Material`](crate::Material) takes
//!   ([`EffectKind`](crate::EffectKind)). Deferred: an effect that reads its own subtree (SwiftUI's `layerEffect`), which needs an
//!   offscreen pass per node; a stateful effect, which a [`GpuPaint`](crate::GpuPaint) serves.
//! - **A name nothing loaded is a logic error.** A shape that names one panics with the name
//!   (`EffectRegistry::resolve`); a theme that names one is refused at its set
//!   ([`ThemeEffectError`](crate::ThemeEffectError)), as a theme file is untrusted data. Refused:
//!   painting nothing, which is quietly wrong; a fallback as a [`FontFamily`](crate::FontFamily)
//!   has, which no effect has an equal of.

pub(crate) mod contract;
pub(crate) mod effect_module;
pub(crate) mod effect_registry;
pub(crate) mod effect_source;
pub(crate) mod error;
pub(crate) mod press_params;
pub(crate) mod stock;

/// The most bytes an instance's parameters hold, so one storage buffer row serves any effect.
pub(crate) const MAX_PARAMS_SIZE: usize = 256;

/// The most image slots an effect binds, as GL and older Metal tiers lack a bindless array.
pub(crate) const MAX_IMAGE_SLOTS: u8 = 4;

#[cfg(test)]
mod tests;
