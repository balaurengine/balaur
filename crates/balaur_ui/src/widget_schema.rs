//! The `widget` component's schema: what a scene file may say about a
//! widget, the presets the picker offers, and the table a `Widget` reads
//! back as.

use anyhow::Result;
use balaur_core::components::ComponentDef;
use balaur_plugin::Registry;

use crate::widget_layer::Widget;

/// The `widget` key, backed by exactly one `Widget` component on the node.
///
/// `clicked` is declared `readonly`: [`crate::widget_input`] writes it every
/// tick and `apply` always clears it, but it is in the schema so that `get`'s
/// output round-trips and the inspector can see it.
pub(crate) fn register_widget_component(reg: &mut Registry<'_>) {
    reg.register_component(
        "widget",
        ComponentDef {
            doc: "A HUD element the widget layer draws every frame: a label, button or panel \
                  anchored to a screen corner or the center, offset in design pixels. A button \
                  records its click in `clicked` and calls the node's `on_click` method.",
            schema: ComponentDef::parse_schema(
                "widget",
                r#"kind = { type = "enum", default = "label", options = ["label", "button", "panel", "row", "column", "scroll", "tab", "draw", "image", "field", "check", "dropdown", "slider", "progress", "grid", "flow", "fold", "dialog", "separator"], description = "The HUD element the widget layer draws" }
text = { type = "string", default = "label", description = "Label or button caption" }
visible = { type = "bool", default = true, description = "Draw the widget; hidden widgets keep their state" }
anchor = { type = "enum", default = "top_left", options = ["top_left", "top_right", "bottom_left", "bottom_right", "center", "fill"], description = "Screen corner or center the offset is measured from; `fill` takes the whole surface less `inset`" }
x = { type = "float", default = 16.0, description = "Horizontal offset from the anchor, in design pixels" }
y = { type = "float", default = 16.0, description = "Vertical offset from the anchor, in design pixels" }
width = { type = "float", default = 0.0, min = 0.0, description = "Panel width in design pixels; 0 sizes to content" }
height = { type = "float", default = 0.0, min = 0.0, description = "Panel height in design pixels; 0 sizes to content" }
font_size = { type = "float", default = 16.0, min = 6.0, description = "Text size in design pixels" }
text_color = { type = "color", default = [0.933, 0.945, 0.957, 1.0], description = "Text color" }
padding = { type = "float", default = 0.0, min = 0.0, description = "Space inside a container's edge, in design pixels" }
gap = { type = "float", default = 8.0, min = 0.0, description = "Space between a container's children, in design pixels" }
align = { type = "enum", default = "start", options = ["start", "center", "end"], description = "Where a container puts its children across its own direction" }
focusable = { type = "bool", default = true, description = "Let focus land here. A widget nothing can activate is never focused whatever this says; set it false to skip one that could be" }
on_focus = { type = "string", default = "", description = "Script method called on this node when focus arrives" }
theme = { type = "asset", asset = "widget_theme", default = "", description = "How this widget and everything under it is drawn; inherited from the nearest ancestor that names one" }
text_key = { type = "string", default = "", description = "A localization key drawn in place of `text`, re-read every frame so a locale switch shows at once" }
on_click = { type = "string", default = "", description = "Script method called on this node when the button is clicked" }
clicked = { type = "bool", default = false, readonly = true, description = "True on the frame the button was clicked" }
grow = { type = "float", default = 0.0, min = 0.0, description = "Share of the leftover space a container hands out along its own direction; 0 takes only what this widget asks for" }
min_width = { type = "float", default = 0.0, min = 0.0, description = "Smallest width a container may give this widget, in design pixels" }
min_height = { type = "float", default = 0.0, min = 0.0, description = "Smallest height a container may give this widget, in design pixels" }
draw = { type = "string", default = "", description = "What fills a `draw` widget: a script method on this node or the nearest scripted ancestor, or `scripts/file.rn:function` for a free function" }
handle = { type = "float", default = 0.0, min = 0.0, description = "How wide a grab the seams between this container's children get, in design pixels; 0 leaves them fixed. A drag writes the new size onto the neighbour that states one" }
active = { type = "string", default = "", description = "Which child a `tab` shows, by node name; empty shows the first" }
layer = { type = "string", default = "", description = "The drawing surface this root belongs to; empty is the default one, and a name nothing has configured takes the default surface" }
wrap = { type = "bool", default = false, description = "Break text to the width the widget was given instead of running past it on one line" }
text_align = { type = "enum", default = "start", options = ["start", "center", "end"], description = "Where text sits in the width the widget was given" }
source = { type = "string", default = "", description = "The project-relative image an `image` widget draws" }
markup = { type = "bool", default = false, description = "Read inline marks in the text: `[b]`, `[i]`, `[color=#hex]`, `[center]`, `[right]`, `[wave amp=N freq=N]` and `[img=path width=N]`; off, brackets are text" }
font_weight = { type = "float", default = 400.0, min = 100.0, max = 900.0, description = "Weight on the CSS scale, resolved against the faces the project ships: 400 regular, 700 bold" }
font_style = { type = "enum", default = "normal", options = ["normal", "italic"], description = "Slant, from an italic face the project ships" }
placeholder = { type = "string", default = "", description = "What a `field` shows while it is empty" }
max_length = { type = "float", default = 0.0, min = 0.0, description = "The most characters a `field` takes; 0 is no limit" }
secret = { type = "bool", default = false, description = "Draw a `field`'s text as dots, for a password" }
numeric = { type = "bool", default = false, description = "Keep a `field` to digits, a sign and a point" }
on_change = { type = "string", default = "", description = "Script method called on this node with a `field`'s text after every edit" }
on_submit = { type = "string", default = "", description = "Script method called on this node with a `field`'s text on Enter, or when focus leaves it" }
checked = { type = "bool", default = false, description = "Whether a `check` is ticked; every click flips it and calls `on_change` with the new state" }
value = { type = "float", default = 0.0, description = "Where a `slider` or `progress` stands, between `min` and `max`; a slider writes it and calls `on_change` with it" }
min = { type = "float", default = 0.0, description = "The low end of a `slider` or `progress`" }
max = { type = "float", default = 1.0, description = "The high end of a `slider` or `progress`" }
step = { type = "float", default = 0.0, min = 0.0, description = "The grid a `slider` snaps to; 0 is continuous" }
options = { type = "strings", default = [], description = "What a `dropdown` offers; `text` is the one chosen, and `on_change` hears the new one" }
columns = { type = "int", default = 2, min = 1, description = "How many children a `grid` puts on each row" }
open = { type = "bool", default = true, description = "Whether a `fold` shows its children; its header flips it and calls `on_change` with the new state" }
inset = { type = "vec4", default = [0.0, 0.0, 0.0, 0.0], description = "Left, top, right and bottom margins a root with `anchor = \"fill\"` keeps from its surface, in design pixels" }
slice = { type = "vec4", default = [0.0, 0.0, 0.0, 0.0], description = "Left, top, right and bottom borders of an `image` kept unstretched, in the picture's own pixels; all zero stretches the whole picture" }
deadzone = { type = "float", default = 0.0, min = 0.0, description = "How far a finger drags a `scroll` before it scrolls, in design pixels, so a tap on a child still lands; 0 scrolls at once" }"#,
            ),
            tags: &["ui"],
            expects: &[],
            apply: Box::new(|eng, entity, params| {
                eng.world_mut()
                    .insert_one(entity, widget_from(params))
                    .map_err(|_| anyhow::anyhow!("node is dead"))
            }),
            remove: Box::new(|eng, entity| {
                let _ = eng.world_mut().remove_one::<Widget>(entity);
                Ok(())
            }),
            get: Box::new(|eng, entity| {
                let world = eng.world();
                let widget = world.get::<&Widget>(entity).ok()?;
                Some(widget_to_toml(&widget))
            }),
        },
    );
}

