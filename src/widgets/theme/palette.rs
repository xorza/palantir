// Default palette: neutral dark grayscale surfaces with a single blue
// accent. Compile-time constants used to build the framework's default
// theme; apps restyle via `Theme`, not by touching these.

use crate::primitives::color::Color;

// backgrounds
pub(crate) const TERMINAL_BG: Color = Color::hex(0x1a1a1a);
pub(crate) const ELEM: Color = Color::hex(0x343434);
pub(crate) const ELEM_HOVER: Color = Color::hex(0x3e3e3e);
pub(crate) const ELEM_ACTIVE: Color = Color::hex(0x4b4b4b);
// borders — TEXT_MUTED tints, not grays: the raw surface grays sit too
// close to ELEM/ELEM_HOVER to read as edges at 1 px.
pub(crate) const BORDER_SOFT: Color = TEXT_MUTED.with_alpha(0.18);
pub(crate) const BORDER_MID: Color = TEXT_MUTED.with_alpha(0.22);
pub(crate) const BORDER_STRONG: Color = TEXT_MUTED.with_alpha(0.35);
pub(crate) const BORDER_FOCUSED: Color = Color::hex(0x105577);
// text
pub(crate) const TEXT: Color = Color::hex(0xffffff);
pub(crate) const TEXT_MUTED: Color = Color::hex(0xaaaaa8);
pub(crate) const TEXT_DISABLED: Color = Color::hex(0x878a8d);
// accent
pub(crate) const ACCENT: Color = Color::hex(0x9adbfb);
