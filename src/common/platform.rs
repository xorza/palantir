//! The input-convention selector.

/// Whose input conventions apply: which key is the command modifier, how a shortcut is labeled.
/// Public so an app labels a command as the host does, such as "⌘S" against "Ctrl+S"; widgets read
/// input through [`Modifiers`](crate::Modifiers) and [`Shortcut`](crate::Shortcut), which
/// apply it already.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Platform {
    /// macOS, where Cmd is the command modifier [`ShortcutMods::CTRL`](crate::ShortcutMods) names.
    Mac,
    /// Windows.
    Windows,
    /// Linux and the other X11 / Wayland targets.
    Linux,
}

/// Each input convention that differs between platforms, named for what it does, so a caller
/// reads why it branches.
impl Platform {
    /// Whether Super (Cmd) is the command modifier, which a host reports as `Modifiers::CTRL`, leaving
    /// Control a key of its own: macOS.
    pub(crate) const fn commands_with_super(self) -> bool {
        matches!(self, Self::Mac)
    }

    /// Whether the system turns a vertical wheel with Shift held into a horizontal scroll itself:
    /// macOS. Elsewhere the host swaps the axes.
    #[cfg_attr(
        not(feature = "winit"),
        expect(dead_code, reason = "only the winit host translates a wheel")
    )]
    pub(crate) const fn turns_shift_wheel(self) -> bool {
        matches!(self, Self::Mac)
    }

    /// Whether Option composes text, so only Cmd and Control command: macOS. Elsewhere Ctrl+Alt
    /// is AltGr, which composes, and Ctrl or Alt alone commands.
    pub(crate) const fn option_composes(self) -> bool {
        matches!(self, Self::Mac)
    }

    /// Whether a shortcut reads as symbols in macOS order, `⌥⇧⌘` and the key, rather than
    /// names joined by `+`.
    pub(crate) const fn labels_with_symbols(self) -> bool {
        matches!(self, Self::Mac)
    }

    /// What a shortcut label calls the Super key where it names it.
    pub(crate) const fn super_label(self) -> &'static str {
        match self {
            Self::Windows => "Win",
            Self::Mac | Self::Linux => "Super",
        }
    }
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
