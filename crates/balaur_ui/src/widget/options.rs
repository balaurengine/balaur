//! Reading a widget's property table: the readers every key goes through,
//! and the keys past the first ones, what taffy reads besides a size and what
//! the egui widget behind each kind takes, read off a table and written back.

use crate::vocabulary::keys as k;
use crate::vocabulary::words as w;
use crate::widget::node::{Bits, EguiOptions, Layout};

/// The taffy fields a table states.
pub(crate) fn read_layout(params: &toml::Value) -> Layout {
    let r = Read(params);
    let bits = |key: &str| Bits::of(r.num(key));
    let quad = |key: &str| r.quad(key).map(Bits::of);
    Layout {
        absolute: r.flag(k::ABSOLUTE),
        max_width: bits(k::MAX_WIDTH),
        max_height: bits(k::MAX_HEIGHT),
        aspect_ratio: bits(k::ASPECT_RATIO),
        margin: quad(k::MARGIN),
        border: quad(k::BORDER),
        box_sizing: r.str(k::BOX_SIZING),
        direction: r.str(k::DIRECTION),
        overflow: r.str(k::OVERFLOW),
        scrollbar_width: bits(k::SCROLLBAR_WIDTH),
        contain: [
            balaur_core::components::has_flag(
                params.get(k::CONTAIN),
                crate::vocabulary::words::contain::LAYOUT,
            ),
            balaur_core::components::has_flag(
                params.get(k::CONTAIN),
                crate::vocabulary::words::contain::PAINT,
            ),
        ],
        align_self: r.str(k::ALIGN_SELF),
        align_content: r.str(k::ALIGN_CONTENT),
        safe_align: r.flag(k::SAFE_ALIGN),
        width_percent: bits(k::WIDTH_PERCENT),
        height_percent: bits(k::HEIGHT_PERCENT),
        wrap_children: r.str(k::WRAP_CHILDREN),
        min_lines: r.num(k::MIN_LINES).clamp(1.0, f32::from(u16::MAX)) as u16,
        basis: bits(k::BASIS),
        shrink: bits(k::SHRINK),
        grid_columns: r.str(k::GRID_COLUMNS),
        grid_rows: r.str(k::GRID_ROWS),
        auto_columns: r.str(k::AUTO_COLUMNS),
        auto_rows: r.str(k::AUTO_ROWS),
        auto_flow: r.str(k::AUTO_FLOW),
        areas: strings(params, k::AREAS),
        row: r.str(k::ROW),
        column: r.str(k::COLUMN),
    }
}

