//! `states`: a name for a look, and the transition between two of them.
//!
//! A state is a table of component properties, so going to one is a patch per
//! component and nothing more. What a designer calls "hover" is a `states`
//! entry rather than a tween a script writes; a script that wants the tween
//! still writes one, and the two coexist.

use anyhow::{Result, anyhow, bail};
use balaur_script::Value;

use crate::App;
use crate::components::ComponentDef;
use crate::hecs::Entity;
use crate::{Engine, hooks};

/// The `states` component: the named looks a node has, and which one it is in.
pub struct States {
    /// Every state, in the order written, each a table of
    /// `<component> = { <property> = value }`.
    pub named: Vec<(String, toml::Table)>,
    /// The state the node is in. Empty means the pose the scene gave it.
    pub current: String,
    /// Seconds a transition takes; zero snaps.
    pub duration: f32,
}

impl States {
    fn table(&self, name: &str) -> Option<&toml::Table> {
        self.named
            .iter()
            .find(|(known, _)| known == name)
            .map(|(_, table)| table)
    }

    /// Every state's name, in order, for a picker and for the digest.
    #[must_use]
    pub fn names(&self) -> Vec<String> {
        self.named.iter().map(|(name, _)| name.clone()).collect()
    }
}

/// Put a node in one of its states.
///
/// The state's table is patched over the components it names, so a state
/// saying only `shape3d = { color = ... }` leaves the shape's size alone.
/// A node already in that state is left alone, so a hook firing every tick
/// costs one comparison.
///
/// # Errors
/// If the node has no `states`, or names a state it does not declare.
pub fn go(eng: &Engine, entity: Entity, name: &str) -> Result<()> {
    let (patches, was) = {
        let world = eng.world();
        let states = world
            .get::<&States>(entity)
            .map_err(|_| anyhow!("this node has no `states`"))?;
        if states.current == name {
            return Ok(());
        }
        let table = states
            .table(name)
            .ok_or_else(|| anyhow!("no state `{name}`; this node has {:?}", states.names()))?;
        let patches: Vec<(String, toml::Value)> = table
            .iter()
            .map(|(component, body)| (component.clone(), body.clone()))
            .collect();
        (patches, states.current.clone())
    };
    for (component, body) in patches {
        let toml::Value::Table(_) = &body else {
            bail!("state `{name}`: `{component}` is a table of properties");
        };
        crate::components::patch(eng, entity, &component, &body)?;
    }
    {
        let world = eng.world();
        if let Ok(mut states) = world.get::<&mut States>(entity) {
            states.current = name.to_string();
        }
    }
    if let Some(host) = eng.script_host() {
        let node = crate::node_id_of(entity);
        if host.has_method(node, hooks::ON_STATE_CHANGED) {
            host.call_on(
                node,
                hooks::ON_STATE_CHANGED,
                &[Value::Str(was), Value::Str(name.to_string())],
            );
        }
    }
    Ok(())
}

fn schema() -> String {
    ComponentDef::schema(&[
        (
            "current",
            r#"{ type = "string", default = "", description = "The state this node is in; empty is the pose the scene gave it" }"#,
        ),
        (
            "duration",
            r#"{ type = "float", default = 0.0, min = 0.0, description = "Seconds a transition takes; zero snaps" }"#,
        ),
    ])
}

/// The `states` component. Every key that is not `current` or `duration` is a
/// state, so the file reads as the thing it is.
pub(crate) fn register_states_component(app: &mut App) {
    app.register_component(
        "states",
        ComponentDef {
            doc: "Named looks this node can be in. Every key beside `current` and `duration` is a state, and each holds a table per component of the properties that state sets: `[nodes.states.hover.shape3d] color = \"#ff8800\"`. `node.go(\"hover\")` patches them over what the node already has, so a state says only what differs.",
            schema: ComponentDef::parse_schema("states", &schema()),
            tags: &["interaction"],
            expects: &[],
            apply: Box::new(|eng, entity, params| {
                let mut named = Vec::new();
                if let Some(table) = params.as_table() {
                    for (key, body) in table {
                        if key == "current" || key == "duration" {
                            continue;
                        }
                        let toml::Value::Table(body) = body else {
                            bail!("state `{key}` is a table of components");
                        };
                        named.push((key.clone(), body.clone()));
                    }
                }
                let current = params
                    .get("current")
                    .and_then(toml::Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let duration = params
                    .get("duration")
                    .and_then(crate::components::as_f64)
                    .unwrap_or(0.0) as f32;
                let next = States {
                    named,
                    // Applied below rather than assumed: the component is set
                    // before the state's own properties are.
                    current: String::new(),
                    duration,
                };
                {
                    let mut world = eng.world_mut();
                    if let Ok(mut states) = world.get::<&mut States>(entity) {
                        *states = next;
                    } else {
                        world
                            .insert_one(entity, next)
                            .map_err(|_| anyhow!("node is dead"))?;
                    }
                }
                if current.is_empty() {
                    return Ok(());
                }
                go(eng, entity, &current)
            }),
            remove: Box::new(|eng, entity| {
                let _ = eng.world_mut().remove_one::<States>(entity);
                Ok(())
            }),
            get: Box::new(|eng, entity| {
                let world = eng.world();
                let states = world.get::<&States>(entity).ok()?;
                let mut map = toml::map::Map::new();
                map.insert(
                    "current".into(),
                    toml::Value::String(states.current.clone()),
                );
                map.insert(
                    "duration".into(),
                    toml::Value::Float(f64::from(states.duration)),
                );
                for (name, body) in &states.named {
                    map.insert(name.clone(), toml::Value::Table(body.clone()));
                }
                Some(toml::Value::Table(map))
            }),
        },
    );
}
