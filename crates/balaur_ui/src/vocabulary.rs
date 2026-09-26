//! The words and keys the `widget` component and the `ui.*` calls spell,
//! written once so a schema, its reader, the matchers and the script
//! constants cannot disagree about a word.

use balaur_core::components::ComponentDef;

/// The closed word sets: what a `kind`, `anchor`, `align_items` or `font_family` may be.
pub(crate) mod words {
    pub(crate) const LABEL: &str = "label";
    pub(crate) const BUTTON: &str = "button";
    pub(crate) const PANEL: &str = "panel";
    pub(crate) const ROW: &str = "row";
    pub(crate) const STACK: &str = "stack";
    pub(crate) const COLUMN: &str = "column";
    pub(crate) const SCROLL: &str = "scroll";
    pub(crate) const TEXT_AREA: &str = "text_area";
    pub(crate) const TABS: &str = "tabs";
    pub(crate) const DRAW: &str = "draw";
    pub(crate) const IMAGE: &str = "image";
    pub(crate) const TEXT_FIELD: &str = "text_field";
    pub(crate) const CHECKBOX: &str = "checkbox";
    pub(crate) const SWITCH: &str = "switch";
    pub(crate) const COLOR_PICKER: &str = "color_picker";
    pub(crate) const DROPDOWN: &str = "dropdown";
    pub(crate) const MENU: &str = "menu";
    pub(crate) const LIST: &str = "list";
    pub(crate) const TREE: &str = "tree";
    pub(crate) const NUMBER_FIELD: &str = "number_field";
    pub(crate) const SLIDER: &str = "slider";
    pub(crate) const PROGRESS_BAR: &str = "progress_bar";
    pub(crate) const GRID: &str = "grid";
    pub(crate) const FLOW: &str = "flow";
    pub(crate) const FOLD: &str = "fold";
    pub(crate) const DIALOG: &str = "dialog";
    pub(crate) const WINDOW: &str = "window";
    pub(crate) const SEPARATOR: &str = "separator";
    pub(crate) const CODE: &str = "code";
    pub(crate) const TABLE: &str = "table";
    pub(crate) const TOAST: &str = "toast";
    /// The widget kinds, in the order the picker offers them.
    pub(crate) const WIDGET_KINDS: &[&str] = &[
        LABEL,
        BUTTON,
        PANEL,
        ROW,
        COLUMN,
        SCROLL,
        TABS,
        DRAW,
        IMAGE,
        TEXT_FIELD,
        TEXT_AREA,
        CHECKBOX,
        SWITCH,
        COLOR_PICKER,
        DROPDOWN,
        MENU,
        LIST,
        TREE,
        TABLE,
        SLIDER,
        NUMBER_FIELD,
        PROGRESS_BAR,
        GRID,
        FLOW,
        FOLD,
        DIALOG,
        TOAST,
        WINDOW,
        SEPARATOR,
        CODE,
        STACK,
    ];

    pub(crate) const CONTAIN: &str = "contain";
    pub(crate) const COVER: &str = "cover";
    /// How a picture sits in the box it was given; empty is the picture's own
    /// size, which is what decides the box instead.
    pub(crate) const NONE_FIT: &str = "none";
    /// A colour token that paints nothing.
    pub(crate) const NONE: &str = "none";
    pub(crate) const FITS: &[&str] = &["", CONTAIN, COVER, FILL, NONE_FIT];

