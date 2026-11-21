//! Chords as data on a node: a `shortcuts` table, each row a chord and the
//! name a press of it raises `shortcut_pressed` with, on the node holding it.

use balaur_core::Engine;
use balaur_core::components::ComponentDef;
use balaur_core::components::derived::{Component, Record, Result, Written};
use balaur_core::hecs::{Entity, World};
use balaur_core::scene::{Children, Parent};
use balaur_plugin::Registry;
use smol_str::SmolStr;

/// What a node holding `shortcuts` announces when one of its chords is
/// pressed, with the row's `name`.
pub(crate) const SHORTCUT_PRESSED_EVENT: &str = "shortcut_pressed";

const EVENTS: &[(&str, &str)] = &[(SHORTCUT_PRESSED_EVENT, "the row's `name`")];

/// Chords a node answers: a press of one raises `shortcut_pressed` with its row's `name` on this node, whose script hears it as `on_shortcut_pressed` and whose bindings as `emitted:shortcut_pressed`. A row answers while its node is shown: hidden, or under a widget that is not drawn, as a `viewport` with no size, it is silent. A widget's own `shortcut` is matched first, then the nodes in tree order, a row before a later one with its chord
#[derive(Clone, Debug, PartialEq, Component)]
#[component(name = "shortcuts")]
pub(crate) struct Shortcuts {
    /// The rows, each `{ chord, name, while_typing, takes_typing }`
    pub(crate) rows: Vec<ShortcutRow>,
}

/// One chord and the name its press carries.
#[derive(Clone, Debug, PartialEq, Record)]
pub(crate) struct ShortcutRow {
    /// The chord, spelled as `[input.actions]` spells one: `cmd+shift+KeyS`, `F5`
    pub(crate) chord: SmolStr,
    /// What `shortcut_pressed` carries
    pub(crate) name: SmolStr,
    /// Whether it fires while a text field holds the keyboard. A chord without cmd, ctrl or alt never does unless it is a function key, since that is typing
    #[property(default = true)]
    pub(crate) while_typing: bool,
    /// Whether it takes its key ahead of a field being typed into, a plain key included: a completion list's arrows, Enter and Escape
    pub(crate) takes_typing: bool,
}

pub(crate) fn register(reg: &mut Registry<'_>) {
    reg.register_component(
        Shortcuts::NAME,
        ComponentDef {
            events: EVENTS,
            tags: &[balaur_core::components::tag::UI],
            ..ComponentDef::written_by::<Shortcuts>(Some(written))
        },
    );
}

/// A row whose chord no key reads, or that names nothing, refuses the write.
fn written(eng: &Engine, entity: Entity, _written: Written) -> Result<()> {
    let world = eng.world();
    let Ok(held) = world.get::<&Shortcuts>(entity) else {
        return Ok(());
    };
    for row in &held.rows {
        if crate::chord::chord(&row.chord).is_none() {
            anyhow::bail!(
                "shortcuts: `{}` is not a chord, as `F5` or `cmd+shift+KeyS`",
                row.chord
            );
        }
        if row.name.is_empty() {
            anyhow::bail!("shortcuts: the row for `{}` names nothing", row.chord);
        }
    }
    Ok(())
}

/// [`pressed`] in a pass with no widget, recorded at once: nothing else is.
pub(crate) fn record_alone(eng: &Engine, ctx: &egui::Context) {
    let edits: Vec<_> = pressed(eng, ctx, &rustc_hash::FxHashMap::default(), &[], &[])
        .into_iter()
        .map(|(node, name)| (node, crate::widget::layer::Edit::Shortcut(name)))
        .collect();
    if !edits.is_empty() {
        crate::widget::input::record(eng, &[], edits, None);
    }
}

/// The viewports with no size as last drawn: shown, but what sits in them is not.
pub(crate) fn shut(placed: &[crate::widget::arena::Placed]) -> Vec<usize> {
    (0..placed.len())
        .filter(|&at| {
            placed[at].widget.kind == crate::vocabulary::words::VIEWPORT
                && crate::widget::arena::screen_of(placed, at, crate::widget::arrange::drawn_at)
                    .is_none()
        })
        .collect()
}

/// The rows pressed this pass, each consumed, and the node it raises on.
/// `shown` is the pass's drawn widgets, which a row under a widget needs its
/// nearest one among; `shut` the viewports with no size, which silence it.
pub(crate) fn pressed(
    eng: &Engine,
    ctx: &egui::Context,
    index_of: &rustc_hash::FxHashMap<u64, usize>,
    shown: &[usize],
    shut: &[usize],
) -> Vec<(Entity, String)> {
    let world = eng.world();
    let mut nodes: Vec<(Vec<usize>, Entity)> = world
        .query::<(Entity, &Shortcuts)>()
        .iter()
        .map(|(entity, _)| entity)
        .filter(|&entity| answers(&world, entity, index_of, shown, shut))
        .map(|entity| (tree_place(&world, entity), entity))
        .collect();
    if nodes.is_empty() {
        return Vec::new();
    }
    nodes.sort();
    let typing = ctx.egui_wants_keyboard_input();
    let mut out = Vec::new();
    for (_, entity) in nodes {
        let Ok(held) = world.get::<&Shortcuts>(entity) else {
            continue;
        };
        for row in &held.rows {
            let Some((modifiers, key)) = crate::chord::chord(&row.chord) else {
                continue;
            };
            let allowed = crate::widget::focus::fires_while_typing(
                row.takes_typing,
                row.while_typing,
                modifiers,
                key,
            );
            if typing && !allowed {
                continue;
            }
            if ctx.input_mut(|input| input.consume_key(modifiers, key)) {
                out.push((entity, row.name.to_string()));
            }
        }
    }
    out
}

// Shown, and drawn where a widget is above it: the nearest one, itself
// included, among this pass's, and not a viewport with no size.
fn answers(
    world: &World,
    entity: Entity,
    index_of: &rustc_hash::FxHashMap<u64, usize>,
    shown: &[usize],
    shut: &[usize],
) -> bool {
    let visible = world
        .get::<&balaur_core::GlobalAppearance>(entity)
        .is_ok_and(|a| a.visible);
    if !visible {
        return false;
    }
    let mut at = Some(entity);
    while let Some(node) = at {
        if let Some(index) = index_of.get(&node.to_bits().get()) {
            return shown.contains(index) && !shut.contains(index);
        }
        at = world.get::<&Parent>(node).ok().map(|p| p.0);
    }
    true
}

// Where the node sits in the tree: its place among its siblings at each
// depth from the root, so sorting these sorts nodes in tree order.
fn tree_place(world: &World, entity: Entity) -> Vec<usize> {
    let mut place = Vec::new();
    let mut at = entity;
    while let Ok(parent) = world.get::<&Parent>(at).map(|p| p.0) {
        let index = world
            .get::<&Children>(parent)
            .ok()
            .and_then(|c| c.0.iter().position(|&child| child == at))
            .unwrap_or(0);
        place.push(index);
        at = parent;
    }
    place.reverse();
    place
}
