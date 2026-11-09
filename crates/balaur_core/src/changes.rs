//! What the writes of a frame changed, for whoever listens (PLAN-events §5).
//!
//! The one way a component changes, its registry's write, records the node,
//! the component and the properties it was asked for. Adding, freeing,
//! renaming and moving a node record themselves too, and announce their
//! tree event from the same call, so the log and the events are one source.
//! Nothing is kept while nobody listens: the pump says whether anyone hears
//! [`crate::events::CHANGED`], and hands the log over as that one event a
//! frame later.
//!
//! The same calls keep the changes under one subtree as they happen, each
//! with what it replaced, for whoever takes them (`scene.keep_changes`): an
//! editor's undo, which cannot wait a frame for its step and needs every
//! write rather than a node's merged (PLAN-editor-as-scene §10.2).

use std::cell::RefCell;
use std::rc::Rc;

use balaur_script::Value;
use smol_str::SmolStr;

use crate::collections::DetHashMap;
use crate::components::ticks::Ticks;
use crate::engine::Engine;
use crate::hecs::Entity;
use crate::node_api::{CHILD_ADDED_EVENT, CHILD_REMOVED_EVENT, RENAMED_EVENT, REPARENTED_EVENT};
use crate::vocabulary::keys;
use crate::vocabulary::words;

const ADDED: &str = "added";
const ATTACHED: &str = "attached";
const FREED: &str = "freed";
const KEYS: &str = "keys";
const MOVED: &str = "moved";
const NODE: &str = "node";
const PLACE: &str = "place";
const PROPERTIES: &str = "properties";
const REMOVED: &str = "removed";
const RENAMED: &str = "renamed";
const WAS: &str = "was";

/// The properties a write named; `None` for the whole component.
type Keys = Option<Vec<SmolStr>>;

/// One write's mark.
enum Change {
    Component {
        node: Entity,
        index: usize,
        keys: Keys,
    },
    Removed {
        node: Entity,
        index: usize,
    },
    Attached {
        node: Entity,
        index: usize,
    },
    Added {
        node: Entity,
        parent: Entity,
    },
    Freed {
        node: Entity,
        parent: Entity,
    },
    Renamed {
        node: Entity,
        was: String,
    },
    Moved {
        node: Entity,
        parent: Entity,
    },
    Script {
        node: Entity,
    },
    Keys {
        node: Entity,
    },
}

/// The log, whether anyone hears it, and the changes kept under one subtree.
#[derive(Default)]
pub struct Changes {
    listening: bool,
    entries: Vec<Change>,
    /// The subtree whose changes are kept as they happen.
    keeping: Option<Entity>,
    kept: Vec<Value>,
    /// Nodes kept as freed in a batch not yet gone, so the next one's place
    /// among its siblings is counted without them: restored last first, each
    /// lands where it was.
    leaving: Vec<Entity>,
}

fn push(eng: &Engine, change: impl FnOnce() -> Change) {
    let Some(changes) = eng.try_resource::<Changes>() else {
        return;
    };
    let mut changes = changes.borrow_mut();
    if changes.listening {
        changes.entries.push(change());
    }
}

/// Keep a change to `node` when it is under the kept subtree: `entry` is
/// built only then, from the node as it still stands.
fn keep(eng: &Engine, node: Entity, kind: &str, entry: impl FnOnce() -> Vec<(String, Value)>) {
    let Some(root) = eng
        .try_resource::<Changes>()
        .and_then(|changes| changes.borrow().keeping)
    else {
        return;
    };
    if !crate::scene::is_within(&eng.world(), node, root) {
        return;
    }
    let mut kept = vec![
        (
            words::datatypes::NODE.to_string(),
            Value::Node(crate::node_id_of(node).0),
        ),
        (words::KIND.to_string(), Value::text(kind)),
        (
            words::locales::ID.to_string(),
            Value::Str(stable_id(eng, node)),
        ),
    ];
    kept.extend(entry());
    if let Some(changes) = eng.try_resource::<Changes>() {
        changes.borrow_mut().kept.push(Value::Map(kept));
    }
}

fn stable_id(eng: &Engine, node: Entity) -> String {
    crate::ids::of(&eng.world(), node).unwrap_or_default()
}

