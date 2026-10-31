//! Playing a window as a person at it would, from outside: [`Locator`](locator::Locator) names
//! where an action lands by what the window shows. The harness and a live window's remote both
//! read it, so a test, a script and an agent name a widget the same way.

pub(crate) mod action;
pub(crate) mod error;
pub(crate) mod locator;
pub(crate) mod play;

#[cfg(test)]
mod tests;
