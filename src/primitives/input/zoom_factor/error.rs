//! Why a number is no [`ZoomFactor`](crate::ZoomFactor).

use std::error;
use std::fmt::{self, Display, Formatter};

/// A factor that is not finite or is below [`f32::MIN_POSITIVE`], whose inverse is not finite.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ZoomFactorError {
    /// The factor refused.
    pub found: f32,
}

impl Display for ZoomFactorError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "a zoom factor must be finite and above 0, not {}",
            self.found
        )
    }
}

impl error::Error for ZoomFactorError {}
