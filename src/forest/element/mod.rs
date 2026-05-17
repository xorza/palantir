//! Per-node element data: `Element` (the per-widget builder form) and
//! the columns `Tree` stores it in:
//!
//! - `widget_id` — identity. Hit-test, state map, damage diff.
//! - `LayoutCore` — mode, size, padding, margin, align, visibility.
//!   Read by every measure / arrange / alignment pass.
//! - `NodeFlags` — 1-byte packed sense / disabled / clip / focusable.
//!   Read densely by cascade / encoder / hit-test.
//! - `BoundsExtras` — sparse side table for transform / position /
//!   grid / min_size / max_size. Allocated only when one differs
//!   from `BoundsExtras::DEFAULT`.
//! - `PanelExtras` — sparse side table for gap / line_gap / justify /
//!   child_align. Allocated only when one differs from `DEFAULT`.
//!
//! Paint chrome (`Background`, ~232 B) lives **outside** `Element`:
//! widgets that paint a background carry their own
//! `chrome: Option<Background>` field and pass it as a side-channel
//! argument through `Ui::node_with_chrome` →
//! `Forest::open_node_with_chrome` → `Tree::open_node_with_chrome`,
//! where it lands in the per-tree `chrome_table` (with `is_noop`
//! filtered out at push). Keeps the hot per-widget `Element` copy at
//! ~128 B instead of 360 B.
//!
//! Fan-out from `Element` to the dense columns happens once in
//! `Element::into_columns`. Adding a field is two local edits: append
//! to the column type and route it in `into_columns`. `Configure`
//! (trait below) provides one chained setter per field on `Element`.

use crate::forest::visibility::Visibility;
use crate::input::sense::Sense;
use crate::layout::types::{
    align::Align, align::HAlign, align::VAlign, clip_mode::ClipMode, grid_cell::GridCell,
    justify::Justify, sizing::Sizes,
};
use crate::primitives::widget_id::WidgetId;
use crate::primitives::{size::Size, spacing::Spacing, transform::TranslateScale};
use glam::Vec2;
use half::f16;
use std::hash::Hash;

/// `(gap, line_gap)` packed as two `f16` lanes in `[u16; 2]` (4 bytes).
/// Lane order: `gap | line_gap`. Same f16 contract as
/// `primitives::corners::Corners` and `primitives::spacing::Spacing`:
/// lossless for integer values up to 2048, ~0.25 px error at 4096. UI
/// gaps never approach that ceiling.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct Gaps([u16; 2]);

impl std::fmt::Debug for Gaps {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Gaps")
            .field("gap", &self.gap())
            .field("line_gap", &self.line_gap())
            .finish()
    }
}

impl std::hash::Hash for Gaps {
    /// Hash both lanes as one `u32` — one hasher call instead of two
    /// `write_u32(...to_bits())`s the previous f32 pair used.
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_u32(u32::from_ne_bytes(bytemuck::cast(self.0)));
    }
}

/// Packed `(justify, align, child_align)` in `u16`.
///
/// Lives on `Element` only — fan-out unpacks back into the dense
/// `LayoutCore.align` + sparse `PanelExtras.{justify, child_align}`
/// columns. Bit layout: 0-2 justify, 3-8 align (h:3, v:3), 9-14
/// child_align (h:3, v:3). Identity is carried by `Element.salt`
/// (a separate enum), not packed here.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct ElementSlots {
    pub(crate) bits: u16,
}

impl ElementSlots {
    const JUSTIFY_MASK: u16 = 0b111;
    const ALIGN_SHIFT: u16 = 3;
    const ALIGN_MASK: u16 = 0b111_111 << Self::ALIGN_SHIFT;
    const CHILD_ALIGN_SHIFT: u16 = 9;
    const CHILD_ALIGN_MASK: u16 = 0b111_111 << Self::CHILD_ALIGN_SHIFT;

    #[inline]
    pub(crate) fn justify(self) -> Justify {
        match self.bits & Self::JUSTIFY_MASK {
            0 => Justify::Start,
            1 => Justify::Center,
            2 => Justify::End,
            3 => Justify::SpaceBetween,
            4 => Justify::SpaceAround,
            _ => unreachable!(),
        }
    }
    #[inline]
    pub(crate) fn align(self) -> Align {
        Align::from_raw(((self.bits & Self::ALIGN_MASK) >> Self::ALIGN_SHIFT) as u8)
    }
    #[inline]
    pub(crate) fn child_align(self) -> Align {
        Align::from_raw(((self.bits & Self::CHILD_ALIGN_MASK) >> Self::CHILD_ALIGN_SHIFT) as u8)
    }
    #[inline]
    pub(crate) fn set_justify(&mut self, j: Justify) {
        self.bits = (self.bits & !Self::JUSTIFY_MASK) | (j as u16);
    }
    #[inline]
    pub(crate) fn set_align(&mut self, a: Align) {
        self.bits = (self.bits & !Self::ALIGN_MASK) | ((a.raw() as u16) << Self::ALIGN_SHIFT);
    }
    #[inline]
    pub(crate) fn set_child_align(&mut self, a: Align) {
        self.bits =
            (self.bits & !Self::CHILD_ALIGN_MASK) | ((a.raw() as u16) << Self::CHILD_ALIGN_SHIFT);
    }
}

