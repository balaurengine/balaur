//! The `widget` component: its definition, and the [`Widget`] the layer reads,
//! built from the [`WidgetProperties`] a scene or a script wrote.
//!
//! A write of one property redoes only the field of the [`Widget`] that
//! property fills, where it fills one alone: the editor writes widget
//! properties one at a time, hundreds while it is used. Any other write
//! builds the [`Widget`] whole.

use std::sync::Arc;

use anyhow::{Result, anyhow, bail};
use balaur_core::Engine;
use balaur_core::components::derived::{Component, Fields, Written};
use balaur_core::components::{ApplyFn, ComponentDef, GetFn};
use balaur_core::hecs::Entity;
use balaur_plugin::Registry;

use crate::vocabulary::{keys as k, words as w};
use crate::widget::node::{Bits, EguiOptions, Widget};
use crate::widget::properties::{Edge, WidgetProperties};
use crate::widget::text_look::TextLook;

/// The class tables a widget was written with, in the order they apply:
/// each its class word and the properties it writes while the screen
/// answers to that word.
pub(crate) type ClassTables = Arc<[(&'static str, toml::map::Map<String, toml::Value>)]>;

/// The `widget` key: a [`WidgetProperties`] on the node, and the [`Widget`]
/// built from it beside.
///
/// Beside its properties a scene may write a table of them per screen class
/// (`[narrow]`, `[touch]`, ...), which the struct does not hold: `apply`
/// hands them to the [`Widget`] and `get` reports them back. `clicked` is
/// declared `readonly`: [`crate::widget::input`] writes it every tick and
/// every write clears it, but it is in the schema so that `get`'s output
/// round-trips and the inspector can see it.
pub(crate) fn register_widget_component(reg: &mut Registry<'_>) {
    let derived = ComponentDef::written_by::<WidgetProperties>(Some(written));
    let (whole, table) = (derived.apply, derived.get);
    // Writes `WidgetProperties`, and the `Widget` built from it with the
    // class tables beside.
    reg.register_component(
        WidgetProperties::NAME,
        ComponentDef {
            events: crate::widget::input::EVENTS,
            tags: &[balaur_core::components::tag::UI],
            apply: Box::new(move |eng, entity, params| apply(eng, entity, params, &whole)),
            get: Box::new(move |eng, entity| read(eng, entity, &table)),
            remove: Box::new(remove),
            ..derived
        },
    );
    // A screen class's table, whose own keys `check_class_tables` refuses.
    reg.accept_keys(WidgetProperties::NAME, |key, _| CLASS_KEYS.contains(&key));
}

/// A whole write: the properties through the derived `apply`, and the class
/// tables beside them onto the [`Widget`] it built.
fn apply(eng: &Engine, entity: Entity, params: &toml::Value, whole: &ApplyFn) -> Result<()> {
    check_class_tables(params)?;
    check_shapes(params)?;
    whole(eng, entity, params)?;
    if let Ok(mut widget) = eng.world().get::<&mut Widget>(entity) {
        widget.classes = classes_of(params);
    }
    Ok(())
}

/// The properties as a table, and the class tables with them: they are not
/// properties, so nothing else would put them back, and a scene saved without
/// them would have lost what it was written with.
fn read(eng: &Engine, entity: Entity, table: &GetFn) -> Option<toml::Value> {
    let mut out = table(eng, entity)?;
    let world = eng.world();
    if let (Some(map), Ok(widget)) = (out.as_table_mut(), world.get::<&Widget>(entity))
        && let Some(classes) = &widget.classes
    {
        for (word, written) in classes.iter() {
            map.insert((*word).to_owned(), toml::Value::Table(written.clone()));
        }
    }
    Some(out)
}

#[allow(
    clippy::unnecessary_wraps,
    reason = "the shape a component's `remove` hook is registered as"
)]
fn remove(eng: &Engine, entity: Entity) -> Result<()> {
    crate::widget::arena::widget_changed(entity);
    let _ = eng.remove_one::<WidgetProperties>(entity);
    let _ = eng.remove_one::<Widget>(entity);
    Ok(())
}