    pub(crate) const TOP_LEFT: &str = "top_left";
    pub(crate) const TOP_RIGHT: &str = "top_right";
    pub(crate) const BOTTOM_LEFT: &str = "bottom_left";
    pub(crate) const BOTTOM_RIGHT: &str = "bottom_right";
    pub(crate) const CENTER: &str = "center";
    pub(crate) const CENTER_LEFT: &str = "center_left";
    pub(crate) const CENTER_RIGHT: &str = "center_right";
    pub(crate) const CENTER_TOP: &str = "center_top";
    pub(crate) const CENTER_BOTTOM: &str = "center_bottom";
    pub(crate) const FILL: &str = "fill";
    pub(crate) const FILL_TOP: &str = "fill_top";
    pub(crate) const FILL_BOTTOM: &str = "fill_bottom";
    pub(crate) const FILL_LEFT: &str = "fill_left";
    pub(crate) const FILL_RIGHT: &str = "fill_right";
    pub(crate) const FILL_ACROSS: &str = "fill_across";
    pub(crate) const FILL_DOWN: &str = "fill_down";
    /// Screen anchors: the four corners, the four edge midpoints, the middle,
    /// the whole surface, and one axis of it along an edge or the middle.
    pub(crate) const ANCHORS: &[&str] = &[
        TOP_LEFT,
        TOP_RIGHT,
        BOTTOM_LEFT,
        BOTTOM_RIGHT,
        CENTER,
        CENTER_LEFT,
        CENTER_RIGHT,
        CENTER_TOP,
        CENTER_BOTTOM,
        FILL,
        FILL_TOP,
        FILL_BOTTOM,
        FILL_LEFT,
        FILL_RIGHT,
        FILL_ACROSS,
        FILL_DOWN,
    ];

    pub(crate) const START: &str = "start";
    pub(crate) const END: &str = "end";
    pub(crate) const LEFT: &str = "left";
    /// A corner radius as round as the box is short: a pill.
    pub(crate) const FULL: &str = "full";
    pub(crate) const TOP: &str = "top";
    pub(crate) const RIGHT: &str = "right";
    pub(crate) const BOTTOM: &str = "bottom";
    /// The four edges a `safe_area` names, in the order an inset is spelled.
    pub(crate) const EDGES: &[&str] = &[LEFT, TOP, RIGHT, BOTTOM];
    /// Where a container puts its children, and where text sits.
    pub(crate) const ALIGNS: &[&str] = &[START, CENTER, END];

    pub(crate) const BOTH: &str = "both";
    pub(crate) const HORIZONTAL: &str = "horizontal";
    pub(crate) const VERTICAL: &str = "vertical";
    /// Which way a scroll moves.
    pub(crate) const AXES: &[&str] = &[BOTH, HORIZONTAL, VERTICAL];

    pub(crate) const BETWEEN: &str = "between";
    pub(crate) const AROUND: &str = "around";
    pub(crate) const EVENLY: &str = "evenly";
    /// How a container spreads its children along its own direction.
    pub(crate) const JUSTIFYS: &[&str] = &[START, CENTER, END, BETWEEN, AROUND, EVENLY];

    pub(crate) const ABOVE: &str = "above";
    pub(crate) const BELOW: &str = "below";
    pub(crate) const POINTER: &str = "pointer";
    /// Where a menu opens against the button that drops it.
    pub(crate) const PLACEMENTS: &[&str] = &[BELOW, ABOVE, POINTER, CENTER];

    pub(crate) const NORMAL: &str = "normal";
    pub(crate) const ITALIC: &str = "italic";
    /// Slant.
    pub(crate) const FONT_STYLES: &[&str] = &[NORMAL, ITALIC];

    pub(crate) const MONO: &str = "mono";
    pub(crate) const HEADING: &str = "heading";
    pub(crate) const ICON: &str = "icon";
    pub(crate) const UI: &str = "ui";
    /// The families a widget may draw in, as the picker offers them.
    pub(crate) const WIDGET_FONTS: &[&str] = &[UI, MONO, HEADING, ICON];

    /// Where a dragged row landed, as `on_move` reports it.
    pub(crate) const BEFORE: &str = "before";
    pub(crate) const INTO: &str = "into";
    pub(crate) const AFTER: &str = "after";

