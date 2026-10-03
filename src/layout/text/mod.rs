//! The layout side of text: the runs a node records, as the measure pass
//! reads them, shapes them, and hands them back in record order.

pub(crate) mod shaped_text;
pub(crate) mod text_runs;
pub(crate) mod text_shape_input;