const _: () = assert!(
    (Justify::SpaceAround as u8) < 8,
    "Justify exceeds 3 bits in ElementSlots",
);

impl Gaps {
    pub(crate) const ZERO: Self = Self([0; 2]);

    #[inline]
    pub(crate) fn gap(self) -> f32 {
        f16::from_bits(self.0[0]).to_f32()
    }

    #[inline]
    pub(crate) fn line_gap(self) -> f32 {
        f16::from_bits(self.0[1]).to_f32()
    }

    #[inline]
    pub(crate) fn set_gap(&mut self, v: f32) {
        self.0[0] = f16::from_f32(v).to_bits();
    }

    #[inline]
    pub(crate) fn set_line_gap(&mut self, v: f32) {
        self.0[1] = f16::from_f32(v).to_bits();
    }
}

/// How a node arranges its children. Stored as a direct byte field
/// on `Element::mode` and `LayoutCore::mode`; the tree itself treats
/// it as an opaque tag. `#[repr(u8)]` keeps the discriminants stable
/// for hashing and bytewise reads.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LayoutMode {
    Leaf = 0,
    HStack = 1,
    VStack = 2,
    /// HStack with overflow wrap: children flow left-to-right; when the
    /// next child wouldn't fit in the remaining main-axis space, wrap to
    /// a new row below. Each row's cross-axis size = max child cross
    /// in that row. `gap` spaces siblings within a row; `line_gap`
    /// spaces rows. Justify applies per row. `Sizing::Fill` on main is
    /// treated as `Hug` (no row-leftover distribution today).
    WrapHStack = 3,
    /// VStack with overflow wrap: same model as `WrapHStack`, axes
    /// swapped (children flow top-to-bottom; wrap to a new column on
    /// the right).
    WrapVStack = 4,
    /// Children all laid out at the same position (top-left of inner rect),
    /// each sized per its own `Sizing`. Used by `Panel`.
    ZStack = 5,
    /// Children placed at their declared `position` (parent-inner coords).
    /// Each child sized per its desired (intrinsic) size. Canvas hugs to the
    /// bounding box of placed children.
    Canvas = 6,
    /// WPF-style grid. The grid def's arena idx lives in
    /// `LayoutCore.mode_payload` (frame-local, only meaningful when the
    /// mode tag is `Grid`). Cap is 65 535 grids per frame (`grid_defs`
    /// is cleared each frame).
    Grid = 7,
    /// Vertical-scroll viewport. Children laid out as VStack with the
    /// Y axis measured as `INF`. The widget builder sets a `transform`
    /// to pan and enables `clip` so children render within the rect.
    ScrollVertical = 8,
    /// Horizontal-scroll viewport. Children laid out as HStack with
    /// the X axis measured as `INF`. Same record-time pan/clip setup
    /// as `ScrollVertical`.
    ScrollHorizontal = 9,
    /// Two-axis scroll viewport. Children laid out as `ZStack` with
    /// both axes unbounded. Same record-time pan/clip setup as the
    /// single-axis variants.
    ScrollBoth = 10,
}

impl LayoutMode {
    /// Mask of axes that consume scroll deltas. Returns `(false, false)`
    /// for non-scroll modes — callers gate on `is_scroll()` first.
    #[inline]
    pub(crate) fn pan_mask(self) -> glam::BVec2 {
        match self {
            Self::ScrollVertical => glam::BVec2::new(false, true),
            Self::ScrollHorizontal => glam::BVec2::new(true, false),
            Self::ScrollBoth => glam::BVec2::TRUE,
            _ => glam::BVec2::FALSE,
        }
    }
}

/// Per-node bounds + parent-relative placement. Set on any
/// `Element` (leaf or panel) whose builder customizes one of these fields.
/// Lifted into a sparse side-table so leaves that touch none of these stay
/// at zero per-node bytes here.
///
/// Stored as `Vec<BoundsExtras>` — 32 B per row, 2 entries per cache line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BoundsExtras {
    pub(crate) position: Vec2,
    pub(crate) grid: GridCell,
    /// Lower clamp on the resolved outer size. Default `Size::ZERO`.
    pub(crate) min_size: Size,
    /// Upper clamp on the resolved outer size. Default `Size::INF`.
    pub(crate) max_size: Size,
}

/// Paired `(min, max)` clamp on the resolved outer size — always read
/// together by `layoutengine` / `intrinsic` / `stack`. Returned by
/// `Tree::size_clamps_of` to avoid 2 separate column lookups at each
/// caller.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SizeClamp {
    pub(crate) min: Size,
    pub(crate) max: Size,
}