    pub(crate) const CMD: &str = "cmd";
    pub(crate) const CTRL: &str = "ctrl";
    pub(crate) const ALT: &str = "alt";
    pub(crate) const SHIFT: &str = "shift";
    /// The pointer's shapes, as a widget's `cursor` names them: every shape
    /// egui carries, under Balaur's names. Hiding the pointer is not a shape;
    /// that is `window.set_cursor_hidden`.
    pub(crate) mod cursor {
        pub(crate) const ARROW: &str = "arrow";
        pub(crate) const HAND: &str = "hand";
        pub(crate) const TEXT: &str = "text";
        pub(crate) const VERTICAL_TEXT: &str = "vertical_text";
        pub(crate) const CROSS: &str = "cross";
        pub(crate) const CELL: &str = "cell";
        pub(crate) const WAIT: &str = "wait";
        pub(crate) const PROGRESS: &str = "progress";
        pub(crate) const HELP: &str = "help";
        pub(crate) const CONTEXT_MENU: &str = "context_menu";
        pub(crate) const MOVE: &str = "move";
        pub(crate) const GRAB: &str = "grab";
        pub(crate) const GRABBING: &str = "grabbing";
        pub(crate) const ALIAS: &str = "alias";
        pub(crate) const COPY: &str = "copy";
        pub(crate) const NO_DROP: &str = "no_drop";
        pub(crate) const FORBIDDEN: &str = "forbidden";
        pub(crate) const ALL_SCROLL: &str = "all_scroll";
        pub(crate) const RESIZE_X: &str = "resize_x";
        pub(crate) const RESIZE_Y: &str = "resize_y";
        pub(crate) const RESIZE_N: &str = "resize_n";
        pub(crate) const RESIZE_E: &str = "resize_e";
        pub(crate) const RESIZE_S: &str = "resize_s";
        pub(crate) const RESIZE_W: &str = "resize_w";
        pub(crate) const RESIZE_NE: &str = "resize_ne";
        pub(crate) const RESIZE_NW: &str = "resize_nw";
        pub(crate) const RESIZE_SE: &str = "resize_se";
        pub(crate) const RESIZE_SW: &str = "resize_sw";
        pub(crate) const RESIZE_NESW: &str = "resize_nesw";
        pub(crate) const RESIZE_NWSE: &str = "resize_nwse";
        pub(crate) const RESIZE_COL: &str = "resize_col";
        pub(crate) const RESIZE_ROW: &str = "resize_row";
        pub(crate) const ZOOM_IN: &str = "zoom_in";
        pub(crate) const ZOOM_OUT: &str = "zoom_out";
        /// Every shape, in the order the picker offers them.
        pub(crate) const ALL: &[&str] = &[
            ARROW,
            HAND,
            TEXT,
            VERTICAL_TEXT,
            CROSS,
            CELL,
            WAIT,
            PROGRESS,
            HELP,
            CONTEXT_MENU,
            MOVE,
            GRAB,
            GRABBING,
            ALIAS,
            COPY,
            NO_DROP,
            FORBIDDEN,
            ALL_SCROLL,
            RESIZE_X,
            RESIZE_Y,
            RESIZE_N,
            RESIZE_E,
            RESIZE_S,
            RESIZE_W,
            RESIZE_NE,
            RESIZE_NW,
            RESIZE_SE,
            RESIZE_SW,
            RESIZE_NESW,
            RESIZE_NWSE,
            RESIZE_COL,
            RESIZE_ROW,
            ZOOM_IN,
            ZOOM_OUT,
        ];
    }
}

