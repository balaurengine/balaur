//! `scene_rows`: a `scroll`'s rows as instances of one scene part, built only
//! while they are in view or near it. A script hands the rows their data with
//! `ui::set_rows`, and each row's script hears its item through `show(data)`.
//! A row not built holds the space it last measured, or the rows' mean until
//! one is measured; a row that scrolls away is hidden and taken again for the
//! next row that comes into view, and at most `per_frame` are built a frame.

use balaur_core::Engine;
use balaur_core::collections::DetHashMap;
use balaur_core::components::ComponentDef;
use balaur_core::components::derived::Component;
use balaur_core::hecs::Entity;
use balaur_core::scene::Children;
use balaur_plugin::Registry;
use balaur_script::Value;

use crate::vocabulary::control::WIDGET;
use crate::vocabulary::keys as k;
use crate::vocabulary::words as w;

/// A `scroll`'s rows, each an instance of one scene part, built only while in view or `ahead` rows from it, at most `per_frame` a frame. `ui::set_rows` hands them their items, and each row's script hears its item through `show(data)` when it is bound to one; a row scrolled away is hidden and bound again to the next. Rows are as tall as their part lays them out, the theme's sizes included, and a row not built holds the space it last measured, or the rows' mean
#[derive(Clone, Debug, PartialEq, Component)]
#[component(name = "scene_rows")]
pub(crate) struct SceneRows {
    /// The part each row is an instance of; its root is a widget
    #[property(asset = "scene")]
    pub(crate) scene: String,
    /// Rows kept built past each edge of the view
    #[property(default = 4, min = 0)]
    pub(crate) ahead: i64,
    /// The most rows built in one frame; a row already built and scrolled away is bound again without counting
    #[property(default = 4, min = 1)]
    pub(crate) per_frame: i64,
}

/// The method a row's script hears its item through.
const SHOW: &str = "show";
/// The column a host's rows sit in, as tall as all of them.
const CONTENT: &str = "Rows";
/// A height that moved by less than this is the one it was.
const HAIR: f32 = 0.5;

/// Every host's items and rows, and the items handed to each since its last
/// frame, which that frame takes: a row's `show` may hand its list new ones.
#[derive(Default)]
pub(crate) struct RowsState {
    hosts: DetHashMap<Entity, Host>,
    asked: DetHashMap<Entity, Vec<Value>>,
}

#[derive(Default)]
struct Host {
    items: Vec<Value>,
    /// Each item's row's height as last measured; 0 for one never built.
    heights: Vec<f32>,
    /// The mean height measured before the items were last replaced, which a
    /// row stands at until one of the new ones is measured.
    known_mean: f32,
    /// The row bound to each item in view, and the top it was placed at.
    shown: DetHashMap<usize, (Entity, f32)>,
    /// Rows built and bound to nothing, hidden.
    spare: Vec<Entity>,
    content: Option<Entity>,
    /// The height the content was given.
    total: f32,
    /// Rows built so far, which numbers the next.
    built: usize,
    /// A scene that would not build as a row, not tried again until it changes.
    refused: Option<String>,
}

pub(crate) fn register(reg: &mut Registry<'_>) {
    reg.register_component(
        SceneRows::NAME,
        ComponentDef {
            tags: &[balaur_core::components::tag::UI],
            ..ComponentDef::written_by::<SceneRows>(None)
        },
    );
    reg.insert_resource(RowsState::default());
    reg.add_system(balaur_core::Stage::Update, scene_rows_system);
}

/// Hand `host`'s rows their items, taken at its next frame: each row in view
/// is bound to its item again, and the rows past the last are let go of.
/// False for a node with no `scene_rows`.
pub fn set_rows(eng: &Engine, host: Entity, items: Vec<Value>) -> bool {
    if eng.world().get::<&SceneRows>(host).is_err() {
        return false;
    }
    eng.resource::<RowsState>()
        .borrow_mut()
        .asked
        .insert(host, items);
    true
}

/// The row bound to item `index` of `host`, while it is built.
pub fn row_of(eng: &Engine, host: Entity, index: usize) -> Option<Entity> {
    let state = eng.resource::<RowsState>();
    let state = state.borrow();
    state
        .hosts
        .get(&host)?
        .shown
        .get(&index)
        .map(|(row, _)| *row)
}