/// A `Widget` back as the property table the inspector and a script read.
fn widget_to_toml(widget: &Widget) -> toml::Value {
    let mut map = toml::map::Map::new();
    map.insert("kind".into(), toml::Value::String(widget.kind.clone()));
    map.insert("text".into(), toml::Value::String(widget.text.clone()));
    map.insert("visible".into(), toml::Value::Boolean(widget.visible));
    map.insert("anchor".into(), toml::Value::String(widget.anchor.clone()));
    map.insert("x".into(), toml::Value::Float(f64::from(widget.x)));
    map.insert("y".into(), toml::Value::Float(f64::from(widget.y)));
    map.insert("width".into(), toml::Value::Float(f64::from(widget.width)));
    map.insert(
        "height".into(),
        toml::Value::Float(f64::from(widget.height)),
    );
    map.insert(
        "font_size".into(),
        toml::Value::Float(f64::from(widget.font_size)),
    );
    map.insert(
        "text_color".into(),
        toml::Value::Array(
            widget
                .text_color
                .iter()
                .map(|c| toml::Value::Float(f64::from(*c)))
                .collect(),
        ),
    );
    map.insert("clicked".into(), toml::Value::Boolean(widget.clicked));
    map.insert(
        "on_click".into(),
        toml::Value::String(widget.on_click.clone()),
    );
    map.insert(
        "padding".into(),
        toml::Value::Float(f64::from(widget.padding)),
    );
    map.insert("gap".into(), toml::Value::Float(f64::from(widget.gap)));
    map.insert("align".into(), toml::Value::String(widget.align.clone()));
    map.insert("focusable".into(), toml::Value::Boolean(widget.focusable));
    map.insert(
        "on_focus".into(),
        toml::Value::String(widget.on_focus.clone()),
    );
    map.insert("theme".into(), toml::Value::String(widget.theme.clone()));
    map.insert(
        "text_key".into(),
        toml::Value::String(widget.text_key.clone()),
    );
    map.insert("grow".into(), toml::Value::Float(f64::from(widget.grow)));
    map.insert(
        "min_width".into(),
        toml::Value::Float(f64::from(widget.min_width)),
    );
    map.insert(
        "min_height".into(),
        toml::Value::Float(f64::from(widget.min_height)),
    );
    map.insert("draw".into(), toml::Value::String(widget.draw.clone()));
    map.insert(
        "handle".into(),
        toml::Value::Float(f64::from(widget.handle)),
    );
    map.insert("active".into(), toml::Value::String(widget.active.clone()));
    map.insert("layer".into(), toml::Value::String(widget.layer.clone()));
    map.insert("wrap".into(), toml::Value::Boolean(widget.wrap));
    map.insert(
        "text_align".into(),
        toml::Value::String(widget.text_align.clone()),
    );
    map.insert("source".into(), toml::Value::String(widget.source.clone()));
    map.insert("markup".into(), toml::Value::Boolean(widget.markup));
    map.insert(
        "font_weight".into(),
        toml::Value::Float(f64::from(widget.font_weight)),
    );
    map.insert(
        "font_style".into(),
        toml::Value::String(widget.font_style.clone()),
    );
    map.insert(
        "placeholder".into(),
        toml::Value::String(widget.placeholder.clone()),
    );
    map.insert(
        "max_length".into(),
        toml::Value::Float(f64::from(widget.max_length)),
    );
    map.insert("secret".into(), toml::Value::Boolean(widget.secret));
    map.insert("numeric".into(), toml::Value::Boolean(widget.numeric));
    map.insert(
        "on_change".into(),
        toml::Value::String(widget.on_change.clone()),
    );
    map.insert(
        "on_submit".into(),
        toml::Value::String(widget.on_submit.clone()),
    );
    controls_to_toml(widget, &mut map);
    toml::Value::Table(map)
}