/// The egui options a table states.
pub(crate) fn read_options(params: &toml::Value) -> EguiOptions {
    let r = Read(params);
    let (s, f, b) = (
        |key: &str| r.str(key),
        |key: &str| r.num(key),
        |key: &str| r.flag(key),
    );
    EguiOptions {
        sense: s(k::SENSE),
        show_tooltip_when_elided: r.on_unless_off(k::SHOW_TOOLTIP_WHEN_ELIDED),
        indeterminate: b(k::INDETERMINATE),
        show_value: b(k::SHOW_VALUE),
        prefix: s(k::PREFIX),
        logarithmic: b(k::LOGARITHMIC),
        smallest_positive: f(k::SMALLEST_POSITIVE),
        largest_finite: f(k::LARGEST_FINITE),
        clamp: s(k::CLAMP),
        clamp_existing: b(k::CLAMP_EXISTING),
        smart_aim: r.on_unless_off(k::SMART_AIM),
        drag_speed: f(k::DRAG_SPEED),
        decimals: params
            .get(k::DECIMALS)
            .and_then(balaur_core::components::as_f64)
            .map_or(-1, |n| n as i32),
        trailing_fill: b(k::TRAILING_FILL),
        handle: s(k::HANDLE),
        handle_aspect: f(k::HANDLE_ASPECT),
        number_format: s(k::NUMBER_FORMAT),
        update_while_editing: r.on_unless_off(k::UPDATE_WHILE_EDITING),
        show_percentage: b(k::SHOW_PERCENTAGE),
        animate: b(k::ANIMATE),
        spacing: params
            .get(k::SPACING)
            .and_then(balaur_core::components::as_f64)
            .map_or(-1.0, |n| n as f32),
        overhang: f(k::OVERHANG),
        list_height: f(k::LIST_HEIGHT),
        alpha: s(k::ALPHA),
        inline: b(k::INLINE),
        editable: r.on_unless_off(k::EDITABLE),
        tab_inserts: b(k::TAB_INSERTS),
        caret_at_end: r.on_unless_off(k::CARET_AT_END),
        clip_text: r.on_unless_off(k::CLIP_TEXT),
        submit_key: s(k::SUBMIT_KEY),
        scrollbar: s(k::SCROLLBAR),
        stick_to_end: b(k::STICK_TO_END),
        scroll_offset: unset_pair(params, k::SCROLL_OFFSET, -1.0),
        min_scrolled_width: f(k::MIN_SCROLLED_WIDTH),
        min_scrolled_height: f(k::MIN_SCROLLED_HEIGHT),
        animated: r.on_unless_off(k::ANIMATED),
        wheel_speed: unset_pair(params, k::WHEEL_SPEED, 1.0),
        drag_scroll: s(k::DRAG_SCROLL),
        wheel_scroll: r.on_unless_off(k::WHEEL_SCROLL),
        drag_cursor: s(k::DRAG_CURSOR),
        tint: unset_quad(params, k::TINT, 1.0),
        region: r.quad(k::REGION),
        angle_degrees: f(k::ANGLE_DEGREES),
        angle_origin: unset_pair(params, k::ANGLE_ORIGIN, 0.5),
        alt_text: s(k::ALT_TEXT),
        popup_gap: params
            .get(k::POPUP_GAP)
            .and_then(balaur_core::components::as_f64)
            .map_or(-1.0, |n| n as f32),
        popup_width: f(k::POPUP_WIDTH),
        placement_fallbacks: strings(params, k::PLACEMENT_FALLBACKS),
        close_on: s(k::CLOSE_ON),
        backdrop_color: params
            .get(k::BACKDROP_COLOR)
            .map_or([0.0, 0.0, 0.0, 140.0 / 255.0], |_| {
                r.quad(k::BACKDROP_COLOR)
            }),
        dismissable: r.on_unless_off(k::DISMISSABLE),
        resizable: b(k::RESIZABLE),
        collapsible: b(k::COLLAPSIBLE),
        closable: r.on_unless_off(k::CLOSABLE),
        movable: b(k::MOVABLE),
        constrain: b(k::CONSTRAIN),
        default_open: r.on_unless_off(k::DEFAULT_OPEN),
        fade_in: b(k::FADE_IN),
    }
}

/// A pair a table may leave out, as the value that means "say nothing".
///
/// Defaults are merged before a table is read, so this matters only to a
/// table built by hand, which a test is.
fn unset_pair(params: &toml::Value, key: &str, unset: f32) -> [f32; 2] {
    if params.get(key).is_some() {
        pair(params, key)
    } else {
        [unset; 2]
    }
}

/// A colour a table may leave out, as the one that changes nothing.
fn unset_quad(params: &toml::Value, key: &str, unset: f32) -> [f32; 4] {
    if params.get(key).is_some() {
        Read(params).quad(key)
    } else {
        [unset; 4]
    }
}

fn float(value: f32) -> toml::Value {
    toml::Value::Float(f64::from(value))
}

fn text(value: &str) -> toml::Value {
    toml::Value::String(value.to_string())
}

fn list(values: &[smol_str::SmolStr]) -> toml::Value {
    toml::Value::Array(values.iter().map(|v| text(v)).collect())
}

