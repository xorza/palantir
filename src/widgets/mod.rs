//! The bundled widgets: one builder type per widget, each recording a
//! `Node` and its chrome into the frame.
//!
//! `clippy::new_without_default` is allowed module-wide. Every widget
//! constructor is `#[track_caller]` — the call site is what mints the
//! widget id — and `#[derive(Default)]` cannot capture one. A hand-written
//! `Default` could, but it would only be a second name for `new()` on a
//! builder that is constructed and consumed in one expression. The answer
//! is the same for every widget, so it is decided once here.
#![allow(clippy::new_without_default)]

/// Implement the `background` builder for a container widget that keeps
/// its override in a field called `chrome: Option<Background>`.
///
/// Eight widgets offer one and every body is the same assignment over
/// the same field. What differs is the *resolution* — which theme slot an
/// unset background falls back to, or whether one exists at all. Invoke
/// it **in the widget's own file**, next to the type.
///
/// `note` is required, and carries that resolution. Unlike
/// `style_setter!` — which derives its slot path from its own arguments,
/// so a note there only adds to it — this macro has nothing to derive a
/// fallback rule from: prose is the rule's only home, and one shared
/// default sentence would quietly stand in for eight different answers.
///
/// Only for the plain case: `Grid` carries generic parameters the macro
/// can't take, and writes the builder by hand — same exemption
/// `impl_configure!` below has for the same type.
macro_rules! impl_background {
    ($ty:ty, $note:expr $(,)?) => {
        impl $ty {
            /// Paint `bg` as this widget's background.
            #[doc = ""]
            #[doc = $note]
            pub fn background(mut self, bg: $crate::primitives::background::Background) -> Self {
                self.chrome = Some(bg);
                self
            }
        }
    };
}

/// Declare the `label` builder for a widget keeping its text in a field
/// called `label: TextInput<'lt>`.
///
/// `Button` and the three toggles take a label the same way, over the
/// same field, through the same conversion. Invoke it inside the
/// builder's own `impl` block, next to the other setters.
///
/// `note` is required and says where the text lands — inside the chip for
/// `Button`, beside the box for the toggles. That is per-widget and lives
/// nowhere else, so it is spelled at the invocation for the same reason
/// `impl_background!` demands its own note.
///
/// Expanded into the caller's block rather than emitting an `impl` of its
/// own, which is what lets it reach `RadioButton<'a, T>`: a macro
/// spelling the header itself would have to name the generic parameters,
/// and that is precisely what exempts that type from `impl_configure!`.
macro_rules! label_setter {
    ($lt:lifetime, $note:expr $(,)?) => {
        /// The text this widget draws. Empty (the default) draws none —
        /// no text child is recorded at all.
        #[doc = ""]
        #[doc = $note]
        pub fn label(
            mut self,
            label: impl Into<$crate::primitives::text_input::TextInput<$lt>>,
        ) -> Self {
            self.label = label.into();
            self
        }
    };
}

/// Declare a themed widget's per-instance style override, and the
/// resolution that pairs with it, from **one** naming of its theme slot.
///
/// Invoke it inside the builder's own `impl` block, next to the other
/// setters, for any widget keeping its override in a field called
/// `style: Option<&'lt T>`. It expands to two methods:
///
/// - `style(…)` — the public setter. Takes anything that converts into
///   `Option<&T>`, so both `.style(&theme)` and `.style(maybe_theme)`
///   compile: "styled or default" is expressible as *data* rather than as a
///   branch around the whole call.
/// - `slot(&self, theme)` — private: the caller's override, or the named
///   slot off the app theme.
///
/// The slot path is written exactly once, in the invocation. Naming it twice
/// — once to copy geometry scalars out before the `&mut Ui` reborrow, once in
/// a fallback closure handed to the look resolver — makes a typo in one half
/// of the pair silent.
macro_rules! style_setter {
    ($lt:lifetime, $theme:ty, $($slot:ident).+ $(, $note:expr)* $(,)?) => {
        #[doc = concat!(
            "Per-instance theme override, replacing [`crate::Theme`]'s `",
            stringify!($($slot).+),
            "` for this instance alone.",
        )]
        ///
        /// Takes an `Option` as readily as a reference, so a caller that
        /// styles some instances and not others passes the `Option` itself
        /// instead of branching: `.style(overrides.as_ref())`.
        $(
            #[doc = ""]
            #[doc = $note]
        )*
        pub fn style(mut self, s: impl Into<Option<&$lt $theme>>) -> Self {
            self.style = s.into();
            self
        }

        /// This instance's theme — the caller's override, or the slot off
        /// `theme`. The one place this widget names its slot.
        #[inline(always)]
        fn slot<'slot>(&self, theme: &'slot $crate::widgets::theme::Theme) -> &'slot $theme
        where
            $lt: 'slot,
        {
            self.style.unwrap_or(&theme.$($slot).+)
        }
    };
}

/// Implement [`Configure`](crate::scene::node::configure::Configure) for widget
/// builders that keep their [`Node`](crate::scene::node::Node) in a
/// field called `node`.
///
/// The trait has one required method and every widget's answer is the
/// same expression over the same field, so spelling the block out per
/// widget is pure noise. Invoke it **in the widget's own file**, next to
/// the type — the impl still lives where the struct does.
///
/// A widget generic over more than its own lifetimes names those
/// parameters first, with any bounds its struct declares:
/// `impl_configure!(<S> ComboBox<'_, S>)`,
/// `impl_configure!(<T: PartialEq> RadioButton<'_, T>)`.
///
/// One widget writes the impl by hand instead: `ContextMenu` has no `node`
/// of its own — it forwards to the `Popup` it wraps, which is delegation
/// the macro's fixed `self.node` body can't express.
macro_rules! impl_configure {
    (<$($param:ident $(: $bound:path)?),*> $ty:ty) => {
        impl<$($param $(: $bound)?,)*> $crate::scene::node::configure::Configure for $ty {
            #[inline]
            fn node_mut(&mut self) -> $crate::scene::node::configure::ConfigureNode<'_> {
                $crate::scene::node::configure::Configure::node_mut(&mut self.node)
            }
        }
    };
    ($ty:ty) => {
        impl_configure!(<> $ty);
    };
}

pub(crate) mod axis_keys;
pub(crate) mod button;
pub(crate) mod checkbox;
pub(crate) mod checkerboard;
pub(crate) mod close_handle;
pub(crate) mod color_button;
pub(crate) mod color_field;
pub(crate) mod color_picker;
pub(crate) mod color_strip;
pub(crate) mod color_surface;
pub(crate) mod color_swatch;
pub(crate) mod combo_box;
pub(crate) mod context_menu;
pub(crate) mod dock;
pub(crate) mod drag_num;
pub(crate) mod drag_value;
pub(crate) mod expander;
pub(crate) mod frame;
pub(crate) mod gpu_view;
pub(crate) mod grid;
pub(crate) mod modal;
pub(crate) mod overlay_response;
mod overlay_scope;
pub(crate) mod panel;
pub(crate) mod popup;
pub(crate) mod progress_bar;
pub(crate) mod radio;
pub(crate) mod response;
pub(crate) mod scroll;
pub(crate) mod select_response;
pub(crate) mod separator;
pub(crate) mod slider;
pub(crate) mod spinner;
pub(crate) mod splitter;
pub(crate) mod switch;
pub(crate) mod tabs;
pub(crate) mod text;
pub(crate) mod text_edit;
pub(crate) mod theme;
pub(crate) mod toggle_chrome;
pub(crate) mod tooltip;
pub(crate) mod value_response;
pub(crate) mod widget;
