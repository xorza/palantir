//! Compile-time platform tag. Use `PLATFORM` (an enum) instead of
//! `cfg!(target_os = "...")` / `#[cfg(target_os = "...")]` at sites
//! that just need a three-way branch. Const-evaluable, so it works
//! inside `const fn` bodies.

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Platform {
    // `PLATFORM` is cfg-selected, so a build constructs exactly one of these and
    // matches against the rest. Each variant carries its own gate rather than the
    // enum a blanket one, so a variant that goes dead on the target it names still
    // gets reported.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    Mac,
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    Win,
    #[cfg_attr(any(target_os = "macos", target_os = "windows"), allow(dead_code))]
    Linux,
}

#[cfg(target_os = "macos")]
pub(crate) const PLATFORM: Platform = Platform::Mac;

#[cfg(target_os = "windows")]
pub(crate) const PLATFORM: Platform = Platform::Win;

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(crate) const PLATFORM: Platform = Platform::Linux;