/// The keys the control kinds added: what a check, slider, dropdown, grid,
/// fold, fill root, sliced image and deadzone scroll carry.
fn controls_to_toml(widget: &Widget, map: &mut toml::map::Map<String, toml::Value>) {
    map.insert("checked".into(), toml::Value::Boolean(widget.checked));
    map.insert("value".into(), toml::Value::Float(f64::from(widget.value)));
    map.insert("min".into(), toml::Value::Float(f64::from(widget.min)));
    map.insert("max".into(), toml::Value::Float(f64::from(widget.max)));
    map.insert("step".into(), toml::Value::Float(f64::from(widget.step)));
    map.insert(
        "options".into(),
        toml::Value::Array(
            widget
                .options
                .iter()
                .map(|o| toml::Value::String(o.clone()))
                .collect(),
        ),
    );
    map.insert(
        "columns".into(),
        toml::Value::Integer(i64::from(widget.columns)),
    );
    map.insert("open".into(), toml::Value::Boolean(widget.open));
    map.insert("inset".into(), four(widget.inset));
    map.insert("slice".into(), four(widget.slice));
    map.insert(
        "deadzone".into(),
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
        ("label", "A line of text", "kind = \"label\""),
        ("field", "A line the player types into", "kind = \"field\""),
        (
            "button",
            "Text that reports its clicks",
            "kind = \"button\"",
        ),
        (
            "panel",
            "A framed box that lays out what is inside it",
            "kind = \"panel\"",
        ),
        (
            "row",
            "Children side by side, sharing the leftover by `grow`",
            "kind = \"row\"",
        ),
        (
            "column",
            "Children stacked, sharing the leftover by `grow`",
            "kind = \"column\"",
        ),
        (
            "scroll",
            "A box that holds its size and clips what runs past it",
            "kind = \"scroll\"",
        ),
        (
            "tab",
            "One child showing, the rest named on a strip above it",
            "kind = \"tab\"",
        ),
        (
            "draw",
            "A rect a script fills, named by `draw`",
            "kind = \"draw\"",
        ),
        (
            "image",
            "A picture from the project, sized by itself or by what it states",
            "kind = \"image\"",
        ),
        ("check", "A box that ticks", "kind = \"check\""),
        (
            "dropdown",
            "One of its `options`, picked from a list",
            "kind = \"dropdown\"",
        ),
        (
            "slider",
            "A number dragged between `min` and `max`",
            "kind = \"slider\"",
        ),
        (
            "progress",
            "A bar filled to `value` between `min` and `max`",
            "kind = \"progress\"",
        ),
        (
            "grid",
            "Children in rows of `columns`, every cell the same size",
            "kind = \"grid\"",
        ),
        (
            "flow",
            "Children left to right, wrapping when the row is full",
            "kind = \"flow\"",
        ),
        (
            "fold",
            "A header that shows or hides what is under it",
            "kind = \"fold\"",
        ),
        (
            "dialog",
            "A panel over everything, with the screen behind it dimmed and deaf",
            "kind = \"dialog\"",
        ),
        (
            "separator",
            "A line between siblings",
            "kind = \"separator\"",
        ),
    ];
    for (name, description, params) in recipes {
        reg.register_preset(
            name,
            preset(description, &["ui"], &[("widget", Some(params))])?,
        );
    }
    Ok(())
}