/// A layout back as the properties it was read from.
pub(crate) fn layout_to_toml(layout: &Layout, map: &mut toml::map::Map<String, toml::Value>) {
    let quad = |sides: [Bits; 4]| four(sides.map(Bits::get));
    let contain = [
        crate::vocabulary::words::contain::LAYOUT,
        crate::vocabulary::words::contain::PAINT,
    ]
    .iter()
    .zip(layout.contain)
    .filter(|(_, on)| *on)
    .map(|(word, _)| text(word))
    .collect();
    for (key, value) in [
        (k::ABSOLUTE, toml::Value::Boolean(layout.absolute)),
        (k::MAX_WIDTH, float(layout.max_width.get())),
        (k::MAX_HEIGHT, float(layout.max_height.get())),
        (k::ASPECT_RATIO, float(layout.aspect_ratio.get())),
        (k::MARGIN, quad(layout.margin)),
        (k::BORDER, quad(layout.border)),
        (k::BOX_SIZING, text(&layout.box_sizing)),
        (k::DIRECTION, text(&layout.direction)),
        (k::OVERFLOW, text(&layout.overflow)),
        (k::SCROLLBAR_WIDTH, float(layout.scrollbar_width.get())),
        (k::CONTAIN, toml::Value::Array(contain)),
        (k::ALIGN_SELF, text(&layout.align_self)),
        (k::ALIGN_CONTENT, text(&layout.align_content)),
        (k::SAFE_ALIGN, toml::Value::Boolean(layout.safe_align)),
        (k::WIDTH_PERCENT, float(layout.width_percent.get())),
        (k::HEIGHT_PERCENT, float(layout.height_percent.get())),
        (k::WRAP_CHILDREN, text(&layout.wrap_children)),
        (
            k::MIN_LINES,
            toml::Value::Integer(i64::from(layout.min_lines)),
        ),
        (k::BASIS, float(layout.basis.get())),
        (k::SHRINK, float(layout.shrink.get())),
        (k::GRID_COLUMNS, text(&layout.grid_columns)),
        (k::GRID_ROWS, text(&layout.grid_rows)),
        (k::AUTO_COLUMNS, text(&layout.auto_columns)),
        (k::AUTO_ROWS, text(&layout.auto_rows)),
        (k::AUTO_FLOW, text(&layout.auto_flow)),
        (k::AREAS, list(&layout.areas)),
        (k::ROW, text(&layout.row)),
        (k::COLUMN, text(&layout.column)),
    ] {
        map.insert(key.into(), value);
    }
}

/// The egui options back as the properties they were read from.
pub(crate) fn options_to_toml(
    options: &EguiOptions,
    map: &mut toml::map::Map<String, toml::Value>,
) {
    let on = toml::Value::Boolean;
    for (key, value) in [
        (k::SENSE, text(&options.sense)),
        (
            k::SHOW_TOOLTIP_WHEN_ELIDED,
            on(options.show_tooltip_when_elided),
        ),
        (k::INDETERMINATE, on(options.indeterminate)),
        (k::SHOW_VALUE, on(options.show_value)),
        (k::PREFIX, text(&options.prefix)),
        (k::LOGARITHMIC, on(options.logarithmic)),
        (k::SMALLEST_POSITIVE, float(options.smallest_positive)),
        (k::LARGEST_FINITE, float(options.largest_finite)),
        (k::CLAMP, text(&options.clamp)),
        (k::CLAMP_EXISTING, on(options.clamp_existing)),
        (k::SMART_AIM, on(options.smart_aim)),
        (k::DRAG_SPEED, float(options.drag_speed)),
        (
            k::DECIMALS,
            toml::Value::Integer(i64::from(options.decimals)),
        ),
        (k::TRAILING_FILL, on(options.trailing_fill)),
        (k::HANDLE, text(&options.handle)),
        (k::HANDLE_ASPECT, float(options.handle_aspect)),
        (k::NUMBER_FORMAT, text(&options.number_format)),
        (k::UPDATE_WHILE_EDITING, on(options.update_while_editing)),
        (k::SHOW_PERCENTAGE, on(options.show_percentage)),
        (k::ANIMATE, on(options.animate)),
        (k::SPACING, float(options.spacing)),
        (k::OVERHANG, float(options.overhang)),
        (k::LIST_HEIGHT, float(options.list_height)),
        (k::ALPHA, text(&options.alpha)),
        (k::INLINE, on(options.inline)),
        (k::EDITABLE, on(options.editable)),
        (k::TAB_INSERTS, on(options.tab_inserts)),
        (k::CARET_AT_END, on(options.caret_at_end)),
        (k::CLIP_TEXT, on(options.clip_text)),
        (k::SUBMIT_KEY, text(&options.submit_key)),
    ] {
        map.insert(key.into(), value);
    }
    scroll_and_rest_to_toml(options, map);
}