/// Panel-only knobs. Read by stack/wrap/grid/zstack drivers on the parent
/// node — leaves never touch them. Sparse so leaves don't allocate.
///
/// `transform` lives here (not on `BoundsExtras`) because **only `Panel`
/// and `Grid` expose `.transform()` in the public API** — every
/// transformed node is already a panel that typically customizes
/// `gap`/`justify`/`child_align`, so the field amortizes against an
/// already-allocated row. Keeps `ExtrasIdx` at 8 B (one fewer slot) and
/// avoids a separate `transform_table` sparse column.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PanelExtras {
    /// Within-line gap (HStack/VStack/WrapHStack/WrapVStack) + between-line
    /// gap (WrapHStack/WrapVStack only) packed as two f16 lanes.
    pub(crate) gaps: Gaps,
    /// Main-axis distribution of leftover space (HStack/VStack only).
    pub(crate) justify: Justify,
    /// Default alignment applied to children with `Auto` axis (panels only).
    pub(crate) child_align: Align,
    /// Pan/zoom transform applied to descendants (post-layout). Layout
    /// runs in untransformed space; cascade composes this with the
    /// ancestor transform for paint/hit-test. `TranslateScale::IDENTITY`
    /// is the no-op sentinel — same convention as `Stroke::ZERO` /
    /// `Shadow::NONE` / `Background::is_noop`; cascade filters identity
    /// at read time rather than carrying an `Option` discriminant.
    pub(crate) transform: TranslateScale,
}

/// `transform` is intentionally omitted: it doesn't affect this node's own
/// paint (the encoder draws the node at its layout rect *before*
/// `PushTransform`; the transform composes into descendants' screen rects via
/// `CascadesEngine`). A parent transform change shows up as descendant screen-rect
/// diffs in `DamageEngine::compute`, the right granularity. Transform IS folded
/// into `subtree_hash` separately (in the tree's rollup loop) so the encode
/// cache invalidates on transform-only changes.
impl std::hash::Hash for BoundsExtras {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        h.write(bytemuck::bytes_of(&self.position));
        self.grid.hash(h);
        self.min_size.hash(h);
        self.max_size.hash(h);
    }
}

/// `transform` is intentionally omitted here — same rationale as
/// `BoundsExtras::hash`: a parent moving its descendants shouldn't
/// dirty-flag its own node hash. Transform is folded into the
/// subtree hash separately in `Tree::compute_hashes`.
impl std::hash::Hash for PanelExtras {
    /// Pack `(gaps, child_align, justify)` into one `u64` write —
    /// gaps occupies the low 32 bits (already a packed `[u16; 2]`),
    /// child_align byte at bit 32, justify byte at bit 40.
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        let gaps_u32 = u32::from_ne_bytes(bytemuck::cast(self.gaps.0));
        let packed = (gaps_u32 as u64)
            | ((self.child_align.raw() as u64) << 32)
            | ((self.justify as u64) << 40);
        h.write_u64(packed);
    }
}

impl BoundsExtras {
    pub(crate) const DEFAULT: Self = Self {
        position: Vec2::ZERO,
        grid: GridCell {
            row: 0,
            col: 0,
            row_span: 1,
            col_span: 1,
        },
        min_size: Size::ZERO,
        max_size: Size::INF,
    };

    /// True when nothing has been customized — push_node skips the side-table
    /// allocation in this case. Exact equality against `DEFAULT` so adding a
    /// field only requires updating `DEFAULT`; no separate predicate to keep
    /// in sync.
    pub(crate) fn is_default(&self) -> bool {
        self == &Self::DEFAULT
    }
}

impl PanelExtras {
    pub(crate) const DEFAULT: Self = Self {
        gaps: Gaps::ZERO,
        justify: Justify::Start,
        child_align: Align::new(HAlign::Auto, VAlign::Auto),
        transform: TranslateScale::IDENTITY,
    };

    pub(crate) fn is_default(&self) -> bool {
        self == &Self::DEFAULT
    }
}

impl Default for BoundsExtras {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl Default for PanelExtras {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Per-node layout column, stored in `Tree::layout`. Read by every
/// pass that runs measure/arrange/alignment math. Held tight so the
/// layout pass pulls only what it reads. `mode` is a direct byte
/// (`#[repr(u8)] LayoutMode`); `bits` packs align(6b) + visibility(2b);
/// the Grid arena idx rides in `mode_payload` (zero for non-Grid).
/// Total 28 B, 2 entries per cache line. Packed paint/input flags
/// (sense / disabled / clip / focusable) live in `Tree::attrs` — a
/// separate 1-byte/node column read by cascade / encoder / hit-test.
#[derive(Clone, Copy, Debug)]
pub(crate) struct LayoutCore {
    pub(crate) size: Sizes,
    pub(crate) padding: Spacing,
    pub(crate) margin: Spacing,
    /// `LayoutMode::Grid` arena index. Zero (and ignored) otherwise.
    pub(crate) mode_payload: u16,
    /// Packed `align(6) | vis(2)`. Direct decode via `align()` /
    /// `visibility()`.
    pub(crate) bits: u8,
    pub(crate) mode: LayoutMode,
}

impl LayoutCore {
    const ALIGN_MASK: u8 = 0b11_1111;
    const VIS_SHIFT: u8 = 6;
    const VIS_MASK: u8 = 0b11 << Self::VIS_SHIFT;

