# Public API surface

Every item the crate exports with the default features plus `golden`, from rustdoc JSON
(`python3 scripts/api_surface.py`). Each type lists its public inherent methods and
associated constants, and the traits it implements. `internals` and `bench` are left out:
they exist for this crate's own tests and benches.

Generated on top of `6f2d806d`. Findings and recommendations are in `API_CHANGES.md`.

`prelude` re-exports these root items: `Align`, `App`, `Axis`, `Background`, `Block`, `Brush`, `Button`, `Checkbox`, `ComboBox`, `Configure`, `ContextMenu`, `Corners`, `DragValue`, `Expander`, `Grid`, `GridCell`, `HAlign`, `InnerResponse`, `Justify`, `Key`, `KeyPress`, `MenuItem`, `Modal`, `Modifiers`, `OverlayResponse`, `Panel`, `PointerButton`, `Popup`, `ProgressBar`, `RadioButton`, `Rect`, `Response`, `RgbaF32`, `Scroll`, `SelectResponse`, `Sense`, `Separator`, `Shadow`, `Shortcut`, `Size`, `SizeSpec`, `Sizing`, `Slider`, `Spacing`, `Spinner`, `Splitter`, `Stroke`, `Switch`, `TabbedView`, `Text`, `TextEdit`, `TextStyle`, `Theme`, `Tooltip`, `Track`, `UVec2`, `Ui`, `VAlign`, `ValueResponse`, `Vec2`, `WidgetId`, `WindowToken`, `fmt`.

