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
                    (k::TEXT_COLOR, r#"{ type = "color", default = [0.0, 0.0, 0.0, 0.0], description = "Text color; fully transparent takes the theme's colour for this widget's role or kind, and failing that a near-white" }"#),
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
                    (k::SOURCE, r#"{ type = "string", default = "", description = "The project-relative image an `image` widget draws, the sheet a `list` cuts its card faces from, and the language a `code` widget highlights" }"#),
                    (k::MARKUP, r#"{ type = "bool", default = false, description = "Read inline marks in the text: `[b]`, `[i]`, `[color=#hex]`, `[center]`, `[right]`, `[wave amp=N freq=N]` and `[img=path width=N]`; off, brackets are text" }"#),
                    (k::FONT_WEIGHT, r#"{ type = "float", default = 400.0, min = 100.0, max = 900.0, description = "Weight on the CSS scale, resolved against the faces the project ships: 400 regular, 700 bold" }"#),
                    (k::FONT_STYLE, &format!(r#"{{ type = "enum", default = "{}", options = [{}], description = "Slant, from an italic face the project ships" }}"#, w::NORMAL, v::options(w::FONT_STYLES))),
                    (k::PLACEHOLDER, r#"{ type = "string", default = "", description = "What a `field` shows while it is empty, the letter a `drag_value` puts before its number, and a `table`'s column names split on U+001F" }"#),
                    (k::MAX_LENGTH, r#"{ type = "float", default = 0.0, min = 0.0, description = "The most characters a `field` takes; 0 is no limit" }"#),
                    (k::SECRET, r#"{ type = "bool", default = false, description = "Draw a `field`'s text as dots, for a password" }"#),
                    (k::NUMERIC, r#"{ type = "bool", default = false, description = "Keep a `field` to digits, a sign and a point" }"#),
                    (k::ON_CHANGE, r#"{ type = "string", default = "", description = "Script method called on this node with a `field`'s text after every edit" }"#),
                    (k::ON_SUBMIT, r#"{ type = "string", default = "", description = "Script method called on this node with a `field`'s text on Enter, or when focus leaves it" }"#),
                    (k::CHECKED, r#"{ type = "bool", default = false, description = "Whether a `check` is ticked; every click flips it and calls `on_change` with the new state" }"#),
                    (k::VALUE, r#"{ type = "float", default = 0.0, description = "Where a `slider`, `drag_value` or `progress` stands, between `min` and `max`; a slider and a drag value write it and call `on_change` with it" }"#),
                    (k::MIN, r#"{ type = "float", default = 0.0, description = "The low end of a `slider` or `progress`; a `drag_value` runs free while this pair is the default 0 and 1" }"#),
                    (k::MAX, r#"{ type = "float", default = 1.0, description = "The high end of a `slider` or `progress`; a `drag_value` runs free while this pair is the default 0 and 1" }"#),
                    (k::STEP, r#"{ type = "float", default = 0.0, min = 0.0, description = "The grid a `slider` snaps to, and how fast a `drag_value` moves under the pointer; 0 is continuous" }"#),
                    (k::COLOR, r#"{ type = "color", default = [1.0, 1.0, 1.0, 1.0], description = "What a `color` swatch holds; `on_change` hears the new one" }"#),
                    (k::ROW_HEIGHT, r#"{ type = "float", default = 0.0, min = 0.0, description = "The pitch of a `list` or `tree` row, in design pixels; 0 takes the font's own line height" }"#),
                    (k::FONT, &format!(r#"{{ type = "enum", default = "{}", options = [{}], description = "Which of the theme's families the widget draws in" }}"#, w::UI, v::options(w::WIDGET_FONTS))),
                    (k::OPTIONS, r#"{ type = "strings", default = [], description = "The items a `dropdown`, `menu`, `list`, `tree` or `table` holds; `text` is the one picked, except on a `menu` where it is the button caption. A `tree` row starts with one tab per level, a `list` or `tree` row splits on U+001F into icon, label, a trailing note and an `#rrggbb` for that row, and a `table` row splits on the same into one cell a column. `on_change` hears every pick" }"#),
                    (k::COLUMNS, r#"{ type = "int", default = 0, min = 0, description = "How many children a `grid` puts on each row, and how many cards a `list` flows into; 0 is the kind's own, which is two for a grid and one line a row for a list" }"#),
                    (k::OPEN, r#"{ type = "bool", default = true, description = "Whether a `fold` shows its children; its header flips it and calls `on_change` with the new state" }"#),
                    (k::INSET, r#"{ type = "vec4", default = [0.0, 0.0, 0.0, 0.0], description = "Left, top, right and bottom margins a root with `anchor = \"fill\"` keeps from its surface, in design pixels" }"#),
                    (k::SLICE, r#"{ type = "vec4", default = [0.0, 0.0, 0.0, 0.0], description = "Left, top, right and bottom borders of an `image` kept unstretched, in the picture's own pixels; all zero stretches the whole picture" }"#),
                    (k::DEADZONE, r#"{ type = "float", default = 0.0, min = 0.0, description = "How far a finger drags a `scroll` before it scrolls, in design pixels, so a tap on a child still lands; 0 scrolls at once" }"#),
                    (k::ROLE, r#"{ type = "string", default = "", description = "A `[roles.<name>]` entry of the widget's theme, taken over its kind's own style; the one place a look is named rather than spelled" }"#),
                    (k::TOOLTIP, r#"{ type = "string", default = "", description = "Text shown after the pointer rests on the widget; still shown when it is `disabled`, which is where it says why" }"#),
                    (k::ICON, r#"{ type = "string", default = "", description = "A glyph from the theme's icon family, drawn before `text`" }"#),
                    (k::DISABLED, r#"{ type = "bool", default = false, description = "Grey the widget out and swallow its clicks" }"#),
                    (k::FILL, r#"{ type = "string", default = "", description = "What is painted behind this widget, as `#rrggbb` or a name from the theme's `[colors]`; empty takes the theme's own" }"#),
                    (k::STROKE, r#"{ type = "string", default = "", description = "The outline around this widget, as `#rrggbb` or a name from the theme's `[colors]`; empty takes the theme's own" }"#),
                    (k::RADIUS, r#"{ type = "float", default = -1.0, description = "Corner radius in design pixels; below zero takes the theme's own, which for a button is as round as its text is tall" }"#),
                    (k::PADDING_X, r#"{ type = "float", default = -1.0, description = "The air either side of a caption, in design pixels; below zero takes the theme's own" }"#),
                    (k::JUSTIFY, &format!(r#"{{ type = "enum", default = "{}", options = [{}], description = "How a container spreads its children along its own direction once they have their sizes" }}"#, w::START, v::options(w::JUSTIFYS))),
                ]),
            ),
            tags: &[balaur_core::components::tag::UI],
            expects: &[],
            apply: Box::new(|eng, entity, params| {
                crate::widget_layer::widget_changed(entity);
                eng.world_mut()
                    .insert_one(entity, widget_from(params))
                    .map_err(|_| anyhow::anyhow!("node is dead"))
            }),
            remove: Box::new(|eng, entity| {
                crate::widget_layer::widget_changed(entity);
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
    map.insert(
        k::PADDING.into(),
        toml::Value::Float(f64::from(widget.padding)),
    );
    map.insert(k::GAP.into(), toml::Value::Float(f64::from(widget.gap)));
    map.insert(
        k::ALIGN.into(),
        toml::Value::String(widget.align.to_string()),
    );
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
    map.insert(
        k::MIN_WIDTH.into(),
        toml::Value::Float(f64::from(widget.min_width)),
    );
    map.insert(
        k::MIN_HEIGHT.into(),
        toml::Value::Float(f64::from(widget.min_height)),
    );
    map.insert(k::DRAW.into(), toml::Value::String(widget.draw.to_string()));
    map.insert(
        k::HANDLE.into(),
        toml::Value::Float(f64::from(widget.handle)),
    );
    map.insert(
        k::ACTIVE.into(),
        toml::Value::String(widget.active.to_string()),
    );
    map.insert(
        k::LAYER.into(),
        toml::Value::String(widget.layer.to_string()),
    );
    map.insert(k::WRAP.into(), toml::Value::Boolean(widget.wrap));
    text_to_toml(widget, &mut map);
    look_to_toml(widget, &mut map);
    controls_to_toml(widget, &mut map);
    toml::Value::Table(map)
}

/// The keys a widget's text carries: where it sits, the face it is drawn in,
/// and what a `field` accepts.
fn text_to_toml(widget: &Widget, map: &mut toml::map::Map<String, toml::Value>) {
    map.insert(
        k::TEXT_ALIGN.into(),
        toml::Value::String(widget.text_align.to_string()),
    );
    map.insert(
        k::SOURCE.into(),
        toml::Value::String(widget.source.to_string()),
    );
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
}

/// The keys a widget's look carries: the role it names, the marks and hover
/// text beside its caption, and the fill, outline and air it states itself.
fn look_to_toml(widget: &Widget, map: &mut toml::map::Map<String, toml::Value>) {
    map.insert(k::ROLE.into(), toml::Value::String(widget.role.to_string()));
    map.insert(
        k::TOOLTIP.into(),
        toml::Value::String(widget.tooltip.to_string()),
    );
    map.insert(k::ICON.into(), toml::Value::String(widget.icon.to_string()));
    map.insert(k::DISABLED.into(), toml::Value::Boolean(widget.disabled));
    map.insert(k::FILL.into(), toml::Value::String(widget.fill.to_string()));
    map.insert(
        k::STROKE.into(),
        toml::Value::String(widget.stroke.to_string()),
    );
    map.insert(
        k::RADIUS.into(),
        toml::Value::Float(f64::from(widget.radius)),
    );
    map.insert(
        k::JUSTIFY.into(),
        toml::Value::String(widget.justify.to_string()),
    );
    map.insert(
        k::PADDING_X.into(),
        toml::Value::Float(f64::from(widget.padding_x)),
    );
}

/// The keys the control kinds added: what a check, slider, dropdown, grid,
/// fold, fill root, sliced image and deadzone scroll carry.
fn controls_to_toml(widget: &Widget, map: &mut toml::map::Map<String, toml::Value>) {
    map.insert(k::CHECKED.into(), toml::Value::Boolean(widget.checked));
    map.insert(k::COLOR.into(), four(widget.color));
    map.insert(k::FONT.into(), toml::Value::String(widget.font.to_string()));
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
/// The four readers every field goes through, so a `widget_from` that grows a
/// property grows by one line rather than by a closure.
struct Read<'a>(&'a toml::Value);

/// Every reader takes the key alone: the schema declares a default for each
/// property and `add`/`patch` merge them in before this runs, so a default
/// written here too would be a second copy of one nothing checks.
impl Read<'_> {
    fn str(&self, key: &str) -> smol_str::SmolStr {
        balaur_core::components::prop_str(self.0, key).into()
    }

    fn num(&self, key: &str) -> f32 {
        balaur_core::components::prop_f32(self.0, key)
    }

    fn flag(&self, key: &str) -> bool {
        balaur_core::components::prop_bool(self.0, key)
    }

    /// One colour's four channels. Hex strings were expanded to floats by
    /// `merge_defaults`, so this only ever reads an array.
    fn quad(&self, key: &str) -> [f32; 4] {
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

fn widget_from(params: &toml::Value) -> Widget {
    let r = Read(params);
    let (s, f) = (|k: &str| r.str(k), |k: &str| r.num(k));
    let mut widget = Widget {
        kind: s(k::KIND),
        text: s(k::TEXT),
        visible: r.flag(k::VISIBLE),
        anchor: s(k::ANCHOR),
        x: f(k::X),
        y: f(k::Y),
        width: f(k::WIDTH),
        height: f(k::HEIGHT),
        font_size: f(k::FONT_SIZE),
        text_color: r.quad(k::TEXT_COLOR),
        color: r.quad(k::COLOR),
        row_height: f(k::ROW_HEIGHT),
        font: s(k::FONT),
        on_click: s(k::ON_CLICK),
        clicked: false,
        padding: f(k::PADDING),
        gap: f(k::GAP),
        align: s(k::ALIGN),
        focusable: r.flag(k::FOCUSABLE),
        on_focus: s(k::ON_FOCUS),
        theme: s(k::THEME),
        text_key: s(k::TEXT_KEY),
        grow: f(k::GROW),
        min_width: f(k::MIN_WIDTH),
        min_height: f(k::MIN_HEIGHT),
        draw: s(k::DRAW),
        handle: f(k::HANDLE),
        active: s(k::ACTIVE),
        layer: s(k::LAYER),
        wrap: r.flag(k::WRAP),
        text_align: s(k::TEXT_ALIGN),
        source: s(k::SOURCE),
        markup: r.flag(k::MARKUP),
        font_weight: f(k::FONT_WEIGHT),
        font_style: s(k::FONT_STYLE),
        placeholder: s(k::PLACEHOLDER),
        max_length: f(k::MAX_LENGTH),
        secret: r.flag(k::SECRET),
        numeric: r.flag(k::NUMERIC),
        on_change: s(k::ON_CHANGE),
        on_submit: s(k::ON_SUBMIT),
        role: s(k::ROLE),
        tooltip: s(k::TOOLTIP),
        icon: s(k::ICON),
        disabled: r.flag(k::DISABLED),
        fill: s(k::FILL),
        stroke: s(k::STROKE),
        radius: f(k::RADIUS),
        justify: s(k::JUSTIFY),
        padding_x: f(k::PADDING_X),
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
    let r = Read(params);
    let (f, b) = (|k: &str| r.num(k), |k: &str| r.flag(k));
    widget.checked = b(k::CHECKED);
    widget.value = f(k::VALUE);
    widget.min = f(k::MIN);
    widget.max = f(k::MAX);
    widget.step = f(k::STEP);
    widget.options = params
        .get(k::OPTIONS)
        .and_then(toml::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(toml::Value::as_str)
                .map(smol_str::SmolStr::new)
                .collect()
        })
        .unwrap_or_default();
    widget.columns = f(k::COLUMNS).max(0.0) as u32;
    widget.open = b(k::OPEN);
    widget.inset = crate::widget_theme::four_of(params.get(k::INSET));
    widget.slice = crate::widget_theme::four_of(params.get(k::SLICE));
    widget.deadzone = f(k::DEADZONE);
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