    #[inline]
    pub(crate) const fn pack_bits(align: Align, vis: Visibility) -> u8 {
        (align.raw() & Self::ALIGN_MASK) | (((vis as u8) << Self::VIS_SHIFT) & Self::VIS_MASK)
    }

    #[inline(always)]
    pub(crate) fn align(&self) -> Align {
        Align::from_raw(self.bits & Self::ALIGN_MASK)
    }

    #[inline(always)]
    pub(crate) fn visibility(&self) -> Visibility {
        let raw = (self.bits & Self::VIS_MASK) >> Self::VIS_SHIFT;
        // SAFETY: `pack_bits` is the only constructor and writes only
        // `Visibility as u8` (0/1/2) into `VIS_MASK`. Branchless decode
        // — `visibility()` is read on every node every measure/arrange.
        unsafe { std::mem::transmute::<u8, Visibility>(raw) }
    }

    /// Fused write of `LayoutCore` + `NodeFlags` into one tail buffer.
    /// `compute_hashes` calls this so the per-node flags byte rides
    /// alongside the packed bits instead of producing a separate
    /// `NodeFlags::hash` → `write_u8` fold. Saves one hasher call per
    /// node per frame.
    ///
    /// `mode_payload` is intentionally **not** hashed: it's the Grid
    /// arena idx, a frame-local slot that shifts with sibling order.
    /// The Grid def's content is hashed separately at `NodeExit` via
    /// `GridDef::hash`.
    #[inline]
    pub(crate) fn hash_with_flags<H: std::hash::Hasher>(&self, flags: NodeFlags, h: &mut H) {
        h.write_u64(self.size.as_u64());
        h.write_u64(self.padding.as_u64());
        h.write_u64(self.margin.as_u64());
        let tail = u32::from_ne_bytes([self.bits, self.mode as u8, flags.bits, 0]);
        h.write_u32(tail);
    }
}

impl std::hash::Hash for LayoutCore {
    /// Three `u64` writes (size/padding/margin) + one `u16` packing
    /// `bits` and the mode tag. `mode_payload` excluded — see
    /// `hash_with_flags`.
    #[inline(always)]
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        h.write_u64(self.size.as_u64());
        h.write_u64(self.padding.as_u64());
        h.write_u64(self.margin.as_u64());
        h.write_u16(u16::from_ne_bytes([self.bits, self.mode as u8]));
    }
}

const _: () = assert!(
    (Visibility::Collapsed as u8) <= (LayoutCore::VIS_MASK >> LayoutCore::VIS_SHIFT),
    "Visibility discriminant exceeds 2 bits",
);

/// Recipe for an [`Element`]'s `WidgetId`. Mirrors egui's
/// `Option<Id>` "raw `id_salt`, resolve at `Ui::make_persistent_id`"
/// pattern: the builder stores the user's intent, resolution happens
/// at record time when the parent context is known. Three sources:
///
/// - [`Salt::Auto`] — `#[track_caller]`-derived. The captured
///   `(file, line, column)` already encodes call-site identity, so
///   the resolved id is the stored [`WidgetId`] **as-is** (no
///   parent-scoping — auto ids stay stable across moves in the tree).
///   `Ui::node`'s built-in occurrence-counter disambiguation handles
///   loops / helper closures that resolve to the same call site.
///
/// - [`Salt::Hash`] — raw user-supplied hash from `.id_salt(key)`.
///   At resolve time the hash is **mixed with the most-recently-
///   opened parent's resolved `WidgetId`** in the current layer
///   (`Layer::Main`'s synthetic viewport counts as a parent — its
///   `Salt::Auto` id is stable across frames). Two `.id_salt("row")`
///   under different parents resolve to distinct ids, so per-widget
///   `StateMap` / focus / animation entries survive subtree moves
///   without manual `WidgetId::with` chaining. Matches egui.
///
/// - [`Salt::Verbatim`] — precomputed [`WidgetId`] from `.id(id)`,
///   used as-is. Escape hatch for ids derived elsewhere
///   (cross-layer popups, sibling pairs sharing a seed). Skips
///   parent-scoping.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Salt {
    Auto(WidgetId),
    Hash(WidgetId),
    Verbatim(WidgetId),
}

impl Salt {
    /// Mix `self` with `parent`'s already-resolved `WidgetId` to
    /// produce the id that will be recorded into the tree. Only
    /// [`Salt::Hash`] consults `parent`; `Auto` and `Verbatim` pass
    /// through. `parent == None` covers the "no user-visible parent"
    /// case (root of a layer, or the first user-recorded widget
    /// under `Layer::Main`'s synthetic viewport).
    #[inline]
    pub(crate) fn resolve(self, parent: Option<WidgetId>) -> WidgetId {
        match self {
            Salt::Auto(id) | Salt::Verbatim(id) => id,
            Salt::Hash(salt) => match parent {
                Some(p) => p.with(salt.0),
                None => salt,
            },
        }
    }

