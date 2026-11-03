//! Why a number is no [`SystemScale`].

use std::error;
use std::fmt::{self, Display, Formatter};

use crate::display::system_scale::SystemScale;

/// A scale outside [`SystemScale::MIN`] ..= [`SystemScale::MAX`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SystemScaleError {
    /// The factor refused.
    pub found: f32,
}

impl Display for SystemScaleError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "a system scale must be from {:e} to {:e}, not {}",
            SystemScale::MIN,
            SystemScale::MAX,
            self.found
        )
    }
}

impl error::Error for SystemScaleError {}
