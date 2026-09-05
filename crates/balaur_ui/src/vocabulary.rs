//! The words and keys the `widget` component and the `ui.*` calls spell,
//! written once so a schema, its reader, the matchers and the script
//! constants cannot disagree about a word.

use balaur_core::components::ComponentDef;

/// The closed word sets: what a `kind`, `anchor`, `align` or `font` may be.
pub(crate) mod words {
    pub(crate) const LABEL: &str = "label";
    pub(crate) const BUTTON: &str = "button";
    pub(crate) const PANEL: &str = "panel";
    pub(crate) const ROW: &str = "row";
    pub(crate) const COLUMN: &str = "column";
    pub(crate) const SCROLL: &str = "scroll";
    pub(crate) const TAB: &str = "tab";
    pub(crate) const DRAW: &str = "draw";
    pub(crate) const IMAGE: &str = "image";
    pub(crate) const FIELD: &str = "field";
    pub(crate) const CHECK: &str = "check";
    pub(crate) const DROPDOWN: &str = "dropdown";
    pub(crate) const SLIDER: &str = "slider";
    pub(crate) const PROGRESS: &str = "progress";
    pub(crate) const GRID: &str = "grid";
    pub(crate) const FLOW: &str = "flow";
    pub(crate) const FOLD: &str = "fold";
    pub(crate) const DIALOG: &str = "dialog";
    pub(crate) const SEPARATOR: &str = "separator";
    /// The widget kinds, in the order the picker offers them.
    pub(crate) const WIDGET_KINDS: &[&str] = &[
        LABEL, BUTTON, PANEL, ROW, COLUMN, SCROLL, TAB, DRAW, IMAGE, FIELD, CHECK, DROPDOWN,
        SLIDER, PROGRESS, GRID, FLOW, FOLD, DIALOG, SEPARATOR,
    ];

    pub(crate) const TOP_LEFT: &str = "top_left";
    pub(crate) const TOP_RIGHT: &str = "top_right";
    pub(crate) const BOTTOM_LEFT: &str = "bottom_left";
    pub(crate) const BOTTOM_RIGHT: &str = "bottom_right";
    pub(crate) const CENTER: &str = "center";
    pub(crate) const FILL: &str = "fill";
    /// Screen anchors.
    pub(crate) const ANCHORS: &[&str] =
        &[TOP_LEFT, TOP_RIGHT, BOTTOM_LEFT, BOTTOM_RIGHT, CENTER, FILL];

    pub(crate) const START: &str = "start";
    pub(crate) const END: &str = "end";
    pub(crate) const LEFT: &str = "left";
    /// Where a container puts its children, and where text sits.
    pub(crate) const ALIGNS: &[&str] = &[START, CENTER, END];

    pub(crate) const NORMAL: &str = "normal";
    pub(crate) const ITALIC: &str = "italic";
    /// Slant.
    pub(crate) const FONT_STYLES: &[&str] = &[NORMAL, ITALIC];

    pub(crate) const MONO: &str = "mono";
    pub(crate) const HEADING: &str = "heading";
    pub(crate) const ICON: &str = "icon";
    pub(crate) const UI: &str = "ui";

    pub(crate) const CMD: &str = "cmd";
    pub(crate) const CTRL: &str = "ctrl";
    pub(crate) const ALT: &str = "alt";
    pub(crate) const SHIFT: &str = "shift";
}