/// Every property key of the `widget` component and every option a `ui.*` call reads.
pub(crate) mod keys {
    /// Which page a `tabs` widget shows.
    pub(crate) const CURRENT_PAGE: &str = "current_page";
    pub(crate) const ALIGN_ITEMS: &str = "align_items";
    pub(crate) const ARROWS: &str = "arrows";
    pub(crate) const COLORS: &str = "colors";
    /// The theme document's asset type, the theme it is written over, its
    /// mode flag, and its table of named sizes.
    pub(crate) const TYPE: &str = "type";
    pub(crate) const BASE: &str = "base";
    pub(crate) const DARK: &str = "dark";
    pub(crate) const SIZES: &str = "sizes";
    pub(crate) const ANCHOR: &str = "anchor";
    pub(crate) const AUTOFOCUS: &str = "autofocus";
    pub(crate) const AVOID_KEYBOARD: &str = "avoid_keyboard";
    pub(crate) const BREAKPOINT_COLOR: &str = "breakpoint_color";
    pub(crate) const BREAKPOINTS: &str = "breakpoints";
    pub(crate) const CHECKED: &str = "checked";
    pub(crate) const GROUP: &str = "group";
    pub(crate) const TOGGLE: &str = "toggle";
    pub(crate) const TRACK: &str = "track";
    pub(crate) const CLICKED: &str = "clicked";
    pub(crate) const CLOSABLE: &str = "closable";
    pub(crate) const COLLAPSIBLE: &str = "collapsible";
    pub(crate) const PICKED_COLOR: &str = "picked_color";
    pub(crate) const COLUMNS: &str = "columns";
    pub(crate) const CONTEXT: &str = "context";
    pub(crate) const CURRENT_FILL: &str = "current_fill";
    pub(crate) const CURRENT_LINE: &str = "current_line";
    pub(crate) const DIAMETER: &str = "diameter";
    pub(crate) const DASHED: &str = "dashed";
    pub(crate) const SCROLL_DEADZONE: &str = "scroll_deadzone";
    pub(crate) const DECIMALS: &str = "decimals";
    pub(crate) const DURATION: &str = "duration";
    pub(crate) const DRAW: &str = "draw";
    pub(crate) const FILL: &str = "fill";
    pub(crate) const FOCUSABLE: &str = "focusable";
    pub(crate) const FONT_FAMILY: &str = "font_family";
    pub(crate) const FONT_SIZE: &str = "font_size";
    pub(crate) const FONT_STYLE: &str = "font_style";
    pub(crate) const FONT_WEIGHT: &str = "font_weight";
    pub(crate) const GAP: &str = "gap";
    pub(crate) const GROW: &str = "grow";
    pub(crate) const GUTTER_COLOR: &str = "gutter_color";
    pub(crate) const GUTTER_WIDTH: &str = "gutter_width";
    pub(crate) const SPLITTER_WIDTH: &str = "splitter_width";
    pub(crate) const HEIGHT: &str = "height";
    pub(crate) const HIGHLIGHT: &str = "highlight";
    pub(crate) const ICON: &str = "icon";
    pub(crate) const ICON_COLOR: &str = "icon_color";
    pub(crate) const ICON_SIZE: &str = "icon_size";
    pub(crate) const IMAGE: &str = "image";
    pub(crate) const INSET: &str = "inset";
    pub(crate) const SYNTAX_COMMENT: &str = "syntax_comment";
    pub(crate) const SYNTAX_IDENTIFIER: &str = "syntax_identifier";
    pub(crate) const SYNTAX_KEYWORD: &str = "syntax_keyword";
    pub(crate) const SYNTAX_NUMBER: &str = "syntax_number";
    pub(crate) const SYNTAX_PUNCTUATION: &str = "syntax_punctuation";
    pub(crate) const SYNTAX_STRING: &str = "syntax_string";
    pub(crate) const SYNTAX_TYPE: &str = "syntax_type";
    pub(crate) const KEEP_OPEN: &str = "keep_open";
    pub(crate) const JUSTIFY: &str = "justify";
    pub(crate) const KIND: &str = "kind";
    pub(crate) const KNOB: &str = "knob";
    pub(crate) const LANGUAGE: &str = "language";
    pub(crate) const LAYER: &str = "layer";
    pub(crate) const LINE_HEIGHT: &str = "line_height";
    pub(crate) const MARKUP: &str = "markup";
    pub(crate) const MAX: &str = "max";
    pub(crate) const AXIS: &str = "axis";
    pub(crate) const MAX_HEIGHT: &str = "max_height";
    pub(crate) const MAX_WIDTH: &str = "max_width";
    pub(crate) const MAX_LENGTH: &str = "max_length";
    pub(crate) const MENU: &str = "menu";
    pub(crate) const MENU_CLICK: &str = "menu_click";
    pub(crate) const MIN: &str = "min";
    pub(crate) const HIDE_NARROWER: &str = "hide_narrower";
    pub(crate) const HIDE_WIDER: &str = "hide_wider";
    pub(crate) const HIDE_SHORTER: &str = "hide_shorter";
    pub(crate) const HIDE_TALLER: &str = "hide_taller";
    pub(crate) const MIN_HEIGHT: &str = "min_height";
    pub(crate) const MIN_WIDTH: &str = "min_width";
    pub(crate) const NUMERIC: &str = "numeric";
    pub(crate) const OFFSET: &str = "offset";
    pub(crate) const ON_CHANGE: &str = "on_change";
    pub(crate) const ON_CLICK: &str = "on_click";
    pub(crate) const PASS_NODE: &str = "pass_node";
    pub(crate) const ON_LINK: &str = "on_link";
    pub(crate) const ON_FOCUS: &str = "on_focus";
    pub(crate) const ON_GUTTER: &str = "on_gutter";
    pub(crate) const ON_SUBMIT: &str = "on_submit";
    pub(crate) const OPEN: &str = "open";
    pub(crate) const OPTIONS: &str = "options";
    pub(crate) const PADDING: &str = "padding";
    pub(crate) const PADDING_X: &str = "padding_x";
    pub(crate) const PADDING_Y: &str = "padding_y";
    pub(crate) const PLACEHOLDER: &str = "placeholder";
    pub(crate) const PLACEMENT: &str = "placement";
    pub(crate) const PREFIX: &str = "prefix";
    pub(crate) const PREFIX_COLOR: &str = "prefix_color";
    pub(crate) const PROBLEM_COLOR: &str = "problem_color";
    pub(crate) const PROBLEMS: &str = "problems";
    pub(crate) const CORNER_RADIUS: &str = "corner_radius";
    /// Whether an overlay answers the pointer. Off for one that is read over
    /// what is behind it, which must keep its clicks.
    pub(crate) const INTERACTIVE: &str = "interactive";
    pub(crate) const ENABLED: &str = "enabled";
    pub(crate) const RAIL: &str = "rail";
    pub(crate) const REGION: &str = "region";
    pub(crate) const RESIZABLE: &str = "resizable";
    pub(crate) const ROLE: &str = "role";
    /// The row a `fold` event names.
    pub(crate) const ROW: &str = "row";
    pub(crate) const SCRIM: &str = "scrim";
    pub(crate) const ROW_HEIGHT: &str = "row_height";
    pub(crate) const TITLES: &str = "titles";
    pub(crate) const WIDTHS: &str = "widths";
    pub(crate) const HEADER: &str = "header";
    pub(crate) const SORT: &str = "sort";
    pub(crate) const SORTABLE: &str = "sortable";
    pub(crate) const REVERSE: &str = "reverse";
    pub(crate) const REORDERABLE: &str = "reorderable";
    pub(crate) const ON_MOVE: &str = "on_move";
    pub(crate) const DRAGGABLE: &str = "draggable";
    pub(crate) const HIDE_ON_CLOSE: &str = "hide_on_close";
    pub(crate) const ON_DROP: &str = "on_drop";
    /// The `[colors]` a `list`, `tree` or `table` paints its parts with.
    pub(crate) const ROW_SELECTED: &str = "row_selected";
    pub(crate) const ROW_SELECTED_TEXT: &str = "row_selected_text";
    pub(crate) const ROW_HOVER: &str = "row_hover";
    pub(crate) const ROW_ACTIVE: &str = "row_active";
    pub(crate) const ROW_STRIPE: &str = "row_stripe";
    pub(crate) const TABLE_HEADER: &str = "table_header";
    pub(crate) const TABLE_RULE: &str = "table_rule";
    pub(crate) const TREE_GUIDE: &str = "tree_guide";
    pub(crate) const ROLES: &str = "roles";
    /// What `ui::contrast_pairs` answers per pair.
    pub(crate) const INK: &str = "ink";
    pub(crate) const RATIO: &str = "ratio";
    pub(crate) const NEED: &str = "need";
    pub(crate) const SECRET: &str = "secret";
    pub(crate) const SELECTABLE: &str = "selectable";
    pub(crate) const SELECTION: &str = "selection";
    pub(crate) const MULTI_SELECT: &str = "multi_select";
    pub(crate) const SHORTCUT: &str = "shortcut";
    pub(crate) const SELECTED: &str = "selected";
    pub(crate) const SEPARATOR: &str = "separator";
    pub(crate) const FIT: &str = "fit";
    pub(crate) const SLICE: &str = "slice";
    pub(crate) const SHEET: &str = "sheet";
    pub(crate) const SPEED: &str = "speed";
    pub(crate) const STEP: &str = "step";
    pub(crate) const STICK_TO_BOTTOM: &str = "stick_to_bottom";
    pub(crate) const STROKE: &str = "stroke";
    pub(crate) const STROKE_WIDTH: &str = "stroke_width";
    pub(crate) const SUBMITTED: &str = "submitted";
    pub(crate) const SUFFIX: &str = "suffix";
    pub(crate) const TEXT: &str = "text";
    pub(crate) const TEXT_ALIGN: &str = "text_align";
    pub(crate) const TEXT_COLOR: &str = "text_color";
    pub(crate) const TEXT_KEY: &str = "text_key";
    pub(crate) const THEME: &str = "theme";
    pub(crate) const TIGHT: &str = "tight";
    pub(crate) const TITLE: &str = "title";
    pub(crate) const TOOLTIP: &str = "tooltip";
    pub(crate) const CURSOR: &str = "cursor";
    pub(crate) const TOP: &str = "top";
    pub(crate) const TRAILING: &str = "trailing";
    pub(crate) const ICON_FILL: &str = "icon_fill";
    pub(crate) const SHOWING: &str = "showing";
    pub(crate) const TRAILING_COLOR: &str = "trailing_color";
    pub(crate) const TRAILING_SIZE: &str = "trailing_size";
    pub(crate) const TRANSPARENT: &str = "transparent";
    pub(crate) const TRUNCATE: &str = "truncate";
    pub(crate) const VALUE: &str = "value";
    pub(crate) const SAFE_AREA: &str = "safe_area";
    pub(crate) const VISIBLE: &str = "visible";
    pub(crate) const WARNING_COLOR: &str = "warning_color";
    pub(crate) const WARNINGS: &str = "warnings";
    pub(crate) const WIDTH: &str = "width";
    pub(crate) const WRAP: &str = "wrap";
    pub(crate) const INDEX: &str = "index";
    pub(crate) const X: &str = "x";
    pub(crate) const Y: &str = "y";
}

