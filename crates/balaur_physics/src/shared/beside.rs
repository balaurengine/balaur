//! Components that sit beside a main one on the same node and hold the rows
//! only some of its kinds read: a convex decomposition's tuning beside a
//! `collider3d`, a rope's length beside a `joint3d`. No property shows on a
//! condition (PLAN-property-types §5), so a row that applies beside one value
//! of another is a component of its own.
//!
//! The main component's build reads its own table with each beside
//! component's written in, its defaults when the node has none, so a node
//! that never names one builds as the main component always did. Writing or
//! removing a beside component builds the main one again.

use balaur_core::Engine;
use balaur_core::components::ComponentDef;
use balaur_core::components::derived::{self, AfterWriteFn, Component, Written};
use balaur_core::hecs::{Entity, World};
use balaur_core::warnings::Warning;
use balaur_plugin::Registry;

use crate::NodeKey;

/// A component beside `Main`, read by the kinds `reads` names.
pub(crate) trait Beside: Component {
    type Main: Component;

    /// Whether a `Main` of this kind reads the rows.
    fn reads(main: &Self::Main) -> bool;
}

/// The beside component `C` as the main one's kind reads it: the node's own
/// where the kind reads one, its defaults otherwise.
pub(crate) fn held<C: Beside + Clone>(world: &World, entity: Entity, main: &C::Main) -> C {
    if C::reads(main) {
        world
            .get::<&C>(entity)
            .map(|held| (*held).clone())
            .unwrap_or_default()
    } else {
        C::default()
    }
}

/// Write `beside`'s rows into `table`, its main component's: over what is
/// there when the main one's kind reads it, else only where a row is
/// missing. Two beside components may share a row's name (a sphere's
/// `radius` and a cloth tube's), so the one the kind reads wins whichever
/// order they are written in.
pub(crate) fn table_in<C: Component>(table: &mut toml::Value, beside: &C, read: bool) {
    let (Some(rows), toml::Value::Table(more)) = (table.as_table_mut(), derived::to_table(beside))
    else {
        return;
    };
    if read {
        rows.extend(more);
    } else {
        for (name, value) in more {
            rows.entry(name).or_insert(value);
        }
    }
}

/// `C` out of a table [`table_in`] wrote: its defaults where `main`'s kind
/// reads none of its rows.
pub(crate) fn from_table<C: Beside>(params: &toml::Value, main: &C::Main) -> anyhow::Result<C> {
    if C::reads(main) {
        derived::from_table(params)
    } else {
        Ok(C::default())
    }
}

/// Register `C` beside its main component, which `expects` names: a write
/// builds the main one again through `rebuild`, its after-write hook, and so
/// does a removal, with `C`'s rows back at their defaults.
///
/// Registered before the main one, so a scene names it to the node first and
/// the main one is built once, with its rows; a node freed takes it off with
/// nothing built again, its components no longer marked by then.
pub(crate) fn register<C: Beside>(
    reg: &mut Registry<'_>,
    expects: &'static [&'static str],
    tags: &'static [&'static str],
    rebuild: AfterWriteFn,
) {
    // `C` on the node; what it builds is the main component's state.
    reg.register_component(
        C::NAME,
        ComponentDef {
            tags,
            expects,
            warnings: Some(Box::new(idle::<C>)),
            remove: Box::new(move |eng, entity| {
                let held = eng.remove_one::<C>(entity).is_ok();
                let main = <C::Main as Component>::NAME;
                if held && balaur_core::components::has(eng, entity, main) {
                    rebuild(eng, entity, Written::Whole)?;
                }
                Ok(())
            }),
            ..ComponentDef::written_by::<C>(Some(rebuild))
        },
    );
}

/// What is off about `C` on a node whose main component's kind reads none of
/// its rows.
fn idle<C: Beside>(eng: &Engine, entity: Entity) -> Vec<Warning> {
    let world = eng.world();
    let Ok(main) = world.get::<&C::Main>(entity) else {
        return Vec::new();
    };
    if C::reads(&main) {
        return Vec::new();
    }
    vec![Warning::whole(format!(
        "this {}'s kind reads none of these rows",
        <C::Main as Component>::NAME
    ))]
}

/// One beside component as a snapshot frame carries it: its rows on every
/// node that holds it, and the restore that puts them back.
pub(crate) struct Carried {
    name: &'static str,
    save: fn(&Engine, &World) -> Vec<(NodeKey, toml::Value)>,
    restore: fn(&Engine, Vec<(NodeKey, toml::Value)>),
}

/// `C` as a frame carries it.
pub(crate) const fn carried<C: Component>() -> Carried {
    Carried {
        name: C::NAME,
        save: crate::asked_rows::<C>,
        restore: restore_rows::<C>,
    }
}

fn restore_rows<C: Component>(eng: &Engine, rows: Vec<(NodeKey, toml::Value)>) {
    let rows = crate::resolved(eng, crate::filled(eng, rows, &[C::NAME]));
    crate::restore_asked::<C>(eng, &rows);
}

/// The rows a frame keeps of every component in `list`, by name.
pub(crate) type Frame = Vec<(String, Vec<(NodeKey, toml::Value)>)>;

/// Every component of `list` as a frame keeps it.
pub(crate) fn save(eng: &Engine, world: &World, list: &[Carried]) -> Frame {
    list.iter()
        .map(|carried| (carried.name.to_owned(), (carried.save)(eng, world)))
        .filter(|(_, rows)| !rows.is_empty())
        .collect()
}

/// Put back every component of `list` as `frame` holds it, and take it off
/// every node the frame leaves out.
pub(crate) fn restore(eng: &Engine, list: &[Carried], mut frame: Frame) {
    for carried in list {
        let rows = frame
            .iter()
            .position(|(name, _)| name == carried.name)
            .map(|at| frame.swap_remove(at).1)
            .unwrap_or_default();
        (carried.restore)(eng, rows);
    }
}