/// Every property key of the `widget` component and every option a `ui.*` call reads.
pub(crate) mod keys {
    pub(crate) const ACTIVE: &str = "active";
    pub(crate) const ALIGN: &str = "align";
    pub(crate) const ANCHOR: &str = "anchor";
    pub(crate) const AUTOFOCUS: &str = "autofocus";
    pub(crate) const BG: &str = "bg";
    pub(crate) const BREAKPOINT_COLOR: &str = "breakpoint_color";
    pub(crate) const BREAKPOINTS: &str = "breakpoints";
    pub(crate) const CHECKED: &str = "checked";
    pub(crate) const CLICKED: &str = "clicked";
    pub(crate) const CLOSABLE: &str = "closable";
    pub(crate) const COLLAPSIBLE: &str = "collapsible";
    pub(crate) const COLOR: &str = "color";
    pub(crate) const COLUMNS: &str = "columns";
    pub(crate) const CURRENT_FILL: &str = "current_fill";
    pub(crate) const CURRENT_LINE: &str = "current_line";
    pub(crate) const D: &str = "d";
    pub(crate) const DASHED: &str = "dashed";
    pub(crate) const DEADZONE: &str = "deadzone";
    pub(crate) const DECIMALS: &str = "decimals";
    pub(crate) const DRAW: &str = "draw";
    pub(crate) const FILL: &str = "fill";
    pub(crate) const FOCUSABLE: &str = "focusable";
    pub(crate) const FONT: &str = "font";
    pub(crate) const FONT_SIZE: &str = "font_size";
    pub(crate) const FONT_STYLE: &str = "font_style";
    pub(crate) const FONT_WEIGHT: &str = "font_weight";
    pub(crate) const GAP: &str = "gap";
    pub(crate) const GROW: &str = "grow";
    pub(crate) const GUTTER_COLOR: &str = "gutter_color";
    pub(crate) const GUTTER_WIDTH: &str = "gutter_width";
    pub(crate) const H: &str = "h";
    pub(crate) const HANDLE: &str = "handle";
    pub(crate) const HEIGHT: &str = "height";
    pub(crate) const HIGHLIGHT: &str = "highlight";
    pub(crate) const HOVER_FILL: &str = "hover_fill";
    pub(crate) const ICON: &str = "icon";
    pub(crate) const ICON_COLOR: &str = "icon_color";
    pub(crate) const ICON_SIZE: &str = "icon_size";
    pub(crate) const IMAGE: &str = "image";
    pub(crate) const INSET: &str = "inset";
    pub(crate) const K_COM: &str = "k_com";
    pub(crate) const K_FN: &str = "k_fn";
    pub(crate) const K_KEY: &str = "k_key";
    pub(crate) const K_NUM: &str = "k_num";
    pub(crate) const K_PUNC: &str = "k_punc";
    pub(crate) const K_STR: &str = "k_str";
    pub(crate) const K_TYPE: &str = "k_type";
    pub(crate) const KIND: &str = "kind";
    pub(crate) const KNOB: &str = "knob";
    pub(crate) const LANGUAGE: &str = "language";
    pub(crate) const LAYER: &str = "layer";
    pub(crate) const LINE_HEIGHT: &str = "line_height";
    pub(crate) const MARKUP: &str = "markup";
    pub(crate) const MAX: &str = "max";
    pub(crate) const MAX_HEIGHT: &str = "max_height";
    pub(crate) const MAX_LENGTH: &str = "max_length";
    pub(crate) const MENU: &str = "menu";
    pub(crate) const MIN: &str = "min";
    pub(crate) const MIN_HEIGHT: &str = "min_height";
    pub(crate) const MIN_WIDTH: &str = "min_width";
    pub(crate) const NUMERIC: &str = "numeric";
    pub(crate) const OFF_FILL: &str = "off_fill";
    pub(crate) const OFF_KNOB: &str = "off_knob";
    pub(crate) const ON_CHANGE: &str = "on_change";
    pub(crate) const ON_CLICK: &str = "on_click";
    pub(crate) const ON_FILL: &str = "on_fill";
    pub(crate) const ON_FOCUS: &str = "on_focus";
    pub(crate) const ON_KNOB: &str = "on_knob";
    pub(crate) const ON_SUBMIT: &str = "on_submit";
    pub(crate) const OPEN: &str = "open";
    pub(crate) const OPTIONS: &str = "options";
    pub(crate) const PADDING: &str = "padding";
    pub(crate) const PADDING_X: &str = "padding_x";
    pub(crate) const PADDING_Y: &str = "padding_y";
    pub(crate) const PLACEHOLDER: &str = "placeholder";
    pub(crate) const PREFIX: &str = "prefix";
    pub(crate) const PREFIX_COLOR: &str = "prefix_color";
    pub(crate) const PROBLEM_COLOR: &str = "problem_color";
    pub(crate) const PROBLEMS: &str = "problems";
    pub(crate) const RADIUS: &str = "radius";
    pub(crate) const RAIL: &str = "rail";
    pub(crate) const RESIZABLE: &str = "resizable";
    pub(crate) const ROLE: &str = "role";
    pub(crate) const ROUND: &str = "round";
    pub(crate) const SCRIM: &str = "scrim";
    pub(crate) const SECRET: &str = "secret";
    pub(crate) const SEPARATOR: &str = "separator";
    pub(crate) const SIZE: &str = "size";
    pub(crate) const SLICE: &str = "slice";
    pub(crate) const SOURCE: &str = "source";
    pub(crate) const SPEED: &str = "speed";
    pub(crate) const STEP: &str = "step";
    pub(crate) const STICK_TO_BOTTOM: &str = "stick_to_bottom";
    pub(crate) const STROKE: &str = "stroke";
    pub(crate) const STRONG: &str = "strong";
    pub(crate) const SUFFIX: &str = "suffix";
    pub(crate) const TEXT: &str = "text";
    pub(crate) const TEXT_ALIGN: &str = "text_align";
    pub(crate) const TEXT_COLOR: &str = "text_color";
    pub(crate) const TEXT_KEY: &str = "text_key";
    pub(crate) const THEME: &str = "theme";
    pub(crate) const TIGHT: &str = "tight";
    pub(crate) const TITLE: &str = "title";
    pub(crate) const TOOLTIP: &str = "tooltip";
    pub(crate) const TOP: &str = "top";
    pub(crate) const TRAILING: &str = "trailing";
    pub(crate) const TRAILING_COLOR: &str = "trailing_color";
    pub(crate) const TRAILING_SIZE: &str = "trailing_size";
    pub(crate) const TRANSPARENT: &str = "transparent";
    pub(crate) const TRUNCATE: &str = "truncate";
    pub(crate) const VALUE: &str = "value";
    pub(crate) const VISIBLE: &str = "visible";
    pub(crate) const W: &str = "w";
    pub(crate) const WARNING_COLOR: &str = "warning_color";
    pub(crate) const WARNINGS: &str = "warnings";
    pub(crate) const WIDTH: &str = "width";
    pub(crate) const WRAP: &str = "wrap";
    pub(crate) const X: &str = "x";
    pub(crate) const Y: &str = "y";
}

/// Schema text from `(key, spec)` lines; see [`ComponentDef::schema`].
pub(crate) fn schema(lines: &[(&str, &str)]) -> String {
    ComponentDef::schema(lines)
}

/// The words a property offers, as its `options` list.
pub(crate) fn options(words: &[&str]) -> String {
    ComponentDef::options(words)
}
