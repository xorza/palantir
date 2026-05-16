// Re-import `palantir` as a self-alias so proc-macros that emit
// `::palantir::Animatable` paths (from `palantir-anim-derive`) resolve
// when the derive is used *inside* the crate (e.g. on `Stroke`,
// `Background`). Outside the crate this path resolves naturally.
#![allow(private_interfaces, private_bounds)]
//! Most parent modules are `pub` so that gated `test_support` submodules
//! (`#[cfg(any(test, feature = "internals"))] pub mod test_support`) are
//! reachable from external benches / integration tests as
//! `palantir::foo::bar::test_support::*`. Many items inside those parents
//! stay `pub(crate)`; a `pub` `test_support` fn signature may name a
//! `pub(crate)` type, but external callers can't instantiate / name it
//! on their side, so the leak is nominal.

extern crate self as palantir;

// Top-level modules are `pub` so that gated `test_support` submodules
// inside them (`#[cfg(any(test, feature = "internals"))] pub mod
// test_support`) are reachable from external benches / integration
// tests as `palantir::foo::bar::test_support::*`. Items inside that
// aren't `pub` remain unreachable to external consumers.
pub mod animation;
pub mod clipboard;
pub(crate) mod common;
pub(crate) mod debug_overlay;
pub mod forest;
pub mod host;
pub mod input;
pub mod layout;
pub mod primitives;
pub mod renderer;
pub(crate) mod shape;
pub mod text;
pub mod ui;
pub mod widgets;

pub use common::frame_arena::FrameArena;

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
pub use host::{FramePresent, Host};
pub use input::keyboard::{Key, KeyPress, KeyboardEvent, Modifiers, TextChunk};
pub use input::pointer::{PointerButton, PointerEvent};
pub use input::policy::InputPolicy;
pub use input::sense::Sense;
pub use input::shortcut::{Mods, Shortcut};
pub use input::subscriptions::{KeyboardSense, PointerSense};
pub use input::{FocusPolicy, InputDelta, InputEvent, ResponseState};
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
pub use primitives::color::ColorU8;
pub use primitives::corners::Corners;
pub use primitives::image::{Image, ImageFit, ImageHandle, ImageRegistry};
pub use primitives::interned_str::InternedStr;
pub use primitives::mesh::{Mesh, MeshVertex};
pub use primitives::rect::Rect;
pub use primitives::shadow::Shadow;
pub use primitives::size::Size;
pub use primitives::spacing::Spacing;
pub use primitives::span::Span;
pub use primitives::stroke::Stroke;
pub use primitives::transform::TranslateScale;
pub use primitives::widget_id::WidgetId;
pub use renderer::DEFAULT_IMAGE_BUDGET_BYTES;
pub use renderer::caches::RenderCaches;
pub use shape::{LineCap, LineJoin, PolylineColors, Shape, TextWrap};
pub use text::cosmic::CosmicMeasure;
pub use text::{FontFamily, TextShaper};
pub use ui::FrameStamp;
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
    TextStyle, Theme, TooltipTheme, WidgetLook,
};
pub use widgets::tooltip::Tooltip;

#[cfg(test)]
mod hot_struct_sizes {
    use crate::forest::element::{BoundsExtras, LayoutCore, NodeFlags, PanelExtras};
    use crate::forest::node::NodeRecord;
    use crate::forest::rollups::{CascadeInputHash, NodeHash};
    use crate::forest::shapes::record::{ChromeRow, ShapeRecord};
    use crate::forest::tree::ExtrasIdx;
    use crate::layout::ShapedText;
    use crate::primitives::span::Span;
    use crate::renderer::frontend::cmd_buffer::{
        DrawMeshPayload, DrawPolylinePayload, DrawRectPayload, DrawTextPayload,
    };
    use crate::renderer::quad::Quad;
    use crate::ui::cascade::{Cascade, HitEntry};
    use crate::ui::damage::region::DamageRegion;

    fn row<T>(name: &str) -> (String, usize, usize) {
        (name.to_string(), size_of::<T>(), align_of::<T>())
    }

    /// `cargo test --lib print_hot_struct_sizes -- --nocapture --ignored`
    #[test]
    #[ignore = "print-only"]
    fn print_hot_struct_sizes() {
        let rows = [
            row::<NodeRecord>("forest::NodeRecord"),
            row::<LayoutCore>("forest::LayoutCore"),
            row::<NodeFlags>("forest::NodeFlags"),
            row::<ExtrasIdx>("forest::ExtrasIdx"),
            row::<BoundsExtras>("forest::BoundsExtras"),
            row::<PanelExtras>("forest::PanelExtras"),
            row::<ShapeRecord>("forest::ShapeRecord"),
            row::<ChromeRow>("forest::ChromeRow"),
            row::<Span>("layout::Span"),
            row::<ShapedText>("layout::ShapedText"),
            row::<NodeHash>("rollups::NodeHash"),
            row::<CascadeInputHash>("rollups::CascadeInputHash"),
            row::<Cascade>("cascade::Cascade"),
            row::<HitEntry>("cascade::HitEntry"),
            row::<DamageRegion>("damage::DamageRegion"),
            row::<DrawRectPayload>("cmd::DrawRectPayload"),
            row::<DrawTextPayload>("cmd::DrawTextPayload"),
            row::<DrawPolylinePayload>("cmd::DrawPolylinePayload"),
            row::<DrawMeshPayload>("cmd::DrawMeshPayload"),
            row::<Quad>("renderer::Quad"),
        ];

        let name_w = rows.iter().map(|(n, ..)| n.len()).max().unwrap_or(0);
        println!();
        println!(
            "{:<w$}  {:>5}  {:>5}",
            "struct",
            "size",
            "align",
            w = name_w
        );
        println!("{:-<w$}  {:->5}  {:->5}", "", "", "", w = name_w);
        for (n, s, a) in &rows {
            println!("{:<w$}  {:>5}  {:>5}", n, s, a, w = name_w);
        }
        println!();
    }
}
