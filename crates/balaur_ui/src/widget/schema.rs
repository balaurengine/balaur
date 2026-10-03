//! The `widget` component's schema: what a scene file may say about a
//! widget, the presets the picker offers, and the table a `Widget` reads
//! back as.

use anyhow::Result;
use balaur_core::components::ComponentDef;
use balaur_plugin::Registry;

use crate::vocabulary::{self as v, keys as k, words as w};
use crate::widget::node::Widget;
use crate::widget::options::{Read, edges_of, four, lines, pair, shares, sides, strings, two};

/// The `widget` key, backed by exactly one `Widget` component on the node.
///
/// `clicked` is declared `readonly`: [`crate::widget::input`] writes it every
/// tick and `apply` always clears it, but it is in the schema so that `get`'s
/// output round-trips and the inspector can see it.
#[allow(
    clippy::too_many_lines,
    reason = "one line per property a scene may state; the list is the schema"
)]
pub(crate) fn register_widget_component(reg: &mut Registry<'_>) {
    balaur_core::components::answers_property(reg.engine(), "widget", Box::new(read_property));
    reg.register_component(
        "widget",
        ComponentDef {
            events: crate::widget::input::EVENTS,
            warnings: None,
            doc: "A HUD element drawn every frame: `kind` picks `label`, `button`, `panel` and more, `anchor` places it in design pixels. A button sets `clicked` and calls `on_click`.",
            schema: ComponentDef::parse_schema(
                "widget",
                &(v::schema(&[
                    (k::KIND, &format!(r#"{{ type = "enum", default = "{}", options = [{}], description = "The HUD element the widget layer draws" }}"#, w::LABEL, v::options(w::WIDGET_KINDS))),
                    (k::TEXT, r#"{ type = "string", default = "label", description = "Label or button caption" }"#),
                    (k::VISIBLE, r#"{ type = "bool", default = true, description = "Draw the widget; hidden widgets keep their state" }"#),
                    (k::SAFE_AREA, &format!(r#"{{ type = "flags", default = [], options = [{}], description = "The edges this root keeps clear of what a notch, a status bar or a home bar covers. Empty by default: a backdrop is meant to reach the edge and a control is not, and a screen often wants content above the notch and its background under the gesture bar", group = "placement" }}"#, v::options(w::EDGES))),
                    (k::ANCHOR, &format!(r#"{{ type = "enum", default = "{}", options = [{}], description = "Corner, edge or middle the offset is measured from: of the surface for a root, of the parent's box inside a `stack`; `fill` takes the whole of it less `inset`" }}"#, w::TOP_LEFT, v::options(w::ANCHORS))),
                    (k::X, r#"{ type = "float", default = 16.0, description = "Horizontal offset from the anchor, in design pixels", group = "placement" }"#),
                    (k::Y, r#"{ type = "float", default = 16.0, description = "Vertical offset from the anchor, in design pixels", group = "placement" }"#),
                    (k::WIDTH, r#"{ type = "float", default = 0.0, min = 0.0, description = "Panel width in design pixels; 0 sizes to content", group = "placement" }"#),
                    (k::HEIGHT, r#"{ type = "float", default = 0.0, min = 0.0, description = "Panel height in design pixels; 0 sizes to content", group = "placement" }"#),
                    (k::FONT_SIZE, r#"{ type = "float", default = 0.0, min = 0.0, description = "Text size in design pixels; 0 takes the size the role or the kind carries", group = "type" }"#),
                    (k::TEXT_COLOR, r#"{ type = "color", default = [0.0, 0.0, 0.0, 0.0], description = "Text color; fully transparent takes the theme's colour for this widget's role or kind, and failing that a near-white", group = "paint" }"#),
                    (k::PADDING, r#"{ type = "vec4", default = [-1.0, -1.0, -1.0, -1.0], description = "Space inside the widget's edge, in design pixels: one number for every side, or left, top, right and bottom. A container keeps it round its children, a `button` and a `label` round their text, and every other kind in its box. A side below zero takes the theme's (`padding_x` across and `padding_y` down, else `padding`), else 8 round a `panel`, 12 either side of a `button`'s or `menu`'s caption and 0 elsewhere; a stated zero is no space at all. On a `text_field`, `text_area` or `code` it is the margin round the text, where below zero takes egui's", group = "layout" }"#),
                    (k::GAP, r#"{ type = "vec2", default = [-1.0, -1.0], description = "Space between a container's children, across and down, in design pixels: a row puts the first between its children and the second between the lines it wraps onto, a column the second between its children, and a grid the first between its columns and the second between its rows. On a `button` the first is the space between its picture, icon and caption. Below zero on an axis takes the theme's own, which is 8 where it says nothing and half the font size on a button, and a stated zero puts them edge to edge", group = "layout" }"#),
                    (k::ALIGN_ITEMS, &format!(r#"{{ type = "enum", default = "{}", options = [{}], description = "Where a container puts its children across its own direction, and a `grid` each child in its cell down: `stretch` makes each as wide (or tall) as the container, `baseline` lines up their first lines of text, the others keep each child's own size", group = "layout" }}"#, w::STRETCH, v::options(w::ITEM_ALIGNS))),
                    (k::AXIS, &format!(r#"{{ type = "enum", default = "{}", options = [{}], description = "Which way a scroll moves, the other way its contents filling the box it was given. A `slider` runs up its box under `vertical`, and a `separator` draws the line it names, `both` leaving it across its parent's direction", group = "layout" }}"#, w::BOTH, v::options(w::AXES))),
                    (k::FOCUSABLE, r#"{ type = "bool", default = true, description = "Let focus land here. A widget nothing can activate is never focused whatever this says; set it false to skip one that could be", group = "events" }"#),
                    (k::ON_FOCUS, r#"{ type = "string", default = "", description = "Script method called when focus arrives, on this node or the nearest ancestor whose script declares it", group = "events" }"#),
                    (k::THEME, &format!(r#"{{ type = "asset", asset = "{}", default = "", description = "How this widget and everything under it is drawn; inherited from the nearest ancestor that names one", group = "paint" }}"#, crate::widget::theme::ASSET_TYPE)),
                    (k::TEXT_KEY, r#"{ type = "string", default = "", description = "A localization key drawn in place of `text`, re-read every frame so a locale switch shows at once", group = "type" }"#),
                    (k::ON_CLICK, r#"{ type = "string", default = "", description = "Script method called when the widget is clicked, on this node or the nearest ancestor whose script declares it. An `image` that names one senses clicks too, which is how a picture becomes a button", group = "events" }"#),
                    (k::CLICKED, r#"{ type = "bool", default = false, readonly = true, description = "True on the frame the button was clicked", group = "events" }"#),
                    (k::PASS_NODE, r#"{ type = "bool", default = false, description = "Hand every handler this widget calls its own node as the last argument, so one method can serve many widgets", group = "events" }"#),
                    (k::INTERACTIVE, r#"{ type = "bool", default = true, description = "Off, the pointer passes through to the scene: the widget is drawn, never hovered or clicked, and `ui.wants_pointer()` stays false over it. A full-screen container over the world wants this", group = "events" }"#),
                    (k::ON_LINK, r#"{ type = "string", default = "", description = "Script method called with the target of a `[url=target]` span in `markup` text that was clicked, on this node or the nearest ancestor whose script declares it", group = "events" }"#),
                    (k::SUFFIX, r#"{ type = "string", default = "", description = "Units drawn after a `number_field`'s or a shown `slider`'s number with a space between, and after a `text_field`'s or `text_area`'s text", group = "type" }"#),
                    (k::PREFIX, r#"{ type = "string", default = "", description = "Text drawn before a `number_field`'s or a shown `slider`'s number with a space between, and before a `text_field`'s or `text_area`'s text", group = "type" }"#),
                    (k::ARROWS, r#"{ type = "bool", default = false, description = "Draw a step up and a step down beside a `number_field`, each moving it by `step` within `min` and `max`", group = "type" }"#),
                    (k::SELECTABLE, r#"{ type = "bool", default = false, description = "Let a drag over this label select its text, and the platform's copy key take it", group = "type" }"#),
                    (k::BREAKPOINTS, r#"{ type = "list", of = { type = "string" }, default = [], description = "The lines a `code` widget dots in its gutter, counting from 1; whole numbers or the text of them. A click on the gutter reports its line through `on_gutter` and the script decides what the mark means", group = "value" }"#),
                    (k::PROBLEMS, r#"{ type = "list", of = { type = "string" }, default = [], description = "The lines a `code` widget underlines as errors, counting from 1, each also marked on the inner edge of its gutter", group = "value" }"#),
                    (k::WARNINGS, r#"{ type = "list", of = { type = "string" }, default = [], description = "The lines a `code` widget underlines as warnings, counting from 1; an error on the same line outranks it", group = "value" }"#),
                    (k::CURRENT_LINE, r#"{ type = "int", default = 0, min = 0, description = "The line a `code` widget fills across its whole width, counting from 1, for the row a debugger is stopped on; 0 fills none", group = "value" }"#),
                    (k::GUTTER_WIDTH, r#"{ type = "float", default = 0.0, min = 0.0, description = "How wide a `code` widget's gutter is, in design pixels; 0 takes the built-in width, which holds four digits", group = "layout" }"#),
                    (k::ON_GUTTER, r#"{ type = "string", default = "", description = "Script method called with the line a click on a `code` widget's gutter landed on, on this node or the nearest ancestor whose script declares it", group = "events" }"#),
                    (k::CONTEXT, r#"{ type = "string", default = "", description = "Name of a `menu` node whose rows open at the pointer on a right click or a long press; give that menu `visible = false` to show no button of its own", group = "events" }"#),
                    (k::GROW, r#"{ type = "float", default = 0.0, min = 0.0, description = "Share of the leftover space a container hands out along its own direction; 0 takes only what this widget asks for", group = "placement" }"#),
                    (k::HIDE_NARROWER, r#"{ type = "float", default = 0.0, min = 0.0, description = "Not drawn while the room is narrower than this many design pixels. The room is the nearest container that states a size or grows, and the screen for a root: a minimum in numbers, where the class words are not fine enough. Zero is no line", group = "placement" }"#),
                    (k::HIDE_WIDER, r#"{ type = "float", default = 0.0, min = 0.0, description = "Not drawn while the room is this wide or wider, in design pixels: a control only a small space wants. Zero is no line", group = "placement" }"#),
                    (k::HIDE_SHORTER, r#"{ type = "float", default = 0.0, min = 0.0, description = "Not drawn while the room is shorter than this many design pixels. Zero is no line", group = "placement" }"#),
                    (k::HIDE_TALLER, r#"{ type = "float", default = 0.0, min = 0.0, description = "Not drawn while the room is this tall or taller, in design pixels. Zero is no line", group = "placement" }"#),
                    (k::MIN_WIDTH, r#"{ type = "float", default = 0.0, min = 0.0, description = "Smallest width a container may give this widget, and a `resizable` window be dragged to, in design pixels", group = "placement" }"#),
                    (k::MIN_HEIGHT, r#"{ type = "float", default = 0.0, min = 0.0, description = "Smallest height a container may give this widget, and a `resizable` window be dragged to, in design pixels", group = "placement" }"#),
                    (k::MAX_WIDTH, r#"{ type = "float", default = 0.0, min = 0.0, description = "Largest width a container may give this widget, and a `resizable` window be dragged to, in design pixels; 0 is no limit", group = "placement" }"#),
                    (k::MAX_HEIGHT, r#"{ type = "float", default = 0.0, min = 0.0, description = "Largest height a container may give this widget, and a `resizable` window be dragged to, in design pixels; 0 is no limit", group = "placement" }"#),
                    (k::WIDTH_PERCENT, r#"{ type = "float", default = 0.0, min = 0.0, description = "Width as a percentage of the container's box inside its padding; 0 leaves it to `width`, which a stated percentage wins over", group = "placement" }"#),
                    (k::HEIGHT_PERCENT, r#"{ type = "float", default = 0.0, min = 0.0, description = "Height as a percentage of the container's box inside its padding; 0 leaves it to `height`, which a stated percentage wins over", group = "placement" }"#),
                    (k::ASPECT_RATIO, r#"{ type = "float", default = 0.0, min = 0.0, description = "Width over height, kept where the layout decides only one of them; 0 keeps none", group = "placement" }"#),
                    (k::ABSOLUTE, r#"{ type = "bool", default = false, description = "Take this child out of its container's run and place it by `inset` against the container's box inside its border, drawn in its turn among its siblings; a root ignores it", group = "placement" }"#),
                    (k::MARGIN, r#"{ type = "vec4", default = [0.0, 0.0, 0.0, 0.0], description = "Space outside this widget's edge that its container keeps clear, left, top, right and bottom, in design pixels; below zero lets a neighbour over it", group = "placement" }"#),
                    (k::BASIS, r#"{ type = "float", default = -1.0, description = "The size this child starts from along its container's direction, before `grow` and `shrink`, in design pixels; below zero is 0 for one that grows and its own size otherwise", group = "placement" }"#),
                    (k::SHRINK, r#"{ type = "float", default = -1.0, description = "Share of the shortfall this child gives up when its container is too small along its direction; below zero is 1 for one that grows and 0 otherwise", group = "placement" }"#),
                    (k::ALIGN_SELF, &format!(r#"{{ type = "enum", default = "{}", options = [{}], description = "Where this child sits across its container's direction; `auto` takes the container's `align_items`", group = "placement" }}"#, w::AUTO, v::options(w::SELF_ALIGNS))),
                    (k::DRAW, r#"{ type = "string", default = "", description = "What fills a `draw` widget: a script method on this node or the nearest scripted ancestor, or `scripts/file.rn:function` for a free function", group = "paint" }"#),
                    (k::SPLITTER_WIDTH, r#"{ type = "float", default = 0.0, min = 0.0, description = "How wide a grab the seams between this container's children get, in design pixels; 0 leaves them fixed. A drag writes the new size onto the neighbour that states one. On a `table` it is the grab between two columns, which is six pixels where it says nothing", group = "value" }"#),
                    (k::CURRENT_PAGE, r#"{ type = "string", default = "", description = "Which child a `tabs` shows, by node name; empty shows the first. A click on the strip writes it and calls `on_change` with the page's name", group = "events" }"#),
                    (k::LAYER, r#"{ type = "string", default = "", description = "The drawing surface this root belongs to; empty is the default one, and a name nothing has configured takes the default surface", group = "placement" }"#),
                    (k::WRAP, r#"{ type = "bool", default = false, description = "Break text to the width the widget was given instead of running past it on one line; a `dropdown` breaks its picked text", group = "type" }"#),
                    (k::TRUNCATE, r#"{ type = "bool", default = false, description = "Cut a caption too long for the width the widget was given and end it with an ellipsis; with neither this nor `wrap` a button clips it at its edge. A `dropdown` cuts its picked text", group = "type" }"#),
                    (k::TRAILING, r#"{ type = "string", default = "", description = "Text a button draws against its far edge, dimmer than its caption: a shortcut, or a menu's caret", group = "type" }"#),
                    (k::SHORTCUT, r#"{ type = "string", default = "", description = "A chord that clicks this widget wherever it is, as `cmd+shift+s` or `f5`; a menu row fires while its menu is shut, and draws the chord against its far edge unless it says its own `trailing`", group = "events" }"#),
                    (k::SHOWING, r#"{ type = "bool", default = false, description = "Holds a menu's rows up from the scene, as a click would; for an offscreen run or a tutorial, since nothing can click there", group = "events" }"#),
                    (k::DURATION, r#"{ type = "float", default = 3.0, min = 0.0, description = "How long a `toast` stays, in seconds, counting the half second it fades over; zero leaves it up until the game takes it away", group = "type" }"#),
                    (k::PLACEMENT, &format!(r#"{{ type = "enum", default = "{}", options = [{}], description = "Where a `menu` opens: under its button, above it, to its right or to its left, each from the side's start (`below` starts at the button's left edge, `right` at its top) or at `_center` or `_end`; at the pointer; or centred on the screen", group = "placement" }}"#, w::BELOW, v::options(w::PLACEMENTS))),
                    (k::PLACEMENT_FALLBACKS, &format!(r#"{{ type = "list", of = {{ type = "enum", default = "{}", options = [{}] }}, default = [], description = "The placements a `menu`'s popup tries, in order, where its own does not fit the screen; empty tries egui's, and naming the placement alone keeps it where it is", group = "placement" }}"#, w::BELOW, v::options(w::SIDE_PLACEMENTS))),
                    (k::POPUP_GAP, r#"{ type = "float", default = -1.0, description = "The space between a `menu`'s button and its popup, in design pixels; below zero takes egui's", group = "placement" }"#),
                    (k::POPUP_WIDTH, r#"{ type = "float", default = 0.0, min = 0.0, description = "How wide a `menu`'s popup starts, in design pixels; 0 sizes it to its rows", group = "placement" }"#),
                    (k::CLOSE_ON, &format!(r#"{{ type = "enum", default = "{}", options = [{}], description = "What shuts a `menu`'s popup besides a row: a click outside it, any click at all, or nothing but Escape", group = "events" }}"#, w::CLICK_OUTSIDE, v::options(w::CLOSE_ONS))),
                    (k::KEEP_OPEN, r#"{ type = "bool", default = false, description = "A menu row that leaves its menu open when clicked, as a toggle does; any other row closes it. A `dropdown` with it stays open past a pick until a click outside it", group = "events" }"#),
                    (k::TEXT_ALIGN, &format!(r#"{{ type = "enum", default = "{}", options = [{}], description = "Where text sits in the width the widget was given, what a `text_field` or `text_area` edits included, and where every cell of a `table` sits in its column; a column whose name ends in `>` pins its own to the right", group = "type" }}"#, w::START, v::options(w::ALIGNS))),
                    (k::IMAGE, r#"{ type = "string", default = "", description = "The project-relative image an `image` widget draws, or the picture a `button` draws before its caption at the caption's height" }"#),
                    (k::SHEET, r#"{ type = "string", default = "", description = "The picture a `list` cuts its card faces from" }"#),
                    (k::LANGUAGE, r#"{ type = "string", default = "", description = "The language a `code` widget highlights" }"#),
                    (k::FIT, &format!(r#"{{ type = "enum", default = "", options = [{}], description = "How an `image` sits in the box it was given: `contain` and `cover` keep its shape, `fill` stretches, `none` leaves it its own size, centred. Empty lets the picture decide the box instead", group = "value" }}"#, v::options(w::FITS))),
                    (k::MARKUP, r#"{ type = "bool", default = false, description = "Read inline marks in the text: `[b]`, `[i]`, `[color=#hex]`, `[center]`, `[right]`, `[wave amp=N freq=N]` and `[img=path width=N]`; off, brackets are text", group = "type" }"#),
                    (k::FONT_WEIGHT, r#"{ type = "float", default = 400.0, min = 100.0, max = 900.0, description = "Weight on the CSS scale, resolved against the faces the project ships: 400 regular, 700 bold", group = "type" }"#),
                    (k::FONT_STYLE, &format!(r#"{{ type = "enum", default = "{}", options = [{}], description = "Upright, the family's italic face, or the upright face slanted; a family with no italic is slanted either way", group = "type" }}"#, w::NORMAL, v::options(w::FONT_STYLES))),
                    (k::PLACEHOLDER, r#"{ type = "string", default = "", description = "What a `text_field` or `text_area` shows while it is empty", group = "value" }"#),
                    (k::MAX_LENGTH, r#"{ type = "float", default = 0.0, min = 0.0, description = "The most characters a `text_field` takes; 0 is no limit", group = "value" }"#),
                    (k::SECRET, r#"{ type = "bool", default = false, description = "Draw a `text_field`'s text as dots, for a password", group = "value" }"#),
                    (k::NUMERIC, r#"{ type = "bool", default = false, description = "Keep a `text_field` to digits, a sign and a point", group = "value" }"#),
                    (k::ON_CHANGE, r#"{ type = "string", default = "", description = "Script method called with a `text_field`'s text after every edit, on this node or the nearest ancestor whose script declares it", group = "events" }"#),
                    (k::ON_SUBMIT, r#"{ type = "string", default = "", description = "Script method called with a `text_field`'s text on Enter, or when focus leaves it, on this node or the nearest ancestor whose script declares it", group = "events" }"#),
                    (k::SUBMITTED, r#"{ type = "bool", default = false, description = "True for the one frame a `text_field` was submitted, the way `clicked` reports a press", group = "events" }"#),
                    (k::CHECKED, r#"{ type = "bool", default = false, description = "Whether a `checkbox` is ticked, every click flipping it and calling `on_change` with the new state; a checked `button` is held down, wearing its pressed look" }"#),
                    (k::GROUP, r#"{ type = "string", default = "", description = "A name this `checkbox` or `toggle` button shares with the ones it is exclusive with: ticking one unticks the rest, and one already ticked stays ticked. Empty leaves it flipping on its own", group = "value" }"#),
                    (k::TOGGLE, r#"{ type = "bool", default = false, description = "A `button` a click holds down and the next releases, flipping `checked` as a `checkbox` does, before `on_click` runs: Godot's toggle mode", group = "value" }"#),
                    (k::VALUE, r#"{ type = "float", default = 0.0, description = "Where a `slider`, `number_field` or `progress_bar` stands, between `min` and `max`; a slider and a drag value write it and call `on_change` with it" }"#),
                    (k::MIN, r#"{ type = "float", default = 0.0, description = "The low end of a `slider` or `progress_bar`; a `number_field` runs free while this pair is the default 0 and 1", group = "value" }"#),
                    (k::MAX, r#"{ type = "float", default = 1.0, description = "The high end of a `slider` or `progress_bar`; a `number_field` runs free while this pair is the default 0 and 1", group = "value" }"#),
                    (k::STEP, r#"{ type = "float", default = 0.0, min = 0.0, description = "The grid a `slider` snaps to, and how far a `number_field`'s `arrows` move it; 0 is continuous, and 1 for the arrows", group = "value" }"#),
                    (k::PICKED_COLOR, r#"{ type = "color", default = [1.0, 1.0, 1.0, 1.0], description = "What a `color_picker` holds; `on_change` hears the new one", group = "paint" }"#),
                    (k::ROW_HEIGHT, r#"{ type = "float", default = 0.0, min = 0.0, description = "The pitch of a `list` or `tree` row, in design pixels; 0 takes the font's own line height", group = "layout" }"#),
                    (k::FONT_FAMILY, &format!(r#"{{ type = "enum", default = "{}", options = [{}], description = "Which of the theme's families the widget draws in", group = "type" }}"#, w::UI, v::options(w::WIDGET_FONTS))),
                    (k::OPTIONS, r#"{ type = "list", of = { type = "string" }, default = [], description = "The items a `dropdown`, `menu`, `list`, `tree` or `table` holds; `text` is the one picked, except on a `menu` where it is the button caption. A `tree` row starts with one tab per level, a `list` or `tree` row splits on U+001F into icon, label, a trailing note, an `#rrggbb` for that row, a key that is never drawn, which two rows with the same label need to stay two rows, and marks, each `name=glyph` from the icon face and joined on U+001E, drawn at the right and reported by `on_mark`; and a `table` row splits on the same into one cell a column. `on_change` hears every pick", group = "value" }"#),
                    (k::GRID_COLUMNS, r#"{ type = "string", default = "", description = "A `grid`'s columns, left to right and space-separated: design pixels (`120`), a share of what is left (`1fr`), a percentage (`25%`), `auto`, `min_content`, `max_content`, `fit_content(120)`, `minmax(80, 1fr)`, and `repeat(3, 1fr)`, whose count may be `auto_fill` or `auto_fit`. Empty is two equal columns. A `list` flows its cards into as many columns as this names, and draws a line a row where it is empty; a `table`'s columns are its `titles`", group = "layout" }"#),
                    (k::GRID_ROWS, r#"{ type = "string", default = "", description = "A `grid`'s rows, top to bottom, in the words `grid_columns` takes; empty makes each row as tall as what is in it", group = "layout" }"#),
                    (k::AUTO_COLUMNS, r#"{ type = "string", default = "", description = "The size of each column a `grid` adds past `grid_columns`, for a child placed beyond them or flowing down under `auto_flow = \"column\"`, in the words `grid_columns` takes without `repeat`; several take turns. Empty is `auto`", group = "layout" }"#),
                    (k::AUTO_ROWS, r#"{ type = "string", default = "", description = "The size of each row a `grid` adds past `grid_rows`, in the words `auto_columns` takes; empty is `auto`", group = "layout" }"#),
                    (k::AUTO_FLOW, &format!(r#"{{ type = "enum", default = "{}", options = [{}], description = "The order a `grid` fills cells with the children that name none: along each row, or down each column; a `_dense` order goes back to fill a hole a bigger child left", group = "layout" }}"#, w::flow::ROW, v::options(w::flow::ALL))),
                    (k::AREAS, r#"{ type = "list", of = { type = "string" }, default = [], description = "A `grid`'s named areas, one string a row of space-separated names with `.` for a cell in none: `[\"head head\", \"side main\"]`. A child names one in `row` and `column`", group = "layout" }"#),
                    (k::ROW, r#"{ type = "string", default = "", description = "On a `grid`'s child: the rows it covers, as a line counted from 1 (`2`, or from the end below zero), a first and last line (`1 / 3`), a span (`span 2`, `2 / span 2`), or the name of one of the grid's `areas`. Empty puts it in the next free cell", group = "layout" }"#),
                    (k::COLUMN, r#"{ type = "string", default = "", description = "On a `grid`'s child: the columns it covers, in the words `row` takes", group = "layout" }"#),
                    (k::SELECTION, r#"{ type = "list", of = { type = "string" }, default = [], description = "The rows a `list`, `tree` or `table` has picked, one of them where it holds one. `text` is the last row clicked, which is where a shift range measures from; `on_change` hears the whole list where the widget holds many, and the row where it holds one", group = "value" }"#),
                    (k::MULTI_SELECT, r#"{ type = "bool", default = false, description = "Let a `list`, `tree` or `table` hold more than one row: the platform's command key toggles a row and shift takes the run from the last one clicked", group = "value" }"#),
                    (k::TITLES, r#"{ type = "list", of = { type = "string" }, default = [], description = "A `table`'s column names, in order, and with them how many columns it has: a name ending in `>` draws its column against the right edge, which is what a column of numbers wants. None takes the first row as the names", group = "value" }"#),
                    (k::WIDTHS, r#"{ type = "list", of = { type = "string" }, default = [], description = "Each `table` column's share of the width, in the order `titles` names them: `[\"2\", \"1\", \"1\"]` gives the first half and the other two a quarter each. Numbers and the text of them both; empty divides the width evenly, and a drag on a seam in the header writes the shares back", group = "layout" }"#),
                    (k::HEADER, r#"{ type = "bool", default = true, description = "Draw the strip that names a `table`'s columns, and a `window`'s title bar. Off, a table's columns are still `titles`', and a table that names none keeps its first row as a row; `ui.window` takes it too", group = "layout" }"#),
                    (k::SORT, r#"{ type = "string", default = "", description = "The `table` column its rows are ordered by, by the name in `titles`; empty leaves them in the order they were given. A cell that starts with a number sorts as one, so `12 KB` follows `3 KB`", group = "value" }"#),
                    (k::SORTABLE, r#"{ type = "bool", default = false, description = "Let a click on a `table`'s header sort by that column, and the next click on the same one turn it round; the column sorted by carries a caret", group = "events" }"#),
                    (k::REVERSE, r#"{ type = "bool", default = false, description = "Take a `table`'s rows the other way round: the `sort` descending, or the order they were given bottom to top where none is named. A `row`, `column` or `flow` lays its children out from its far end", group = "value" }"#),
                    (k::REORDERABLE, r#"{ type = "bool", default = false, description = "Let a drag move a row of a `list` or a `tree`. The kind moves nothing itself: it draws where the row would land and calls `on_move`, and the rows are the script's to reorder", group = "events" }"#),
                    (k::ON_MOVE, r#"{ type = "string", default = "", description = "Script method called when a dragged row is dropped, with the row moved, the row it landed on, and `before`, `after` or `into`, on this node or the nearest ancestor whose script declares it", group = "events" }"#),
                    (k::ON_MARK, r#"{ type = "string", default = "", description = "Script method called with `#{ row, mark }` when a mark at the right of a `list` or `tree` row is clicked, on this node or the nearest ancestor whose script declares it. The click picks nothing", group = "events" }"#),
                    (k::DRAGGABLE, r#"{ type = "bool", default = false, description = "Let a drag carry a card of a `list` with `grid_columns` out of it, drawn under the pointer; `on_drop` says where it was let go", group = "events" }"#),
                    (k::HIDE_ON_CLOSE, r#"{ type = "bool", default = true, description = "Whether a `window`'s close button shuts it; off, the button only emits `close_request` and the script decides", group = "events" }"#),
                    (k::ON_DROP, r#"{ type = "string", default = "", description = "Script method called with the card a drag let go outside the list, on this node or the nearest ancestor whose script declares it; the pointer is where it landed", group = "events" }"#),
                    (k::OPEN, r#"{ type = "bool", default = true, description = "Whether a `fold` shows its children; its header flips it and calls `on_change` with the new state", group = "events" }"#),
                    (k::TITLE_BAR, r#"{ type = "bool", default = false, description = "On a `fold`'s child: drawn in the fold's header after its arrow and caption, as Godot's title bar control is", group = "layout" }"#),
                    (k::INSET, r#"{ type = "vec4", default = [0.0, 0.0, 0.0, 0.0], description = "Left, top, right and bottom margins a root with `anchor = \"fill\"` keeps from its surface, and how far an `absolute` child sits in from each edge of its container inside its border, where a side below zero is left free; in design pixels", group = "placement" }"#),
                    (k::AVOID_KEYBOARD, r#"{ type = "bool", default = false, description = "On a root: measure the bottom of the surface from the top of the on-screen keyboard, so a form or a chat bar stays above it; nothing on a desktop", group = "placement" }"#),
                    (k::SLICE, r#"{ type = "vec4", default = [0.0, 0.0, 0.0, 0.0], description = "Left, top, right and bottom borders of an `image` kept unstretched, in the picture's own pixels; all zero stretches the whole picture", group = "paint" }"#),
                    (k::SCROLL_DEADZONE, r#"{ type = "float", default = 0.0, min = 0.0, description = "How far a finger drags a `scroll` before it scrolls, in design pixels, so a tap on a child still lands; 0 scrolls at once", group = "value" }"#),
                    (k::ROLE, r#"{ type = "string", default = "", description = "A `[roles.<name>]` entry of the widget's theme, taken over its kind's own style; the one place a look is named rather than spelled", group = "paint" }"#),
                    (k::TOOLTIP, r#"{ type = "string", default = "", description = "Text shown after the pointer rests on the widget; still shown while `enabled` is off, which is where it says why", group = "type" }"#),
                    (k::CURSOR, &format!(r#"{{ type = "enum", default = "{}", options = [{}], description = "The pointer's shape while it is over the widget: `hand` over anything that opens on a click, `none` to hide it; `arrow` is the platform's own", group = "type" }}"#, w::cursor::ARROW, v::options(w::cursor::ALL))),
                    (k::ICON, r#"{ type = "string", default = "", description = "A glyph from the theme's icon family, drawn before `text`", group = "paint" }"#),
                    (k::ICON_COLOR, r#"{ type = "string", default = "", description = "What that glyph is tinted with, as `#rrggbb` or a name from the theme's `[colors]`; empty takes the role's own", group = "paint" }"#),
                    (k::ICON_SIZE, r#"{ type = "float", default = -1.0, description = "That glyph's size in design pixels; below zero takes the caption's", group = "paint" }"#),
                    (k::ENABLED, r#"{ type = "bool", default = true, description = "Off, the widget is greyed out and swallows its clicks" }"#),
                    (k::FILL, r#"{ type = "string", default = "", description = "What is painted behind this widget, an `image`'s picture included, as `#rrggbb` or a name from the theme's `[colors]`; empty takes the theme's own", group = "paint" }"#),
                    (k::STROKE, r#"{ type = "string", default = "", description = "The outline around this widget, as `#rrggbb` or a name from the theme's `[colors]`; empty takes the theme's own", group = "paint" }"#),
                    (k::CORNER_RADIUS, r#"{ type = "float", default = -1.0, description = "Corner radius in design pixels, a `progress_bar`'s and an `image`'s included; below zero takes the theme's own, which for a button is as round as its text is tall", group = "paint" }"#),
                    (k::JUSTIFY, &format!(r#"{{ type = "enum", default = "{}", options = [{}], description = "How a container spreads its children along its own direction once they have their sizes, and a `grid` its columns: `stretch` grows a grid's `auto` columns into the room, and in a row or a column is `start`", group = "layout" }}"#, w::START, v::options(w::JUSTIFYS))),
                    (k::ALIGN_CONTENT, &format!(r#"{{ type = "enum", default = "{}", options = [{}], description = "How a container that wraps spreads its lines across its direction, and a `grid` its rows, where they leave room: `stretch` shares the room among them", group = "layout" }}"#, w::STRETCH, v::options(w::CONTENT_ALIGNS))),
                    (k::SAFE_ALIGN, r#"{ type = "bool", default = false, description = "Where `align_items`, `align_self`, `align_content` or `justify` would push a child past its container's start, put it at the start instead, so the start stays in view", group = "layout" }"#),
                    (k::WRAP_CHILDREN, &format!(r#"{{ type = "enum", default = "{}", options = [{}], description = "Whether a container's children run onto more lines when they do not fit: `auto` wraps a `flow` and keeps every other container on one line; `balance` evens the lines out over at least `min_lines`; a `_reverse` word stacks the lines the other way", group = "layout" }}"#, w::AUTO, v::options(w::wrapping::ALL))),
                    (k::MIN_LINES, r#"{ type = "int", default = 1, min = 1, description = "How many lines a wrapping container divides its children between at least: `balance` evens them over this many, and any wrap shares a known cross size among them", group = "layout" }"#),
                    (k::BORDER, r#"{ type = "vec4", default = [0.0, 0.0, 0.0, 0.0], description = "The width of this widget's outline on each side, left, top, right and bottom, in design pixels: the layout keeps it clear inside the edge as it keeps `padding`, and the `stroke` is painted in exactly that band. All zero leaves the stroke at the theme's `stroke_width`, over the padding", group = "layout" }"#),
                    (k::BOX_SIZING, &format!(r#"{{ type = "enum", default = "{}", options = [{}], description = "What `width` and `height` measure: `border`, the whole box, or `content`, the box inside `padding` and `border`", group = "layout" }}"#, w::sizing::BORDER, v::options(w::sizing::ALL))),
                    (k::DIRECTION, &format!(r#"{{ type = "enum", default = "{}", options = [{}], description = "Which way a row and a grid's columns run: `right_to_left` starts them at the right edge", group = "layout" }}"#, w::direction::LEFT_TO_RIGHT, v::options(w::direction::ALL))),
                    (k::OVERFLOW, &format!(r#"{{ type = "enum", default = "{}", options = [{}], description = "What the layout keeps for content past this widget's box: `scroll` keeps `scrollbar_width` clear inside its edge, the others nothing. Layout only: a container never gives a child a floor of its own content here (`min_width` is that floor), a `scroll` kind is what scrolls, and each kind clips as it draws", group = "layout" }}"#, w::overflow::VISIBLE, v::options(w::overflow::ALL))),
                    (k::SCROLLBAR_WIDTH, r#"{ type = "float", default = 0.0, min = 0.0, description = "The room a box with `overflow = \"scroll\"` keeps clear inside its edge for a bar, in design pixels", group = "layout" }"#),
                    (k::CONTAIN, &format!(r#"{{ type = "flags", default = [], options = [{}], description = "Containment: `layout` or `paint` makes the box lay out on its own, and `layout` also gives it no baseline for a parent's `baseline` alignment", group = "layout" }}"#, v::options(w::contain::ALL))),
                    (k::SENSE, &format!(r#"{{ type = "enum", default = "{}", options = [{}], description = "What the pointer may do to a `button`: `click` reports a press let go as a click; under `click_and_drag` a press that moves past egui's drag distance is a drag and no click; `drag` and `hover` never click", group = "events" }}"#, w::CLICK, v::options(w::SENSES))),
                    (k::SHOW_TOOLTIP_WHEN_ELIDED, r#"{ type = "bool", default = true, description = "Show a `label`'s whole text when the pointer rests on it and `truncate` cut it short", group = "type" }"#),
                    (k::INDETERMINATE, r#"{ type = "bool", default = false, description = "Draw a `checkbox` as neither ticked nor clear, for a group whose members disagree; a click still flips `checked`", group = "value" }"#),
                    (k::SHOW_VALUE, r#"{ type = "bool", default = false, description = "Draw a `slider`'s number beside its track, for the reader to drag or type into", group = "value" }"#),
                    (k::LOGARITHMIC, r#"{ type = "bool", default = false, description = "Space a `slider`'s track logarithmically, for a range across orders of magnitude", group = "value" }"#),
                    (k::SMALLEST_POSITIVE, r#"{ type = "float", default = 0.0, min = 0.0, description = "The smallest number above zero a `logarithmic` slider reaches, for a range that starts at or crosses zero; 0 takes egui's millionth", group = "value" }"#),
                    (k::LARGEST_FINITE, r#"{ type = "float", default = 0.0, min = 0.0, description = "The largest number a `logarithmic` slider reaches short of an unbounded end; 0 is no limit", group = "value" }"#),
                    (k::CLAMP, &format!(r#"{{ type = "enum", default = "{}", options = [{}], description = "Whether a `slider` holds its number inside `min` and `max`: `always`, one the scene set outside them included; `edits`, only what the reader enters; `never`, only the track", group = "value" }}"#, w::ALWAYS, v::options(w::CLAMPS))),
                    (k::CLAMP_EXISTING, r#"{ type = "bool", default = false, description = "Pull a `number_field`'s number back inside `min` and `max` where the scene set it outside them; off, it is shown as set and only an edit is bounded", group = "value" }"#),
                    (k::SMART_AIM, r#"{ type = "bool", default = true, description = "Round a dragged `slider` to the simplest number near the pointer", group = "value" }"#),
                    (k::DRAG_SPEED, r#"{ type = "float", default = 0.0, min = 0.0, description = "How far a `number_field`, or a `slider`'s shown number, moves per design pixel dragged; 0 takes egui's, which is 1 on a number field and scales with the range on a slider", group = "value" }"#),
                    (k::DECIMALS, r#"{ type = "int", default = -1, description = "How many decimals a `slider`'s or `number_field`'s number shows; below zero lets egui decide", group = "value" }"#),
                    (k::TRAILING_FILL, r#"{ type = "bool", default = false, description = "Fill a `slider`'s track from its start up to the handle", group = "paint" }"#),
                    (k::HANDLE, &format!(r#"{{ type = "enum", default = "{}", options = [{}], description = "A `slider`'s handle: a `circle`, or a `rect` `handle_aspect` times as wide as the track is tall", group = "paint" }}"#, w::CIRCLE, v::options(w::HANDLES))),
                    (k::HANDLE_ASPECT, r#"{ type = "float", default = 0.5, min = 0.0, description = "How wide a `rect` handle is against its height", group = "paint" }"#),
                    (k::NUMBER_FORMAT, &format!(r#"{{ type = "enum", default = "{}", options = [{}], description = "The base a `slider` or `number_field` writes its number in; the others than `decimal` write it whole", group = "value" }}"#, w::DECIMAL, v::options(w::NUMBER_FORMATS))),
                    (k::UPDATE_WHILE_EDITING, r#"{ type = "bool", default = true, description = "Write a `slider`'s or `number_field`'s number as the reader types it; off, only once the typing is left", group = "value" }"#),
                    (k::SHOW_PERCENTAGE, r#"{ type = "bool", default = false, description = "Draw a `progress_bar`'s fill as a percentage on it, in place of its `text`", group = "value" }"#),
                    (k::ANIMATE, r#"{ type = "bool", default = false, description = "Shimmer a `progress_bar` that is short of full, with a spinner at its end unless it states a `corner_radius`. It repaints every frame while it is on screen and short of full, and never otherwise", group = "paint" }"#),
                    (k::SPACING, r#"{ type = "float", default = -1.0, description = "The room a `separator` takes across its line, in design pixels, the line drawn down its middle; below zero is 6", group = "layout" }"#),
                    (k::OVERHANG, r#"{ type = "float", default = 0.0, description = "How far a `separator`'s line runs past each end of the room it was given, in design pixels; below zero stops it short", group = "layout" }"#),
                    (k::LIST_HEIGHT, r#"{ type = "float", default = 0.0, min = 0.0, description = "The tallest a `dropdown`'s list grows before it scrolls, in design pixels; 0 takes egui's", group = "layout" }"#),
                    (k::ALPHA, &format!(r#"{{ type = "enum", default = "{}", options = [{}], description = "What a `color_picker` offers for alpha: `none` keeps the colour opaque, `blend` offers transparency, `additive` that and additive blending", group = "value" }}"#, w::BLEND, v::options(w::ALPHAS))),
                    (k::INLINE, r#"{ type = "bool", default = false, description = "Draw a `color_picker`'s picker in place, rather than behind a swatch that opens it", group = "value" }"#),
                    (k::EDITABLE, r#"{ type = "bool", default = true, description = "Let the reader change a `text_field`, `text_area` or `code`'s text; off, it can be neither edited nor selected", group = "value" }"#),
                    (k::TAB_INSERTS, r#"{ type = "bool", default = false, description = "Let Tab type a tab into a `text_field`, `text_area` or `code` rather than move focus on", group = "value" }"#),
                    (k::CARET_AT_END, r#"{ type = "bool", default = true, description = "Put the caret at the end of a `text_field`, `text_area` or `code`'s text when it first takes focus; off, at the start", group = "value" }"#),
                    (k::CLIP_TEXT, r#"{ type = "bool", default = true, description = "Cut a `text_field`'s text at its edge; off, the field grows to show it whole", group = "value" }"#),
                    (k::SUBMIT_KEY, r#"{ type = "string", default = "", description = "The chord that submits a `text_field`, as `enter` or `cmd+enter`; empty is Enter. A `text_area` submits when focus leaves it whatever this says", group = "events" }"#),
                    (k::SCROLLBAR, &format!(r#"{{ type = "enum", default = "{}", options = [{}], description = "When a `scroll`, `list`, `tree` or `table` has its bars: while its content runs past it, always, or never. The bars float, so one shows once the pointer is over the box", group = "value" }}"#, w::AUTO, v::options(w::SCROLLBARS))),
                    (k::STICK_TO_END, r#"{ type = "bool", default = false, description = "Keep a `scroll`, `list`, `tree` or `table` at its end as content arrives, until the reader scrolls away from it; `ui.scroll` takes it too", group = "value" }"#),
                    (k::SCROLL_OFFSET, r#"{ type = "vec2", default = [-1.0, -1.0], description = "Where a `scroll`, `list`, `tree` or `table` is scrolled to, across and down, in design pixels: written as the reader scrolls, and a new one written by the scene or a script scrolls it there. Below zero on an axis leaves that axis where it is; `ui.scroll` takes it too", group = "value" }"#),
                    (k::MIN_SCROLLED_WIDTH, r#"{ type = "float", default = 0.0, min = 0.0, description = "The narrowest a `scroll`, `list`, `tree` or `table` draws while its content runs past it sideways, in design pixels; 0 takes egui's", group = "value" }"#),
                    (k::MIN_SCROLLED_HEIGHT, r#"{ type = "float", default = 0.0, min = 0.0, description = "The shortest a `scroll`, `list`, `tree` or `table` draws while its content runs past it downwards, in design pixels; 0 takes egui's", group = "value" }"#),
                    (k::ANIMATED, r#"{ type = "bool", default = true, description = "Ease a `scroll`, `list`, `tree` or `table` to where it is sent, such as a row scrolled into view, rather than jump", group = "value" }"#),
                    (k::WHEEL_SPEED, r#"{ type = "vec2", default = [1.0, 1.0], description = "How far a mouse wheel scrolls a `scroll`, `list`, `tree` or `table`, across and down, as a multiple of egui's own", group = "value" }"#),
                    (k::DRAG_SCROLL, &format!(r#"{{ type = "enum", default = "{}", options = [{}], description = "When dragging a `scroll`, `list`, `tree` or `table`'s content scrolls it: on a touch screen, always, or never; a `scroll_deadzone` takes the drag over itself", group = "value" }}"#, w::TOUCH, v::options(w::DRAG_SCROLLS))),
                    (k::WHEEL_SCROLL, r#"{ type = "bool", default = true, description = "Let the mouse wheel scroll a `scroll`, `list`, `tree` or `table`", group = "value" }"#),
                    (k::DRAG_CURSOR, &format!(r#"{{ type = "enum", default = "{}", options = [{}], description = "The pointer's shape while a drag scrolls a `scroll`, `list`, `tree` or `table`'s content; `arrow` leaves it as it is", group = "value" }}"#, w::cursor::ARROW, v::options(w::cursor::ALL))),
                    (k::TINT, r#"{ type = "color", default = [1.0, 1.0, 1.0, 1.0], description = "What an `image` is multiplied by; white draws it as it is", group = "paint" }"#),
                    (k::REGION, r#"{ type = "vec4", default = [0.0, 0.0, 0.0, 0.0], description = "The part of an `image` drawn, as left, top, width and height in the picture's own pixels, which is how an atlas shows one tile; a zero width or height draws the whole picture. `ui.image` takes it too", group = "paint" }"#),
                    (k::ANGLE_DEGREES, r#"{ type = "float", default = 0.0, description = "How far an `image` is turned clockwise about `angle_origin`, in degrees; a turned image keeps square corners", group = "paint" }"#),
                    (k::ANGLE_ORIGIN, r#"{ type = "vec2", default = [0.5, 0.5], description = "The point an `image` turns about, as fractions of its box from the top left: `[0.5, 0.5]` is its middle", group = "paint" }"#),
                    (k::ALT_TEXT, r#"{ type = "string", default = "", description = "Text an `image` draws in place of a picture that will not load", group = "type" }"#),
                    (k::BACKDROP_COLOR, r#"{ type = "color", default = [0.0, 0.0, 0.0, 0.549], description = "What a `dialog` dims the screen behind it with", group = "paint" }"#),
                    (k::DISMISSABLE, r#"{ type = "bool", default = true, description = "Let Escape or a click on the dim shut a `dialog`; off, only the scene does", group = "events" }"#),
                    (k::RESIZABLE, r#"{ type = "bool", default = false, description = "Give a `window` a grip in its bottom-right corner that a drag resizes it by, writing `width` and `height` within `min_width`, `min_height`, `max_width` and `max_height`; `ui.window` takes it too, on by default there", group = "events" }"#),
                    (k::COLLAPSIBLE, r#"{ type = "bool", default = false, description = "Give a `window`'s title bar an arrow that folds the window to its bar and back; `ui.window` takes it too", group = "events" }"#),
                    (k::CLOSABLE, r#"{ type = "bool", default = true, description = "Draw a `window`'s cross; `ui.window` takes it too", group = "events" }"#),
                    (k::DEFAULT_OPEN, r#"{ type = "bool", default = true, description = "Whether a `collapsible` window starts unfolded; `ui.window` takes it too", group = "events" }"#),
                    (k::MOVABLE, r#"{ type = "bool", default = false, description = "Let a drag move this root and write its `x` and `y`: a `window` by its title bar, any other root by its whole box where no child takes the press; `ui.window` takes it too, on by default there", group = "placement" }"#),
                    (k::CONSTRAIN, r#"{ type = "bool", default = false, description = "Keep a `movable` root inside its surface while it is dragged; `ui.window` takes it too", group = "placement" }"#),
                    (k::FADE_IN, r#"{ type = "bool", default = false, description = "Fade a root in as it appears, over egui's fade time", group = "paint" }"#),
                ]) + "\n" + &text_look_schema()),
            ),
            tags: &[balaur_core::components::tag::UI],
            expects: &[],
            apply: Box::new(apply_widget),
            remove: Box::new(remove_widget),
            get: Box::new(read_widget),
        },
    );
    // A screen class's table, whose own keys `check_class_tables` refuses.
    reg.accept_keys("widget", |key, _| CLASS_KEYS.contains(&key));
}

/// Refuse a `gap` written as one number: it is a pair now, and a number read
/// as one would lay a scene out differently from what it says.
fn check_shapes(params: &toml::Value) -> Result<()> {
    let tables = std::iter::once(params).chain(class_tables(params).map(|(_, table)| table));
    for table in tables {
        if table
            .get(k::GAP)
            .is_some_and(|gap| balaur_core::components::as_f64(gap).is_some())
        {
            anyhow::bail!("widget: `gap` takes [across, down], not one number");
        }
    }
    Ok(())
}

/// Refuse a key a class table invents, as the base table's are refused.
fn check_class_tables(eng: &balaur_core::Engine, params: &toml::Value) -> Result<()> {
    let registry = eng.resource::<balaur_core::components::ComponentRegistry>();
    let registry = registry.borrow();
    let declared = registry
        .def("widget")
        .and_then(|def| def.schema.as_table())
        .ok_or_else(|| anyhow::anyhow!("widget: the component has no schema"))?;
    for (word, table) in class_tables(params) {
        let Some(table) = table.as_table() else {
            anyhow::bail!("widget: `{word}` is a screen class and takes a table of properties");
        };
        for key in table.keys() {
            if key == k::KIND {
                anyhow::bail!(
                    "widget: `{word}.{key}` -- a widget cannot change kind with the screen"
                );
            }
            if !declared.contains_key(key) {
                anyhow::bail!("widget: `{word}.{key}` is not a widget property");
            }
        }
    }
    Ok(())
}

/// Put the widget a table describes on the node, in place of whatever it
/// had. The arena is told, since its copy is now a frame behind.
fn apply_widget(
    eng: &balaur_core::Engine,
    entity: balaur_core::hecs::Entity,
    params: &toml::Value,
) -> Result<()> {
    check_class_tables(eng, params)?;
    check_shapes(params)?;
    let widget = widget_from(params);
    crate::widget::grid::check(&widget.layout)?;
    let chord = &widget.egui.submit_key;
    if !chord.is_empty() && crate::immediate::chord(chord).is_none() {
        anyhow::bail!("widget: `submit_key` `{chord}` is not a chord, as `enter` or `cmd+enter`");
    }
    crate::widget::arena::widget_changed(entity);
    eng.world_mut()
        .insert_one(entity, widget)
        .map_err(|_| anyhow::anyhow!("node is dead"))
}

#[allow(
    clippy::unnecessary_wraps,
    reason = "the shape a component's `remove` hook is registered as"
)]
fn remove_widget(eng: &balaur_core::Engine, entity: balaur_core::hecs::Entity) -> Result<()> {
    crate::widget::arena::widget_changed(entity);
    let _ = eng.world_mut().remove_one::<Widget>(entity);
    Ok(())
}

/// One property, straight off the component, for the properties a control
/// reports. A pooled control reads what it now holds twice a frame, and
/// building the whole table for that was a twelfth of the editor's frame.
///
/// `None` for anything else, which reads the table and indexes it as before.
fn read_property(
    eng: &balaur_core::Engine,
    entity: balaur_core::hecs::Entity,
    key: &str,
) -> Option<toml::Value> {
    let world = eng.world();
    let widget = world.get::<&Widget>(entity).ok()?;
    match key {
        k::TEXT => Some(toml::Value::String(widget.text.to_string())),
        k::KIND => Some(toml::Value::String(widget.kind.to_string())),
        k::VALUE => Some(toml::Value::Float(f64::from(widget.value))),
        k::CHECKED => Some(toml::Value::Boolean(widget.checked)),
        k::CLICKED => Some(toml::Value::Boolean(widget.clicked)),
        // A pooled control asks every frame whether its field was submitted,
        // and a missing answer here built the whole forty-key table to say
        // false: four in five of the editor's table builds were this key.
        k::SUBMITTED => Some(toml::Value::Boolean(widget.submitted)),
        k::ROLE => Some(toml::Value::String(widget.role.to_string())),
        k::VISIBLE => Some(toml::Value::Boolean(widget.visible)),
        k::PICKED_COLOR => Some(toml::Value::Array(
            widget
                .color
                .iter()
                .map(|c| toml::Value::Float(f64::from(*c)))
                .collect(),
        )),
        _ => None,
    }
}

fn read_widget(
    eng: &balaur_core::Engine,
    entity: balaur_core::hecs::Entity,
) -> Option<toml::Value> {
    let world = eng.world();
    let widget = world.get::<&Widget>(entity).ok()?;
    Some(widget_to_toml(&widget))
}

/// The class tables a widget was authored with, as the map every property is
/// then written into.
fn class_tables_of(widget: &Widget) -> toml::map::Map<String, toml::Value> {
    let mut map = toml::map::Map::new();
    if let Some(authored) = widget.authored.as_ref() {
        for (word, table) in class_tables(authored) {
            map.insert(word.to_string(), table.clone());
        }
    }
    map
}

/// The sizes a widget asks of the room around it: its minimums, and the
/// surface lines it is not drawn past.
fn write_room(map: &mut toml::map::Map<String, toml::Value>, widget: &Widget) {
    for (key, value) in [
        (k::MIN_WIDTH, widget.min_width),
        (k::MIN_HEIGHT, widget.min_height),
        (k::HIDE_NARROWER, widget.hide_narrower),
        (k::HIDE_WIDER, widget.hide_wider),
        (k::HIDE_SHORTER, widget.hide_shorter),
        (k::HIDE_TALLER, widget.hide_taller),
    ] {
        map.insert(key.into(), toml::Value::Float(f64::from(value)));
    }
}

/// A `Widget` back as the property table the inspector and a script read.
///
/// The class tables come back with it: they are not properties of the widget,
/// so nothing above would put them back, and a scene saved without them would
/// have lost what it was authored with.
#[allow(
    clippy::too_many_lines,
    reason = "one insert per property a scene may state; the list is the schema"
)]
fn widget_to_toml(widget: &Widget) -> toml::Value {
    let mut map = class_tables_of(widget);
    map.insert(k::KIND.into(), toml::Value::String(widget.kind.to_string()));
    map.insert(k::TEXT.into(), toml::Value::String(widget.text.to_string()));
    map.insert(k::VISIBLE.into(), toml::Value::Boolean(widget.visible));
    map.insert(
        k::ANCHOR.into(),
        toml::Value::String(widget.anchor.to_string()),
    );
    map.insert(k::X.into(), toml::Value::Float(f64::from(widget.x)));
    map.insert(k::Y.into(), toml::Value::Float(f64::from(widget.y)));
    map.insert(k::WIDTH.into(), toml::Value::Float(f64::from(widget.width)));
    map.insert(
        k::HEIGHT.into(),
        toml::Value::Float(f64::from(widget.height)),
    );
    map.insert(
        k::FONT_SIZE.into(),
        toml::Value::Float(f64::from(widget.font_size)),
    );
    map.insert(
        k::TEXT_COLOR.into(),
        toml::Value::Array(
            widget
                .text_color
                .iter()
                .map(|c| toml::Value::Float(f64::from(*c)))
                .collect(),
        ),
    );
    map.insert(k::CLICKED.into(), toml::Value::Boolean(widget.clicked));
    map.insert(
        k::ON_CLICK.into(),
        toml::Value::String(widget.on_click.to_string()),
    );
    map.insert(k::PASS_NODE.into(), toml::Value::Boolean(widget.pass_node));
    map.insert(
        k::INTERACTIVE.into(),
        toml::Value::Boolean(!widget.pointer_through),
    );
    reach_to_toml(widget, &mut map);
    map.insert(k::PADDING.into(), four(widget.padding));
    map.insert(k::GAP.into(), two(widget.gap));
    map.insert(
        k::ALIGN_ITEMS.into(),
        toml::Value::String(widget.align.to_string()),
    );
    map.insert(k::AXIS.into(), toml::Value::String(widget.axis.to_string()));
    map.insert(k::FOCUSABLE.into(), toml::Value::Boolean(widget.focusable));
    map.insert(
        k::ON_FOCUS.into(),
        toml::Value::String(widget.on_focus.to_string()),
    );
    map.insert(
        k::THEME.into(),
        toml::Value::String(widget.theme.to_string()),
    );
    map.insert(
        k::TEXT_KEY.into(),
        toml::Value::String(widget.text_key.to_string()),
    );
    map.insert(k::GROW.into(), toml::Value::Float(f64::from(widget.grow)));
    write_room(&mut map, widget);
    map.insert(k::DRAW.into(), toml::Value::String(widget.draw.to_string()));
    map.insert(
        k::SPLITTER_WIDTH.into(),
        toml::Value::Float(f64::from(widget.handle)),
    );
    map.insert(
        k::CURRENT_PAGE.into(),
        toml::Value::String(widget.active.to_string()),
    );
    map.insert(
        k::LAYER.into(),
        toml::Value::String(widget.layer.to_string()),
    );
    map.insert(k::WRAP.into(), toml::Value::Boolean(widget.wrap));
    map.insert(k::TRUNCATE.into(), toml::Value::Boolean(widget.truncate));
    map.insert(k::KEEP_OPEN.into(), toml::Value::Boolean(widget.keep_open));
    map.insert(
        k::TRAILING.into(),
        toml::Value::String(widget.trailing.to_string()),
    );
    map.insert(
        k::SHORTCUT.into(),
        toml::Value::String(widget.shortcut.to_string()),
    );
    map.insert(k::SHOWING.into(), toml::Value::Boolean(widget.showing));
    map.insert(
        k::PLACEMENT.into(),
        toml::Value::String(widget.placement.to_string()),
    );
    map.insert(
        k::DURATION.into(),
        toml::Value::Float(f64::from(widget.duration)),
    );
    text_to_toml(widget, &mut map);
    look_to_toml(widget, &mut map);
    controls_to_toml(widget, &mut map);
    code_to_toml(widget, &mut map);
    crate::widget::options::layout_to_toml(&widget.layout, &mut map);
    crate::widget::options::options_to_toml(&widget.egui, &mut map);
    widget.text_look.put(&mut map);
    toml::Value::Table(map)
}

/// The text keys `text2d` shares, as schema lines.
fn text_look_schema() -> String {
    let lines = crate::widget::text_look::TextLook::schema();
    let borrowed: Vec<(&str, &str)> = lines
        .iter()
        .map(|(key, line)| (*key, line.as_str()))
        .collect();
    v::schema(&borrowed)
}

/// The keys that say what a widget answers to: the menu a right click opens,
/// the link a click reports, and what a number and a label let the player do.
fn reach_to_toml(widget: &Widget, map: &mut toml::map::Map<String, toml::Value>) {
    map.insert(
        k::CONTEXT.into(),
        toml::Value::String(widget.context.to_string()),
    );
    map.insert(
        k::ON_LINK.into(),
        toml::Value::String(widget.on_link.to_string()),
    );
    map.insert(
        k::SELECTABLE.into(),
        toml::Value::Boolean(widget.selectable),
    );
    map.insert(
        k::SUFFIX.into(),
        toml::Value::String(widget.suffix.to_string()),
    );
    map.insert(k::ARROWS.into(), toml::Value::Boolean(widget.arrows));
    map.insert(k::DRAGGABLE.into(), toml::Value::Boolean(widget.draggable));
    map.insert(
        k::HIDE_ON_CLOSE.into(),
        toml::Value::Boolean(widget.hide_on_close),
    );
    map.insert(
        k::ON_DROP.into(),
        toml::Value::String(widget.on_drop.to_string()),
    );
}

/// What a `code` widget marks in its gutter and how wide that gutter is.
///
/// The lines go back as text: the property is declared `strings`, which is the
/// closest list the schema has, and a reader takes either spelling.
fn code_to_toml(widget: &Widget, map: &mut toml::map::Map<String, toml::Value>) {
    map.insert(
        k::ON_GUTTER.into(),
        toml::Value::String(widget.on_gutter.to_string()),
    );
    for (key, lines) in [
        (k::BREAKPOINTS, &widget.breakpoints),
        (k::PROBLEMS, &widget.problems),
        (k::WARNINGS, &widget.warnings),
    ] {
        let rows = lines
            .iter()
            .map(|line| toml::Value::String(line.to_string()))
            .collect();
        map.insert(key.into(), toml::Value::Array(rows));
    }
    map.insert(
        k::CURRENT_LINE.into(),
        toml::Value::Integer(i64::from(widget.current_line)),
    );
    map.insert(
        k::GUTTER_WIDTH.into(),
        toml::Value::Float(f64::from(widget.gutter_width)),
    );
}

/// The keys a widget's text carries: where it sits, the face it is drawn in,
/// and what a `text_field` accepts.
fn text_to_toml(widget: &Widget, map: &mut toml::map::Map<String, toml::Value>) {
    map.insert(
        k::TEXT_ALIGN.into(),
        toml::Value::String(widget.text_align.to_string()),
    );
    // One field under three names: which one a widget reads depends on its kind.
    let named = match widget.kind.as_str() {
        w::CODE => k::LANGUAGE,
        w::LIST => k::SHEET,
        _ => k::IMAGE,
    };
    for key in [k::IMAGE, k::SHEET, k::LANGUAGE] {
        let value = if key == named {
            widget.source.to_string()
        } else {
            String::new()
        };
        map.insert(key.into(), toml::Value::String(value));
    }
    map.insert(k::FIT.into(), toml::Value::String(widget.fit.to_string()));
    map.insert(k::MARKUP.into(), toml::Value::Boolean(widget.markup));
    map.insert(
        k::FONT_WEIGHT.into(),
        toml::Value::Float(f64::from(widget.font_weight)),
    );
    map.insert(
        k::FONT_STYLE.into(),
        toml::Value::String(widget.font_style.to_string()),
    );
    map.insert(
        k::PLACEHOLDER.into(),
        toml::Value::String(widget.placeholder.to_string()),
    );
    map.insert(
        k::MAX_LENGTH.into(),
        toml::Value::Float(f64::from(widget.max_length)),
    );
    map.insert(k::SECRET.into(), toml::Value::Boolean(widget.secret));
    map.insert(k::NUMERIC.into(), toml::Value::Boolean(widget.numeric));
    map.insert(
        k::ON_CHANGE.into(),
        toml::Value::String(widget.on_change.to_string()),
    );
    map.insert(
        k::ON_SUBMIT.into(),
        toml::Value::String(widget.on_submit.to_string()),
    );
    map.insert(k::SUBMITTED.into(), toml::Value::Boolean(widget.submitted));
}

/// The keys a widget's look carries: the role it names, the marks and hover
/// text beside its caption, and the fill, outline and air it states itself.
fn look_to_toml(widget: &Widget, map: &mut toml::map::Map<String, toml::Value>) {
    map.insert(k::ROLE.into(), toml::Value::String(widget.role.to_string()));
    map.insert(
        k::TOOLTIP.into(),
        toml::Value::String(widget.tooltip.to_string()),
    );
    map.insert(
        k::CURSOR.into(),
        toml::Value::String(widget.cursor.to_string()),
    );
    map.insert(k::ICON.into(), toml::Value::String(widget.icon.to_string()));
    map.insert(
        k::ICON_COLOR.into(),
        toml::Value::String(widget.icon_color.to_string()),
    );
    map.insert(
        k::ICON_SIZE.into(),
        toml::Value::Float(f64::from(widget.icon_size)),
    );
    map.insert(k::ENABLED.into(), toml::Value::Boolean(!widget.disabled));
    map.insert(k::FILL.into(), toml::Value::String(widget.fill.to_string()));
    map.insert(
        k::STROKE.into(),
        toml::Value::String(widget.stroke.to_string()),
    );
    map.insert(
        k::CORNER_RADIUS.into(),
        toml::Value::Float(f64::from(widget.radius)),
    );
    map.insert(
        k::JUSTIFY.into(),
        toml::Value::String(widget.justify.to_string()),
    );
}

/// The keys the control kinds added: what a check, slider, dropdown, grid,
/// fold, fill root, sliced image and deadzone scroll carry.
fn controls_to_toml(widget: &Widget, map: &mut toml::map::Map<String, toml::Value>) {
    map.insert(k::CHECKED.into(), toml::Value::Boolean(widget.checked));
    map.insert(k::TOGGLE.into(), toml::Value::Boolean(widget.toggle));
    map.insert(
        k::GROUP.into(),
        toml::Value::String(widget.group.to_string()),
    );
    map.insert(k::PICKED_COLOR.into(), four(widget.color));
    map.insert(
        k::FONT_FAMILY.into(),
        toml::Value::String(widget.font.to_string()),
    );
    map.insert(
        k::ROW_HEIGHT.into(),
        toml::Value::Float(f64::from(widget.row_height)),
    );
    map.insert(k::VALUE.into(), toml::Value::Float(f64::from(widget.value)));
    map.insert(k::MIN.into(), toml::Value::Float(f64::from(widget.min)));
    map.insert(k::MAX.into(), toml::Value::Float(f64::from(widget.max)));
    map.insert(k::STEP.into(), toml::Value::Float(f64::from(widget.step)));
    map.insert(
        k::OPTIONS.into(),
        toml::Value::Array(
            widget
                .options
                .iter()
                .map(|o| toml::Value::String(o.to_string()))
                .collect(),
        ),
    );
    map.insert(
        k::SELECTION.into(),
        toml::Value::Array(
            widget
                .selection
                .iter()
                .map(|row| toml::Value::String(row.to_string()))
                .collect(),
        ),
    );
    map.insert(k::MULTI_SELECT.into(), toml::Value::Boolean(widget.multi));
    map.insert(
        k::TITLES.into(),
        toml::Value::Array(
            widget
                .titles
                .iter()
                .map(|title| toml::Value::String(title.to_string()))
                .collect(),
        ),
    );
    map.insert(
        k::WIDTHS.into(),
        toml::Value::Array(
            widget
                .widths
                .iter()
                .map(|share| toml::Value::String(format!("{share}")))
                .collect(),
        ),
    );
    map.insert(k::HEADER.into(), toml::Value::Boolean(widget.header));
    map.insert(k::SORT.into(), toml::Value::String(widget.sort.to_string()));
    map.insert(k::SORTABLE.into(), toml::Value::Boolean(widget.sortable));
    map.insert(k::REVERSE.into(), toml::Value::Boolean(widget.reverse));
    map.insert(
        k::REORDERABLE.into(),
        toml::Value::Boolean(widget.reorderable),
    );
    for (key, handler) in [(k::ON_MOVE, &widget.on_move), (k::ON_MARK, &widget.on_mark)] {
        map.insert(key.into(), toml::Value::String(handler.to_string()));
    }
    map.insert(k::OPEN.into(), toml::Value::Boolean(widget.open));
    map.insert(k::TITLE_BAR.into(), toml::Value::Boolean(widget.title_bar));
    map.insert(k::INSET.into(), four(widget.inset));
    map.insert(
        k::AVOID_KEYBOARD.into(),
        toml::Value::Boolean(widget.avoid_keyboard),
    );
    map.insert(
        k::SAFE_AREA.into(),
        toml::Value::Array(
            w::EDGES
                .iter()
                .zip(widget.safe_area)
                .filter(|(_, on)| *on)
                .map(|(name, _)| toml::Value::String((*name).to_string()))
                .collect(),
        ),
    );
    map.insert(k::SLICE.into(), four(widget.slice));
    map.insert(
        k::SCROLL_DEADZONE.into(),
        toml::Value::Float(f64::from(widget.deadzone)),
    );
}

/// The widget kinds as recipes, so the picker offers "Column" rather than
/// "a `widget`, then set `kind`".
///
/// Presets, not node types: balaur has no classes, and one for UI alone would
/// be a second model of what a node is (`balaur_core::presets`).
pub(crate) fn register_widget_presets(reg: &mut Registry<'_>) -> Result<()> {
    use balaur_core::presets::preset;
    let recipes = [
        (w::LABEL, "A line of text"),
        (w::TEXT_FIELD, "A line the player types into"),
        (w::BUTTON, "Text that reports its clicks"),
        (w::PANEL, "A framed box that lays out what is inside it"),
        (
            w::ROW,
            "Children side by side, sharing the leftover by `grow`",
        ),
        (
            w::COLUMN,
            "Children stacked, sharing the leftover by `grow`",
        ),
        (
            w::SCROLL,
            "A box that holds its size and clips what runs past it",
        ),
        (
            w::STACK,
            "Children over one another, each placed in the box by its `anchor`",
        ),
        (
            w::TABS,
            "One child showing, the rest named on a strip above it",
        ),
        (w::DRAW, "A rect a script fills, named by `draw`"),
        (
            w::IMAGE,
            "A picture from the project, sized by itself or by what it states",
        ),
        (w::CHECKBOX, "A box that ticks"),
        (w::DROPDOWN, "One of its `options`, picked from a list"),
        (w::SLIDER, "A number dragged between `min` and `max`"),
        (
            w::PROGRESS_BAR,
            "A bar filled to `value` between `min` and `max`",
        ),
        (
            w::GRID,
            "Children in the columns `grid_columns` names, two equal ones where it names none",
        ),
        (
            w::FLOW,
            "Children left to right, wrapping when the row is full",
        ),
        (w::FOLD, "A header that shows or hides what is under it"),
        (
            w::TOAST,
            "A message that stacks at its anchor and leaves when its `duration` is up",
        ),
        (
            w::DIALOG,
            "A panel over everything, with the screen behind it dimmed and deaf",
        ),
        ("separator", "A line between siblings"),
        (
            w::WINDOW,
            "A panel with a title bar, moved by dragging it and shut by its cross",
        ),
    ];
    for (name, description) in recipes {
        // A window is dragged by its title bar; no other root moves unasked.
        let params = if name == w::WINDOW {
            format!("{} = \"{name}\"\n{} = true", k::KIND, k::MOVABLE)
        } else {
            format!("{} = \"{name}\"", k::KIND)
        };
        reg.register_preset(
            name,
            preset(
                description,
                &[balaur_core::components::tag::UI],
                &[("widget", Some(params.as_str()))],
            )?,
        );
    }
    Ok(())
}

/// A `Widget` built from a full property table (defaults already merged).
#[allow(
    clippy::too_many_lines,
    reason = "one line per property a scene may state; the literal is the list"
)]
fn widget_from(params: &toml::Value) -> Widget {
    let r = Read(params);
    let (s, f) = (|k: &str| r.str(k), |k: &str| r.num(k));
    let mut widget = Widget {
        kind: s(k::KIND),
        group: s(k::GROUP),
        text: s(k::TEXT),
        visible: r.flag(k::VISIBLE),
        anchor: s(k::ANCHOR),
        x: f(k::X),
        y: f(k::Y),
        width: f(k::WIDTH),
        height: f(k::HEIGHT),
        font_size: f(k::FONT_SIZE),
        text_color: r.quad(k::TEXT_COLOR),
        color: r.quad(k::PICKED_COLOR),
        row_height: f(k::ROW_HEIGHT),
        font: s(k::FONT_FAMILY),
        on_click: s(k::ON_CLICK),
        pass_node: r.flag(k::PASS_NODE),
        pointer_through: !r.on_unless_off(k::INTERACTIVE),
        context: s(k::CONTEXT),
        on_link: s(k::ON_LINK),
        selectable: r.flag(k::SELECTABLE),
        suffix: s(k::SUFFIX),
        arrows: r.flag(k::ARROWS),
        on_gutter: s(k::ON_GUTTER),
        padding: sides(params, k::PADDING),
        gap: pair(params, k::GAP),
        align: s(k::ALIGN_ITEMS),
        axis: s(k::AXIS),
        focusable: r.flag(k::FOCUSABLE),
        on_focus: s(k::ON_FOCUS),
        theme: s(k::THEME),
        text_key: s(k::TEXT_KEY),
        grow: f(k::GROW),
        min_width: f(k::MIN_WIDTH),
        min_height: f(k::MIN_HEIGHT),
        draw: s(k::DRAW),
        handle: f(k::SPLITTER_WIDTH),
        active: s(k::CURRENT_PAGE),
        layer: s(k::LAYER),
        wrap: r.flag(k::WRAP),
        truncate: r.flag(k::TRUNCATE),
        keep_open: r.flag(k::KEEP_OPEN),
        trailing: s(k::TRAILING),
        shortcut: s(k::SHORTCUT),
        showing: r.flag(k::SHOWING),
        placement: s(k::PLACEMENT),
        duration: f(k::DURATION),
        text_align: s(k::TEXT_ALIGN),
        source: r.first(&[k::IMAGE, k::SHEET, k::LANGUAGE]),
        fit: s(k::FIT),
        markup: r.flag(k::MARKUP),
        font_weight: f(k::FONT_WEIGHT),
        font_style: s(k::FONT_STYLE),
        placeholder: s(k::PLACEHOLDER),
        max_length: f(k::MAX_LENGTH),
        secret: r.flag(k::SECRET),
        numeric: r.flag(k::NUMERIC),
        on_change: s(k::ON_CHANGE),
        on_submit: s(k::ON_SUBMIT),
        submitted: r.flag(k::SUBMITTED),
        role: s(k::ROLE),
        tooltip: s(k::TOOLTIP),
        cursor: s(k::CURSOR),
        icon: s(k::ICON),
        icon_color: s(k::ICON_COLOR),
        icon_size: f(k::ICON_SIZE),
        disabled: !r.on_unless_off(k::ENABLED),
        fill: s(k::FILL),
        stroke: s(k::STROKE),
        radius: f(k::CORNER_RADIUS),
        justify: s(k::JUSTIFY),
        on_mark: s(k::ON_MARK),
        layout: crate::widget::options::read_layout(params),
        egui: std::sync::Arc::new(crate::widget::options::read_options(params)),
        text_look: std::sync::Arc::new(crate::widget::text_look::TextLook::read(params)),
        ..Widget::default()
    };
    read_controls(&mut widget, params);
    widget.authored = authored(params);
    widget
}

/// The table as the scene wrote it, kept only where a class table can
/// override it later.
fn authored(params: &toml::Value) -> Option<std::sync::Arc<toml::Value>> {
    class_tables(params)
        .next()
        .is_some()
        .then(|| std::sync::Arc::new(params.clone()))
}

/// Every class table this widget carries, in the order an override applies:
/// the input class, then the height, then the width, later winning. A phone
/// held upright answers `touch`, `tall` and `narrow`, in that order.
pub(crate) const CLASS_KEYS: [&str; 7] = [
    balaur_core::tags::TOUCH,
    balaur_core::tags::POINTER,
    balaur_core::facts::SHORT,
    balaur_core::facts::TALL,
    balaur_core::facts::NARROW,
    balaur_core::facts::MEDIUM,
    balaur_core::facts::WIDE,
];

/// The `[nodes.widget.<class>]` tables a widget states, in `CLASS_KEYS` order.
fn class_tables(params: &toml::Value) -> impl Iterator<Item = (&str, &toml::Value)> {
    CLASS_KEYS.into_iter().filter_map(move |word| {
        let table = params.get(word)?;
        table.as_table().map(|_| (word, table))
    })
}

/// The widget a scene authored, read again for the classes in force.
///
/// `None` where it carries no class table, which is almost every widget: the
/// caller keeps the one it has rather than building a second.
pub(crate) fn for_classes(widget: &Widget, active: &[&str]) -> Option<Widget> {
    let authored = widget.authored.as_ref()?;
    let base = authored.as_table()?;
    let mut merged = base.clone();
    let mut changed = false;
    // Broad to narrow, so the narrowest class named wins the key.
    for (word, table) in class_tables(authored) {
        if !active.contains(&word) {
            continue;
        }
        for (key, value) in table.as_table()? {
            merged.insert(key.clone(), value.clone());
            changed = true;
        }
    }
    changed.then(|| widget_from(&toml::Value::Table(merged)))
}

/// The keys the control kinds read: a check's tick, a slider's range, a
/// dropdown's options, a grid's columns, a fold's state, a fill root's
/// insets, an image's slice and a scroll's deadzone.
fn read_controls(widget: &mut Widget, params: &toml::Value) {
    let r = Read(params);
    let (f, b) = (|k: &str| r.num(k), |k: &str| r.flag(k));
    widget.checked = b(k::CHECKED);
    widget.toggle = b(k::TOGGLE);
    widget.group = r.str(k::GROUP);
    widget.value = f(k::VALUE);
    widget.min = f(k::MIN);
    widget.max = f(k::MAX);
    widget.step = f(k::STEP);
    widget.options = strings(params, k::OPTIONS);
    widget.selection = strings(params, k::SELECTION);
    widget.multi = b(k::MULTI_SELECT);
    widget.titles = strings(params, k::TITLES);
    widget.widths = shares(params, k::WIDTHS);
    widget.header = b(k::HEADER);
    widget.sort = r.str(k::SORT);
    widget.sortable = b(k::SORTABLE);
    widget.reverse = b(k::REVERSE);
    widget.reorderable = b(k::REORDERABLE);
    widget.on_move = r.str(k::ON_MOVE);
    widget.draggable = b(k::DRAGGABLE);
    widget.hide_on_close = b(k::HIDE_ON_CLOSE);
    widget.on_drop = r.str(k::ON_DROP);
    widget.open = b(k::OPEN);
    widget.title_bar = b(k::TITLE_BAR);
    widget.inset = crate::widget::theme::four_of(params.get(k::INSET));
    widget.avoid_keyboard = b(k::AVOID_KEYBOARD);
    widget.safe_area = edges_of(params.get(k::SAFE_AREA));
    widget.hide_narrower = f(k::HIDE_NARROWER);
    widget.hide_wider = f(k::HIDE_WIDER);
    widget.hide_shorter = f(k::HIDE_SHORTER);
    widget.hide_taller = f(k::HIDE_TALLER);
    widget.slice = crate::widget::theme::four_of(params.get(k::SLICE));
    widget.deadzone = f(k::SCROLL_DEADZONE);
    widget.breakpoints = lines(params, k::BREAKPOINTS);
    widget.problems = lines(params, k::PROBLEMS);
    widget.warnings = lines(params, k::WARNINGS);
    widget.current_line = f(k::CURRENT_LINE).max(0.0) as u32;
    widget.gutter_width = f(k::GUTTER_WIDTH);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widget::theme::Style;

    fn widget(params: &toml::Value) -> Widget {
        widget_from(params)
    }

    /// A widget that states no size reads as 0, which is what tells `face`
    /// to take the size its role or its kind carries. A default of 16 here
    /// made that sentinel unreachable, so no role ever set a text size.
    #[test]
    fn an_unstated_size_leaves_the_role_to_answer() {
        let bare = widget(&toml::toml! { kind = "label" role = "meta" }.into());
        assert!(
            bare.font_size.abs() < f32::EPSILON,
            "the default shadowed the role"
        );
        let stated = widget(&toml::toml! { kind = "label" font_size = 13.0 }.into());
        assert!(
            (stated.font_size - 13.0).abs() < f32::EPSILON,
            "a stated size must still win"
        );
    }

    #[test]
    fn a_widget_lets_the_pointer_through_only_when_it_says_so() {
        let through = widget(&toml::toml! { kind = "panel" interactive = false }.into());
        assert!(through.pointer_through);
        assert!(!widget(&toml::toml! { kind = "panel" }.into()).pointer_through);
    }

    #[test]
    fn a_cursor_word_reads_onto_the_widget() {
        let hand = widget(&toml::toml! { kind = "label" cursor = "hand" }.into());
        assert_eq!(hand.cursor, "hand");
        let bare = widget(&toml::toml! { kind = "label" }.into());
        assert!(
            bare.cursor.is_empty(),
            "the schema's `arrow` default is merged by add and patch, not read here"
        );
    }

    /// The shaper is told which family to use. It shaped everything in `ui`
    /// before, so a node label could not be mono however it asked.
    #[test]
    fn the_shaper_is_told_the_role_s_family() {
        let widget = widget(&toml::toml! { kind = "label" role = "meta" }.into());
        let style = Style {
            font: Some("mono".to_string()),
            ..Style::default()
        };
        let font = egui::FontId::new(11.0, egui::FontFamily::Proportional);
        let request = crate::widget::text::text_request(&widget, "editing", None, &font, &style);
        assert_eq!(request.family, "mono");
    }

    /// A widget naming its own family keeps it over the role's.
    #[test]
    fn a_widget_s_own_family_wins() {
        let widget = widget(&toml::toml! { kind = "label" font_family = "heading" }.into());
        let style = Style {
            font: Some("mono".to_string()),
            ..Style::default()
        };
        let font = egui::FontId::new(11.0, egui::FontFamily::Proportional);
        let request = crate::widget::text::text_request(&widget, "x", None, &font, &style);
        assert_eq!(request.family, "heading");
    }
}
