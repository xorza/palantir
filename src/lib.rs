// Re-import `palantir` as a self-alias so proc-macros that emit
// `::palantir::Animatable` paths (from `palantir-anim-derive`) resolve
// when the derive is used *inside* the crate (e.g. on `Stroke`,
// `Background`). Outside the crate this path resolves naturally.
extern crate self as palantir;

pub(crate) mod animation;
pub mod clipboard;
pub(crate) mod common;
pub(crate) mod debug_overlay;
pub(crate) mod forest;
pub(crate) mod host;
pub(crate) mod input;
pub(crate) mod layout;
pub(crate) mod primitives;
pub(crate) mod renderer;
pub(crate) mod shape;
#[cfg(any(test, feature = "internals"))]
pub mod support;
pub(crate) mod text;
pub(crate) mod ui;
pub(crate) mod widgets;

pub use animation::animatable::Animatable;
pub use animation::easing::Easing;
pub use animation::{AnimSlot, AnimSpec};
// Same-name re-export: the derive lives in the macro namespace,
// the trait in the type namespace — `use palantir::Animatable;` pulls
// both, and `#[derive(Animatable)]` works alongside `T: Animatable`.
pub use debug_overlay::DebugOverlayConfig;
pub use forest::element::{Configure, Element, LayoutMode};
pub use forest::tree::Layer;
pub use forest::visibility::Visibility;
pub use forest::widget_id::WidgetId;
pub use host::Host;
pub use input::keyboard::{Key, KeyPress, Modifiers, TextChunk};
pub use input::sense::Sense;
pub use input::shortcut::{Mods, Shortcut};
pub use input::{FocusPolicy, InputEvent, PointerButton, ResponseState};
pub use layout::types::align::{Align, HAlign, VAlign};
pub use layout::types::clip_mode::ClipMode;
pub use layout::types::display::Display;
pub use layout::types::grid_cell::GridCell;
pub use layout::types::justify::Justify;
pub use layout::types::sizing::{Sizes, Sizing};
pub use layout::types::track::Track;
pub use palantir_anim_derive::Animatable;
pub use primitives::background::Background;
pub use primitives::brush::{
    Brush, ConicGradient, Interp, LinearGradient, RadialGradient, Spread, Stop,
};
pub use primitives::color::Color;
pub use primitives::color::Srgb8;
pub use primitives::corners::Corners;
pub use primitives::mesh::{Mesh, MeshVertex};
pub use primitives::rect::Rect;
pub use primitives::shadow::Shadow;
pub use primitives::size::Size;
pub use primitives::spacing::Spacing;
pub use primitives::stroke::Stroke;
pub use primitives::transform::TranslateScale;
pub use shape::{LineCap, LineJoin, PolylineColors, Shape, TextWrap};
pub use text::cosmic::CosmicMeasure;
pub use text::{FontFamily, TextShaper};
pub use ui::Ui;
pub use ui::frame_report::FrameReport;
pub use widgets::Response;
pub use widgets::button::Button;
pub use widgets::context_menu::{ContextMenu, ContextMenuResponse, MenuItem};
pub use widgets::frame::Frame;
pub use widgets::grid::Grid;
pub use widgets::panel::Panel;
pub use widgets::popup::{ClickOutside, Popup};
pub use widgets::scroll::Scroll;
pub use widgets::text::Text;
pub use widgets::text_edit::TextEdit;
pub use widgets::theme::{
    AnimatedLook, ButtonTheme, ContextMenuTheme, MenuItemTheme, ScrollbarTheme, TextEditTheme,
    TextStyle, Theme, WidgetLook,
};
