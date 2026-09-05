//! The `widget` component's schema: what a scene file may say about a
//! widget, the presets the picker offers, and the table a `Widget` reads
//! back as.

use anyhow::Result;
use balaur_core::components::ComponentDef;
use balaur_plugin::Registry;

use crate::vocabulary::{self as v, keys as k, words as w};
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
                &v::schema(&[
                    (k::KIND, &format!(r#"{{ type = "enum", default = "{}", options = [{}], description = "The HUD element the widget layer draws" }}"#, w::LABEL, v::options(w::WIDGET_KINDS))),
                    (k::TEXT, r#"{ type = "string", default = "label", description = "Label or button caption" }"#),
                    (k::VISIBLE, r#"{ type = "bool", default = true, description = "Draw the widget; hidden widgets keep their state" }"#),
                    (k::ANCHOR, &format!(r#"{{ type = "enum", default = "{}", options = [{}], description = "Screen corner or center the offset is measured from; `fill` takes the whole surface less `inset`" }}"#, w::TOP_LEFT, v::options(w::ANCHORS))),
                    (k::X, r#"{ type = "float", default = 16.0, description = "Horizontal offset from the anchor, in design pixels" }"#),
                    (k::Y, r#"{ type = "float", default = 16.0, description = "Vertical offset from the anchor, in design pixels" }"#),
                    (k::WIDTH, r#"{ type = "float", default = 0.0, min = 0.0, description = "Panel width in design pixels; 0 sizes to content" }"#),
                    (k::HEIGHT, r#"{ type = "float", default = 0.0, min = 0.0, description = "Panel height in design pixels; 0 sizes to content" }"#),
                    (k::FONT_SIZE, r#"{ type = "float", default = 16.0, min = 6.0, description = "Text size in design pixels" }"#),
                    (k::TEXT_COLOR, r#"{ type = "color", default = [0.933, 0.945, 0.957, 1.0], description = "Text color" }"#),
                    (k::PADDING, r#"{ type = "float", default = 0.0, min = 0.0, description = "Space inside a container's edge, in design pixels" }"#),
                    (k::GAP, r#"{ type = "float", default = 8.0, min = 0.0, description = "Space between a container's children, in design pixels" }"#),
                    (k::ALIGN, &format!(r#"{{ type = "enum", default = "{}", options = [{}], description = "Where a container puts its children across its own direction" }}"#, w::START, v::options(w::ALIGNS))),
                    (k::FOCUSABLE, r#"{ type = "bool", default = true, description = "Let focus land here. A widget nothing can activate is never focused whatever this says; set it false to skip one that could be" }"#),
                    (k::ON_FOCUS, r#"{ type = "string", default = "", description = "Script method called on this node when focus arrives" }"#),
                    (k::THEME, &format!(r#"{{ type = "asset", asset = "{}", default = "", description = "How this widget and everything under it is drawn; inherited from the nearest ancestor that names one" }}"#, crate::widget_theme::ASSET_TYPE)),
                    (k::TEXT_KEY, r#"{ type = "string", default = "", description = "A localization key drawn in place of `text`, re-read every frame so a locale switch shows at once" }"#),
                    (k::ON_CLICK, r#"{ type = "string", default = "", description = "Script method called on this node when the button is clicked" }"#),
                    (k::CLICKED, r#"{ type = "bool", default = false, readonly = true, description = "True on the frame the button was clicked" }"#),
                    (k::GROW, r#"{ type = "float", default = 0.0, min = 0.0, description = "Share of the leftover space a container hands out along its own direction; 0 takes only what this widget asks for" }"#),
                    (k::MIN_WIDTH, r#"{ type = "float", default = 0.0, min = 0.0, description = "Smallest width a container may give this widget, in design pixels" }"#),
                    (k::MIN_HEIGHT, r#"{ type = "float", default = 0.0, min = 0.0, description = "Smallest height a container may give this widget, in design pixels" }"#),
                    (k::DRAW, r#"{ type = "string", default = "", description = "What fills a `draw` widget: a script method on this node or the nearest scripted ancestor, or `scripts/file.rn:function` for a free function" }"#),
                    (k::HANDLE, r#"{ type = "float", default = 0.0, min = 0.0, description = "How wide a grab the seams between this container's children get, in design pixels; 0 leaves them fixed. A drag writes the new size onto the neighbour that states one" }"#),
                    (k::ACTIVE, r#"{ type = "string", default = "", description = "Which child a `tab` shows, by node name; empty shows the first" }"#),
                    (k::LAYER, r#"{ type = "string", default = "", description = "The drawing surface this root belongs to; empty is the default one, and a name nothing has configured takes the default surface" }"#),
                    (k::WRAP, r#"{ type = "bool", default = false, description = "Break text to the width the widget was given instead of running past it on one line" }"#),
                    (k::TEXT_ALIGN, &format!(r#"{{ type = "enum", default = "{}", options = [{}], description = "Where text sits in the width the widget was given" }}"#, w::START, v::options(w::ALIGNS))),
                    (k::SOURCE, r#"{ type = "string", default = "", description = "The project-relative image an `image` widget draws" }"#),
                    (k::MARKUP, r#"{ type = "bool", default = false, description = "Read inline marks in the text: `[b]`, `[i]`, `[color=#hex]`, `[center]`, `[right]`, `[wave amp=N freq=N]` and `[img=path width=N]`; off, brackets are text" }"#),
                    (k::FONT_WEIGHT, r#"{ type = "float", default = 400.0, min = 100.0, max = 900.0, description = "Weight on the CSS scale, resolved against the faces the project ships: 400 regular, 700 bold" }"#),
                    (k::FONT_STYLE, &format!(r#"{{ type = "enum", default = "{}", options = [{}], description = "Slant, from an italic face the project ships" }}"#, w::NORMAL, v::options(w::FONT_STYLES))),
                    (k::PLACEHOLDER, r#"{ type = "string", default = "", description = "What a `field` shows while it is empty" }"#),
                    (k::MAX_LENGTH, r#"{ type = "float", default = 0.0, min = 0.0, description = "The most characters a `field` takes; 0 is no limit" }"#),
                    (k::SECRET, r#"{ type = "bool", default = false, description = "Draw a `field`'s text as dots, for a password" }"#),
                    (k::NUMERIC, r#"{ type = "bool", default = false, description = "Keep a `field` to digits, a sign and a point" }"#),
                    (k::ON_CHANGE, r#"{ type = "string", default = "", description = "Script method called on this node with a `field`'s text after every edit" }"#),
                    (k::ON_SUBMIT, r#"{ type = "string", default = "", description = "Script method called on this node with a `field`'s text on Enter, or when focus leaves it" }"#),
                    (k::CHECKED, r#"{ type = "bool", default = false, description = "Whether a `check` is ticked; every click flips it and calls `on_change` with the new state" }"#),
                    (k::VALUE, r#"{ type = "float", default = 0.0, description = "Where a `slider` or `progress` stands, between `min` and `max`; a slider writes it and calls `on_change` with it" }"#),
                    (k::MIN, r#"{ type = "float", default = 0.0, description = "The low end of a `slider` or `progress`" }"#),
                    (k::MAX, r#"{ type = "float", default = 1.0, description = "The high end of a `slider` or `progress`" }"#),
                    (k::STEP, r#"{ type = "float", default = 0.0, min = 0.0, description = "The grid a `slider` snaps to; 0 is continuous" }"#),
                    (k::OPTIONS, r#"{ type = "strings", default = [], description = "What a `dropdown` offers; `text` is the one chosen, and `on_change` hears the new one" }"#),
                    (k::COLUMNS, r#"{ type = "int", default = 2, min = 1, description = "How many children a `grid` puts on each row" }"#),
                    (k::OPEN, r#"{ type = "bool", default = true, description = "Whether a `fold` shows its children; its header flips it and calls `on_change` with the new state" }"#),
                    (k::INSET, r#"{ type = "vec4", default = [0.0, 0.0, 0.0, 0.0], description = "Left, top, right and bottom margins a root with `anchor = \"fill\"` keeps from its surface, in design pixels" }"#),
                    (k::SLICE, r#"{ type = "vec4", default = [0.0, 0.0, 0.0, 0.0], description = "Left, top, right and bottom borders of an `image` kept unstretched, in the picture's own pixels; all zero stretches the whole picture" }"#),
                    (k::DEADZONE, r#"{ type = "float", default = 0.0, min = 0.0, description = "How far a finger drags a `scroll` before it scrolls, in design pixels, so a tap on a child still lands; 0 scrolls at once" }"#),
                ]),
            ),
            tags: &[balaur_core::components::tag::UI],
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
    map.insert(k::KIND.into(), toml::Value::String(widget.kind.clone()));
    map.insert(k::TEXT.into(), toml::Value::String(widget.text.clone()));
    map.insert(k::VISIBLE.into(), toml::Value::Boolean(widget.visible));
    map.insert(k::ANCHOR.into(), toml::Value::String(widget.anchor.clone()));
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
        toml::Value::String(widget.on_click.clone()),
    );
    map.insert(
        k::PADDING.into(),
        toml::Value::Float(f64::from(widget.padding)),
    );
    map.insert(k::GAP.into(), toml::Value::Float(f64::from(widget.gap)));
    map.insert(k::ALIGN.into(), toml::Value::String(widget.align.clone()));
    map.insert(k::FOCUSABLE.into(), toml::Value::Boolean(widget.focusable));
    map.insert(
        k::ON_FOCUS.into(),
        toml::Value::String(widget.on_focus.clone()),
    );
    map.insert(k::THEME.into(), toml::Value::String(widget.theme.clone()));
    map.insert(
        k::TEXT_KEY.into(),
        toml::Value::String(widget.text_key.clone()),
    );
    map.insert(k::GROW.into(), toml::Value::Float(f64::from(widget.grow)));
    map.insert(
        k::MIN_WIDTH.into(),
        toml::Value::Float(f64::from(widget.min_width)),
    );
    map.insert(
        k::MIN_HEIGHT.into(),
        toml::Value::Float(f64::from(widget.min_height)),
    );
    map.insert(k::DRAW.into(), toml::Value::String(widget.draw.clone()));
    map.insert(
        k::HANDLE.into(),
        toml::Value::Float(f64::from(widget.handle)),
    );
    map.insert(k::ACTIVE.into(), toml::Value::String(widget.active.clone()));
    map.insert(k::LAYER.into(), toml::Value::String(widget.layer.clone()));
    map.insert(k::WRAP.into(), toml::Value::Boolean(widget.wrap));
    map.insert(
        k::TEXT_ALIGN.into(),
        toml::Value::String(widget.text_align.clone()),
    );
    map.insert(k::SOURCE.into(), toml::Value::String(widget.source.clone()));
    map.insert(k::MARKUP.into(), toml::Value::Boolean(widget.markup));
    map.insert(
        k::FONT_WEIGHT.into(),
        toml::Value::Float(f64::from(widget.font_weight)),
    );
    map.insert(
        k::FONT_STYLE.into(),
        toml::Value::String(widget.font_style.clone()),
    );
    map.insert(
        k::PLACEHOLDER.into(),
        toml::Value::String(widget.placeholder.clone()),
    );
    map.insert(
        k::MAX_LENGTH.into(),
        toml::Value::Float(f64::from(widget.max_length)),
    );
    map.insert(k::SECRET.into(), toml::Value::Boolean(widget.secret));
    map.insert(k::NUMERIC.into(), toml::Value::Boolean(widget.numeric));
    map.insert(
        k::ON_CHANGE.into(),
        toml::Value::String(widget.on_change.clone()),
    );
    map.insert(
        k::ON_SUBMIT.into(),
        toml::Value::String(widget.on_submit.clone()),
    );
    controls_to_toml(widget, &mut map);
    toml::Value::Table(map)
}

/// The keys the control kinds added: what a check, slider, dropdown, grid,
/// fold, fill root, sliced image and deadzone scroll carry.
fn controls_to_toml(widget: &Widget, map: &mut toml::map::Map<String, toml::Value>) {
    map.insert(k::CHECKED.into(), toml::Value::Boolean(widget.checked));
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
                .map(|o| toml::Value::String(o.clone()))
                .collect(),
        ),
    );
    map.insert(
        k::COLUMNS.into(),
        toml::Value::Integer(i64::from(widget.columns)),
    );
    map.insert(k::OPEN.into(), toml::Value::Boolean(widget.open));
    map.insert(k::INSET.into(), four(widget.inset));
    map.insert(k::SLICE.into(), four(widget.slice));
    map.insert(
        k::DEADZONE.into(),
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
        (w::FIELD, "A line the player types into"),
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
            w::TAB,
            "One child showing, the rest named on a strip above it",
        ),
        ("draw", "A rect a script fills, named by `draw`"),
        (
            w::IMAGE,
            "A picture from the project, sized by itself or by what it states",
        ),
        (w::CHECK, "A box that ticks"),
        (w::DROPDOWN, "One of its `options`, picked from a list"),
        (w::SLIDER, "A number dragged between `min` and `max`"),
        (
            w::PROGRESS,
            "A bar filled to `value` between `min` and `max`",
        ),
        (
            w::GRID,
            "Children in rows of `columns`, every cell the same size",
        ),
        (
            w::FLOW,
            "Children left to right, wrapping when the row is full",
        ),
        (w::FOLD, "A header that shows or hides what is under it"),
        (
            w::DIALOG,
            "A panel over everything, with the screen behind it dimmed and deaf",
        ),
        ("separator", "A line between siblings"),
    ];
    for (name, description) in recipes {
        let params = format!("{} = \"{name}\"", k::KIND);
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
            .get(k::TEXT_COLOR)
            .and_then(|v| v.as_array())
            .and_then(|a| a.get(i))
            .and_then(balaur_core::components::as_f64)
            .unwrap_or(default) as f32
    };
    let mut widget = Widget {
        kind: s(k::KIND, w::LABEL),
        text: s(k::TEXT, w::LABEL),
        visible: params
            .get(k::VISIBLE)
            .and_then(toml::Value::as_bool)
            .unwrap_or(true),
        anchor: s(k::ANCHOR, w::TOP_LEFT),
        x: f(k::X, 16.0),
        y: f(k::Y, 16.0),
        width: f(k::WIDTH, 0.0),
        height: f(k::HEIGHT, 0.0),
        font_size: f(k::FONT_SIZE, 16.0),
        text_color: [
            channel(0, 0.933),
            channel(1, 0.945),
            channel(2, 0.957),
            channel(3, 1.0),
        ],
        on_click: s(k::ON_CLICK, ""),
        clicked: false,
        padding: f(k::PADDING, 0.0),
        gap: f(k::GAP, 8.0),
        align: s(k::ALIGN, w::START),
        focusable: params
            .get(k::FOCUSABLE)
            .and_then(toml::Value::as_bool)
            .unwrap_or(true),
        on_focus: s(k::ON_FOCUS, ""),
        theme: s(k::THEME, ""),
        text_key: s(k::TEXT_KEY, ""),
        grow: f(k::GROW, 0.0),
        min_width: f(k::MIN_WIDTH, 0.0),
        min_height: f(k::MIN_HEIGHT, 0.0),
        draw: s(k::DRAW, ""),
        handle: f(k::HANDLE, 0.0),
        active: s(k::ACTIVE, ""),
        layer: s(k::LAYER, ""),
        wrap: params
            .get(k::WRAP)
            .and_then(toml::Value::as_bool)
            .unwrap_or(false),
        text_align: s(k::TEXT_ALIGN, w::START),
        source: s(k::SOURCE, ""),
        markup: params
            .get(k::MARKUP)
            .and_then(toml::Value::as_bool)
            .unwrap_or(false),
        font_weight: f(k::FONT_WEIGHT, 400.0),
        font_style: s(k::FONT_STYLE, w::NORMAL),
        placeholder: s(k::PLACEHOLDER, ""),
        max_length: f(k::MAX_LENGTH, 0.0),
        secret: params
            .get(k::SECRET)
            .and_then(toml::Value::as_bool)
            .unwrap_or(false),
        numeric: params
            .get(k::NUMERIC)
            .and_then(toml::Value::as_bool)
            .unwrap_or(false),
        on_change: s(k::ON_CHANGE, ""),
        on_submit: s(k::ON_SUBMIT, ""),
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
    widget.checked = b(k::CHECKED, false);
    widget.value = f(k::VALUE, 0.0);
    widget.min = f(k::MIN, 0.0);
    widget.max = f(k::MAX, 1.0);
    widget.step = f(k::STEP, 0.0);
    widget.options = params
        .get(k::OPTIONS)
        .and_then(toml::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(toml::Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    widget.columns = (f(k::COLUMNS, 2.0).max(1.0)) as u32;
    widget.open = b(k::OPEN, true);
    widget.inset = crate::widget_theme::four_of(params.get(k::INSET));
    widget.slice = crate::widget_theme::four_of(params.get(k::SLICE));
    widget.deadzone = f(k::DEADZONE, 0.0);
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
