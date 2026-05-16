// Default palette: Ayu Mirage High Contrast. Mirrors
// `assets/reference-palette.toml` — that file is the hand-edited source
// of truth; these consts are the compile-time copy used to build the
// framework defaults. Keep in sync when the palette changes.

use crate::primitives::color::Color;

// backgrounds
pub(crate) const TERMINAL_BG: Color = Color::hex(0x1a1a1a);
pub(crate) const ELEM: Color = Color::hex(0x343434);
pub(crate) const ELEM_HOVER: Color = Color::hex(0x3e3e3e);
pub(crate) const ELEM_ACTIVE: Color = Color::hex(0x4b4b4b);
// borders
pub(crate) const BORDER_FOCUSED: Color = Color::hex(0x105577);
// text
pub(crate) const TEXT: Color = Color::hex(0xffffff);
pub(crate) const TEXT_MUTED: Color = Color::hex(0xaaaaa8);
pub(crate) const TEXT_DISABLED: Color = Color::hex(0x878a8d);
// accent
pub(crate) const ACCENT: Color = Color::hex(0x9adbfb);
