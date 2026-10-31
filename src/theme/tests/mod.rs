mod floors;
mod metrics;
mod serialization;
mod state_looks;
mod text_scale;
mod tooltip;

use ron::ser::PrettyConfig;
use serde::Serialize;

use crate::theme::Theme;
use crate::theme::button::ButtonTheme;
use crate::theme::context_menu::ContextMenuTheme;
use crate::theme::context_menu::menu_item::MenuItemTheme;
use crate::theme::drag_value::DragValueTheme;
use crate::theme::look::WidgetLook;
use crate::theme::look::animated_look::AnimatedLook;
use crate::theme::look::stateful_look::StatefulLook;
use crate::theme::modal::ModalTheme;
use crate::theme::palette::Palette;
use crate::theme::progress_bar::ProgressBarTheme;
use crate::theme::scrollbar::ScrollbarTheme;
use crate::theme::separator::SeparatorTheme;
use crate::theme::slider::SliderTheme;
use crate::theme::spinner::SpinnerTheme;
use crate::theme::splitter::SplitterTheme;
use crate::theme::text_edit::TextEditTheme;
use crate::theme::text_style::TextStyle;
use crate::theme::toggle::ToggleTheme;
use crate::theme::tooltip::TooltipTheme;
use ron::ser;
use static_assertions::{assert_impl_all, assert_not_impl_any};

assert_not_impl_any!(Theme: Copy);
assert_not_impl_any!(ButtonTheme: Copy);
assert_not_impl_any!(ContextMenuTheme: Copy);
assert_not_impl_any!(MenuItemTheme: Copy);
assert_not_impl_any!(DragValueTheme: Copy);
assert_not_impl_any!(ModalTheme: Copy);
assert_not_impl_any!(Palette: Copy);
assert_not_impl_any!(ProgressBarTheme: Copy);
assert_not_impl_any!(ScrollbarTheme: Copy);
assert_not_impl_any!(SeparatorTheme: Copy);
assert_not_impl_any!(SliderTheme: Copy);
assert_not_impl_any!(SpinnerTheme: Copy);
assert_not_impl_any!(SplitterTheme: Copy);
assert_not_impl_any!(TextEditTheme: Copy);
assert_not_impl_any!(ToggleTheme: Copy);
assert_not_impl_any!(TooltipTheme: Copy);
assert_not_impl_any!(AnimatedLook: Copy);
assert_not_impl_any!(StatefulLook: Copy);
assert_not_impl_any!(WidgetLook: Copy);

// The one theme type that is `Copy`; it rides per-row and per-widget paths.
assert_impl_all!(TextStyle: Copy);

/// A theme as RON text — what the tests below serialize, edit and compare.
fn pretty<T: Serialize>(value: &T) -> String {
    ser::to_string_pretty(value, PrettyConfig::default()).expect("serialize")
}

/// `each_mut` hands out every state's look once, in the order the fields declare them.
#[test]
fn each_mut_reaches_every_state_in_order() {
    let mut look = StatefulLook::default();
    for (size, state) in [1.0, 2.0, 3.0, 4.0].into_iter().zip(look.each_mut()) {
        state.text.font_size = Some(size);
    }
    assert_eq!(
        [&look.normal, &look.hovered, &look.active, &look.disabled]
            .map(|state| state.text.font_size),
        [Some(1.0), Some(2.0), Some(3.0), Some(4.0)]
    );
}