fn component_name(eng: &Engine, index: usize) -> Value {
    let name = eng
        .try_resource::<crate::components::ComponentRegistry>()
        .and_then(|registry| {
            registry
                .borrow()
                .at(index)
                .map(|(name, _)| name.to_string())
        });
    Value::Str(name.unwrap_or_default())
}

/// What component `index` held before a write, as a script value.
fn was_value(was: Option<toml::Value>) -> Value {
    was.and_then(|held| crate::node_api::from_toml(&held).ok())
        .unwrap_or(Value::Nil)
}

/// Whether a change to `node` is kept, so a write reads what it replaces
/// first; nothing else pays for that read.
pub(crate) fn keeps(eng: &Engine, node: Entity) -> bool {
    Feeds::of(eng).keeps(eng, node)
}

/// A place among siblings as a script integer.
fn place_value(place: usize) -> i64 {
    i64::try_from(place).unwrap_or(i64::MAX)
}

/// Where `node` sits among its parent's children, the ones a batch is freeing
/// before it not counted.
fn place_of(eng: &Engine, node: Entity, leaving: &[Entity]) -> i64 {
    let world = eng.world();
    let Ok(parent) = world.get::<&crate::scene::Parent>(node).map(|p| p.0) else {
        return 0;
    };
    world
        .get::<&crate::scene::Children>(parent)
        .ok()
        .and_then(|children| {
            children
                .0
                .iter()
                .filter(|child| !leaving.contains(child))
                .position(|&child| child == node)
        })
        .map_or(0, place_value)
}

/// Component `index` of `node` was written: the keys of `asked`, or all of
/// it, over `was`, what it held before.
pub(crate) fn component(
    eng: &Engine,
    node: Entity,
    index: usize,
    asked: Option<&toml::Value>,
    was: Option<toml::Value>,
) {
    crate::components::ticks::changed(eng, node, index);
    let keys: Keys = asked
        .and_then(toml::Value::as_table)
        .map(|table| table.keys().map(SmolStr::new).collect());
    keep(eng, node, keys::COMPONENT, || {
        vec![
            (keys::COMPONENT.into(), component_name(eng, index)),
            (PROPERTIES.into(), properties(keys.as_deref())),
            (WAS.into(), was_value(was)),
        ]
    });
    push(eng, || Change::Component { node, index, keys });
}

/// What a write of one property feeds: the log and the ticks, found once by a
/// caller that writes again and again.
pub(crate) struct Feeds {
    changes: Option<Rc<RefCell<Changes>>>,
    ticks: Option<Rc<RefCell<Ticks>>>,
}

impl Feeds {
    pub(crate) fn of(eng: &Engine) -> Self {
        Self {
            changes: eng.try_resource::<Changes>(),
            ticks: eng.try_resource::<Ticks>(),
        }
    }

    /// Whether a change to `node` is kept, so a write reads what it replaces
    /// first; nothing else pays for that read.
    pub(crate) fn keeps(&self, eng: &Engine, node: Entity) -> bool {
        self.changes
            .as_ref()
            .and_then(|changes| changes.borrow().keeping)
            .is_some_and(|root| crate::scene::is_within(&eng.world(), node, root))
    }

    /// One property of component `index` of `node` was written over `was`,
    /// what the component held before.
    pub(crate) fn property(
        &self,
        eng: &Engine,
        node: Entity,
        index: usize,
        key: &SmolStr,
        was: Option<toml::Value>,
    ) {
        if let Some(ticks) = &self.ticks {
            ticks.borrow_mut().changed(eng.change_tick(), node, index);
        }
        let Some(changes) = &self.changes else {
            return;
        };
        let (keeping, listening) = {
            let changes = changes.borrow();
            (changes.keeping.is_some(), changes.listening)
        };
        if keeping {
            keep(eng, node, keys::COMPONENT, || {
                vec![
                    (keys::COMPONENT.into(), component_name(eng, index)),
                    (
                        PROPERTIES.into(),
                        Value::List(vec![Value::text(key.as_str())]),
                    ),
                    (WAS.into(), was_value(was)),
                ]
            });
        }
        if listening {
            changes.borrow_mut().entries.push(Change::Component {
                node,
                index,
                keys: Some(vec![key.clone()]),
            });
        }
    }
}

