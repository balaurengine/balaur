//! Scene-tree UI elements: the `widget` component.
//!
//! A node carrying a `widget` component is a piece of game UI (label,
//! button, or panel) anchored to the screen. The component is registered
//! through the standard component registry, so widgets are addable and
//! editable in the editor and show up in the scene tree like any node.
//! Buttons record clicks into the component (`clicked` in `get_component`,
//! reset each frame).

use std::rc::Rc;

use balaur_core::components::ComponentDef;
use balaur_core::hecs::Entity;
use balaur_core::{App, Engine};
use egui::{pos2, vec2, Align2, Color32, Stroke};

use crate::theme::family;
use crate::widget_theme::WidgetTheme;

/// A component colour (`[r, g, b, a]` in 0..=1) as egui's 8-bit one.
fn rgba_color(rgba: [f32; 4]) -> Color32 {
    let channel = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgba_unmultiplied(
        channel(rgba[0]),
        channel(rgba[1]),
        channel(rgba[2]),
        channel(rgba[3]),
    )
}

#[derive(Clone)]
pub struct Widget {
    pub kind: String,
    pub text: String,
    /// Hidden widgets draw nothing and take no clicks, but keep their state.
    pub visible: bool,
    pub anchor: String,
    pub x: f32,
    pub y: f32,
    /// Panel size in design pixels; 0 sizes to content. A minimum on buttons.
    pub width: f32,
    pub height: f32,
    /// Height of the widget's text, in design pixels.
    pub font_size: f32,
    /// The text's colour as `[r, g, b, a]` in 0..=1, the same representation
    /// the `color` component uses.
    pub text_color: [f32; 4],
    /// Method on this node's script, called when the widget is clicked.
    /// Empty means nothing is connected. A name rather than a function value:
    /// scene files cannot hold closures, and a name works on any backend.
    pub on_click: String,
    pub clicked: bool,
    /// Space inside a container's edge, in design pixels.
    pub padding: f32,
    /// Space between a container's children.
    pub gap: f32,
    /// Cross-axis placement of a container's children.
    pub align: String,
    /// Whether focus may land here, for a widget that could take it.
    pub focusable: bool,
    /// Method on this node's script, called when focus arrives.
    pub on_focus: String,
    /// A `widget_theme` reference, or empty to take the one above.
    pub theme: String,
    /// A localization key drawn instead of `text` when it is set.
    pub text_key: String,
}

/// Whether focus can land on this widget.
///
/// Derived rather than declared: focus exists to activate something, so a
/// widget with nothing to activate is never a stop on the way to one. The
/// `focusable` flag can only take a candidate out, never put one in.
fn takes_focus(widget: &Widget) -> bool {
    widget.visible && widget.focusable && (widget.kind == "button" || !widget.on_click.is_empty())
}

/// Whether this kind lays its widget children out rather than ignoring them.
///
/// A `panel` counts: it already draws a frame, and a frame with things in it
/// is what a menu is made of. One with no children behaves exactly as before.
fn lays_out(kind: &str) -> bool {
    matches!(kind, "row" | "column" | "panel")
}

/// Where and whether the widget layer draws. Games leave the default (full
/// window); editors point it at their viewport and enable it during play.
pub struct WidgetLayerConfig {
    pub enabled: bool,
    /// Design-px rect (x, y, w, h); None = whole screen.
    pub rect: Option<[f32; 4]>,
}

impl Default for WidgetLayerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            rect: None,
        }
    }
}

