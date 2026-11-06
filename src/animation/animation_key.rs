//! [`AnimationKey`]: the cause of an animated target.

use crate::common::content_hash::ContentHash;
use std::hash::Hash;

/// What an animated target follows from, as a widget's state. A new target under the same key
/// means its inputs moved, as during a theme transition, and the value follows it at once while
/// a running motion keeps its offset and clock; a new key starts a motion. A request with no key
/// treats every new target as a new motion. As UIKit's additive animations add a new animation's
/// delta to the model value (WWDC 2014, session 236). A type of its own, so the compiler refuses a
/// tab's key here.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AnimationKey(ContentHash);

impl AnimationKey {
    /// The key of `cause`, by its hash: equal causes give equal keys.
    pub fn of(cause: impl Hash) -> Self {
        Self(ContentHash::of(cause))
    }
}