/// Component `index` went onto `node`, which had none.
pub(crate) fn attached(eng: &Engine, node: Entity, index: usize) {
    crate::components::ticks::added(eng, node, index);
    keep(eng, node, ATTACHED, || {
        vec![(keys::COMPONENT.into(), component_name(eng, index))]
    });
    push(eng, || Change::Attached { node, index });
}

/// Component `index` came off `node`, holding `was`.
pub(crate) fn removed(eng: &Engine, node: Entity, index: usize, was: Option<toml::Value>) {
    crate::components::ticks::removed(eng, node, index);
    keep(eng, node, REMOVED, || {
        vec![
            (keys::COMPONENT.into(), component_name(eng, index)),
            (WAS.into(), was_value(was)),
        ]
    });
    push(eng, || Change::Removed { node, index });
}

/// `child` was added under `parent`, which hears `child_added` now.
pub(crate) fn added(eng: &Engine, parent: Entity, child: Entity) {
    keep(eng, child, ADDED, || {
        vec![(keys::PARENT.into(), Value::Str(stable_id(eng, parent)))]
    });
    push(eng, || Change::Added {
        node: child,
        parent,
    });
    let child = Value::Node(crate::node_id_of(child).0);
    crate::events::announce(eng, parent, CHILD_ADDED_EVENT, child);
}

/// `child` is about to be freed from under `parent`, which hears
/// `child_removed` now, while the child can still be read. A kept change
/// carries the subtree as scene text, to be built again.
pub(crate) fn freed(eng: &Engine, parent: Entity, child: Entity) {
    keep(eng, child, FREED, || {
        let leaving = eng
            .try_resource::<Changes>()
            .map(|changes| changes.borrow().leaving.clone())
            .unwrap_or_default();
        let text = crate::scene_toml::to_toml(eng, child, None)
            .and_then(|document| Ok(toml::to_string(&document)?))
            .map_or_else(
                |why| {
                    tracing::warn!("keeping a freed node: {why:#}");
                    Value::Nil
                },
                Value::Str,
            );
        vec![
            (keys::PARENT.into(), Value::Str(stable_id(eng, parent))),
            (PLACE.into(), Value::Int(place_of(eng, child, &leaving))),
            (WAS.into(), text),
        ]
    });
    if let Some(changes) = eng.try_resource::<Changes>() {
        let mut changes = changes.borrow_mut();
        if changes.keeping.is_some() {
            changes.leaving.push(child);
        }
    }
    push(eng, || Change::Freed {
        node: child,
        parent,
    });
    let child = Value::Node(crate::node_id_of(child).0);
    crate::events::announce(eng, parent, CHILD_REMOVED_EVENT, child);
}

/// `node` was renamed from `was`, and hears `renamed` now.
pub(crate) fn renamed(eng: &Engine, node: Entity, was: String) {
    keep(eng, node, RENAMED, || {
        vec![(WAS.into(), Value::Str(was.clone()))]
    });
    push(eng, || Change::Renamed {
        node,
        was: was.clone(),
    });
    crate::events::announce(eng, node, RENAMED_EVENT, Value::Str(was));
}

/// The script `node` names is about to change from `was`, nil for none.
pub(crate) fn scripted(eng: &Engine, node: Entity) {
    keep(eng, node, words::SCRIPT, || {
        let world = eng.world();
        let was = world
            .get::<&crate::scene::NodeScript>(node)
            .ok()
            .map_or(Value::Nil, |script| {
                Value::Map(vec![
                    (keys::SOURCE.into(), Value::Str(script.path.clone())),
                    (keys::PROPS.into(), Value::Map(script.props.clone())),
                ])
            });
        vec![(WAS.into(), was)]
    });
    push(eng, || Change::Script { node });
}

/// One of `node`'s own keys is about to change: `visible`, `tint`,
/// `z_index`, `z_as_relative`, `process` or `tags`. A kept
/// change carries them all as they were.
pub(crate) fn node_keys(eng: &Engine, node: Entity) {
    keep(eng, node, KEYS, || {
        vec![(WAS.into(), crate::scene_toml::node_keys_value(eng, node))]
    });
    push(eng, || Change::Keys { node });
}

/// Where a node stood before it moved: its parent and its place there.
pub(crate) struct Stood {
    pub parent: Entity,
    pub place: usize,
}