    /// `true` for [`Salt::Hash`] / [`Salt::Verbatim`] — caller-supplied
    /// ids. `SeenIds::record` uses this to flag explicit collisions
    /// (caller bugs) with the magenta debug overlay while leaving
    /// auto collisions silent.
    #[inline]
    pub(crate) fn is_explicit(self) -> bool {
        matches!(self, Salt::Hash(_) | Salt::Verbatim(_))
    }
}

/// Per-node config: identity + spatial layout + interaction + paint flags.
/// Every widget builder owns one and forwards it to `Ui::node`. `Configure` (the
/// trait below) gives chained setters for all fields by impl'ing one method.
///
/// Fields are grouped by who reads them: identity, own-size (every parent),
/// mode-specific (only certain parents read these), interaction, and paint.
#[derive(Clone, Copy, Debug)]
pub struct Element {
    // ---- Identity + layout-algorithm selector --------------------------------
    /// Recipe for this node's `WidgetId`. Resolution happens inside
    /// [`crate::Ui::node`] (and the parallel [`crate::Ui::make_persistent_id`]
    /// callable by widgets that need the recorded id pre-`node` for
    /// theme picking / focus / animation slots) — `Element` itself
    /// never carries a resolved id. Mirrors egui's "builder stores
    /// raw `id_salt`, `Ui::make_persistent_id` mixes in the parent's
    /// id at `.show()`" pattern.
    pub(crate) salt: Salt,
    pub(crate) mode: LayoutMode,
    /// `LayoutMode::Grid` arena idx. Set by `Grid::show` once the def is
    /// pushed; ignored for every other `mode`.
    pub(crate) mode_payload: u16,

    // ---- Own size + alignment (read by every parent layout) ------------------
    pub(crate) size: Sizes,
    pub(crate) min_size: Size,
    pub(crate) max_size: Size,
    pub(crate) padding: Spacing,
    pub(crate) margin: Spacing,

    // ---- Mode-specific: only read when the parent or self has the right mode.
    // Inert otherwise.
    /// Within-line gap + between-line gap packed as two f16 lanes.
    /// `gaps.gap()` (HStack/VStack/WrapHStack/WrapVStack) is the
    /// sibling spacing; `gaps.line_gap()` (WrapHStack/WrapVStack only)
    /// is the row/column spacing. Both ignored by Leaf/ZStack/Canvas/
    /// Grid (Grid uses its own row_gap/col_gap).
    /// Within-line + between-line gaps packed as two f16 lanes.
    /// Private — read/written via inline accessors that hide the
    /// bit-layout (`element.gap()` / `element.set_gap(g)`).
    gaps: Gaps,

    /// Packed `(justify, align, child_align)` in `u16`.
    /// Private — read/written via inline accessors.
    slots: ElementSlots,
    /// Absolute position inside a `Canvas` parent (parent-inner coordinates).
    /// Defaults to `Vec2::ZERO`. Ignored when the parent isn't a `Canvas`.
    pub(crate) position: Vec2,
    /// Cell + span inside a `Grid` parent. Defaults to `(0, 0)` placement and
    /// `(1, 1)` span. Ignored when the parent isn't a `Grid`.
    pub(crate) grid: GridCell,

    /// Packed paint/input flags (sense, disabled, focusable, clip). One
    /// `u8`, mirrors the `NodeFlags` column `into_columns` writes to —
    /// no per-field decode at fan-out. Private — read/written via inline
    /// accessors (`element.sense()` / `element.set_sense(s)`, etc.).
    flags: NodeFlags,

    // ---- Paint + cascade -----------------------------------------------------
    /// WPF-style three-state visibility. `Hidden` keeps the node's slot in
    /// layout but suppresses paint + input; `Collapsed` zeros the slot and
    /// skips the subtree everywhere. Lives on `LayoutCore` (not `NodeFlags`)
    /// because measure's fast-path reads it next to size/margin.
    pub(crate) visibility: Visibility,
    /// Pan/zoom applied to descendants (post-layout, like WPF's `RenderTransform`).
    /// `TranslateScale::IDENTITY` = no transform. The transform composes
    /// with any ancestor transform; descendants render and hit-test in
    /// the world coordinates the cumulative transform produces. Origin
    /// is the top-left of the panel's logical-rect — the caller
    /// composes its own pivot by pre/post-translation.
    pub(crate) transform: TranslateScale,
}

/// Per-node columns derived from one `Element`. Single fan-out point —
/// `Tree::open_node` calls `Element::into_columns` once and moves each
/// field into its column. Adding an `Element` field is a one-line edit
/// here (one in the new column type, one routing line in `into_columns`).
pub(crate) struct ElementColumns {
    pub(crate) widget_id: WidgetId,
    pub(crate) layout: LayoutCore,
    pub(crate) attrs: NodeFlags,
    pub(crate) bounds: BoundsExtras,
    pub(crate) panel: PanelExtras,
}