/// A `Widget` built from a full property table (defaults already merged).
fn widget_from(params: &toml::Value) -> Widget {
    let s = |key: &str, default: &str| {
        params
            .get(key)
            .and_then(|v| v.as_str())
            .unwrap_or(default)
            .to_string()
    };
    let f = |key: &str, default: f64| {
        params
            .get(key)
            .and_then(balaur_core::components::as_f64)
            .unwrap_or(default) as f32
    };
    // Hex strings were expanded to floats by `merge_defaults`.
    let channel = |i: usize, default: f64| {
        params
            .get("text_color")
            .and_then(|v| v.as_array())
            .and_then(|a| a.get(i))
            .and_then(balaur_core::components::as_f64)
            .unwrap_or(default) as f32
    };
    let mut widget = Widget {
        kind: s("kind", "label"),
        text: s("text", "label"),
        visible: params
            .get("visible")
            .and_then(toml::Value::as_bool)
            .unwrap_or(true),
        anchor: s("anchor", "top_left"),
        x: f("x", 16.0),
        y: f("y", 16.0),
        width: f("width", 0.0),
        height: f("height", 0.0),
        font_size: f("font_size", 16.0),
        text_color: [
            channel(0, 0.933),
            channel(1, 0.945),
            channel(2, 0.957),
            channel(3, 1.0),
        ],
        on_click: s("on_click", ""),
        clicked: false,
        padding: f("padding", 0.0),
        gap: f("gap", 8.0),
        align: s("align", "start"),
        focusable: params
            .get("focusable")
            .and_then(toml::Value::as_bool)
            .unwrap_or(true),
        on_focus: s("on_focus", ""),
        theme: s("theme", ""),
        text_key: s("text_key", ""),
        grow: f("grow", 0.0),
        min_width: f("min_width", 0.0),
        min_height: f("min_height", 0.0),
        draw: s("draw", ""),
        handle: f("handle", 0.0),
        active: s("active", ""),
        layer: s("layer", ""),
        wrap: params
            .get("wrap")
            .and_then(toml::Value::as_bool)
            .unwrap_or(false),
        text_align: s("text_align", "start"),
        source: s("source", ""),
        markup: params
            .get("markup")
            .and_then(toml::Value::as_bool)
            .unwrap_or(false),
        font_weight: f("font_weight", 400.0),
        font_style: s("font_style", "normal"),
        placeholder: s("placeholder", ""),
        max_length: f("max_length", 0.0),
        secret: params
            .get("secret")
            .and_then(toml::Value::as_bool)
            .unwrap_or(false),
        numeric: params
            .get("numeric")
            .and_then(toml::Value::as_bool)
            .unwrap_or(false),
        on_change: s("on_change", ""),
        on_submit: s("on_submit", ""),
        checked: false,
        value: 0.0,
        min: 0.0,
        max: 1.0,
        step: 0.0,
        options: Vec::new(),
        columns: 2,
        open: true,
        inset: [0.0; 4],
        slice: [0.0; 4],
        deadzone: 0.0,
    };
    read_controls(&mut widget, params);
    widget
}