/// What a write owes past the properties: the [`Widget`] the layer reads,
/// `clicked` cleared, which only the event pump sets, and the arena told its
/// copy is a frame behind.
fn written(eng: &Engine, entity: Entity, written: Written) -> Result<()> {
    if let Written::Property(index) = written
        && one_property(eng, entity, index)
    {
        return Ok(());
    }
    rebuild(eng, entity)
}

/// Redo the one field of the [`Widget`] property `index` fills, where it
/// fills one alone. `false` leaves the write to a whole rebuild.
fn one_property(eng: &Engine, entity: Entity, index: usize) -> bool {
    let Some(name) = WidgetProperties::NAMES.get(index) else {
        return false;
    };
    let inspector = {
        let world = eng.world();
        let (Ok(asked), Ok(mut widget)) = (
            world.get::<&WidgetProperties>(entity),
            world.get::<&mut Widget>(entity),
        ) else {
            return false;
        };
        if !put_one(&mut widget, &asked, name) {
            return false;
        }
        widget.kind == w::INSPECTOR
    };
    told(eng, entity, inspector);
    true
}

/// Build the [`Widget`] whole from the properties, refusing what the types
/// alone cannot judge: a grid track that does not parse, a `submit_key` that
/// is not a chord.
fn rebuild(eng: &Engine, entity: Entity) -> Result<()> {
    let mut next = {
        let world = eng.world();
        let Ok(asked) = world.get::<&WidgetProperties>(entity) else {
            return Ok(());
        };
        let mut next = widget_of(&asked);
        crate::widget::beside::put_all(&world, entity, &asked, &mut next);
        next.classes = world
            .get::<&Widget>(entity)
            .ok()
            .and_then(|held| held.classes.clone());
        next
    };
    dress(eng, &mut next);
    crate::widget::grid::check(&next.layout)?;
    let inspector = next.kind == w::INSPECTOR;
    eng.insert_one(entity, next)
        .map_err(|_| anyhow!("node is dead"))?;
    told(eng, entity, inspector);
    Ok(())
}

/// Tell the arena that a widget was written, and the inspectors' state where
/// the widget is one.
pub(crate) fn told(eng: &Engine, entity: Entity, inspector: bool) {
    crate::widget::arena::widget_changed(entity);
    if inspector {
        crate::widget::inspector::noticed(eng, entity);
    }
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
            bail!("widget: `gap` takes [across, down], not one number");
        }
    }
    Ok(())
}

/// Refuse what a class table could not write: a key it invents, a row no
/// component the widget's kind reads holds, a value of the wrong shape.
fn check_class_tables(params: &toml::Value) -> Result<()> {
    let kind = params
        .get(k::KIND)
        .and_then(toml::Value::as_str)
        .unwrap_or(w::LABEL);
    for (word, table) in class_tables(params) {
        let Some(table) = table.as_table() else {
            bail!("widget: `{word}` is a screen class and takes a table of properties");
        };
        for (key, value) in table {
            if key == k::KIND {
                bail!("widget: `{word}.{key}` -- a widget cannot change kind with the screen");
            }
            let written = match WidgetProperties::NAMES.iter().position(|name| name == key) {
                Some(index) => WidgetProperties::default().write(index, value),
                None => crate::widget::beside::check_class_row(kind, key, value),
            };
            written.map_err(|why| anyhow!("widget: `{word}.{key}` {why}"))?;
        }
    }
    Ok(())
}