impl Element {
    /// Build an `Element` with a call-site-derived auto id. `#[track_caller]`
    /// propagates through every widget constructor that's also marked
    /// `#[track_caller]`, so `Foo::new()` at the user's source line yields a
    /// distinct id per call site. Override with [`Configure::id_salt`] /
    /// [`Configure::id`] when call order isn't stable across frames.
    #[track_caller]
    pub(crate) fn new(mode: LayoutMode) -> Self {
        Self {
            salt: Salt::Auto(WidgetId::auto_stable()),
            mode,
            mode_payload: 0,
            size: Sizes::default(),
            min_size: Size::ZERO,
            max_size: Size::INF,
            padding: Spacing::ZERO,
            margin: Spacing::ZERO,
            gaps: Gaps::ZERO,
            slots: ElementSlots::default(),
            position: Vec2::ZERO,
            grid: GridCell::default(),
            flags: NodeFlags::default(),
            visibility: Visibility::Visible,
            transform: TranslateScale::IDENTITY,
        }
    }

    // ---- Inline accessors over the packed `flags` / `slots` / `gaps` ----
    // Hide the bit-layout from widget call sites — they write
    // `element.set_sense(s)` instead of `element.flags.set_sense(s)`.
    // Each method is a one-hop delegate to a `#[inline]` packed-storage
    // method, so it inlines straight through at the call site with no
    // extra call frame.

    #[inline]
    pub(crate) fn sense(&self) -> Sense {
        self.flags.sense()
    }
    #[inline]
    pub(crate) fn is_disabled(&self) -> bool {
        self.flags.is_disabled()
    }
    #[inline]
    pub(crate) fn is_focusable(&self) -> bool {
        self.flags.is_focusable()
    }
    #[inline]
    pub(crate) fn clip_mode(&self) -> ClipMode {
        self.flags.clip_mode()
    }
    #[inline]
    pub(crate) fn set_sense(&mut self, s: Sense) {
        self.flags.set_sense(s);
    }
    #[inline]
    pub(crate) fn set_disabled(&mut self, v: bool) {
        self.flags.set_disabled(v);
    }
    #[inline]
    pub(crate) fn set_focusable(&mut self, v: bool) {
        self.flags.set_focusable(v);
    }
    #[inline]
    pub(crate) fn set_clip(&mut self, c: ClipMode) {
        self.flags.set_clip(c);
    }

    #[inline]
    pub(crate) fn justify(&self) -> Justify {
        self.slots.justify()
    }
    #[inline]
    pub(crate) fn align(&self) -> Align {
        self.slots.align()
    }
    #[inline]
    pub(crate) fn child_align(&self) -> Align {
        self.slots.child_align()
    }
    #[inline]
    pub(crate) fn set_justify(&mut self, j: Justify) {
        self.slots.set_justify(j);
    }
    #[inline]
    pub(crate) fn set_align(&mut self, a: Align) {
        self.slots.set_align(a);
    }
    #[inline]
    pub(crate) fn set_child_align(&mut self, a: Align) {
        self.slots.set_child_align(a);
    }

    #[inline]
    pub(crate) fn set_gap(&mut self, v: f32) {
        self.gaps.set_gap(v);
    }
    /// Copy the gap pair from `other` (used by widgets like `Scroll`
    /// that split themselves into outer/inner nodes).
    #[inline]
    pub(crate) fn set_gaps_from(&mut self, other: &Element) {
        self.gaps = other.gaps;
    }

    /// Fan this `Element` out into the per-NodeId columns `Tree` stores.
    /// Single routing point — adding a field is one edit in the column
    /// type and one in the routing block. `widget_id` is supplied by
    /// the caller (resolved from `self.salt` upstream in
    /// `Forest::open_node`) so `Element` itself never carries a
    /// resolved id.
    #[inline(always)]
    pub(crate) fn into_columns(self, widget_id: WidgetId) -> ElementColumns {
        ElementColumns {
            widget_id,
            layout: LayoutCore {
                size: self.size,
                padding: self.padding,
                margin: self.margin,
                mode_payload: self.mode_payload,
                bits: LayoutCore::pack_bits(self.slots.align(), self.visibility),
                mode: self.mode,
            },
            attrs: self.flags,
            bounds: BoundsExtras {
                position: self.position,
                grid: self.grid,
                min_size: self.min_size,
                max_size: self.max_size,
            },
            panel: PanelExtras {
                gaps: self.gaps,
                justify: self.slots.justify(),
                child_align: self.slots.child_align(),
                transform: self.transform,
            },
        }
    }
}

/// Mixin: any widget builder that holds an `Element` gets the chained
/// setters (`.size()`, `.padding()`, `.sense()`, `.disabled()`, …) for
/// free by impl'ing just `element_mut`.
pub trait Configure: Sized {
    fn element_mut(&mut self) -> &mut Element;

