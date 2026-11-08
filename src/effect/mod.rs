//! App-written effects: an effect is a WGSL function palantir wraps in its own fragment stage, so
//! it stays correct under partial repaint and clips. This module holds its name, its parameters,
//! its source and the registry that loads it. Decisions: `.notes/effects-proposal.md` until the
//! proposal lands.

pub(crate) mod contract;
pub(crate) mod effect_kind;
pub(crate) mod effect_name;
pub(crate) mod effect_params;
pub(crate) mod effect_registry;
pub(crate) mod effect_source;
pub(crate) mod error;

/// The most bytes an instance's parameters hold, so one storage buffer row serves any effect.
pub(crate) const MAX_PARAMS_SIZE: usize = 256;

/// The most image slots an effect binds, as GL and older Metal tiers lack a bindless array.
pub(crate) const MAX_IMAGE_SLOTS: u8 = 4;

#[cfg(test)]
mod tests;
