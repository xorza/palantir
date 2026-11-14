//! The rule in `Response`'s doc, held over every widget's response: no wrapper derefs to it.

use crate::widget_core::inner_response::InnerResponse;
use crate::widget_core::value_response::ValueResponse;
use crate::widgets::editable_label::EditableLabelResponse;
use crate::widgets::expander::ExpanderResponse;
use crate::widgets::tabs::tab_strip::TabStripResponse;
use crate::widgets::tabs::tabbed_view::TabbedViewResponse;
use crate::widgets::text_edit::TextEditResponse;
use static_assertions::assert_not_impl_any;
use std::ops::Deref;

assert_not_impl_any!(InnerResponse<()>: Deref);
assert_not_impl_any!(ValueResponse: Deref);
assert_not_impl_any!(TextEditResponse: Deref);
assert_not_impl_any!(ExpanderResponse<()>: Deref);
assert_not_impl_any!(EditableLabelResponse: Deref);
assert_not_impl_any!(TabStripResponse: Deref);
assert_not_impl_any!(TabbedViewResponse<()>: Deref);