impl Stood {
    /// Where `node` stands now, or `None` at the root.
    pub(crate) fn of(world: &crate::hecs::World, node: Entity) -> Option<Self> {
        let parent = world.get::<&crate::scene::Parent>(node).ok()?.0;
        let place = world
            .get::<&crate::scene::Children>(parent)
            .ok()
            .and_then(|children| children.0.iter().position(|&child| child == node))
            .unwrap_or(0);
        Some(Self { parent, place })
    }
}

fn kept_move(eng: &Engine, node: Entity, parent: Entity, stood: Option<&Stood>) {
    keep(eng, node, MOVED, || {
        let was = stood.map_or(Value::Nil, |stood| {
            Value::Map(vec![
                (
                    keys::PARENT.into(),
                    Value::Str(stable_id(eng, stood.parent)),
                ),
                (PLACE.into(), Value::Int(place_value(stood.place))),
            ])
        });
        vec![
            (keys::PARENT.into(), Value::Str(stable_id(eng, parent))),
            (WAS.into(), was),
        ]
    });
}

/// `node` moved from where it `stood` to `parent`: the old parent hears
/// `child_removed`, the new one `child_added`, and the node `reparented` with
/// the one it left.
pub(crate) fn moved(eng: &Engine, node: Entity, stood: Option<&Stood>, parent: Entity) {
    kept_move(eng, node, parent, stood);
    push(eng, || Change::Moved { node, parent });
    let moved = Value::Node(crate::node_id_of(node).0);
    let left = stood.map_or(Value::Nil, |stood| {
        Value::Node(crate::node_id_of(stood.parent).0)
    });
    if let Some(stood) = stood {
        crate::events::announce(eng, stood.parent, CHILD_REMOVED_EVENT, moved.clone());
    }
    crate::events::announce(eng, parent, CHILD_ADDED_EVENT, moved);
    crate::events::announce(eng, node, REPARENTED_EVENT, left);
}

/// `node` took another place among its siblings: a move that leaves the
/// parent, so no tree event is announced.
pub(crate) fn reordered(eng: &Engine, node: Entity, stood: &Stood) {
    kept_move(eng, node, stood.parent, Some(stood));
    push(eng, || Change::Moved {
        node,
        parent: stood.parent,
    });
}

/// A batch of frees is announced: the places of the next are counted afresh.
pub(crate) fn announced_leaving(eng: &Engine) {
    if let Some(changes) = eng.try_resource::<Changes>() {
        changes.borrow_mut().leaving.clear();
    }
}

/// Keep every change under `root` from now on, each with what it replaced;
/// `None` stops. What was kept before is dropped.
pub fn keep_changes(eng: &Engine, root: Option<Entity>) {
    if let Some(changes) = eng.try_resource::<Changes>() {
        let mut changes = changes.borrow_mut();
        changes.keeping = root;
        changes.kept.clear();
        changes.leaving.clear();
    }
}

/// The changes kept since the last take, oldest first, as `{ node, kind, id,
/// .. }` maps; taking them empties the list.
pub fn take_changes(eng: &Engine) -> Vec<Value> {
    eng.try_resource::<Changes>()
        .map_or_else(Vec::new, |changes| {
            std::mem::take(&mut changes.borrow_mut().kept)
        })
}

/// What the pump says, once for each scope someone hears it from: the log
/// since the last pump as the payload of [`crate::events::CHANGED`], all of
/// it for `None` and the entries under a root for `Some(root)`, one entry for
/// each node and component however often it was written; a scope with nothing
/// in it says nothing. The log is emptied, and the next frame keeps one only
/// while `scopes` is not empty.
pub(crate) fn take(eng: &Engine, scopes: &[Option<Entity>]) -> Vec<(Option<Entity>, Value)> {
    let Some(changes) = eng.try_resource::<Changes>() else {
        return vec![];
    };
    let entries = {
        let mut changes = changes.borrow_mut();
        changes.listening = !scopes.is_empty();
        std::mem::take(&mut changes.entries)
    };
    if entries.is_empty() {
        return vec![];
    }
    let names = crate::components::names(eng);
    let world = eng.world();
    scopes
        .iter()
        .filter_map(|scope| {
            let within: Vec<&Change> = entries
                .iter()
                .filter(|change| scope.is_none_or(|root| under(&world, root, change)))
                .collect();
            (!within.is_empty()).then(|| (*scope, payload(&names, &within)))
        })
        .collect()
}

