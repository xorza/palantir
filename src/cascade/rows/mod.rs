//! The row tables the cascade walk produces and `input` consumes: per-node
//! [`EntryRow`](entry_row::EntryRow)s for response lookup, the interactive-only
//! [`HitRow`](hit_row::HitRow) table, and the declared scopes, Tab stops, arrow
//! groups and roots focus and key routing resolve against.

pub(crate) mod arrow_group_row;
pub(crate) mod entry_row;
pub(crate) mod hit_row;
pub(crate) mod root_row;
pub(crate) mod scope_row;
pub(crate) mod tab_stop_row;