/// The `widget` key, backed by exactly one `Widget` component on the node.
///
/// `clicked` is declared `readonly`: the widget layer writes it every frame
/// (see `settle_clicks`) and `apply` always clears it, but it is in the
/// schema so that `get`'s output round-trips and the inspector can see it.
pub(crate) fn register_widget_component(app: &mut App) {
    app.register_component(
        "widget",
        ComponentDef {
            doc: "A HUD element the widget layer draws every frame: a label, button or panel \
                  anchored to a screen corner or the center, offset in design pixels. A button \
                  records its click in `clicked` and calls the node's `on_click` method.",
            schema: ComponentDef::parse_schema(
                "widget",
                r#"kind = { type = "enum", default = "label", options = ["label", "button", "panel", "row", "column"], description = "The HUD element the widget layer draws" }
text = { type = "string", default = "label", description = "Label or button caption" }
visible = { type = "bool", default = true, description = "Draw the widget; hidden widgets keep their state" }
anchor = { type = "enum", default = "top_left", options = ["top_left", "top_right", "bottom_left", "bottom_right", "center"], description = "Screen corner or center the offset is measured from" }
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
clicked = { type = "bool", default = false, readonly = true, description = "True on the frame the button was clicked" }"#,
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
                let mut map = toml::map::Map::new();
                map.insert("kind".into(), toml::Value::String(widget.kind.clone()));
                map.insert("text".into(), toml::Value::String(widget.text.clone()));
                map.insert("visible".into(), toml::Value::Boolean(widget.visible));
                map.insert("anchor".into(), toml::Value::String(widget.anchor.clone()));
                map.insert("x".into(), toml::Value::Float(f64::from(widget.x)));
                map.insert("y".into(), toml::Value::Float(f64::from(widget.y)));
                map.insert("width".into(), toml::Value::Float(f64::from(widget.width)));
                map.insert("height".into(), toml::Value::Float(f64::from(widget.height)));
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
                map.insert(
                    "focusable".into(),
                    toml::Value::Boolean(widget.focusable),
                );
                map.insert(
                    "on_focus".into(),
                    toml::Value::String(widget.on_focus.clone()),
                );
                map.insert("theme".into(), toml::Value::String(widget.theme.clone()));
                map.insert(
                    "text_key".into(),
                    toml::Value::String(widget.text_key.clone()),
                );
                Some(toml::Value::Table(map))
            }),
        },
    );
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
    Widget {
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
    }
}

/// Which widget the keyboard and the pad are pointing at.
///
/// One per screen, because that is what focus means: the thing an `accept`
/// would activate. Held as a resource rather than on the widget so that
/// moving it is one write, and so a script can ask without walking the tree.
#[derive(Default)]
pub struct UiFocus {
    /// The focused widget, or `None` before anything has taken focus.
    pub focused: Option<Entity>,
    /// Set by `focus_next` and friends and consumed by the next draw, so a
    /// script can move focus outside the pass that will act on it.
    pub pending: Option<Move>,
}

/// What a script or the keyboard asked focus to do.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Move {
    Next,
    Previous,
    /// Activate what is focused, as a click would.
    Accept,
}

/// One widget and the widgets laid out inside it, as an arena so the draw can
/// recurse without holding a borrow of the world.
struct Placed {
    entity: Entity,
    widget: Widget,
    children: Vec<usize>,
}

/// The widget forest, in scene-tree order.
///
/// A widget's parent is its nearest *widget* ancestor, not its parent node:
/// a menu is usually a panel with an empty grouping node or two inside it,
/// and the layout should not care. Tree order is sibling order, which is what
/// makes a row read left to right the way the scene reads top to bottom.
fn forest(eng: &Engine) -> (Vec<Placed>, Vec<usize>) {
    use balaur_core::scene::Children;
    let world = eng.world();
    let mut arena: Vec<Placed> = Vec::new();
    let mut roots: Vec<usize> = Vec::new();
    // (node, the arena index of the widget laying it out, if any)
    let mut stack: Vec<(Entity, Option<usize>)> = vec![(eng.root(), None)];
    while let Some((entity, owner)) = stack.pop() {
        let mut next_owner = owner;
        if let Ok(widget) = world.get::<&Widget>(entity) {
            let index = arena.len();
            let widget = Widget::clone(&widget);
            arena.push(Placed {
                entity,
                widget: widget.clone(),
                children: Vec::new(),
            });
            match owner {
                Some(parent) => arena[parent].children.push(index),
                None => roots.push(index),
            }
            // Only a container adopts what is under it; a label with nodes
            // beneath it leaves them to be anchored on their own.
            next_owner = lays_out(&widget.kind).then_some(index);
        }
        if let Ok(children) = world.get::<&Children>(entity) {
            // Pushed in reverse so the stack pops them in declaration order.
            for child in children.0.iter().rev() {
                stack.push((*child, next_owner));
            }
        }
    }
    (arena, roots)
}

/// What the keyboard asked this frame, if the game has not asked already.
///
/// egui is where the widget layer's input comes from — clicks arrive that way
/// — so keys do too, and no dependency on the input plugin is needed for a
/// menu to work with a keyboard. A gamepad reaches focus through
/// `ui.focus_next()` and friends, wired to actions by whoever assembles the
/// plugins, which is the crate that knows about both.
fn keyboard_move(ctx: &egui::Context) -> Option<Move> {
    use egui::Key;
    ctx.input(|i| {
        let shifted_tab = i.key_pressed(Key::Tab) && i.modifiers.shift;
        if i.key_pressed(Key::ArrowUp) || i.key_pressed(Key::ArrowLeft) || shifted_tab {
            return Some(Move::Previous);
        }
        if i.key_pressed(Key::ArrowDown)
            || i.key_pressed(Key::ArrowRight)
            || i.key_pressed(Key::Tab)
        {
            return Some(Move::Next);
        }
        if i.key_pressed(Key::Enter) || i.key_pressed(Key::Space) {
            return Some(Move::Accept);
        }
        None
    })
}