    /// Override this widget's id with a hash of `key`, scoped to the
    /// parent. The stored hash is mixed with the parent node's
    /// already-disambiguated [`WidgetId`] at [`crate::forest::Forest::open_node`]
    /// time, so `.id_salt("row")` resolves to distinct ids under
    /// different parents — same scoping rule egui uses. At the root
    /// (no parent) the salt hash is used as-is. Use whenever the
    /// default call-site-derived id wouldn't survive across frames or
    /// loop iterations — e.g. a `for` loop where each iteration must
    /// keep per-widget state separate. Marks the id as
    /// [`Salt::Hash`]: same-parent sibling collisions are disambiguated
    /// (so state stays well-formed) but flagged with a magenta runtime
    /// outline because they're caller bugs. For an unscoped "use this
    /// exact id" override, see [`Self::id`].
    fn id_salt(mut self, key: impl Hash) -> Self {
        self.element_mut().salt = Salt::Hash(WidgetId::from_hash(key));
        self
    }

    /// Override this widget's id with a precomputed [`WidgetId`] used
    /// verbatim — **not** mixed with the parent. Use when the id was
    /// derived elsewhere and must match exactly (parent → child via
    /// [`WidgetId::with`], a shared seed for sibling widgets across
    /// layers, cross-frame state lookups that key off a domain id).
    /// For the parent-scoped path, prefer [`Self::id_salt`]. Stores
    /// the id as [`Salt::Verbatim`].
    fn id(mut self, id: WidgetId) -> Self {
        self.element_mut().salt = Salt::Verbatim(id);
        self
    }

    /// Re-derive an auto id at the *current* call site. Use when a builder
    /// helper constructs the widget (so `*::new()` resolved to the helper's
    /// source location) and you want each caller to get a distinct id —
    /// `helper().auto_id().show(ui)` reads the caller's `(file, line, col)`.
    #[track_caller]
    fn auto_id(mut self) -> Self {
        self.element_mut().salt = Salt::Auto(WidgetId::auto_stable());
        self
    }

    fn size(mut self, s: impl Into<Sizes>) -> Self {
        let s = s.into();
        s.w().debug_assert_non_negative();
        s.h().debug_assert_non_negative();
        self.element_mut().size = s;
        self
    }
    fn min_size(mut self, s: impl Into<Size>) -> Self {
        let s = s.into();
        debug_assert!(
            s.w >= 0.0 && s.h >= 0.0,
            "min_size must be non-negative on both axes, got {s:?}",
        );
        self.element_mut().min_size = s;
        self
    }
    fn max_size(mut self, s: impl Into<Size>) -> Self {
        let s = s.into();
        assert!(
            s.w >= 0.0 && s.h >= 0.0,
            "max_size must be non-negative on both axes, got {s:?}",
        );
        self.element_mut().max_size = s;
        self
    }
    fn padding(mut self, p: impl Into<Spacing>) -> Self {
        self.element_mut().padding = p.into();
        self
    }
    fn margin(mut self, m: impl Into<Spacing>) -> Self {
        self.element_mut().margin = m.into();
        self
    }
    /// Absolute position inside a `Canvas` parent (parent-inner coords).
    /// Ignored by other layout modes.
    fn position(mut self, p: impl Into<Vec2>) -> Self {
        self.element_mut().position = p.into();
        self
    }
    /// Cell `(row, col)` inside a `Grid` parent. Default `(0, 0)`. Ignored
    /// outside a Grid parent.
    fn grid_cell(mut self, (row, col): (u16, u16)) -> Self {
        let g = &mut self.element_mut().grid;
        g.row = row;
        g.col = col;
        self
    }
    /// Span `(row_span, col_span)` inside a `Grid` parent. Default `(1, 1)`.
    /// Spans are clamped at layout time to the grid's bounds. Ignored outside
    /// a Grid parent.
    fn grid_span(mut self, (rs, cs): (u16, u16)) -> Self {
        let g = &mut self.element_mut().grid;
        g.row_span = rs.max(1);
        g.col_span = cs.max(1);
        self
    }
    /// Logical-px space between siblings within a line. Read by
    /// HStack/VStack and the within-line direction of WrapHStack/
    /// WrapVStack. Grid has its own `gap_xy` and ignores this field.
    fn gap(mut self, g: f32) -> Self {
        self.element_mut().gaps.set_gap(g);
        self
    }
    /// Logical-px space between *lines* for WrapHStack/WrapVStack —
    /// the cross-axis spacing between wrap rows/columns. Inert in
    /// every other layout mode. Pair with `.gap(...)` for the within-
    /// line spacing.
    fn line_gap(mut self, g: f32) -> Self {
        self.element_mut().gaps.set_line_gap(g);
        self
    }
    /// Main-axis distribution of leftover space for `HStack`/`VStack`.
    /// Ignored when any child has `Sizing::Fill` on the main axis.
    fn justify(mut self, j: Justify) -> Self {
        self.element_mut().slots.set_justify(j);
        self
    }
    /// Alignment inside the parent's inner rect. For single-axis use the
    /// [`Align::h`] / [`Align::v`] constructors.
    fn align(mut self, a: Align) -> Self {
        self.element_mut().slots.set_align(a);
        self
    }
    /// Default alignment applied to children when their own axis is `Auto`.
    /// Mirrors CSS `align-items`. For single-axis defaults use the
    /// [`Align::h`] / [`Align::v`] constructors.
    fn child_align(mut self, a: Align) -> Self {
        self.element_mut().slots.set_child_align(a);
        self
    }
    fn sense(mut self, s: Sense) -> Self {
        self.element_mut().flags.set_sense(s);
        self
    }
    /// Suppress this node's interactions and cascade to all descendants.
    fn disabled(mut self, d: bool) -> Self {
        self.element_mut().flags.set_disabled(d);
        self
    }
    /// Mark this node as eligible to take keyboard focus on press.
    /// Default `false`. Only editable widgets (TextEdit) opt in. Disabled
    /// or invisible nodes are excluded from focus regardless of this
    /// flag — same cascade rule as `Sense`.
    fn focusable(mut self, f: bool) -> Self {
        self.element_mut().flags.set_focusable(f);
        self
    }
    /// Three-state visibility. See [`Visibility`].
    fn visibility(mut self, v: Visibility) -> Self {
        self.element_mut().visibility = v;
        self
    }
    /// Shorthand for [`Visibility::Hidden`]: keeps the slot, hides paint + input.
    fn hidden(self) -> Self {
        self.visibility(Visibility::Hidden)
    }
    /// Shorthand for [`Visibility::Collapsed`]: skip the node entirely (zero slot).
    fn collapsed(self) -> Self {
        self.visibility(Visibility::Collapsed)
    }

