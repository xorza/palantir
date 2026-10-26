//! Palantir-native input: the host-facing event vocabulary and the per-widget response the record pass reads back. The state machine consuming the events is `input_state`.

#[cfg(feature = "internals")]
pub(crate) mod bench;
pub(crate) mod ime_preedit;
pub(crate) mod input_event;
pub(crate) mod interaction;
pub(crate) mod key_class;
pub(crate) mod keyboard;
pub(crate) mod pointer;
pub(crate) mod policy;
pub(crate) mod scroll_targets;
pub(crate) mod sense;
pub(crate) mod shortcut;
pub(crate) mod watch;
pub(crate) mod zoom_factor;