// Whether `change` happened under `root`, the root included: a freed node by
// the parent it left, which still stands.
fn under(world: &crate::hecs::World, root: Entity, change: &Change) -> bool {
    let node = match change {
        Change::Freed { parent, .. } => *parent,
        Change::Component { node, .. }
        | Change::Removed { node, .. }
        | Change::Attached { node, .. }
        | Change::Added { node, .. }
        | Change::Renamed { node, .. }
        | Change::Moved { node, .. }
        | Change::Script { node }
        | Change::Keys { node } => *node,
    };
    let mut at = Some(node);
    while let Some(entity) = at {
        if entity == root {
            return true;
        }
        at = world.get::<&crate::scene::Parent>(entity).ok().map(|p| p.0);
    }
    false
}

// The entries as the event's payload: a list of `{node, kind, ..}` maps, a
// node's component once with its keys gathered, in first-written order.
fn payload(names: &[String], entries: &[&Change]) -> Value {
    let node = |entity: Entity| Value::Node(crate::node_id_of(entity).0);
    let component = |index: usize| Value::Str(names.get(index).cloned().unwrap_or_default());
    let mut out: Vec<Vec<(String, Value)>> = vec![];
    let mut seen: DetHashMap<(Entity, usize), (usize, Keys)> = DetHashMap::default();
    for change in entries {
        let entry = match change {
            Change::Component {
                node: entity,
                index,
                keys,
            } => {
                if let Some((at, held)) = seen.get_mut(&(*entity, *index)) {
                    match (held.as_mut(), keys) {
                        (Some(held), Some(keys)) => {
                            for key in keys {
                                if !held.contains(key) {
                                    held.push(key.clone());
                                }
                            }
                        }
                        _ => *held = None,
                    }
                    let properties = properties(held.as_deref());
                    out[*at].retain(|(name, _)| name != words::PROPERTIES);
                    out[*at].push((PROPERTIES.into(), properties));
                    continue;
                }
                seen.insert((*entity, *index), (out.len(), keys.clone()));
                vec![
                    (NODE.into(), node(*entity)),
                    (keys::KIND.into(), Value::text(keys::COMPONENT)),
                    (keys::COMPONENT.into(), component(*index)),
                    (PROPERTIES.into(), properties(keys.as_deref())),
                ]
            }
            Change::Removed {
                node: entity,
                index,
            } => vec![
                (NODE.into(), node(*entity)),
                (keys::KIND.into(), Value::text(REMOVED)),
                (keys::COMPONENT.into(), component(*index)),
            ],
            Change::Attached {
                node: entity,
                index,
            } => vec![
                (NODE.into(), node(*entity)),
                (keys::KIND.into(), Value::text(ATTACHED)),
                (keys::COMPONENT.into(), component(*index)),
            ],
            Change::Added {
                node: entity,
                parent,
            } => vec![
                (NODE.into(), node(*entity)),
                (keys::KIND.into(), Value::text(ADDED)),
                (keys::PARENT.into(), node(*parent)),
            ],
            Change::Freed {
                node: entity,
                parent,
            } => vec![
                (NODE.into(), node(*entity)),
                (keys::KIND.into(), Value::text(FREED)),
                (keys::PARENT.into(), node(*parent)),
            ],
            Change::Renamed { node: entity, was } => vec![
                (NODE.into(), node(*entity)),
                (keys::KIND.into(), Value::text(RENAMED)),
                (WAS.into(), Value::Str(was.clone())),
            ],
            Change::Moved {
                node: entity,
                parent,
            } => vec![
                (NODE.into(), node(*entity)),
                (keys::KIND.into(), Value::text(MOVED)),
                (keys::PARENT.into(), node(*parent)),
            ],
            Change::Script { node: entity } => vec![
                (NODE.into(), node(*entity)),
                (keys::KIND.into(), Value::text(words::SCRIPT)),
            ],
            Change::Keys { node: entity } => vec![
                (NODE.into(), node(*entity)),
                (keys::KIND.into(), Value::text(KEYS)),
            ],
        };
        out.push(entry);
    }
    Value::List(out.into_iter().map(Value::Map).collect())
}

fn properties(keys: Option<&[SmolStr]>) -> Value {
    keys.map_or(Value::Nil, |keys| {
        Value::List(keys.iter().map(|key| Value::text(key.as_str())).collect())
    })
}