/// Move focus, or say which widget an `accept` activated.
///
/// Order is the order the widgets are drawn in, which is the order the scene
/// declares them — so focus walks a menu the way the tree reads.
fn advance(eng: &Engine, placed: &[Placed], asked: Option<Move>) -> Option<Entity> {
    let stops: Vec<Entity> = placed
        .iter()
        .filter(|one| takes_focus(&one.widget))
        .map(|one| one.entity)
        .collect();
    let focus = eng.try_resource::<UiFocus>()?;
    let mut focus = focus.borrow_mut();
    // A focused widget that was hidden, freed or made unfocusable is no
    // longer a place focus can be.
    if focus.focused.is_some_and(|e| !stops.contains(&e)) {
        focus.focused = None;
    }
    let asked = focus.pending.take().or(asked)?;
    if stops.is_empty() {
        return None;
    }
    let at = focus
        .focused
        .and_then(|e| stops.iter().position(|s| *s == e));
    match asked {
        Move::Accept => return focus.focused,
        // Wraps, because a menu is a ring: past the last entry is the first.
        Move::Next => {
            let next = at.map_or(0, |i| (i + 1) % stops.len());
            focus.focused = Some(stops[next]);
        }
        Move::Previous => {
            let previous = at.map_or(stops.len() - 1, |i| (i + stops.len() - 1) % stops.len());
            focus.focused = Some(stops[previous]);
        }
    }
    None
}

/// Draw every widget entity. Runs inside the frame's egui pass, after the
/// scripts' `draw_ui`.
pub(crate) fn draw(eng: &Engine, ctx: &egui::Context, scale: f32) {
    let Some(layer) = eng.try_resource::<WidgetLayerConfig>() else {
        return;
    };
    let (enabled, rect) = {
        let layer = layer.borrow();
        (layer.enabled, layer.rect)
    };
    if !enabled {
        return;
    }
    let screen = ctx.viewport_rect();
    let area = match rect {
        Some([x, y, w, h]) => {
            egui::Rect::from_min_size(pos2(x * scale, y * scale), vec2(w * scale, h * scale))
        }
        None => screen,
    };
    let (placed, roots) = forest(eng);
    let was_focused = eng
        .try_resource::<UiFocus>()
        .and_then(|f| f.borrow().focused);
    let accepted = advance(eng, &placed, keyboard_move(ctx));
    let focused = eng
        .try_resource::<UiFocus>()
        .and_then(|f| f.borrow().focused);
    let mut painting = Painting {
        eng,
        arena: &placed,
        scale,
        focused,
        theme: Rc::new(WidgetTheme::default()),
        // An `accept` is a click by another name: same `clicked`, same
        // `on_click`, so it starts the frame's list rather than a second one.
        clicked: accepted.into_iter().collect(),
    };
    for root in roots {
        let widget = &placed[root].widget;
        if !widget.visible {
            continue;
        }
        let ox = widget.x * scale;
        let oy = widget.y * scale;
        let pos = match widget.anchor.as_str() {
            "top_right" => pos2(area.max.x - ox, area.min.y + oy),
            "bottom_left" => pos2(area.min.x + ox, area.max.y - oy),
            "bottom_right" => pos2(area.max.x - ox, area.max.y - oy),
            "center" => pos2(area.center().x + ox, area.center().y + oy),
            _ => pos2(area.min.x + ox, area.min.y + oy),
        };
        let align = match widget.anchor.as_str() {
            "top_right" => Align2::RIGHT_TOP,
            "bottom_left" => Align2::LEFT_BOTTOM,
            "bottom_right" => Align2::RIGHT_BOTTOM,
            "center" => Align2::CENTER_CENTER,
            _ => Align2::LEFT_TOP,
        };
        egui::Area::new(egui::Id::new(("balaur-widget", placed[root].entity)))
            .order(egui::Order::Middle)
            .pivot(align)
            .fixed_pos(pos)
            .show(ctx, |ui| draw_one(ui, &mut painting, root));
    }
    let clicked = std::mem::take(&mut painting.clicked);
    let widgets: Vec<(Entity, Widget)> = placed
        .iter()
        .map(|one| (one.entity, one.widget.clone()))
        .collect();
    settle_clicks(eng, &widgets, &clicked);
    // After the clicks, so a handler that moved focus itself is not undone by
    // this frame's arrival.
    if focused != was_focused {
        announce_focus(eng, &widgets, focused);
    }
}