fn scene_rows_system(eng: &Engine, _: f32) {
    let hosts: Vec<(Entity, SceneRows)> = {
        let world = eng.world();
        world
            .query::<(Entity, &SceneRows)>()
            .iter()
            // copy: the settings outlive the world's borrow, which rows' scripts take.
            .map(|(entity, rows)| (entity, rows.clone()))
            .collect()
    };
    {
        let state = eng.resource::<RowsState>();
        let world = eng.world();
        let mut state = state.borrow_mut();
        state.hosts.retain(|host, _| world.contains(*host));
        state.asked.retain(|host, _| world.contains(*host));
    }
    for (host, rows) in hosts {
        // Out of the map while scripts run, so a `set_rows` they make is
        // queued for the next frame rather than lost.
        let (held, asked) = {
            let state = eng.resource::<RowsState>();
            let mut state = state.borrow_mut();
            (
                state.hosts.shift_remove(&host),
                state.asked.shift_remove(&host),
            )
        };
        let mut held = held.unwrap_or_default();
        if let Some(items) = asked {
            replace_items(eng, &mut held, items);
        }
        lay(eng, host, &rows, &mut held);
        eng.resource::<RowsState>()
            .borrow_mut()
            .hosts
            .insert(host, held);
    }
}

/// `items` in place of the host's: every row let go of, to be bound again,
/// and the heights forgotten, their mean kept to stand for them.
fn replace_items(eng: &Engine, held: &mut Host, items: Vec<Value>) {
    let mean = mean_height(&held.heights);
    if mean > 0.0 {
        held.known_mean = mean;
    }
    held.heights = vec![0.0; items.len()];
    held.items = items;
    let shown: Vec<Entity> = held.shown.drain(..).map(|(_, (row, _))| row).collect();
    for row in shown {
        balaur_core::scene::set_visible(eng, row, false);
        held.spare.push(row);
    }
}

/// One frame of `host`'s rows: measured, the range in view found, the rows
/// out of it let go of and the ones in it bound, placed and sized.
fn lay(eng: &Engine, host: Entity, rows: &SceneRows, held: &mut Host) {
    let Some(content) = content_of(eng, host, held) else {
        return;
    };
    {
        let world = eng.world();
        held.shown.retain(|_, (row, _)| world.contains(*row));
        held.spare.retain(|row| world.contains(*row));
    }
    measure(held);
    let mean = match mean_height(&held.heights) {
        measured if measured > 0.0 => measured,
        _ => held.known_mean,
    };
    let mut tops = Vec::with_capacity(held.items.len() + 1);
    let mut total = 0.0;
    for &height in &held.heights {
        tops.push(total);
        total += if height > 0.0 { height } else { mean };
    }
    let (view, ahead) = in_view(eng, host, &tops, total, rows, mean);
    // Before a row is measured the column is the view's height, so the first
    // rows are laid out and drawn, and so measured.
    if mean <= 0.0 && !held.items.is_empty() {
        total = crate::widget_rect(host).map_or(1.0, |rect| rect.height());
    }
    let wanted = view.start.saturating_sub(ahead)..(view.end + ahead).min(held.items.len());
    let gone: Vec<usize> = held
        .shown
        .keys()
        .copied()
        .filter(|at| !wanted.contains(at))
        .collect();
    for at in gone {
        if let Some((row, _)) = held.shown.shift_remove(&at) {
            balaur_core::scene::set_visible(eng, row, false);
            held.spare.push(row);
        }
    }
    // The rows in view first, then those past each edge.
    let order = view
        .clone()
        .chain(wanted.start..view.start)
        .chain(view.end..wanted.end);
    bind(eng, content, rows, held, order);
    for (at, (row, placed)) in &mut held.shown {
        let top = tops[*at];
        if (top - *placed).abs() >= HAIR || placed.is_nan() {
            write(
                eng,
                *row,
                k::INSET,
                &toml::Value::Array(
                    [0.0, top, 0.0, -1.0]
                        .map(|side| toml::Value::Float(f64::from(side)))
                        .into(),
                ),
            );
            *placed = top;
        }
    }
    if (total - held.total).abs() >= HAIR {
        write(
            eng,
            content,
            k::HEIGHT,
            &toml::Value::Float(f64::from(total.max(1.0))),
        );
        held.total = total;
    }
}

/// A row for each item of `order` that has none: a spare one, else one built,
/// at most `per_frame` built; each told its item.
fn bind(
    eng: &Engine,
    content: Entity,
    rows: &SceneRows,
    held: &mut Host,
    order: impl Iterator<Item = usize>,
) {
    if held
        .refused
        .as_deref()
        .is_some_and(|scene| scene != rows.scene)
    {
        held.refused = None;
    }
    let mut builds = usize::try_from(rows.per_frame).unwrap_or(1).max(1);
    for at in order {
        if held.shown.contains_key(&at) {
            continue;
        }
        let row = match held.spare.pop() {
            Some(row) => row,
            None if builds > 0 && held.refused.is_none() => {
                builds -= 1;
                let Some(row) = build(eng, content, &rows.scene, held) else {
                    held.refused = Some(rows.scene.clone());
                    break;
                };
                row
            }
            None => break,
        };
        balaur_core::scene::set_visible(eng, row, true);
        held.shown.insert(at, (row, f32::NAN));
        if let Some(scripts) = eng.script_host() {
            scripts.call_on(
                balaur_core::node_id_of(row),
                SHOW,
                std::slice::from_ref(&held.items[at]),
            );
        }
    }
}