/// What a pooled control's spec names beside the `widget` properties it
/// carries, which `ui::fill_strip` and `ui::fill_rows` read.
pub(crate) mod pool {
    /// The component every pooled node carries.
    pub(crate) const WIDGET: &str = "widget";
    /// The callback that hears the reader's edit.
    pub(crate) const ON: &str = "on";
    /// A spec's children: the spec is a group, one node holding them.
    pub(crate) const CONTROLS: &str = "controls";
    /// A row's label, and the roles its label and control column wear.
    pub(crate) const LABEL: &str = "label";
    pub(crate) const LABEL_ROLE: &str = "label_role";
    pub(crate) const SLOT_ROLE: &str = "slot_role";
    /// A row spanning both columns, with no label beside it.
    pub(crate) const FULL: &str = "full";
    /// The roles a row wears where its spec names none.
    pub(crate) const TEXT_LABEL: &str = "text_label";
    pub(crate) const LAYOUT_CELLS: &str = "layout_cells";
}

/// The state tables a theme entry holds, by CSS's words. `checked` answers
/// to the property of that name, `disabled` to `enabled = false`.
pub(crate) mod states {
    pub(crate) const DISABLED: &str = "disabled";
    pub(crate) const HOVER: &str = "hover";
    pub(crate) const ACTIVE: &str = "active";
    pub(crate) const FOCUS: &str = "focus";
}