/// The keys the control kinds read: a check's tick, a slider's range, a
/// dropdown's options, a grid's columns, a fold's state, a fill root's
/// insets, an image's slice and a scroll's deadzone.
fn read_controls(widget: &mut Widget, params: &toml::Value) {
    let f = |key: &str, default: f64| {
        params
            .get(key)
            .and_then(balaur_core::components::as_f64)
            .unwrap_or(default) as f32
    };
    let b = |key: &str, default: bool| {
        params
            .get(key)
            .and_then(toml::Value::as_bool)
            .unwrap_or(default)
    };
    widget.checked = b("checked", false);
    widget.value = f("value", 0.0);
    widget.min = f("min", 0.0);
    widget.max = f("max", 1.0);
    widget.step = f("step", 0.0);
    widget.options = params
        .get("options")
        .and_then(toml::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(toml::Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    widget.columns = (f("columns", 2.0).max(1.0)) as u32;
    widget.open = b("open", true);
    widget.inset = crate::widget_theme::four_of(params.get("inset"));
    widget.slice = crate::widget_theme::four_of(params.get("slice"));
    widget.deadzone = f("deadzone", 0.0);
}

/// Four numbers as the array a `vec4` property holds.
fn four(values: [f32; 4]) -> toml::Value {
    toml::Value::Array(
        values
            .iter()
            .map(|v| toml::Value::Float(f64::from(*v)))
            .collect(),
    )
}