/// What one draw pass carries down the widget tree.
///
/// A struct rather than six arguments: the recursion is three functions deep
/// and every one of them was growing a parameter per feature.
struct Painting<'a> {
    eng: &'a Engine,
    arena: &'a [Placed],
    scale: f32,
    focused: Option<Entity>,
    /// The theme in force here, inherited unless a widget names its own.
    theme: Rc<WidgetTheme>,
    clicked: Vec<Entity>,
}

/// The theme in force for a widget: its own, or the nearest ancestor's.
///
/// Resolved once per frame per root rather than per widget, because a screen
/// has one look and walking up the tree for every button to find it out would
/// be work with a known answer.
fn theme_of(eng: &Engine, reference: &str, inherited: &Rc<WidgetTheme>) -> Rc<WidgetTheme> {
    if reference.is_empty() {
        return inherited.clone();
    }
    match balaur_core::assets::load_typed::<WidgetTheme>(eng, reference) {
        Ok(theme) => theme,
        Err(err) => {
            // Once per reference: a missing theme is a typo in a scene file,
            // and repeating it sixty times a second buries everything else.
            static WARNED: std::sync::Mutex<Option<std::collections::BTreeSet<String>>> =
                std::sync::Mutex::new(None);
            if let Ok(mut seen) = WARNED.lock() {
                let seen = seen.get_or_insert_with(std::collections::BTreeSet::new);
                if seen.insert(reference.to_string()) {
                    tracing::warn!("widget theme '{reference}': {err:#}");
                }
            }
            inherited.clone()
        }
    }
}

/// Draw one widget and, when it is a container, what is laid out inside it.
///
/// A child's `anchor`, `x` and `y` are ignored: it is placed by its parent,
/// and a menu that moved when you nudged one entry would not be a menu.
fn draw_one(ui: &mut egui::Ui, at: &mut Painting<'_>, index: usize) {
    let placed = &at.arena[index];
    let widget = &placed.widget;
    if !widget.visible {
        return;
    }
    // Restored before returning, so a themed subtree does not leak its look
    // onto whatever the caller draws next.
    let outer = at.theme.clone();
    at.theme = theme_of(at.eng, &widget.theme, &outer);
    draw_themed(ui, at, index);
    at.theme = outer;
}

/// What a widget shows: its key, translated in the locale in force, or its
/// literal text. Resolved every frame, which is why a locale switch shows on
/// the next one without anything having to be told.
fn caption(eng: &Engine, widget: &Widget) -> String {
    if widget.text_key.is_empty() {
        return widget.text.clone();
    }
    balaur_core::strings::tr(eng, &widget.text_key, &[])
}

/// Everything a widget kind draws, with the theme already resolved.
fn draw_themed(ui: &mut egui::Ui, at: &mut Painting<'_>, index: usize) {
    let placed = &at.arena[index];
    let widget = &placed.widget;
    let caption = caption(at.eng, widget);
    let style = at.theme.style(&widget.kind);
    let (scale, focused) = (at.scale, at.focused);
    let color = rgba_color(widget.text_color);
    let font = egui::FontId::new(widget.font_size * scale, family("ui"));
    match widget.kind.as_str() {
        "button" => {
            // Without a theme a button is as round as its text is tall,
            // which is the pill the layer has always drawn.
            let radius = egui::CornerRadius::same(
                (style.radius.unwrap_or(widget.font_size) * scale).min(120.0) as u8,
            );
            let mut button =
                egui::Button::new(egui::RichText::new(&caption).font(font).color(color))
                    .corner_radius(radius)
                    .stroke(Stroke::new(
                        style.stroke_width,
                        style.stroke.unwrap_or(color),
                    ))
                    .min_size(vec2(widget.width, widget.height) * scale);
            if let Some(fill) = style.fill {
                button = button.fill(fill);
            }
            let response = ui.add(button);
            if response.clicked() {
                at.clicked.push(placed.entity);
            }
            if focused == Some(placed.entity) {
                // Drawn rather than egui's own focus ring: the ring follows
                // egui's keyboard focus, and this follows the scene's.
                ui.painter().rect_stroke(
                    response.rect.expand(2.0),
                    radius,
                    Stroke::new(2.0, style.stroke.unwrap_or(color)),
                    egui::StrokeKind::Outside,
                );
            }
        }
        "panel" => {
            let margin = egui::Margin::same(style.padding.map_or(8.0, |p| p * scale) as i8);
            egui::Frame::new()
                .fill(style.fill.unwrap_or(Color32::from_black_alpha(96)))
                .corner_radius(egui::CornerRadius::same(
                    style.radius.map_or(8.0, |r| r * scale) as u8,
                ))
                .stroke(
                    style
                        .stroke
                        .map_or(Stroke::NONE, |c| Stroke::new(style.stroke_width, c)),
                )
                .inner_margin(margin)
                .show(ui, |ui| {
                    // `width`/`height` size the frame, margins included.
                    let min = vec2(widget.width, widget.height) * scale - margin.sum();
                    ui.set_min_size(min.max(egui::Vec2::ZERO));
                    if !caption.is_empty() {
                        ui.label(egui::RichText::new(&caption).font(font).color(color));
                    }
                    // A panel with nothing in it is the panel it always was.
                    lay_out(ui, at, index, Axis::Column);
                });
        }
        "row" => contain(ui, at, index, Axis::Row),
        "column" => contain(ui, at, index, Axis::Column),
        _ => {
            ui.label(egui::RichText::new(&caption).font(font).color(color));
        }
    }
}

