//! Palantir-native input: the host-facing event vocabulary, the state machine consuming it, and the per-widget response the record pass reads back.

#[cfg(feature = "bench")]
pub(crate) mod bench;
pub(crate) mod capture;
pub(crate) mod event_outcome;
pub(crate) mod ime_preedit;
pub(crate) mod input_event;
pub(crate) mod input_queue;
pub(crate) mod input_state;
pub(crate) mod interaction;
pub(crate) mod key_class;
pub(crate) mod keyboard;
pub(crate) mod pointer;
pub(crate) mod policy;
pub(crate) mod scope;
pub(crate) mod scroll_targets;
pub(crate) mod sense;
pub(crate) mod shortcut;
pub(crate) mod target_scroll_delta;
pub(crate) mod watch;
pub(crate) mod zoom_factor;
