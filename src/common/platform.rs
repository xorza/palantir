//! The input-convention selector.

/// Whose input conventions apply: which key is the command modifier, how a shortcut is labeled.
/// Public so an app labels a command as the host does, such as "⌘S" against "Ctrl+S"; widgets read
/// input through [`Modifiers`](crate::Modifiers) and [`Shortcut`](crate::Shortcut), which
/// apply it already.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Platform {
    /// macOS, where Cmd is the command modifier [`ShortcutMods::ctrl`](crate::ShortcutMods) names.
    Mac,
    /// Windows.
    Windows,
    /// Linux and the other X11 / Wayland targets.
    Linux,
}

/// The conventions of the platform this build targets; const-evaluable. Anything but macOS and
/// Windows reads as [`Platform::Linux`].
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