/// The scroll, picture, popup, dialog, window and root options. Split from
/// [`options_to_toml`] under `MAX_FN_LINES`; the seam is the scroll's.
fn scroll_and_rest_to_toml(options: &EguiOptions, map: &mut toml::map::Map<String, toml::Value>) {
    let on = toml::Value::Boolean;
    for (key, value) in [
        (k::SCROLLBAR, text(&options.scrollbar)),
        (k::STICK_TO_END, on(options.stick_to_end)),
        (k::SCROLL_OFFSET, two(options.scroll_offset)),
        (k::MIN_SCROLLED_WIDTH, float(options.min_scrolled_width)),
        (k::MIN_SCROLLED_HEIGHT, float(options.min_scrolled_height)),
        (k::ANIMATED, on(options.animated)),
        (k::WHEEL_SPEED, two(options.wheel_speed)),
        (k::DRAG_SCROLL, text(&options.drag_scroll)),
        (k::WHEEL_SCROLL, on(options.wheel_scroll)),
        (k::DRAG_CURSOR, text(&options.drag_cursor)),
        (k::TINT, four(options.tint)),
        (k::REGION, four(options.region)),
        (k::ANGLE_DEGREES, float(options.angle_degrees)),
        (k::ANGLE_ORIGIN, two(options.angle_origin)),
        (k::ALT_TEXT, text(&options.alt_text)),
        (k::POPUP_GAP, float(options.popup_gap)),
        (k::POPUP_WIDTH, float(options.popup_width)),
        (k::PLACEMENT_FALLBACKS, list(&options.placement_fallbacks)),
        (k::CLOSE_ON, text(&options.close_on)),
        (k::BACKDROP_COLOR, four(options.backdrop_color)),
        (k::DISMISSABLE, on(options.dismissable)),
        (k::RESIZABLE, on(options.resizable)),
        (k::COLLAPSIBLE, on(options.collapsible)),
        (k::CLOSABLE, on(options.closable)),
        (k::MOVABLE, on(options.movable)),
        (k::CONSTRAIN, on(options.constrain)),
        (k::DEFAULT_OPEN, on(options.default_open)),
        (k::FADE_IN, on(options.fade_in)),
    ] {
        map.insert(key.into(), value);
    }
}

/// The four readers every field goes through, so a `widget_from` that grows a
/// property grows by one line rather than by a closure.
pub(crate) struct Read<'a>(pub(crate) &'a toml::Value);