/// The theme's tokens by name: the sources a theme states, and what Rust
/// reads of what is derived from them.
pub(crate) mod tokens {
    pub(crate) use super::keys::{
        FONT_SIZE, STROKE_WIDTH, SYNTAX_COMMENT, SYNTAX_IDENTIFIER, SYNTAX_KEYWORD, SYNTAX_NUMBER,
        SYNTAX_PUNCTUATION, SYNTAX_STRING, SYNTAX_TYPE,
    };

    pub(crate) const BACKGROUND: &str = "background";
    pub(crate) const FOREGROUND: &str = "foreground";
    /// How far apart the surfaces step, in OKLab lightness; a number in `[colors]`.
    pub(crate) const CONTRAST: &str = "contrast";
    pub(crate) const PRIMARY: &str = "primary";
    pub(crate) const SECONDARY: &str = "secondary";
    pub(crate) const SUCCESS: &str = "success";
    pub(crate) const WARNING: &str = "warning";
    pub(crate) const DANGER: &str = "danger";

    /// What a family's token paints, after its name: `primary_fill`.
    pub(crate) const FILL: &str = "fill";
    pub(crate) const FILL_HOVER: &str = "fill_hover";
    pub(crate) const TEXT: &str = "text";
    pub(crate) const BG: &str = "bg";

