//! Compile-time platform tag. Use `PLATFORM` (an enum) instead of
//! `cfg!(target_os = "...")` / `#[cfg(target_os = "...")]` at sites
//! that just need a three-way branch. Const-evaluable, so it works
//! inside `const fn` bodies.

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
// `PLATFORM` is cfg-selected, so on any one target the other two
// variants are never constructed. That is the point of the type.
#[allow(dead_code)]
pub(crate) enum Platform {
    Mac,
    Win,
    Linux,
}

#[cfg(target_os = "macos")]
pub(crate) const PLATFORM: Platform = Platform::Mac;

#[cfg(target_os = "windows")]
pub(crate) const PLATFORM: Platform = Platform::Win;

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(crate) const PLATFORM: Platform = Platform::Linux;