/// Each bound row's height as the last pass laid it out.
fn measure(held: &mut Host) {
    for (at, (row, _)) in &held.shown {
        if let Some(rect) = crate::widget_rect(*row)
            && (rect.height() - held.heights[*at]).abs() >= HAIR
        {
            held.heights[*at] = rect.height();
        }
    }
}

/// The mean of the heights measured; 0 before any is.
fn mean_height(heights: &[f32]) -> f32 {
    let measured: Vec<f32> = heights.iter().copied().filter(|h| *h > 0.0).collect();
    if measured.is_empty() {
        return 0.0;
    }
    measured.iter().sum::<f32>() / measured.len() as f32
}

/// The items whose rows the scroll's view shows, each row by its own
/// bottom, and how many to keep past each edge. With nothing measured yet,
/// the first rows, so one is.
fn in_view(
    eng: &Engine,
    host: Entity,
    tops: &[f32],
    total: f32,
    rows: &SceneRows,
    mean: f32,
) -> (std::ops::Range<usize>, usize) {
    let ahead = usize::try_from(rows.ahead).unwrap_or(0);
    if mean <= 0.0 {
        return (0..tops.len().min(ahead.max(1)), 0);
    }
    let offset = eng
        .world()
        .get::<&crate::Widget>(host)
        .map_or(0.0, |widget| widget.egui.scroll_offset[1].max(0.0));
    let high = crate::widget_rect(host).map_or(0.0, |rect| rect.height());
    let bottom = (offset + high).min(total);
    // The row whose span holds the offset: the last to start at or above it.
    let first = tops.partition_point(|top| *top <= offset).saturating_sub(1);
    let last = tops
        .partition_point(|top| *top < bottom)
        .max(first + 1)
        .min(tops.len());
    (first.min(last)..last, ahead)
}

/// The column `host`'s rows sit in, made with the first frame.
fn content_of(eng: &Engine, host: Entity, held: &mut Host) -> Option<Entity> {
    if let Some(content) = held
        .content
        .filter(|content| eng.world().contains(*content))
    {
        return Some(content);
    }
    let content = eng.spawn_node(CONTENT, host);
    let mut column = toml::value::Table::new();
    column.insert(k::KIND.into(), toml::Value::String(w::COLUMN.into()));
    let made =
        balaur_core::components::add(eng, content, WIDGET, Some(&toml::Value::Table(column)));
    if let Err(why) = made {
        tracing::error!("scene_rows: the rows' column: {why:#}");
        return None;
    }
    // A column made again holds none of the rows the last one did.
    held.content = Some(content);
    held.total = 0.0;
    held.shown.clear();
    held.spare.clear();
    Some(content)
}

/// One more row of `scene` under `content`, its root taken out of the
/// column's run to sit where it is placed.
fn build(eng: &Engine, content: Entity, scene: &str, held: &mut Host) -> Option<Entity> {
    held.built += 1;
    let before = balaur_core::scene::child_count(&eng.world(), content);
    let built = balaur_core::project::instantiate_scene_file_as(
        eng,
        scene,
        content,
        format!("{}/{}/", content.to_bits(), held.built),
    );
    let added: Vec<Entity> = eng
        .world()
        .get::<&Children>(content)
        .map(|children| children.0.iter().skip(before).copied().collect())
        .unwrap_or_default();
    let row = match (built, added.as_slice()) {
        (Ok(()), [row]) => *row,
        (built, _) => {
            balaur_core::scene::free_nodes(eng, &added);
            if let Err(why) = built {
                tracing::error!("scene_rows: {scene}: {why:#}");
            } else {
                tracing::error!(
                    "scene_rows: {scene} has {} root nodes; a row is one",
                    added.len()
                );
            }
            return None;
        }
    };
    balaur_core::scene::rename(&eng.world(), row, &format!("row_{}", held.built));
    balaur_core::scene::announce_added_since(eng, content, before);
    write(eng, row, k::ABSOLUTE, &toml::Value::Boolean(true));
    Some(row)
}

fn write(eng: &Engine, entity: Entity, key: &str, value: &toml::Value) {
    if let Err(why) = balaur_core::components::set_property(eng, entity, WIDGET, key, value) {
        tracing::error!("scene_rows: {key}: {why:#}");
    }
}