/// Which way a container stacks what is inside it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Axis {
    Row,
    Column,
}

/// A bare container: padding, then the children along `axis`.
fn contain(ui: &mut egui::Ui, at: &mut Painting<'_>, index: usize, axis: Axis) {
    let widget = &at.arena[index].widget;
    let scale = at.scale;
    let pad = widget.padding * scale;
    egui::Frame::new()
        .inner_margin(egui::Margin::same(pad as i8))
        .show(ui, |ui| {
            let min = vec2(widget.width, widget.height) * scale;
            ui.set_min_size((min - egui::Vec2::splat(pad * 2.0)).max(egui::Vec2::ZERO));
            lay_out(ui, at, index, axis);
        });
}

/// The children themselves, gapped and aligned.
fn lay_out(ui: &mut egui::Ui, at: &mut Painting<'_>, index: usize, axis: Axis) {
    let placed = &at.arena[index];
    if placed.children.is_empty() {
        return;
    }
    let gap = placed.widget.gap * at.scale;
    let cross = match placed.widget.align.as_str() {
        "center" => egui::Align::Center,
        "end" => egui::Align::Max,
        _ => egui::Align::Min,
    };
    let layout = match axis {
        Axis::Row => egui::Layout::left_to_right(cross),
        Axis::Column => egui::Layout::top_down(cross),
    };
    // Copied out: the closure needs `at` mutably, and `placed` borrows it.
    let children = placed.children.clone();
    ui.with_layout(layout, |ui| {
        ui.spacing_mut().item_spacing = match axis {
            Axis::Row => vec2(gap, ui.spacing().item_spacing.y),
            Axis::Column => vec2(ui.spacing().item_spacing.x, gap),
        };
        for child in &children {
            draw_one(ui, at, *child);
        }
    });
}

/// Tell the newly focused widget's script that focus arrived.
///
/// Only on the change: a handler firing every frame focus merely *stayed*
/// would be a different event, and not a useful one.
fn announce_focus(eng: &Engine, widgets: &[(Entity, Widget)], focused: Option<Entity>) {
    let Some(entity) = focused else { return };
    let Some((_, widget)) = widgets.iter().find(|(e, _)| *e == entity) else {
        return;
    };
    if widget.on_focus.is_empty() {
        return;
    }
    if let Some(host) = eng.script_host() {
        host.call_on(balaur_core::node_id_of(entity), &widget.on_focus, &[]);
    }
}

/// Record this frame's clicks on each widget, then fire their `on_click`.
///
/// Dispatch happens after the world borrow is released: a handler may spawn,
/// free or reparent nodes, and it must not do that mid-iteration.
fn settle_clicks(eng: &Engine, widgets: &[(Entity, Widget)], clicked: &[Entity]) {
    let mut signals: Vec<(Entity, String)> = Vec::new();
    {
        let world = eng.world();
        for (entity, _) in widgets {
            if let Ok(mut w) = world.get::<&mut Widget>(*entity) {
                w.clicked = clicked.contains(entity);
                if w.clicked && !w.on_click.is_empty() {
                    signals.push((*entity, w.on_click.clone()));
                }
            }
        }
    }
    if let Some(host) = eng.script_host() {
        for (entity, method) in signals {
            // No payload: the handler runs on the widget's own node, so
            // `self.node` already is the thing that was clicked.
            host.call_on(balaur_core::node_id_of(entity), &method, &[]);
        }
    }
}