    /// Generic clip setter. Most callers use the [`Self::clip_rect`]
    /// / [`Self::clip_rounded`] sugars instead.
    fn clip(mut self, mode: ClipMode) -> Self {
        self.element_mut().flags.set_clip(mode);
        self
    }

    /// Axis-aligned scissor clip on this node's rect.
    fn clip_rect(self) -> Self {
        self.clip(ClipMode::Rect)
    }

    /// Rounded-corner stencil clip — shape comes from the chrome's
    /// radius (set via [`Self::background`]). Calling this without
    /// a chrome leaves the radius at zero, equivalent to
    /// [`Self::clip_rect`].
    fn clip_rounded(self) -> Self {
        self.clip(ClipMode::Rounded)
    }
}

/// Packed paint/input flags. One byte.
///
/// `bits`: 0-3=sense bitflags (HOVER|CLICK|DRAG|SCROLL), 4=disabled,
/// 5-6=clip mode, 7=focusable. `Element` uses the same packed form
/// during recording; fan-out is a single byte copy.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct NodeFlags {
    pub(crate) bits: u8,
}

impl NodeFlags {
    const SENSE_MASK: u8 = 0b1111;
    const DISABLED: u8 = 1 << 4;
    const CLIP_SHIFT: u8 = 5;
    const CLIP_MASK: u8 = 0b11 << Self::CLIP_SHIFT;
    const FOCUSABLE: u8 = 1 << 7;

    #[inline]
    pub(crate) fn sense(self) -> Sense {
        Sense::from_bits_truncate(self.bits & Self::SENSE_MASK)
    }
    #[inline]
    pub(crate) fn is_disabled(self) -> bool {
        self.bits & Self::DISABLED != 0
    }
    #[inline]
    pub(crate) fn clip_mode(self) -> ClipMode {
        match (self.bits & Self::CLIP_MASK) >> Self::CLIP_SHIFT {
            0 => ClipMode::None,
            1 => ClipMode::Rect,
            2 => ClipMode::Rounded,
            _ => unreachable!(),
        }
    }
    #[inline]
    pub(crate) fn is_focusable(self) -> bool {
        self.bits & Self::FOCUSABLE != 0
    }

    #[inline]
    pub(crate) fn set_sense(&mut self, s: Sense) {
        self.bits = (self.bits & !Self::SENSE_MASK) | (s.bits() & Self::SENSE_MASK);
    }
    #[inline]
    pub(crate) fn set_disabled(&mut self, v: bool) {
        self.bits = (self.bits & !Self::DISABLED) | (if v { Self::DISABLED } else { 0 });
    }
    #[inline]
    pub(crate) fn set_clip(&mut self, c: ClipMode) {
        self.bits = (self.bits & !Self::CLIP_MASK) | ((c as u8) << Self::CLIP_SHIFT);
    }
    #[inline]
    pub(crate) fn set_focusable(&mut self, v: bool) {
        self.bits = (self.bits & !Self::FOCUSABLE) | (if v { Self::FOCUSABLE } else { 0 });
    }
}

const _: () = assert!(
    (ClipMode::Rounded as u8) <= (NodeFlags::CLIP_MASK >> NodeFlags::CLIP_SHIFT),
    "ClipMode discriminant exceeds 2 bits",
);
const _: () = assert!(
    Sense::all().bits() <= NodeFlags::SENSE_MASK,
    "Sense uses more than 4 bits",
);

#[cfg(test)]
mod tests;