    /// A family's token for one job: `of(PRIMARY, FILL)` is `primary_fill`.
    pub(crate) fn of(family: &str, part: &str) -> String {
        format!("{family}_{part}")
    }

    /// The ink drawn on a family's fill: `text_on_primary`.
    pub(crate) fn on(family: &str) -> String {
        format!("text_on_{family}")
    }

    pub(crate) const BG_APP: &str = "bg_app";
    pub(crate) const BG_PANEL: &str = "bg_panel";
    pub(crate) const BG_CONTROL: &str = "bg_control";
    pub(crate) const BG_CONTROL_HOVER: &str = "bg_control_hover";
    pub(crate) const BORDER_DEFAULT: &str = "border_default";
    pub(crate) const TEXT_DEFAULT: &str = "text_default";
    pub(crate) const TEXT_MUTED: &str = "text_muted";
    pub(crate) const TEXT_SUBTLE: &str = "text_subtle";
    pub(crate) const PRIMARY_TEXT: &str = "primary_text";
    pub(crate) const PRIMARY_FILL: &str = "primary_fill";
    pub(crate) const PRIMARY_BG: &str = "primary_bg";
    pub(crate) const TEXT_ON_PRIMARY: &str = "text_on_primary";
    pub(crate) const SECONDARY_TEXT: &str = "secondary_text";
    pub(crate) const WARNING_TEXT: &str = "warning_text";
    pub(crate) const DANGER_TEXT: &str = "danger_text";
    pub(crate) const GRID_MINOR: &str = "grid_minor";
    pub(crate) const GRID_MAJOR: &str = "grid_major";
    pub(crate) const NODE_DEFAULT: &str = "node_default";
    pub(crate) const BRAND_PLATE: &str = "brand_plate";
    pub(crate) const INPUT_RIPPLE: &str = "input_ripple";

    pub(crate) const FONT_SIZE_SMALL: &str = "font_size_small";
    pub(crate) const FONT_SIZE_LARGE: &str = "font_size_large";
    pub(crate) const FONT_SIZE_TITLE: &str = "font_size_title";
    pub(crate) const RADIUS: &str = "radius";
    pub(crate) const RADIUS_SMALL: &str = "radius_small";
    pub(crate) const RADIUS_LARGE: &str = "radius_large";
    pub(crate) const CONTROL_HEIGHT: &str = "control_height";
    pub(crate) const CONTROL_HEIGHT_SMALL: &str = "control_height_small";
    pub(crate) const CONTROL_HEIGHT_LARGE: &str = "control_height_large";
    pub(crate) const CONTROL_HEIGHT_TOUCH: &str = "control_height_touch";

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn a_spelled_family_token_is_the_one_the_palette_builds() {
            assert_eq!(of(PRIMARY, TEXT), PRIMARY_TEXT);
            assert_eq!(of(PRIMARY, FILL), PRIMARY_FILL);
            assert_eq!(of(PRIMARY, BG), PRIMARY_BG);
            assert_eq!(on(PRIMARY), TEXT_ON_PRIMARY);
            assert_eq!(of(SECONDARY, TEXT), SECONDARY_TEXT);
            assert_eq!(of(WARNING, TEXT), WARNING_TEXT);
            assert_eq!(of(DANGER, TEXT), DANGER_TEXT);
        }
    }
}

/// Font weights on the CSS scale: the regular one, and where bold starts.
pub(crate) mod weights {
    pub(crate) const REGULAR: f32 = 400.0;
    /// From here a weight draws in the bold face, and counts as bold for
    /// WCAG's large-text rule.
    pub(crate) const BOLD_FROM: f32 = 600.0;
}

/// Schema text from `(key, spec)` lines; see [`ComponentDef::schema`].
pub(crate) fn schema(lines: &[(&str, &str)]) -> String {
    ComponentDef::schema(lines)
}

/// The words a property offers, as its `options` list.
pub(crate) fn options(words: &[&str]) -> String {
    ComponentDef::options(words)
}