/// Every reader takes the key alone: the schema declares a default for each
/// property and `add`/`patch` merge them in before this runs, so a default
/// written here too would be a second copy of one nothing checks.
impl Read<'_> {
    pub(crate) fn str(&self, key: &str) -> smol_str::SmolStr {
        balaur_core::components::prop_str(self.0, key).into()
    }

    pub(crate) fn num(&self, key: &str) -> f32 {
        balaur_core::components::prop_f32(self.0, key)
    }

    pub(crate) fn flag(&self, key: &str) -> bool {
        balaur_core::components::prop_bool(self.0, key)
    }

    /// A switch that is on unless the table turns it off, so a table that
    /// never met the schema's defaults still reads as on.
    pub(crate) fn on_unless_off(&self, key: &str) -> bool {
        self.0
            .get(key)
            .and_then(toml::Value::as_bool)
            .unwrap_or(true)
    }

    /// The first of `keys` the table names, for one field three kinds spell
    /// three ways.
    pub(crate) fn first(&self, keys: &[&str]) -> smol_str::SmolStr {
        let mut named = keys.iter().map(|key| self.str(key));
        named.find(|value| !value.is_empty()).unwrap_or_default()
    }

    /// One colour's four channels. Hex strings were expanded to floats by
    /// `merge_defaults`, so this only ever reads an array.
    pub(crate) fn quad(&self, key: &str) -> [f32; 4] {
        let channel = |i: usize| {
            self.0
                .get(key)
                .and_then(|v| v.as_array())
                .and_then(|a| a.get(i))
                .and_then(balaur_core::components::as_f64)
                .unwrap_or_default() as f32
        };
        [channel(0), channel(1), channel(2), channel(3)]
    }
}

/// Four sides from a key that takes one number for all of them or four for
/// left, top, right and bottom, so `padding = 8.0` and
/// `padding = [50.0, 0.0, 50.0, 32.0]` are both what they read as.
pub(crate) fn sides(params: &toml::Value, key: &str) -> [f32; 4] {
    let read = Read(params);
    match params.get(key).map(toml::Value::is_array) {
        Some(true) => read.quad(key),
        _ => [read.num(key); 4],
    }
}

/// The 1-based lines a code widget's gutter property names.
///
/// Numbers and the text of them both: a scene writes `["12", "40"]` under the
/// `strings` type the schema declares, and a script handed a list of line
/// numbers writes those.
pub(crate) fn lines(params: &toml::Value, key: &str) -> Vec<u32> {
    params
        .get(key)
        .and_then(toml::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| match item {
                    toml::Value::String(text) => text.trim().parse().ok(),
                    other => balaur_core::components::as_f64(other)
                        .filter(|line| *line >= 1.0)
                        .map(|line| line as u32),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The numbers a `strings` property holds, the way `lines` reads them: a
/// scene writes `["2", "1"]` under the type the schema declares, and a script
/// handed a list of shares writes those.
pub(crate) fn shares(params: &toml::Value, key: &str) -> Vec<f32> {
    params
        .get(key)
        .and_then(toml::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| match item {
                    toml::Value::String(text) => text.trim().parse().ok(),
                    other => balaur_core::components::as_f64(other).map(|share| share as f32),
                })
                .filter(|share: &f32| *share > 0.0)
                .collect()
        })
        .unwrap_or_default()
}

/// The strings a `strings` property holds, and none where it says nothing.
pub(crate) fn strings(params: &toml::Value, key: &str) -> Vec<smol_str::SmolStr> {
    params
        .get(key)
        .and_then(toml::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(toml::Value::as_str)
                .map(smol_str::SmolStr::new)
                .collect()
        })
        .unwrap_or_default()
}

/// Two numbers as the array a `vec2` property holds.
pub(crate) fn two(values: [f32; 2]) -> toml::Value {
    toml::Value::Array(
        values
            .iter()
            .map(|v| toml::Value::Float(f64::from(*v)))
            .collect(),
    )
}

/// The two numbers a `vec2` property holds.
pub(crate) fn pair(params: &toml::Value, key: &str) -> [f32; 2] {
    let quad = Read(params).quad(key);
    [quad[0], quad[1]]
}

/// Four numbers as the array a `vec4` property holds.
pub(crate) fn four(values: [f32; 4]) -> toml::Value {
    toml::Value::Array(
        values
            .iter()
            .map(|v| toml::Value::Float(f64::from(*v)))
            .collect(),
    )
}

/// Which of the four edges a `flags` property names, in `EDGES` order.
pub(crate) fn edges_of(value: Option<&toml::Value>) -> [bool; 4] {
    let mut out = [false; 4];
    for (slot, name) in out.iter_mut().zip(w::EDGES) {
        *slot = balaur_core::components::has_flag(value, name);
    }
    out
}
