//! Compile-time platform tag.

/// The host family the crate was compiled for, published so widgets branch on [`PLATFORM`] instead of restating `cfg`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Platform {
    /// macOS, where Cmd is the command modifier [`ShortcutMods::ctrl`](crate::ShortcutMods) names.
    Mac,
    /// Windows.
    Windows,
    /// Linux and the other X11 / Wayland targets.
    Linux,
}

/// The platform this build targets; const-evaluable. Anything but macOS and Windows reads as [`Platform::Linux`].
pub const PLATFORM: Platform = {
    #[cfg(target_os = "macos")]
    {
        Platform::Mac
    }
    #[cfg(target_os = "windows")]
    {
        Platform::Windows
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Platform::Linux
    }
};