/// The class tables a whole write carries, `None` where it carries none.
fn classes_of(params: &toml::Value) -> Option<ClassTables> {
    let found: Vec<_> = class_tables(params)
        .filter_map(|(word, table)| Some((word, table.as_table()?.clone())))
        .collect();
    (!found.is_empty()).then(|| found.into())
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
fn class_tables(params: &toml::Value) -> impl Iterator<Item = (&'static str, &toml::Value)> {
    CLASS_KEYS.into_iter().filter_map(move |word| {
        let table = params.get(word)?;
        table.as_table().map(|_| (word, table))
    })
}

/// Read the style a widget's `text_style` names into its text look.
pub(crate) fn dress(eng: &Engine, widget: &mut Widget) {
    if widget.text_look.text_style.is_empty() {
        return;
    }
    let style = balaur_text::text_style::of(eng, &widget.text_look.text_style);
    Arc::make_mut(&mut widget.text_look).dress_with(style);
}

/// Read again the style of every widget naming a style file once the asset
/// cache moved, so a style saved in the editor shows on every widget naming
/// it, as a material does on every body naming it. `read_at` is the
/// generation last read under.
pub(crate) fn reread_text_styles(eng: &Engine, read_at: &mut u64) {
    let generation = balaur_core::assets::generation(eng);
    if std::mem::replace(read_at, generation) == generation {
        return;
    }
    let named: Vec<(Entity, smol_str::SmolStr)> = eng
        .world()
        .query::<(Entity, &Widget)>()
        .iter()
        .filter(|(_, widget)| balaur_text::text_style::names_a_file(&widget.text_look.text_style))
        .map(|(entity, widget)| (entity, widget.text_look.text_style.clone()))
        .collect();
    for (entity, reference) in named {
        let style = balaur_text::text_style::of(eng, &reference);
        if let Ok(mut widget) = eng.world().get::<&mut Widget>(entity) {
            Arc::make_mut(&mut widget.text_look).dress_with(style);
        }
        crate::widget::arena::widget_changed(entity);
    }
}

/// The widget a scene wrote, read again for the classes in force: its
/// properties with each class table it carries for them written over.
///
/// `None` where it carries no class table in force, which is almost every
/// widget: the caller keeps the one it has rather than building a second.
pub(crate) fn for_classes(
    eng: &Engine,
    entity: Entity,
    widget: &Widget,
    active: &[&str],
) -> Option<Widget> {
    let tables = widget.classes.as_ref()?;
    let mut merged: Option<WidgetProperties> = None;
    let mut beside: Vec<(&str, &toml::Value)> = Vec::new();
    let world = eng.world();
    let held = || world.get::<&WidgetProperties>(entity).ok();
    // Broad to narrow, so the narrowest class named wins the key.
    for (word, table) in tables.iter() {
        if !active.contains(word) {
            continue;
        }
        for (key, value) in table {
            let Some(index) = WidgetProperties::NAMES.iter().position(|name| name == key) else {
                beside.push((key.as_str(), value));
                continue;
            };
            if merged.is_none() {
                merged = Some(WidgetProperties::clone(&*held()?));
            }
            // A value the property refuses leaves what the widget holds.
            let _ = merged.as_mut()?.write(index, value);
        }
    }
    if merged.is_none() && beside.is_empty() {
        return None;
    }
    let merged = match merged {
        Some(merged) => merged,
        None => WidgetProperties::clone(&*held()?),
    };
    let mut classed = widget_of(&merged);
    crate::widget::beside::put_all(&world, entity, &merged, &mut classed);
    crate::widget::beside::put_class_rows(&world, entity, &merged.kind.0, &beside, &mut classed);
    drop(world);
    classed.classes.clone_from(&widget.classes);
    dress(eng, &mut classed);
    Some(classed)
}

/// A [`Widget`] built from the properties, its text style not yet read.
pub(crate) fn widget_of(asked: &WidgetProperties) -> Widget {
    let mut widget = Widget {
        visible: true,
        ..Widget::default()
    };
    put_every(&mut widget, asked);
    widget
}

/// The egui options of a widget, to write one: a copy where the arena still
/// shares them.
fn egui(widget: &mut Widget) -> &mut EguiOptions {
    Arc::make_mut(&mut widget.egui)
}

/// Which of the four edges a `safe_area` names, in `EDGES` order.
fn edges(named: &[Edge]) -> [bool; 4] {
    let mut out = [false; 4];
    for (slot, edge) in out.iter_mut().zip(w::EDGES) {
        *slot = named.iter().any(|held| held.0 == *edge);
    }
    out
}

/// Whether `contain` names layout containment, then paint containment.
fn containment(named: &[crate::widget::properties::Containment]) -> [bool; 2] {
    [w::contain::LAYOUT, w::contain::PAINT].map(|word| named.iter().any(|held| held.0 == word))
}

/// Redo the fields of `widget` that `names` fill, each a property with a
/// field of its own: the event pump writes a reader's edit onto the
/// properties, and the widget follows them here.
pub(crate) fn follow(widget: &mut Widget, asked: &WidgetProperties, names: &[&str]) {
    for name in names {
        let alone = put_one(widget, asked, name);
        debug_assert!(alone, "`{name}` fills a field of its own");
    }
}

/// Each property's place in the [`Widget`], written once for both ways it is
/// filled: `put_every` builds a widget whole, `put_one` redoes the field one
/// property fills.
///
/// `alone` is a property with a field of its own; `whole` is one read into a
/// part built with others, or checked once the widget is built, which a write
/// of it rebuilds the widget for: `kind` above all, which decides the
/// components beside it the widget reads.
macro_rules! cache_fields {
    (
        $widget:ident, $asked:ident;
        alone { $($key:path => $write:expr;)* }
        whole [$($whole_key:path),* $(,)?] { $($whole:expr;)* }
    ) => {
        fn put_every($widget: &mut Widget, $asked: &WidgetProperties) {
            $($write;)*
            $($whole;)*
        }

        fn put_one($widget: &mut Widget, $asked: &WidgetProperties, name: &str) -> bool {
            match name {
                $($key => $write,)*
                $($whole_key)|* => return false,
                _ => return false,
            }
            true
        }

        /// Every property the table places, alone or whole.
        #[cfg(test)]
        const PLACED: &[&str] = &[$($key,)* $($whole_key,)*];
    };
}

cache_fields! {
    widget, asked;
    alone {
        k::TEXT => widget.text = asked.text.clone();
        k::ENABLED => widget.disabled = !asked.enabled;
        k::ANCHOR => widget.anchor = asked.anchor.0.clone();
        k::X => widget.x = asked.x;
        k::Y => widget.y = asked.y;
        k::WIDTH => widget.width = asked.width;
        k::HEIGHT => widget.height = asked.height;
        k::WIDTH_PERCENT => widget.layout.width_percent = Bits::of(asked.width_percent);
        k::HEIGHT_PERCENT => widget.layout.height_percent = Bits::of(asked.height_percent);
        k::MIN_WIDTH => widget.min_width = asked.min_width;
        k::MIN_HEIGHT => widget.min_height = asked.min_height;
        k::MAX_WIDTH => widget.layout.max_width = Bits::of(asked.max_width);
        k::MAX_HEIGHT => widget.layout.max_height = Bits::of(asked.max_height);
        k::ASPECT_RATIO => widget.layout.aspect_ratio = Bits::of(asked.aspect_ratio);
        k::MARGIN => widget.layout.margin = asked.margin.map(Bits::of);
        k::GROW => widget.grow = asked.grow;
        k::SHRINK => widget.layout.shrink = Bits::of(asked.shrink);
        k::BASIS => widget.layout.basis = Bits::of(asked.basis);
        k::ALIGN_SELF => widget.layout.align_self = asked.align_self.0.clone();
        k::ABSOLUTE => widget.layout.absolute = asked.absolute;
        k::INSET => widget.inset = asked.inset;
        k::WHOLE_WINDOW => widget.whole_window = asked.whole_window;
        k::SAFE_AREA => widget.safe_area = edges(&asked.safe_area);
        k::MOVABLE => egui(widget).movable = asked.movable;
        k::CONSTRAIN => egui(widget).constrain = asked.constrain;
        k::HIDE_NARROWER => widget.hide_narrower = asked.hide_narrower;
        k::HIDE_WIDER => widget.hide_wider = asked.hide_wider;
        k::HIDE_SHORTER => widget.hide_shorter = asked.hide_shorter;
        k::HIDE_TALLER => widget.hide_taller = asked.hide_taller;
        k::PADDING => widget.padding = asked.padding.0;
        k::GAP => widget.gap = asked.gap;
        k::ALIGN_ITEMS => widget.align = asked.align_items.0.clone();
        k::JUSTIFY => widget.justify = asked.justify.0.clone();
        k::ALIGN_CONTENT => widget.layout.align_content = asked.align_content.0.clone();
        k::WRAP_CHILDREN => widget.layout.wrap_children = asked.wrap_children.0.clone();
        k::MIN_LINES => widget.layout.min_lines =
            asked.min_lines.clamp(1, i64::from(u16::MAX)) as u16;
        k::DIRECTION => widget.layout.direction = asked.direction.0.clone();
        k::OVERFLOW => widget.layout.overflow = asked.overflow.0.clone();
        k::BORDER => widget.layout.border = asked.border.map(Bits::of);
        k::BOX_SIZING => widget.layout.box_sizing = asked.box_sizing.0.clone();
        k::SAFE_ALIGN => widget.layout.safe_align = asked.safe_align;
        k::CONTAIN => widget.layout.contain = containment(&asked.contain);
        k::SCROLLBAR_WIDTH => widget.layout.scrollbar_width = Bits::of(asked.scrollbar_width);
        k::AXIS => widget.axis = asked.axis.0.clone();
        k::AUTO_FLOW => widget.layout.auto_flow = asked.auto_flow.0.clone();
        k::HEADER => widget.header = asked.header;
        k::TITLE_BAR => widget.title_bar = asked.title_bar;
        k::ROW_HEIGHT => widget.row_height = asked.row_height;
        k::THEME => widget.theme = asked.theme.clone();
        k::ROLE => widget.role = asked.role.clone();
        k::FILL => widget.fill = asked.fill.clone();
        k::STROKE => widget.stroke = asked.stroke.clone();
        k::TEXT_COLOR => widget.text_color = asked.text_color;
        k::CORNER_RADIUS => widget.radius = asked.corner_radius;
        k::DASHED => widget.dashed = asked.dashed;
        k::FADE_IN => egui(widget).fade_in = asked.fade_in;
        k::ICON => widget.icon = asked.icon.clone();
        k::ICON_COLOR => widget.icon_color = asked.icon_color.clone();
        k::ICON_SIZE => widget.icon_size = asked.icon_size;
        k::IMAGE => widget.source = asked.image.clone();
        k::SLICE => widget.slice = asked.slice;
        k::FONT => widget.font = asked.font.clone();
        k::FONT_SIZE => widget.font_size = asked.font_size;
        k::FONT_WEIGHT => widget.font_weight = asked.font_weight as f32;
        k::FONT_STYLE => widget.font_style = asked.font_style.0.clone();
        k::TEXT_KEY => widget.text_key = asked.text_key.clone();
        k::MARKUP => widget.markup = asked.markup;
        k::TEXT_ALIGN => widget.text_align = asked.text_align.0.clone();
        k::WRAP => widget.wrap = asked.wrap;
        k::TRUNCATE => widget.truncate = asked.truncate;
        k::SHOW_TOOLTIP_WHEN_ELIDED => egui(widget).show_tooltip_when_elided =
            asked.show_tooltip_when_elided;
        k::PREFIX => egui(widget).prefix = asked.prefix.clone();
        k::SUFFIX => widget.suffix = asked.suffix.clone();
        k::TRAILING => widget.trailing = asked.trailing.clone();
        k::TOOLTIP => widget.tooltip = asked.tooltip.clone();
        k::SELECTABLE => widget.selectable = asked.selectable;
        k::ON_CLICK => widget.on_click = asked.on_click.clone();
        k::ON_CHANGE => widget.on_change = asked.on_change.clone();
        k::ON_SUBMIT => widget.on_submit = asked.on_submit.clone();
        k::ON_FOCUS => widget.on_focus = asked.on_focus.clone();
        k::ON_LINK => widget.on_link = asked.on_link.clone();
        k::ON_DOUBLE_CLICK => widget.on_double_click = asked.on_double_click.clone();
        k::ON_BLUR => widget.on_blur = asked.on_blur.clone();
        k::ON_COMMIT => widget.on_commit = asked.on_commit.clone();
        k::ON_CARET => widget.on_caret = asked.on_caret.clone();
        k::ON_ACTIVATE => widget.on_activate = asked.on_activate.clone();
        k::ON_FOLD => widget.on_fold = asked.on_fold.clone();
        k::ON_OPENED => widget.on_opened = asked.on_opened.clone();
        k::ON_CLOSED => widget.on_closed = asked.on_closed.clone();
        k::ON_SCROLLED => widget.on_scrolled = asked.on_scrolled.clone();
        k::ON_CLOSE_REQUEST => widget.on_close_request = asked.on_close_request.clone();
        k::INTERACTIVE => widget.pointer_through = !asked.interactive;
        k::FOCUSABLE => widget.focusable = asked.focusable;
        k::NEIGHBOUR_UP => widget.neighbours[0] = asked.neighbour_up.clone();
        k::NEIGHBOUR_DOWN => widget.neighbours[1] = asked.neighbour_down.clone();
        k::NEIGHBOUR_LEFT => widget.neighbours[2] = asked.neighbour_left.clone();
        k::NEIGHBOUR_RIGHT => widget.neighbours[3] = asked.neighbour_right.clone();
        k::SENSE => egui(widget).sense = asked.sense.0.clone();
        k::CURSOR => widget.cursor = asked.cursor.0.clone();
        k::SHORTCUT => widget.shortcut = asked.shortcut.clone();
        k::WHILE_TYPING => widget.while_typing = asked.while_typing;
        k::TAKES_TYPING => widget.takes_typing = asked.takes_typing;
        k::CONTEXT => widget.context = asked.context.clone();
        k::OPEN => widget.open = asked.open;
        k::CURRENT_PAGE => widget.active = asked.current_page.clone();
        k::KEEP_OPEN => widget.keep_open = asked.keep_open;
        k::CHECKED => widget.checked = asked.checked;
        k::OPTIONS => widget.options.clone_from(&asked.options);
        k::ITEMS => widget.items.clone_from(&asked.items);
        k::SELECTION => widget.selection.clone_from(&asked.selection);
        k::MULTI_SELECT => widget.multi = asked.multi_select;
        k::RADIO_GROUP => widget.radio_group = asked.radio_group.clone();
        k::TOGGLE => widget.toggle = asked.toggle;
        k::INDETERMINATE => egui(widget).indeterminate = asked.indeterminate;
        k::SPLITTER_WIDTH => widget.handle = asked.splitter_width;
        k::REVERSE => widget.reverse = asked.reverse;
    }
    whole [
        k::KIND, k::GRID_COLUMNS, k::GRID_ROWS, k::AUTO_COLUMNS, k::AUTO_ROWS, k::AREAS, k::ROW,
        k::COLUMN, k::TEXT_STYLE, k::TEXT_BACKGROUND, k::LINE_BREAK, k::TRUNCATE_AT, k::MAX_LINES,
        k::TAB_WIDTH,
    ] {
        widget.kind = asked.kind.0.clone();
        widget.layout.grid_columns = asked.grid_columns.clone();
        widget.layout.grid_rows = asked.grid_rows.clone();
        widget.layout.auto_columns = asked.auto_columns.clone();
        widget.layout.auto_rows = asked.auto_rows.clone();
        widget.layout.areas.clone_from(&asked.areas);
        widget.layout.row = asked.row.clone();
        widget.layout.column = asked.column.clone();
        widget.text_look = Arc::new(TextLook::of(asked));
    }
}

/// A value of property `index` other than what `held` holds, for a test
/// writing each property in turn.
#[cfg(test)]
pub(crate) fn changed_value<F: Fields>(held: &F, index: usize) -> toml::Value {
    let value = held.read(index).expect("every property reads");
    let spec = F::specs().swap_remove(index);
    let options = spec
        .get("options")
        .or_else(|| spec.get("of").and_then(|of| of.get("options")))
        .and_then(toml::Value::as_array);
    match (&value, options) {
        (toml::Value::String(word), Some(options)) => options
            .iter()
            .find(|option| option.as_str() != Some(word))
            .cloned()
            .expect("an enum offers two words"),
        (toml::Value::Array(_), Some(options)) => toml::Value::Array(vec![options[1].clone()]),
        (toml::Value::String(_), None) => toml::Value::String("7".into()),
        (toml::Value::Boolean(flag), _) => toml::Value::Boolean(!flag),
        (toml::Value::Integer(number), _) => toml::Value::Integer(number + 3),
        (toml::Value::Float(number), _) => toml::Value::Float(number + 3.0),
        (toml::Value::Array(items), None) if items.is_empty() => {
            let of = spec.get("of").and_then(|of| of.get("type"));
            toml::Value::Array(vec![
                if of.and_then(toml::Value::as_str) == Some("record") {
                    toml::Value::Table(toml::map::Map::new())
                } else {
                    toml::Value::String("7".into())
                },
            ])
        }
        (toml::Value::Array(items), None) => toml::Value::Array(
            items
                .iter()
                .map(|item| toml::Value::Float(item.as_float().unwrap_or(0.0) + 0.25))
                .collect(),
        ),
        (other, _) => panic!("no changed value for {other}"),
    }
}

#[cfg(test)]
mod tests {
    use balaur_core::components::derived::from_table;

    use super::*;
    use crate::widget::theme::Style;

    fn widget(params: &toml::Value) -> Widget {
        widget_of(&from_table(params).expect("the table reads"))
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
        assert_eq!(bare.cursor, "arrow", "an unstated cursor is the schema's");
    }

    /// The shaper is told the role's font, which the theme read out of its
    /// `[fonts]` when the role named one there.
    #[test]
    fn the_shaper_is_told_the_role_s_font() {
        let widget = widget(&toml::toml! { kind = "label" role = "meta" }.into());
        let style = Style {
            font: Some("fonts/JetBrainsMono-Regular.ttf".into()),
            ..Style::default()
        };
        let font = egui::FontId::new(11.0, egui::FontFamily::Proportional);
        let request = crate::widget::text::text_request(
            (&widget, balaur_text::FontProject::Host),
            "editing",
            None,
            &font,
            &style,
        );
        assert_eq!(request.font, "fonts/JetBrainsMono-Regular.ttf");
    }

    /// A widget naming its own font keeps it over the role's.
    #[test]
    fn a_widget_s_own_font_wins() {
        let widget =
            widget(&toml::toml! { kind = "label" font = "fonts/SourceSans3-Semibold.ttf" }.into());
        let style = Style {
            font: Some("fonts/JetBrainsMono-Regular.ttf".into()),
            ..Style::default()
        };
        let font = egui::FontId::new(11.0, egui::FontFamily::Proportional);
        let request = crate::widget::text::text_request(
            (&widget, balaur_text::FontProject::Host),
            "x",
            None,
            &font,
            &style,
        );
        assert_eq!(request.font, "fonts/SourceSans3-Semibold.ttf");
    }

    /// The table places every property once: one it forgot would be left at
    /// the zero of its field whatever a scene says.
    #[test]
    fn every_property_has_one_place() {
        for name in WidgetProperties::NAMES {
            let places = PLACED.iter().filter(|placed| *placed == name).count();
            assert_eq!(places, 1, "`{name}` is placed {places} times");
        }
        assert_eq!(PLACED.len(), WidgetProperties::NAMES.len());
    }

    /// A one-property write fills exactly what a whole build fills: every
    /// property the table puts alone, written to a value of its own, leaves
    /// the widget as building it whole from the same properties does.
    #[test]
    fn one_property_fills_what_a_whole_build_fills() {
        let base = WidgetProperties::default();
        for (index, name) in WidgetProperties::NAMES.iter().enumerate() {
            let mut asked = base.clone();
            let mut widget = widget_of(&asked);
            let changed = changed_value::<WidgetProperties>(&asked, index);
            asked
                .write(index, &changed)
                .expect("the value fits the property");
            if !put_one(&mut widget, &asked, name) {
                continue;
            }
            assert_eq!(
                format!("{widget:?}"),
                format!("{:?}", widget_of(&asked)),
                "`{name}` written alone left the widget other than a whole build"
            );
        }
    }
}
