//! Compile-time platform tag.

/// The host family the crate was compiled for.
///
/// Published because platform conventions are a widget's business, not
/// only the framework's: which modifier starts word navigation, which
/// chord submits, which corner a menu prefers. A widget outside this
/// crate branches on the same three cases the bundled ones do, and
/// reads them from [`PLATFORM`] rather than restating the `cfg`
/// spelling at every site.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Platform {
    Mac,
    Win,
    Linux,
}

/// The platform this build targets. Prefer it to
/// `cfg!(target_os = "...")` wherever a three-way branch is what the
/// site wants. Const-evaluable, so it works inside a `const fn` body.
/// Everything that is neither macOS nor Windows reads as
/// [`Platform::Linux`].
pub const PLATFORM: Platform = {
    #[cfg(target_os = "macos")]
    {
        Platform::Mac
    }
    #[cfg(target_os = "windows")]
    {
        Platform::Win
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Platform::Linux
    }
};