```text
module           golden
struct           golden::Tolerance
    fields: per_channel, max_ratio
    fn diff(self, actual, expected)
    traits: Clone, Copy, Debug, Default
struct           golden::DiffReport
    fields: max_channel_delta, differing_pixels, differing_ratio, diff_image, tolerance
    const fn passes(self)
    traits: Debug
struct           golden::Goldens
    fn new(root)
    const fn tolerance(self, tolerance)
    fn assert_matches(self, name, actual)
    fn assert_same(self, name, actual, expected)
    traits: Clone, Debug
module           widget
struct           widget::AnimSlot
    const fn new(name)
    traits: Clone, Copy, Debug, Eq, From, Hash, PartialEq
trait            widget::Animatable
    items: lerp, sub, add, scale, magnitude_squared, settle_distance_squared, zero, normalize_for_spring
struct           widget::Span
    const fn new(start, len)
    traits: Clone, Copy, Debug, Default, Eq, From, PartialEq, Pod, StructuralPartialEq, Zeroable
struct           widget::Mesh
    const fn new()
    fn with_capacity(vertices, indices)
    fn clear(self)
    fn is_noop(self)
    fn content_hash(self)
    fn vertex(self, pos, color)
    fn triangle(self, a, b, c)
    fn append(self, other)
    fn bbox(self)
    fn with_known_bbox(self, bbox)
    fn filled_triangle(a, b, c, color)
    fn filled_polygon(points, color)
    traits: Clone, Debug, Default
struct           widget::MeshVertex
    fields: pos, color
    fn new(pos, color)
    traits: Clone, Copy, Debug, Default, PartialEq, Pod, StructuralPartialEq, Zeroable
struct           widget::Sums
    fields: horizontal, vertical
    traits: Clone, Copy, Debug
module           widget::domain
module           widget::domain::vec2
function         widget::domain::vec2::approx_eq(a, b)
function         widget::domain::vec2::band_fraction(pos, extent, band)
function         widget::domain::vec2::fraction_or(v, fallback)
function         widget::domain::vec2::length_at_least(v, min)
function         widget::domain::vec2::is_offset(v)
function         widget::domain::vec2::offset(v)
function         widget::domain::vec2::is_length(v)
function         widget::domain::vec2::length(v)
constant         widget::domain::EPS
function         widget::domain::approx_zero(v)
function         widget::domain::approx_eq(a, b)
function         widget::domain::paints_nothing(v)
function         widget::domain::share_of(n, d)
function         widget::domain::band_fraction(pos, extent, band)
function         widget::domain::is_offset(v)
function         widget::domain::offset(v)
function         widget::domain::is_length(v)
function         widget::domain::length(v)
function         widget::domain::is_extent(v)
function         widget::domain::extent(v)
function         widget::domain::is_gap(v)
function         widget::domain::gap(v)
function         widget::domain::is_positive(v)
function         widget::domain::positive(v)
function         widget::domain::is_angle(v)
function         widget::domain::angle(v)
function         widget::domain::is_color(c)
function         widget::domain::color(c)
function         widget::domain::is_count(n)
function         widget::domain::count(n)
function         widget::domain::is_power_of_two_in(n, max)
function         widget::domain::power_of_two_in(n, max)
function         widget::domain::is_range(r)
function         widget::domain::range(r)
function         widget::domain::is_fraction(v)
function         widget::domain::fraction(v)
function         widget::domain::fraction_or(v, fallback)
function         widget::domain::turn(v)
function         widget::domain::index(i, len)
function         widget::domain::length_at_least(v, min)
enum             widget::ContentType
    variants: Mask, Color
    traits: Clone, Copy, Debug, Eq, PartialEq, StructuralPartialEq
struct           widget::RasterImage
    fields: content, size, bearing, data
    traits: Clone, Copy, Debug
module           widget::curves
function         widget::curves::linear(t)
function         widget::curves::square(t)
function         widget::curves::sine(t)
struct           widget::PaintAnim
    fields: channel, timing, curve
    fn alpha(from, to)
    fn turn(from, to)
    const fn with_alpha(self, from, to)
    const fn with_turn(self, from, to)
    const fn period(self, period)
    const fn started_at(self, at)
    const fn repeat(self, repeat)
    const fn steps(self, n)
    const fn curve(self, curve)
    traits: Clone, Copy, Debug
struct           widget::PaintChannel
    fields: alpha, turn
    traits: Clone, Copy, Debug, PartialEq, StructuralPartialEq
type_alias       widget::PaintCurve
enum             widget::PaintRepeat
    variants: Once, Forever, Settle
    traits: Clone, Copy, Debug, Eq, PartialEq, StructuralPartialEq
enum             widget::PaintSteps
    variants: Continuous, Steps
    traits: Clone, Copy, Debug, Eq, PartialEq, StructuralPartialEq
struct           widget::PaintTiming
    fields: started_at, period, repeat, steps
    traits: Clone, Copy, Debug, PartialEq, StructuralPartialEq
trait            widget::Lower
    items: 
struct           widget::Shape
    const fn rect(rect)
    const fn owner_rect()
    const fn windowed_rect(rect)
    const fn owner_windowed_rect()
    const fn triangle(a, b, c)
    const fn line(a, b, stroke)
    fn polyline(points, stroke)
    const fn cubic_bezier(p0, p1, p2, p3, stroke)
    const fn quadratic_bezier(p0, p1, p2, stroke)
    const fn arc(center, radius, start_angle, sweep, stroke)
    const fn circle(center, radius, stroke)
    const fn text(text, font)
    const fn shadow(shadow)
    fn image(handle)
    fn icon(handle)
    const fn mesh(mesh)
    traits: Clone, Copy, Debug
struct           widget::CurveShape
    fn ramp(self, ramp)
    fn cap(self, cap)
    traits: Clone, Debug
enum             widget::IconFit
    variants: Contain, Fill, None
    traits: Clone, Copy, Debug, Default, Eq, PartialEq, StructuralPartialEq
struct           widget::IconShape
    fn at(self, rect)
    fn fit(self, fit)
    fn tint(self, tint)
    fn desaturate(self, desaturate)
    traits: Clone, Copy, Debug
struct           widget::ImageShape
    fn at(self, rect)
    fn fit(self, fit)
    fn min_filter(self, min_filter)
    fn mag_filter(self, mag_filter)
    fn downsample(self, downsample)
    fn tint(self, tint)
    traits: Clone, Debug
struct           widget::MeshShape
    fn at(self, rect)
    fn tint(self, tint)
    traits: Clone, Debug
struct           widget::PolylineShape
    const fn per_point(self, colors)
    const fn per_segment(self, colors)
    fn cap(self, cap)
    fn join(self, join)
    traits: Clone, Debug
struct           widget::RectShape
    fn fill(self, fill)
    fn border(self, border)
    fn corners(self, corners)
    traits: Clone, Debug
struct           widget::ShadowShape
    fn at(self, rect)
    fn corners(self, corners)
    traits: Clone, Debug
enum             widget::LineCap
    variants: Butt, Square, Round
    traits: Clone, Copy, Debug, Default, Eq, Hash, PartialEq, StructuralPartialEq
enum             widget::LineJoin
    variants: Miter, Bevel, Round
    traits: Clone, Copy, Debug, Default, Eq, Hash, PartialEq, StructuralPartialEq
struct           widget::TextShape
    const fn at_origin(self, origin)
    fn color(self, color)
    fn wrap(self, wrap)
    fn align(self, align)
    fn family(self, family)
    fn weight(self, weight)
    fn slant(self, slant)
    traits: Clone, Debug
struct           widget::TriangleShape
    fn fill(self, fill)
    fn border(self, border)
    fn radius(self, radius)
    traits: Clone, Debug
struct           widget::GlyphFont
    fields: size_px, line_height_px, family, weight, slant
    const fn new(size_px)
    traits: Clone, Copy, Debug, PartialEq, StructuralPartialEq
struct           widget::TextGlyphs
    fn line(self, text, font, scale, out)
    fn measure(self, text, font)
    fn rasterize(self, glyph)
    traits: Debug
struct           widget::Caret
    fields: x, y_top, line_height
    traits: Clone, Copy, Debug, PartialEq, StructuralPartialEq
struct           widget::TextProbe
    const fn size(self)
    fn text_hash(self)
    fn hash_of(text)
    fn caret_at(self, byte_offset)
    fn byte_at(self, x, y)
    fn selection_rects(self, range, out)
    traits: Debug
struct           widget::GlyphRasterKey
    traits: Clone, Copy, Debug, Eq, Hash, PartialEq, StructuralPartialEq
struct           widget::PlacedGlyph
    fields: raster_key, x, y
    traits: Clone, Copy, Debug
struct           widget::TextRun
    fields: text, font, wrap, align, max_width_px
    traits: Clone, Copy, Debug
struct           widget::ConfigureWidget
    fn id_salt(self, key)
    const fn id(self, id)
    fn auto_id(self)
    fn size(self, s)
    fn default_size(self, s)
    fn min_size(self, s)
    fn max_size(self, s)
    fn padding(self, p)
    fn margin(self, m)
    const fn transform(self, t)
    fn position(self, p)
    fn grid_cell(self, cell)
    fn gap(self, g)
    fn line_gap(self, g)
    const fn justify(self, j)
    const fn align(self, a)
    const fn child_align(self, a)
    const fn sense(self, s)
    fn add_sense(self, s)
    const fn disabled(self, d)
    const fn focusable(self, f)
    const fn input_scope(self, takes)
    const fn visibility(self, v)
    const fn hidden(self)
    const fn collapsed(self)
    const fn clip(self, mode)
    const fn clip_rect(self)
    const fn clip_rounded(self)
    const fn default_id(self, id)
    fn default_padding(self, p)
    fn default_margin(self, m)
    const fn default_align(self, a)
    fn default_gap(self, g)
    fn default_min_size(self, s)
    fn default_max_size(self, s)
    fn default_clip(self, mode)
    traits: Debug
trait            widget::ThemeDefaults
    items: default_id, default_size, default_padding, default_margin, default_align, default_gap, default_min_size, default_max_size, default_clip
struct           widget::Widget
    fn leaf()
    fn hstack()
    fn vstack()
    fn wrap_hstack()
    fn wrap_vstack()
    fn zstack()
    fn canvas()
    fn grid()
    fn resolve(self, ui)
    fn response(self, ui)
    fn record(self, ui, chrome, body)
    fn show(self, ui, chrome, body)
    const fn authored_size(self)
    const fn authored_min_size(self)
    const fn authored_max_size(self)
    const fn authored_padding(self)
    const fn authored_margin(self)
    const fn authored_transform(self)
    const fn authored_position(self)
    const fn authored_grid_cell(self)
    fn authored_gap(self)
    fn authored_line_gap(self)
    const fn authored_justify(self)
    const fn authored_align(self)
    const fn authored_child_align(self)
    const fn authored_sense(self)
    const fn authored_disabled(self)
    const fn authored_focusable(self)
    const fn authored_input_scope(self)
    const fn authored_visibility(self)
    const fn authored_clip(self)
    fn grid_tracks(self, ui, rows, cols)
    fn adopt_placement(self, from)
    traits: Configure, Debug
struct           widget::LookPlan
    fn apply(self, ui, widget)
    traits: Debug
trait            widget::ThemeSlot
    items: Pick, look, defaults, plan
extern-reexport  widget::Animatable  -> palantir_anim_derive::Animatable
enum             BatchKind
    variants: Setup, PreClear, Mask, Quads, Text, Mesh, Image, Curve, Icon
    assoc_const COUNT
    assoc_const ALL
    const fn label(self)
    traits: Clone, Copy, Debug, Eq, PartialEq, StructuralPartialEq
struct           GpuPassStats
    fn last_pass_ms(self)
    fn last_kind_ms(self, kind)
    fn last_pipeline_stats(self)
    fn last_main_pass_cpu_ms(self)
    traits: Clone, Debug, Default
struct           PipelineStats
    fields: vertex_shader_invocations, clipper_invocations, clipper_primitives_out, fragment_shader_invocations, compute_shader_invocations
    traits: Clone, Copy, Debug, Default, Eq, PartialEq, StructuralPartialEq
extern-reexport  wgpu  -> wgpu
struct           AnimSpec
    assoc_const FAST
    assoc_const MEDIUM
    assoc_const SNAP
    assoc_const SPRING
    const fn duration(secs, ease)
    fn spring(stiffness, damping)
    const fn is_instant(self)
    traits: Clone, Copy, Debug, Deserialize, PartialEq, Serialize, StructuralPartialEq
enum             Easing
    variants: Linear, OutCubic, InOutCubic, OutQuart, OutBack
    const fn apply(self, t)
    traits: Clone, Copy, Debug, Deserialize, PartialEq, Serialize, StructuralPartialEq
trait            App
    items: update, record
struct           Clipboard
    fn text(self)
    fn set_text(self, text)
    traits: Clone, Debug
struct           ClipboardUnavailable
    traits: Clone, Copy, Debug, Display, Eq, Error, PartialEq, StructuralPartialEq
constant         PLATFORM
enum             Platform
    variants: Mac, Win, Linux
    traits: Clone, Copy, Debug, Eq, Hash, PartialEq, StructuralPartialEq
struct           DebugOverlayConfig
    fields: damage_rect, dim_undamaged, frame_stats
    traits: Clone, Copy, Debug, Default, Eq, PartialEq, StructuralPartialEq
struct           Display
    fields: physical, system_scale, user_scale, pixel_snap, refresh_millihertz
    const fn from_physical(physical, system_scale)
    const fn scale_factor(self)
    fn logical_size(self)
    fn system_logical_size(self)
    fn logical_rect(self)
    fn raster_eq(self, other)
    traits: Clone, Copy, Debug, Default, PartialEq, StructuralPartialEq
struct           UserScale
    assoc_const ONE
    assoc_const LADDER
    assoc_const MIN
    assoc_const MAX
    const fn new(factor)
    const fn get(self)
    const fn applied_to(self, system_scale)
    fn stepped_up(self)
    fn stepped_down(self)
    const fn percent(self)
    traits: Clone, Copy, Debug, Default, PartialEq, PartialOrd, StructuralPartialEq
struct           DeviceRequirements
    fields: features, limits
    assoc_const FEATURES
    assoc_const GPU_TIMING_FEATURES
    fn negotiate(adapter, optional)
    fn met_by(device)
    traits: Clone, Debug
enum             PowerPreference
    variants: Any, LowPower, HighPerformance
    traits: Clone, Copy, Debug, Default, Eq, PartialEq, StructuralPartialEq
struct           Gpu
    const fn new(device, queue)
    traits: Clone, Debug
struct           RequestedGpu
    fields: adapter, gpu
    fn headless(power_preference, optional)
    traits: Debug
enum             SurfaceError
    variants: Create, Device, Incompatible, MissingSrgb, MissingUsages
    traits: Debug, Display, Error, From
struct           DriverError
    traits: Debug, Display, Error
enum             GpuRequestError
    variants: NoBackend, RequestAdapter, Requirements, RequestDevice
    traits: Debug, Display, Error, From
enum             UnmetRequirements
    variants: Features, Limit
    traits: Clone, Debug, Display, Eq, Error, PartialEq, StructuralPartialEq
struct           RenderTarget
    traits: Clone, Copy, Debug, From
struct           TargetFormat
    traits: Clone, Copy, Debug, Eq, From, Hash, PartialEq, StructuralPartialEq
trait            Clock
    items: now, skip, deadline
struct           FixedClock
    const fn new(now)
    fn advance(self, dt)
    traits: Clock, Debug, Default
struct           RealtimeClock
    fn new()
    traits: Clock, Debug, Default
struct           OffscreenHost
    assoc_const WINDOW
    const fn builder(gpu)
    const fn ui(self)
    fn on_input(self, event)
    fn frame(self, target, system_scale, app)
    const fn gpu_pass_stats(self)
    traits: Debug
struct           OffscreenHostBuilder
    fn fonts(self, scope)
    fn shaper(self, shaper)
    const fn collect_gpu_stats(self, collect)
    fn clock(self, clock)
    const fn retained_target(self, retained)
    const fn pixel_snap(self, pixel_snap)
    const fn system_clipboard(self, system)
    fn build(self)
    traits: Debug
struct           WinitHost
    fn builder(first_token)
    fn handle(self)
    fn run(self)
    traits: ApplicationHandler, Debug
struct           WinitHostBuilder
    fn config(self, config)
    fn window(self, window)
    fn title(self, title)
    const fn fonts(self, scope)
    const fn vsync(self, vsync)
    const fn power_preference(self, pref)
    const fn collect_gpu_stats(self, collect)
    const fn pixel_snap(self, pixel_snap)
    fn build(self, create_app)
    traits: Debug
struct           WinitHostConfig
    fields: window, vsync, power_preference, collect_gpu_stats, fonts, pixel_snap
    traits: Clone, Debug, Default
struct           HostDisconnected
    traits: Clone, Copy, Debug, Display, Eq, Error, PartialEq, StructuralPartialEq
enum             WinitHostError
    variants: CreateEventLoop, RunEventLoop, CreateWindow, Surface, Gpu
    traits: Debug, Display, Error, From
struct           HostHandle
    fn request_repaint(self, win)
    fn run_on_main(self, f)
    fn quit(self)
    traits: Clone, Debug
enum             UserEvent
    variants: Repaint, RunOnMain, Quit
    traits: ApplicationHandler, Debug
enum             InputEvent
    variants: PointerMoved, PointerLeft, PointerPressed, PointerReleased, ScrollPixels, ScrollLines, Zoom, KeyDown, ModifiersChanged, SurfaceFocusLost
    traits: Clone, Copy, Debug
enum             ButtonPhase
    variants: Idle, Down, Held, Up
    traits: Clone, Copy, Debug, Default, PartialEq, StructuralPartialEq
struct           ButtonState
    fields: phase, drag
    const fn held(self)
    const fn clicked(self)
    const fn released(self)
    const fn press_count(self)
    const fn click_count(self)
    const fn double_clicked(self)
    traits: Clone, Copy, Debug, Default, PartialEq, StructuralPartialEq
enum             Drag
    variants: None, Started, Active, Stopped
    const fn delta(self)
    const fn dragging(self)
    const fn started(self)
    const fn stopped(self)
    traits: Clone, Copy, Debug, Default, PartialEq, StructuralPartialEq
struct           InputDelta
    fields: requests_repaint
    traits: Clone, Copy, Debug, Default, Eq, PartialEq, StructuralPartialEq
struct           PointerAction
    fields: id, button, edge
    traits: Clone, Copy, Debug, Eq, PartialEq, StructuralPartialEq
enum             PointerEdge
    variants: Pressed, Clicked, DragStarted, DragStopped
    traits: Clone, Copy, Debug, Eq, PartialEq, StructuralPartialEq
struct           ResponseState
    fields: rect, layout_rect, transform, pointer_local, pointer_over, disabled, focused, left, right, middle, scroll
    const fn hovered(self)
    const fn clicked(self)
    const fn double_clicked(self)
    const fn any_clicked(self)
    const fn button(self, button)
    const fn pressed(self)
    fn press_fraction(self, band)
    traits: Clone, Copy, Debug, Default
struct           ScrollDelta
    fields: pixels, lines, zoom
    fn pan(self, line_px)
    traits: Clone, Copy, Debug, Default, PartialEq, StructuralPartialEq
enum             KeyClass
    variants: Text, Edit, Caret, Page, Focus, Cycle, Escape, Accel
    fn of(press)
    traits: Clone, Copy, Debug, Eq, Hash, PartialEq, StructuralPartialEq
struct           KeyFilter
    assoc_const TEXT
    assoc_const EDIT
    assoc_const CARET
    assoc_const PAGE
    assoc_const FOCUS
    assoc_const CYCLE
    assoc_const ESCAPE
    assoc_const ACCEL
    assoc_const NONE
    assoc_const ALL
    const fn is_empty(self)
    const fn contains(self, other)
    const fn intersects(self, other)
    const fn union(self, other)
    const fn difference(self, other)
    const fn insert(self, other)
    const fn remove(self, other)
    const fn set(self, other, on)
    assoc_const TEXT_FIELD
    const fn takes(self, class)
    fn accepts(self, press)
    traits: BitOr, Clone, Copy, Debug, Default, Eq, Hash, PartialEq, StructuralPartialEq
enum             Key
    variants: ArrowLeft, ArrowRight, ArrowUp, ArrowDown, Backspace, Delete, Home, End, PageUp, PageDown, Enter, Tab, Escape, F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12, Char, Other
    traits: Clone, Copy, Debug, Eq, Hash, PartialEq, StructuralPartialEq
struct           KeyPress
    fields: key, mods, repeat, physical, text
    traits: Clone, Copy, Debug, Eq, PartialEq, StructuralPartialEq
struct           KeyText
    assoc_const CAP
    assoc_const EMPTY
    fn new(text)
    fn from_char(c)
    fn as_str(self)
    fn is_empty(self)
    traits: Clone, Copy, Debug, Default, Eq, PartialEq, StructuralPartialEq
struct           Modifiers
    fields: shift, ctrl, alt, mac_ctrl
    assoc_const NONE
    const fn any_command(self)
    traits: Clone, Copy, Debug, Default, Eq, Hash, PartialEq, StructuralPartialEq
enum             PointerButton
    variants: Left, Right, Middle
    traits: Clone, Copy, Debug, Eq, Hash, PartialEq, StructuralPartialEq
enum             PointerEvent
    variants: Move, Down, Up, Scroll, Zoom, Leave
    traits: Clone, Copy, Debug, PartialEq, StructuralPartialEq
enum             FocusPolicy
    variants: PreserveOnMiss, ClearOnMiss
    traits: Clone, Copy, Debug, Default, Eq, PartialEq, StructuralPartialEq
enum             InputPolicy
    variants: Always, OnDelta
    traits: Clone, Copy, Debug, Default, Eq, PartialEq, StructuralPartialEq
struct           Sense
    assoc_const HOVER
    assoc_const CLICK
    assoc_const DRAG
    assoc_const SCROLL
    assoc_const PINCH
    assoc_const NONE
    assoc_const ALL
    const fn is_empty(self)
    const fn contains(self, other)
    const fn intersects(self, other)
    const fn union(self, other)
    const fn difference(self, other)
    const fn insert(self, other)
    const fn remove(self, other)
    const fn set(self, other, on)
    assoc_const ABSORB_POINTER
    traits: BitOr, Clone, Copy, Debug, Default, Eq, Hash, PartialEq, StructuralPartialEq
struct           Shortcut
    fields: mods, key
    const fn new(mods, key)
    const fn key(key)
    const fn ctrl(c)
    const fn ctrl_shift(c)
    fn matches(self, kp)
    traits: Clone, Copy, Debug, Display, Eq, Hash, PartialEq, StructuralPartialEq
struct           ShortcutMods
    fields: ctrl, shift, alt
    const fn any_command(self)
    assoc_const NONE
    assoc_const SHIFT
    assoc_const CTRL
    assoc_const CTRL_SHIFT
    const fn from_event(m)
    traits: Clone, Copy, Debug, Default, Eq, Hash, PartialEq, StructuralPartialEq
struct           KeyboardWake
    assoc_const KEY
    assoc_const MODIFIER
    assoc_const NONE
    assoc_const ALL
    const fn is_empty(self)
    const fn contains(self, other)
    const fn intersects(self, other)
    const fn union(self, other)
    const fn difference(self, other)
    const fn insert(self, other)
    const fn remove(self, other)
    const fn set(self, other, on)
    traits: BitOr, Clone, Copy, Debug, Default, Eq, Hash, PartialEq, StructuralPartialEq
struct           PointerWake
    assoc_const BUTTONS
    assoc_const MOVE
    assoc_const SCROLL
    assoc_const PINCH
    assoc_const NONE
    assoc_const ALL
    const fn is_empty(self)
    const fn contains(self, other)
    const fn intersects(self, other)
    const fn union(self, other)
    const fn difference(self, other)
    const fn insert(self, other)
    const fn remove(self, other)
    const fn set(self, other, on)
    traits: BitOr, Clone, Copy, Debug, Default, Eq, Hash, PartialEq, StructuralPartialEq
struct           ZoomFactor
    assoc_const ONE
    const fn new(factor)
    fn from_wheel(step, notches)
    fn combine(self, rhs)
    const fn get(self)
    traits: Clone, Copy, Debug, Default, PartialEq, PartialOrd, StructuralPartialEq
struct           Corners
    assoc_const ZERO
    fn as_array(self)
    fn from_array(v)
    fn all(r)
    fn new(tl, tr, br, bl)
    fn top(r)
    fn bottom(r)
    fn left(r)
    fn right(r)
    fn top_bottom(top, bottom)
    fn diag_main(r)
    fn diag_anti(r)
    fn scaled_by(self, scale)
    const fn approx_zero(self)
    traits: Clone, Copy, Debug, Default, Deserialize, Eq, From, Hash, PartialEq, Pod, Serialize, StructuralPartialEq, Zeroable
struct           Rect
    fields: min, size
    assoc_const ZERO
    const fn new(x, y, w, h)
    const fn from_min_max(min, max)
    const fn max(self)
    const fn center(self)
    const fn area(self)
    const fn is_paint_empty(self)
    const fn contains(self, p)
    const fn contains_rect(self, other)
    const fn inflated(self, amount)
    const fn deflated(self, amount)
    fn inscribed_for_corners(self, corners)
    fn inflated_by(self, s)
    fn deflated_by(self, s)
    const fn intersects(self, other)
    const fn intersect(self, other)
    const fn clamp_to(self, bounds)
    const fn union(self, other)
    traits: Clone, Copy, Debug, Default, Hash, PartialEq, Pod, StructuralPartialEq, Zeroable
struct           Size
    fields: w, h
    assoc_const ZERO
    assoc_const INF
    const fn new(w, h)
    const fn approx_zero(self)
    const fn is_paint_empty(self)
    const fn min(self, other)
    const fn max(self, other)
    const fn scaled_by(self, factor)
    traits: Clone, Copy, Debug, Default, Deserialize, From, Hash, PartialEq, Pod, Serialize, StructuralPartialEq, Zeroable
struct           Spacing
    assoc_const ZERO
    fn as_array(self)
    fn from_array(v)
    fn all(v)
    fn xy(x, y)
    fn new(left, top, right, bottom)
    fn horizontal_sum(self)
    fn vertical_sum(self)
    fn sums(self)
    traits: Add, Clone, Copy, Debug, Default, Deserialize, Eq, From, Hash, PartialEq, Pod, Serialize, StructuralPartialEq, Zeroable
struct           Align
    const fn new(h, v)
    const fn h(h)
    const fn v(v)
    const fn halign(self)
    const fn valign(self)
    assoc_const TOP_LEFT
    assoc_const TOP
    assoc_const TOP_RIGHT
    assoc_const LEFT
    assoc_const CENTER
    assoc_const RIGHT
    assoc_const BOTTOM_LEFT
    assoc_const BOTTOM
    assoc_const BOTTOM_RIGHT
    assoc_const STRETCH
    traits: Clone, Copy, Debug, Default, Eq, Hash, PartialEq, StructuralPartialEq
enum             HAlign
    variants: Auto, Left, Center, Right, Stretch
    traits: Clone, Copy, Debug, Default, Eq, PartialEq, StructuralPartialEq
enum             VAlign
    variants: Auto, Top, Center, Bottom, Stretch
    traits: Clone, Copy, Debug, Default, Eq, PartialEq, StructuralPartialEq
struct           Anchor
    const fn at_point(point)
    const fn above(rect)
    const fn below(rect)
    const fn left_of(rect)
    const fn right_of(rect)
    const fn align(self, align)
    const fn gap(self, px)
    traits: Clone, Copy, Debug
enum             AnchorAlign
    variants: Start, Center, End
    traits: Clone, Copy, Debug, Default, Eq, PartialEq, StructuralPartialEq
enum             Axis
    variants: X, Y
    traits: Clone, Copy, Debug, Eq, Hash, PartialEq, StructuralPartialEq
enum             ClipMode
    variants: None, Rect, Rounded
    const fn is_clip(self)
    traits: Clone, Copy, Debug, Default, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, StructuralPartialEq
struct           GridCell
    fields: row, col, row_span, col_span
    const fn at(row, col)
    const fn span(self, row_span, col_span)
    const fn along(axis, main)
    traits: Clone, Copy, Debug, Default, From, Hash, PartialEq, Pod, StructuralPartialEq, Zeroable
enum             Justify
    variants: Start, Center, End, SpaceBetween, SpaceAround
    traits: Clone, Copy, Debug, Default, Eq, Hash, PartialEq, StructuralPartialEq
struct           SizeSpec
    const fn new(w, h)
    const fn w(self)
    const fn h(self)
    traits: Clone, Copy, Debug, Default, From, Hash, PartialEq
struct           Sizing
    assoc_const HUG
    assoc_const FILL
    const fn fixed(value)
    const fn fill(weight)
    const fn share(weight)
    const fn split(fraction)
    const fn fixed_value(self)
    const fn fill_weight(self)
    const fn is_hug(self)
    traits: Clone, Copy, Debug, Default, From, Hash, PartialEq, StructuralPartialEq
struct           Track
    const fn new(size)
    assoc_const HUG
    assoc_const FILL
    const fn fixed(v)
    const fn fill(weight)
    const fn min(self, min)
    const fn max(self, max)
    traits: Clone, Copy, Debug, From, Hash, PartialEq, StructuralPartialEq
enum             Visibility
    variants: Visible, Hidden, Collapsed
    const fn is_visible(self)
    const fn is_collapsed(self)
    traits: Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, StructuralPartialEq
struct           Background
    fields: fill, border, corners, shadow
    assoc_const NONE
    const fn is_noop(self)
    fn fill(brush)
    fn rounded(brush, corners)
    const fn with_border(self, border)
    const fn with_shadow(self, shadow)
    traits: Animatable, Clone, Debug, Default, Deserialize, PartialEq, Serialize, StructuralPartialEq
enum             Brush
    variants: Solid, Linear, Radial, Conic
    assoc_const TRANSPARENT
    const fn is_noop(self)
    const fn as_solid(self)
    traits: Animatable, Clone, Debug, Default, Deserialize, From, PartialEq, Serialize, StructuralPartialEq
struct           ColorRamp
    fields: stops, interp
    fn new(stops)
    fn two_stop(c0, c1)
    const fn with_interp(self, interp)
    const fn is_noop(self)
    traits: Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize, StructuralPartialEq
struct           ConicGeometry
    fields: center, start_angle
    traits: Clone, Copy, Debug, Deserialize, GradientGeometry, PartialEq, Serialize, StructuralPartialEq
type_alias       ConicGradient
type_alias       ConicGradientBuilder
struct           GradientBuilder
    fn stop(self, offset, color)
    const fn with_spread(self, spread)
    const fn with_interp(self, interp)
    fn build(self)
    traits: Clone, Debug, From
struct           LinearGeometry
    fields: angle
    traits: Clone, Copy, Debug, Deserialize, GradientGeometry, PartialEq, Serialize, StructuralPartialEq
type_alias       LinearGradient
type_alias       LinearGradientBuilder
struct           RadialGeometry
    fields: center, radius
    traits: Clone, Copy, Debug, Deserialize, GradientGeometry, PartialEq, Serialize, StructuralPartialEq
type_alias       RadialGradient
type_alias       RadialGradientBuilder
struct           GradientStops
    fn new(stops)
    traits: Clone, Copy, Debug, Deref, Deserialize, Eq, Hash, PartialEq, Serialize, StructuralPartialEq
struct           Stop
    fn new(offset, color)
    const fn offset(self)
    const fn color(self)
    traits: Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, StructuralPartialEq
struct           Gradient
    fields: geometry, ramp, spread
    fn builder(center, start_angle)
    fn new(center, start_angle, stops)
    fn two_stop(c0, c1)
    fn builder(angle)
    fn new(angle, stops)
    fn two_stop(angle, c0, c1)
    fn builder(center, radius)
    fn new(center, radius, stops)
    fn two_stop(c0, c1)
    const fn with_spread(self, spread)
    const fn with_interp(self, interp)
    const fn is_noop(self)
    traits: Clone, Debug, Deserialize, From, Hash, PartialEq, Serialize, StructuralPartialEq
trait            GradientGeometry
    items: DEFAULT_INTERP, axis_lanes, hash_geometry, has_nan
enum             Interp
    variants: Oklab, Linear
    traits: Clone, Copy, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize, StructuralPartialEq
enum             Spread
    variants: Pad, Repeat, Reflect
    traits: Clone, Copy, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize, StructuralPartialEq
struct           RgbaF32
    fields: r, g, b, a
    assoc_const TRANSPARENT
    assoc_const WHITE
    assoc_const BLACK
    const fn is_noop(self)
    const fn new(r, g, b, a)
    const fn srgb(r, g, b)
    const fn srgba(r, g, b, a)
    const fn with_alpha(self, a)
    fn lerp(self, other, t)
    const fn from_srgba(bytes)
    const fn hex(rgb)
    const fn hexa(rgba)
    fn to_srgba_u8(self)
    traits: Animatable, Clone, Copy, Debug, Default, Deserialize, From, FromStr, Hash, PartialEq, Pod, Serialize, StructuralPartialEq, Zeroable
enum             ColorCoords
    variants: Okhsv, Hsv
    fn new(model, color, fallback_hue)
    const fn model(self)
    fn to_color(self)
    fn with_model(self, model)
    const fn hue(self)
    const fn sat(self)
    const fn val(self)
    const fn set_hue(self, h)
    const fn set_sat(self, s)
    const fn set_val(self, v)
    traits: Clone, Copy, Debug, Default, PartialEq, StructuralPartialEq
enum             ColorModel
    variants: Okhsv, Hsv
    assoc_const ALL
    const fn label(self)
    fn slice(self, hue)
    traits: Clone, Copy, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize, StructuralPartialEq
enum             HueSlice
    variants: Okhsv, Hsv
    fn color(self, s, v)
    traits: Clone, Copy, Debug
struct           Hsv
    fields: h, s, v
    const fn new(h, s, v)
    fn to_color(self)
    fn from_color(color, fallback_hue)
    traits: Clone, Copy, Debug, Default, PartialEq, StructuralPartialEq
struct           Okhsv
    fields: h, s, v
    const fn new(h, s, v)
    fn to_color(self)
    fn slice(hue)
    fn from_color(color, fallback_hue)
    traits: Clone, Copy, Debug, Default, PartialEq, StructuralPartialEq
struct           OkhsvSlice
    fn color(self, s, v)
    traits: Clone, Copy, Debug
struct           SrgbaU8
    fields: r, g, b, a
    const fn new(r, g, b, a)
    const fn rgb(r, g, b)
    const fn hex(rgb)
    const fn hexa(rgba)
    traits: Clone, Copy, Debug, Default, Eq, From, Hash, PartialEq, Pod, StructuralPartialEq, Zeroable
struct           Image
    fn from_srgba8(size, pixels)
    fn blank(size)
    const fn size(self)
    fn texels(self)
    fn texels_mut(self)
    fn fill_with(self, texel)
    fn row_mut(self, row)
    fn repeat_row(self, row)
    traits: Clone, Debug, Eq, PartialEq, StructuralPartialEq
enum             ImageDownsample
    variants: Single, Mean, Peak
    traits: Clone, Copy, Debug, Default, Eq, PartialEq, StructuralPartialEq
enum             ImageFilter
    variants: Linear, Nearest
    traits: Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, StructuralPartialEq
enum             ImageFit
    variants: Fill, Contain, Cover, None, Tile
    traits: Clone, Copy, Debug, Default, PartialEq, StructuralPartialEq
struct           Shadow
    fields: color, offset, blur, spread, inset
    assoc_const NONE
    const fn drop(color, offset, blur)
    const fn with_spread(self, spread)
    const fn inset(self)
    const fn is_noop(self)
    traits: Animatable, Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize, StructuralPartialEq
struct           InternedStr
    const fn is_empty(self)
    traits: Clone, Copy, Debug, From
enum             TextInput
    variants: Borrowed, Owned, Interned
    traits: Debug, Default, From
enum             Layer
    variants: Main, Popup, Modal, Menu, Tooltip, Debug
    traits: Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, StructuralPartialEq
extern-reexport  IVec2  -> glam::IVec2
extern-reexport  UVec2  -> glam::UVec2
extern-reexport  Vec2  -> glam::Vec2
struct           GpuFrameCtx
    fields: device, queue, encoder, target, size_px, full_px, offset_px, display_scale, raster_scale, dt
    traits: Debug
struct           GpuInitCtx
    fields: device, target_format, text
    traits: Debug
struct           IconHandle
    const fn view_box(self)
    traits: Clone, Copy, Debug, PartialEq, StructuralPartialEq
struct           IconSet
    fn handle(self, icon)
    fn by_name(self, name)
    fn shape(self, icon)
    traits: Clone, Debug
struct           IconDef
    fields: name, view_box, svg, tintable, filtered
    traits: Clone, Copy, Debug
struct           IconId
    traits: Clone, Copy, Debug, Eq, Hash, PartialEq, StructuralPartialEq
struct           IconTable
    const fn baked(icons, svg)
    fn from_svgs(sources)
    traits: Debug
struct           TranslateScale
    assoc_const IDENTITY
    const fn is_identity(self)
    const fn new(translation, scale)
    const fn from_translation(t)
    const fn from_scale(s)
    const fn from_scale_about(center, s)
    const fn from_translate_scale_about(translation, center, s)
    const fn anchored_at(self, origin)
    const fn compose(self, other)
    const fn apply_point(self, p)
    const fn inverse_vector(self, v)
    const fn apply_rect(self, r)
    traits: Clone, Copy, Debug, Default, PartialEq, StructuralPartialEq
struct           WidgetId
    fn from_hash(h)
    fn with(self, h)
    fn auto_stable()
    traits: Clone, Copy, Debug, Default, Eq, Hash, PartialEq, Pod, StructuralPartialEq, Zeroable
struct           Stroke
    fields: color, width
    assoc_const ZERO
    const fn is_noop(self)
    const fn new(color, width)
    traits: Animatable, Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize, StructuralPartialEq
struct           ImageLoadError
    fields: size, max_dimension
    traits: Clone, Copy, Debug, Display, Eq, Error, PartialEq, StructuralPartialEq
trait            GpuPaint
    items: init, paint
struct           ImageHandle
    fn size(self)
    fn update(self, image)
    traits: Clone, Debug
enum             FontLoadError
    variants: Io, NoFaces
    traits: Debug, Display, Error
struct           FontFamily
    assoc_const SANS
    assoc_const MONO
    fn named(name)
    fn name(self)
    traits: Clone, Copy, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize, StructuralPartialEq
enum             FontScope
    variants: Bundled, System
    traits: Clone, Copy, Debug, Eq, PartialEq, StructuralPartialEq
enum             FontSlant
    variants: Normal, Italic
    traits: Clone, Copy, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize, StructuralPartialEq
enum             FontSource
    variants: Bytes, File
    traits: Clone, Debug, From
struct           FontWeight
    assoc_const THIN
    assoc_const EXTRA_LIGHT
    assoc_const LIGHT
    assoc_const REGULAR
    assoc_const MEDIUM
    assoc_const SEMI_BOLD
    assoc_const BOLD
    assoc_const EXTRA_BOLD
    assoc_const BLACK
    const fn new(weight)
    const fn value(self)
    traits: Clone, Copy, Debug, Default, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, StructuralPartialEq
struct           TextShaper
    fn new()
    fn with_fonts(scope)
    fn load_font(self, source)
    fn font_available(self, family)
    fn font_families(self)
    fn glyphs(self)
    traits: Clone, Debug, Default
enum             TextWrap
    variants: SingleLine, Scroll, Truncate, Ellipsis, Wrap, WrapWithOverflow
    traits: Clone, Copy, Debug, Default, Eq, Hash, PartialEq, StructuralPartialEq
struct           Ui
    const fn theme(self)
    fn set_theme(self, theme)
    const fn watch_pointer(self, flags)
    const fn watch_keyboard(self, flags)
    fn watch_key(self, sc)
    fn pointer_events(self)
    fn keyboard_events(self)
    fn key_pressed(self, sc)
    fn escape_pressed(self)
    fn request_relayout(self)
    const fn now(self)
    const fn set_cursor(self, cursor)
    const fn cursor(self)
    const fn set_vsync(self, vsync)
    const fn vsync(self)
    fn request_repaint(self)
    fn request_repaint_after(self, after)
    fn open_window(self, token, config)
    fn close_window(self, token)
    const fn close_requested(self)
    const fn keep_open(self)
    fn window_geometry(self)
    fn debug_overlay(self)
    fn set_debug_overlay(self, overlay)
    fn window_open(self, token)
    fn add_shape(self, shape)
    fn load_icons(self, table)
    fn load_image(self, image)
    fn load_font(self, source)
    fn font_available(self, family)
    fn font_families(self)
    const fn max_image_dimension(self)
    fn clipboard(self)
    fn fmt(self, args)
    fn intern(self, text)
    fn add_shape_animated(self, shape, anim)
    fn layer(self, layer)
    fn release_input_scope(self, id)
    fn response_for(self, id)
    fn state_or_default(self, id)
    fn with_state(self, id, body)
    fn state(self, id)
    fn state_mut(self, id)
    fn animate(self, id, slot, target, spec)
    const fn focused_id(self)
    fn focus_within(self, ancestor)
    fn hover_within(self, ancestor)
    const fn display(self)
    fn user_scale(self)
    fn set_user_scale(self, scale)
    const fn frame_id(self)
    const fn render_frame_id(self)
    fn probe_text(self, run)
    fn pointer_actions(self)
    const fn set_focus(self, id)
    const fn clear_focus(self)
    const fn pointer_pos(self)
    fn pointer_local(self, id)
    const fn modifiers(self)
    const fn peek_pointer_pos(self)
    fn peek_pointer_local(self, id)
    const fn peek_modifiers(self)
    const fn focus_policy(self)
    const fn set_focus_policy(self, p)
    const fn input_policy(self)
    const fn set_input_policy(self, p)
    traits: Debug
enum             FramePaint
    variants: Skip, Full, Partial
    traits: Clone, Copy, Debug, Eq, PartialEq, StructuralPartialEq
struct           FrameReport
    fields: repaint_requested, repaint_after
    const fn paint(self)
    traits: Debug
struct           LayerScope
    const fn fixed_at(self, point)
    const fn anchored(self, anchor)
    fn max_size(self, size)
    fn show(self, body)
    traits: Debug
trait            Configure
    items: configure, id_salt, id, auto_id, size, min_size, max_size, padding, margin, transform, position, grid_cell, gap, line_gap, justify, align, child_align, sense, add_sense, disabled, focusable, input_scope, visibility, hidden, collapsed, clip, clip_rect, clip_rounded
struct           OverlayResponse
    fields: dismissed, close_requested, inner
    const fn closed(self)
    traits: Clone, Copy, Debug, Default
struct           InnerResponse
    fields: response, inner
    traits: Debug
struct           Response
    fields: id
    fn eager(id, ui, state)
    fn snapshot(self)
    traits: Debug, Deref
struct           ResponseSnapshot
    fields: id, state
    traits: Clone, Copy, Debug, Deref
struct           SelectResponse
    fields: response, changed
    traits: Debug
struct           ValueResponse
    fields: response, changed, committed
    traits: Debug
struct           WidgetLook
    fields: background, text
    fn to_animated(self, ambient_text)
    traits: Clone, Debug, Default, Deserialize, PartialEq, Serialize, StructuralPartialEq
struct           AnimatedLook
    fields: background, text
    traits: Animatable, Clone, Debug, Default, PartialEq, StructuralPartialEq
struct           StatefulLook
    fields: normal, hovered, active, disabled
    const fn pick(self, state, active)
    traits: Clone, Debug, Default, Deserialize, PartialEq, Serialize, StructuralPartialEq
struct           SlotDefaults
    fields: padding, margin, anim
    traits: Clone, Copy, Debug, Deserialize, Serialize
struct           Block
    fn new()
    fn show(self, ui)
    const fn background(self, bg)
    traits: Configure, Debug
struct           Button
    fn new()
    fn style(self, s)
    fn label(self, label)
    const fn text_wrap(self, wrap)
    const fn text_align(self, a)
    fn show(self, ui)
    traits: Configure, Debug
struct           Checkbox
    fn new(value)
    fn label(self, label)
    fn style(self, s)
    fn show(self, ui)
    traits: Configure, Debug
struct           CloseHandle
    fn close(self)
    const fn requested(self)
    traits: Debug, Default
struct           ColorButton
    fn new(color)
    const fn alpha(self, on)
    const fn model(self, model)
    const fn history(self, on)
    fn style(self, s)
    fn show(self, ui)
    traits: Configure, Debug
struct           ColorField
    fn new(coords)
    fn downsample(self, n)
    fn style(self, s)
    fn show(self, ui)
    traits: Configure, Debug
struct           ColorPicker
    fn new(color)
    const fn alpha(self, on)
    const fn model(self, model)
    const fn history(self, on)
    const fn swatches(self, colors)
    fn downsample(self, n)
    fn style(self, s)
    fn show(self, ui)
    traits: Configure, Debug
struct           ColorStrip
    fn for_hue(coords)
    fn for_alpha(color)
    fn downsample(self, n)
    fn style(self, s)
    fn show(self, ui)
    traits: Configure, Debug
struct           ColorSwatch
    fn new(color)
    fn style(self, s)
    fn show(self, ui)
    traits: Configure, Debug
struct           ComboBox
    fn new(selected, options)
    fn labeled(selected, options, label)
    fn style(self, s)
    fn button_style(self, s)
    fn show(self, ui)
    traits: Configure, Debug
struct           ContextMenu
    fn for_id(for_id)
    fn style(self, s)
    fn attach(ui, snapshot)
    fn show(self, ui, body)
    fn open(ui, for_id, point)
    fn close(ui, for_id)
    fn is_open(ui, for_id)
    const fn background(self, bg)
    traits: Configure, Debug
struct           MenuItem
    fn new(label)
    fn style(self, s)
    const fn shortcut(self, s)
    const fn shortcut_hint(self, shortcut)
    fn separator()
    fn show(self, ui, popup)
    traits: Configure, Debug
struct           MenuSeparator
    fn new()
    fn style(self, s)
    fn show(self, ui)
    traits: Configure, Debug
enum             AllowedSplits
    variants: All, Row, Column, None
    fn allows(self, side)
    traits: Clone, Copy, Debug, Default, Eq, PartialEq, StructuralPartialEq
enum             DockNode
    variants: Split, Group
    traits: Clone, Debug, Deserialize, PartialEq, Serialize, StructuralPartialEq
struct           DockSplit
    fields: dir, ratio, first, second
    traits: Clone, Copy, Debug, Deserialize, PartialEq, Serialize, StructuralPartialEq
struct           NodeIdx
    traits: Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize, StructuralPartialEq
enum             DockDrop
    variants: Into, Split
    traits: Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, StructuralPartialEq
enum             DockOp
    variants: ActivateTab, OpenTab, CloseTab, MoveTab, SetRatio, FocusPane
    traits: Clone, Copy, Debug, Deserialize, PartialEq, Serialize, StructuralPartialEq
struct           DockPath
    assoc_const ROOT
    fn first(self)
    fn second(self)
    traits: Clone, Copy, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize, StructuralPartialEq
struct           DockState
    assoc_const ROOT
    assoc_const RATIO_MIN
    assoc_const RATIO_MAX
    fn new(seed, pinned)
    fn max_depth(self, depth)
    const fn allowed_splits(self, allowed)
    const fn pinned(self)
    const fn focused(self)
    fn node(self, idx)
    fn groups(self)
    fn all_tabs(self)
    fn active_tabs(self)
    fn primary(self)
    fn find_tab(self, tab)
    fn group(self, id)
    fn apply(self, op)
    fn find_or_insert(self, tab, group)
    fn retain_tabs(self, keep)
    fn can_split(self, group)
    fn dock_id(self)
    fn pane_id(self, group)
    fn content_id(self, group)
    fn strip_id(self, group)
    fn splitter_id(self, path)
    fn tab_key(tab)
    fn scan(self, ui, ops)
    fn content_size(self, ui, group)
    traits: Clone, Debug, Deserialize, PartialEq, Serialize, StructuralPartialEq
struct           TabAddress
    fields: group, index
    traits: Clone, Copy, Debug, Eq, PartialEq, StructuralPartialEq
trait            DockTab
    items: 
struct           DockTabMenu
    fields: tab, group, ops, close
    traits: Debug
trait            DockTabs
    items: Tab, title, content, closable, draggable, badge, icon, tab_menu
struct           DockView
    fn new(state, ops)
    const fn min_pane(self, px)
    const fn overflow(self, overflow)
    fn style(self, s)
    fn show(self, ui, tabs)
    fn run(ui, state, tabs)
    traits: Configure, Debug
enum             DockError
    variants: NonCanonical, NodeOutOfRange, SplitNesting, SplitRatio, UnreachableSlots, MissingPinnedTab, DuplicateGroup, EmptyGroup, ActiveTabOutOfRange, DuplicateTab, MissingFocusedGroup, GroupAllocator
    traits: Clone, Copy, Debug, Display, Error, PartialEq, StructuralPartialEq
enum             SplitDir
    variants: Row, Column
    traits: Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, StructuralPartialEq
enum             SplitSide
    variants: Left, Right, Top, Bottom
    const fn dir(self)
    traits: Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, StructuralPartialEq
struct           TabGroup
    fields: id, tabs, active
    fn active_tab(self)
    traits: Clone, Debug, Deserialize, PartialEq, Serialize, StructuralPartialEq
struct           TabGroupId
    traits: Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize, StructuralPartialEq
enum             DragNum
    variants: I64, F64
    traits: Debug, From
struct           DragValue
    fn new(value)
    const fn speed(self, speed)
    const fn range(self, range)
    const fn decimals(self, n)
    const fn suffix(self, s)
    const fn editable(self, on)
    fn style(self, s)
    fn show(self, ui)
    traits: Configure, Debug
struct           Expander
    fn new(label)
    const fn start_open(self, open)
    const fn open(self, open)
    const fn keep_body(self, keep)
    fn style(self, s)
    fn show(self, ui, body)
    traits: Configure, Debug
struct           ExpanderResponse
    fields: response, inner, toggled, openness
    traits: Debug
struct           GpuView
    fn new(paint)
    const fn repaint(self, repaint)
    fn show(self, ui)
    traits: Configure, Debug
struct           Grid
    fn new()
    fn rows(self, rows)
    fn cols(self, cols)
    const fn background(self, bg)
    fn show(self, ui, body)
    traits: Configure, Debug
struct           Modal
    fn new()
    fn style(self, s)
    const fn backdrop(self, c)
    fn show(self, ui, body)
    const fn background(self, bg)
    traits: Configure, Debug
struct           Panel
    fn show(self, ui, body)
    fn hstack()
    fn vstack()
    fn wrap_hstack()
    fn wrap_vstack()
    fn zstack()
    fn canvas()
    const fn background(self, bg)
    traits: Configure, Debug
struct           Popup
    fn new(anchor)
    fn below(rect)
    fn above(rect)
    fn left_of(rect)
    fn right_of(rect)
    const fn layer(self, layer)
    const fn click_outside(self, m)
    fn default_background(self, bg)
    const fn anchored(self, anchor)
    fn show(self, ui, body)
    const fn background(self, bg)
    traits: Configure, Debug
enum             ClickOutside
    variants: Block, Dismiss, PassThrough
    traits: Clone, Copy, Debug, Eq, PartialEq, StructuralPartialEq
struct           ProgressBar
    fn new(fraction)
    fn style(self, s)
    fn show(self, ui)
    traits: Configure, Debug
struct           RadioButton
    fn new(current, value)
    fn label(self, label)
    fn style(self, s)
    fn show(self, ui)
    traits: Configure, Debug
struct           Scroll
    fn vertical()
    fn horizontal()
    fn both()
    fn pan_by(self, delta)
    fn zoom_by(self, factor)
    fn style(self, s)
    const fn bar_mode(self, mode)
    const fn overlay_bars(self)
    const fn hide_bars(self)
    fn content_margin(self, m)
    fn zoomable(self)
    fn zoomable_with(self, cfg)
    fn show(self, ui, body)
    const fn background(self, bg)
    traits: Configure, Debug
enum             BarMode
    variants: Reserved, Overlay, Hidden
    traits: Clone, Copy, Debug, Default, Eq, PartialEq, StructuralPartialEq
struct           ZoomConfig
    fields: modifier, pivot
    fn new(range, step)
    traits: Clone, Debug, Default
enum             ZoomModifier
    variants: Ctrl, Always, PinchOnly
    traits: Clone, Copy, Debug, Eq, PartialEq, StructuralPartialEq
enum             ZoomPivot
    variants: Pointer, Center
    traits: Clone, Copy, Debug, Eq, PartialEq, StructuralPartialEq
struct           Separator
    fn horizontal()
    fn vertical()
    const fn from_widget(widget, axis)
    fn style(self, s)
    const fn thickness(self, px)
    const fn color(self, c)
    fn show(self, ui)
    traits: Configure, Debug
struct           Slider
    fn new(value, range)
    const fn step(self, step)
    const fn decimals(self, n)
    fn style(self, s)
    fn show(self, ui)
    traits: Configure, Debug
struct           Spinner
    fn new()
    fn style(self, s)
    const fn diameter(self, px)
    const fn color(self, c)
    const fn thickness(self, px)
    fn show(self, ui)
    traits: Configure, Debug
struct           Splitter
    fn horizontal(ratio)
    fn vertical(ratio)
    const fn min_pane(self, px)
    fn style(self, s)
    fn show(self, ui, body)
    traits: Configure, Debug
enum             SplitHalf
    variants: First, Second
    traits: Clone, Copy, Debug, Eq, PartialEq, StructuralPartialEq
struct           Switch
    fn new(value)
    fn label(self, label)
    fn style(self, s)
    fn show(self, ui)
    traits: Configure, Debug
enum             TabBadge
    variants: None, Idle, On
    fn reserved(self)
    fn inked(self)
    traits: Clone, Copy, Debug, Default, Eq, PartialEq, StructuralPartialEq
struct           TabItem
    fields: key, label, closable, draggable, badge, icon
    const fn new(key, label)
    traits: Clone, Copy, Debug
enum             TabOverflow
    variants: Scroll, Menu
    traits: Clone, Copy, Debug, Default, Eq, PartialEq, StructuralPartialEq
struct           TabStrip
    fn new(items)
    fn selected(self, selected)
    const fn focused(self, focused)
    const fn overflow(self, overflow)
    fn style(self, s)
    fn chip_id(strip, key)
    fn close_id(strip, key)
    fn show(self, ui)
    traits: Configure, Debug
struct           TabStripResponse
    fields: response, clicked, keyed, menu_picked, closed, drag_started, drag_stopped
    fn activated(self)
    traits: Debug
struct           TabbedView
    fn new(selected, options)
    fn labeled(selected, options, label)
    const fn closable(self, closable)
    const fn reorderable(self, reorderable)
    const fn overflow(self, overflow)
    fn style(self, s)
    fn show(self, ui, body)
    traits: Configure, Debug
struct           TabbedViewResponse
    fields: response, action
    traits: Debug
enum             TabsAction
    variants: Activated, Closed, Reordered
    traits: Clone, Copy, Debug, Eq, PartialEq, StructuralPartialEq
struct           Text
    fn new(text)
    fn style(self, s)
    const fn color(self, color)
    const fn font_size(self, px)
    const fn line_height(self, mult)
    const fn family(self, family)
    const fn weight(self, weight)
    const fn slant(self, slant)
    const fn bold(self)
    const fn italic(self)
    const fn text_wrap(self, wrap)
    const fn text_align(self, a)
    fn show(self, ui)
    traits: Configure, Debug
struct           TextEdit
    fn new(text)
    fn style(self, s)
    const fn color(self, color)
    const fn font_size(self, px)
    const fn line_height(self, mult)
    const fn family(self, family)
    const fn weight(self, weight)
    const fn slant(self, slant)
    const fn bold(self)
    const fn italic(self)
    const fn select_all_on_focus(self)
    const fn escape_falls_through(self)
    const fn max_chars(self, n)
    const fn text_align(self, a)
    const fn multiline(self, on)
    const fn placeholder(self, s)
    fn adopt_placement(self, from)
    fn show(self, ui)
    traits: Configure, Debug
struct           TextEditResponse
    fields: response, changed, submitted, cancelled, gained_focus, lost_focus
    traits: Debug
struct           Theme
    fields: button, checkbox, radio, switch, scrollbar, text_edit, drag_value, context_menu, combo_box, modal, color_picker, tooltip, progress_bar, separator, slider, spinner, splitter, tabs, dock, expander, text, window_clear, panel_background, panel_clip
    fn scale_text(self, factor)
    fn from_palette(p)
    traits: Clone, Debug, Default, Deserialize, Serialize
struct           ButtonTheme
    fields: looks, defaults
    fn from_palette(p)
    fn menu_button(p)
    const fn pick(self, state)
    traits: Clone, Debug, Default, Deserialize, Serialize, ThemeSlot
struct           ColorPickerTheme
    fields: field_width, field_height, bar_thickness, chip_size, swatch_size, handle_radius, handle_width, handle_outer, handle_inner, checker_light, checker_dark, checker_cell, border, border_width, gap, popup, popup_padding, value, hex, label
    fn from_palette(p)
    traits: Clone, Debug, Default, Deserialize, Serialize
struct           ComboBoxTheme
    fields: gap, arrow_size, arrow_stroke
    const fn from_palette(_p)
    traits: Clone, Debug, Default, Deserialize, Serialize
struct           ContextMenuTheme
    fields: panel, padding, min_width, gap, item, separator
    fn with_radius(self, panel, chip)
    fn from_palette(p)
    traits: Clone, Debug, Default, Deserialize, Serialize
struct           MenuItemTheme
    fields: looks, shortcut, gap, defaults
    const fn pick(self, state)
    fn from_palette(p)
    traits: Clone, Debug, Default, Deserialize, Serialize, ThemeSlot
struct           DockTheme
    fields: preview_fill, preview_stroke, preview_corner, caret_width, ghost, ghost_padding, ghost_offset, edge_fraction
    fn from_palette(p)
    traits: Clone, Debug, Default, Deserialize, Serialize
struct           DragValueTheme
    fields: chip, editor
    fn from_chip(chip, text_edit)
    fn from_palette(p)
    traits: Clone, Debug, Default, Deserialize, Serialize
struct           ExpanderTheme
    fields: looks, arrow_size, arrow_radius, arrow_closed_angle, arrow_open_angle, gap, indent, body_padding, defaults
    fn from_palette(p)
    traits: Clone, Debug, Default, Deserialize, Serialize, ThemeSlot
struct           ModalTheme
    fields: panel, backdrop, padding, min_width
    fn from_palette(p)
    traits: Clone, Debug, Default, Deserialize, Serialize
struct           Palette
    fields: text, text_muted, text_disabled, window_bg, elem, elem_mid, elem_strong, border_focused, accent
    assoc_const DEFAULT
    const fn border_soft(self)
    const fn border_mid(self)
    const fn border_strong(self)
    fn popup_panel(self)
    traits: Clone, Debug, Default, Deserialize, PartialEq, Serialize, StructuralPartialEq
struct           ProgressBarTheme
    fields: track, fill, thickness
    const fn from_palette(p)
    traits: Clone, Debug, Default, Deserialize, Serialize
struct           ScrollbarTheme
    fields: thickness, gap, min_thumb_px, track, thumb, thumb_hovered, thumb_active
    fn from_palette(p)
    traits: Clone, Debug, Default, Deserialize, Serialize
struct           SeparatorTheme
    fields: color, thickness, margin
    const fn from_palette(p)
    fn menu_separator(p)
    traits: Clone, Debug, Default, Deserialize, Serialize
struct           SliderTheme
    fields: track, fill, knob, knob_size, track_thickness
    const fn from_palette(p)
    traits: Clone, Debug, Default, Deserialize, Serialize
struct           SpinnerTheme
    fields: color, diameter, sweep, speed, thickness_ratio, min_thickness
    const fn from_palette(p)
    traits: Clone, Debug, Default, Deserialize, Serialize
struct           SplitterTheme
    fields: grab_thickness, rule, rule_thickness, hovered, active
    const fn from_palette(p)
    traits: Clone, Debug, Default, Deserialize, Serialize
struct           TabsTheme
    fields: active, inactive, accent, accent_idle, accent_thickness, strip, strip_padding, gap, hline, hline_thickness, corner, chip_padding, trailing_inset, min_width, max_width, close, close_size, badge, badge_size, label_gap, defaults
    const fn pick(self, state, selected)
    const fn cap(self, focused)
    fn from_palette(p)
    traits: Clone, Debug, Default, Deserialize, Serialize, ThemeSlot
struct           TextEditTheme
    fields: looks, placeholder, caret, caret_width, selection, defaults
    const fn pick(self, state)
    fn corner_centring(self, text, at)
    fn from_palette(p)
    traits: Clone, Debug, Default, Deserialize, Serialize, ThemeSlot
struct           TextStyle
    fields: font_size_px, color, line_height_mult, family, weight, slant
    fn font(self)
    fn line_height_for(self, font_size_px)
    const fn with_font_size(self, px)
    const fn with_color(self, c)
    const fn with_line_height_mult(self, mult)
    const fn with_weight(self, weight)
    const fn with_slant(self, slant)
    const fn bold(self)
    const fn italic(self)
    traits: Animatable, Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize, StructuralPartialEq
struct           TextStyleOverrides
    fields: color, font_size_px, line_height_mult, family, weight, slant
    assoc_const NONE
    fn apply(self, base)
    const fn with_font_size(self, px)
    const fn with_color(self, c)
    const fn with_line_height_mult(self, mult)
    const fn with_weight(self, weight)
    const fn with_slant(self, slant)
    traits: Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize, StructuralPartialEq
struct           ToggleTheme
    fields: unchecked, checked, indicator, box_size, indicator_stroke, check_pts, indicator_inset, gap, track_aspect, defaults
    const fn pick(self, state, checked)
    fn checkbox(p)
    fn radio(p)
    fn switch(p)
    traits: Clone, Debug, Deserialize, Serialize, ThemeSlot
struct           TooltipTheme
    fields: panel, text, padding, max_size, delay, warmup, gap
    fn from_palette(p)
    traits: Clone, Debug, Default, Deserialize, Serialize
struct           Tooltip
    fn on(snapshot)
    fn style(self, s)
    fn label(self, label)
    const fn delay(self, delay)
    const fn when_disabled(self, yes)
    fn show(self, ui)
    const fn background(self, bg)
    traits: Configure, Debug
struct           TooltipResponse
    fields: visible
    traits: Clone, Copy, Debug
enum             CursorIcon
    variants: Default, Pointer, Text, Grab, Grabbing, Move, Crosshair, EwResize, NsResize, NotAllowed
    traits: Clone, Copy, Debug, Default, Eq, PartialEq, StructuralPartialEq
enum             Vsync
    variants: On, Off
    traits: Clone, Copy, Debug, Default, Eq, PartialEq, StructuralPartialEq
struct           WindowConfig
    fields: title, inner_size, min_inner_size, placement, icon, app_id
    fn new(title)
    const fn inner_size(self, size)
    const fn min_inner_size(self, size)
    const fn position(self, position)
    const fn placement(self, placement)
    const fn maximized(self, maximized)
    fn icon(self, icon)
    fn app_id(self, app_id)
    traits: Clone, Debug, Default
struct           WindowGeometry
    fields: inner_size, placement
    traits: Clone, Copy, Debug, Default
struct           WindowPlacement
    fields: position, maximized
    traits: Clone, Copy, Debug, Default, Eq, PartialEq, StructuralPartialEq
struct           WindowToken
    traits: Clone, Copy, Debug, Eq, Hash, PartialEq, StructuralPartialEq
macro            fmt
```
