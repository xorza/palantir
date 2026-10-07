//! Cross-cutting utilities. Submodules are `pub(crate)` at `crate::common::<sub>::<item>`, except [`flag_set`], which holds a macro reached textually.

pub(crate) mod app_setting;
pub(crate) mod block_arena;
pub(crate) mod clipboard;
pub(crate) mod content_hash;
pub(crate) mod counters;
pub(crate) mod expiry_wheel;
#[macro_use]
pub(crate) mod flag_set;
pub(crate) mod hash;
pub(crate) mod id_counter;
pub(crate) mod index16;
pub(crate) mod platform;
pub(crate) mod span;
pub(crate) mod time;
pub(crate) mod tracy;
pub(crate) mod typed_stores;
