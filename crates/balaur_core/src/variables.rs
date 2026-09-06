//! `[variables]`: the scene's own typed values, and the change event.
//!
//! A variable is simulation state, so it is in the digest and a rollback
//! restores it. What sets one is a script, a binding or the page the game is
//! embedded in; what reads one is any of those plus a binding's `when`.

use std::collections::BTreeMap;

use anyhow::{Result, bail};
use balaur_script::Value;

use crate::Engine;

/// One variable's declared type, which is what a value written into it is
/// coerced to. Spelled the way an `exports()` spec is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VarType {
    Bool,
    Int,
    Float,
    Text,
}

impl VarType {
    fn of(word: &str) -> Option<Self> {
        match word {
            "bool" => Some(Self::Bool),
            "int" => Some(Self::Int),
            "float" => Some(Self::Float),
            "string" | "text" => Some(Self::Text),
            _ => None,
        }
    }

    /// The word a schema row reports, for the editor's picker.
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::Bool => "bool",
            Self::Int => "int",
            Self::Float => "float",
            Self::Text => "string",
        }
    }

    /// Read a value as this type. A number written into a bool is its
    /// truthiness, so a page setting `1` gets what it meant.
    fn coerce(self, value: &Value) -> Value {
        match self {
            Self::Bool => Value::Bool(match value {
                Value::Bool(b) => *b,
                Value::Num(n) => *n != 0.0,
                Value::Str(s) => !s.is_empty(),
                _ => false,
            }),
            Self::Int => Value::Num(as_num(value).trunc()),
            Self::Float => Value::Num(as_num(value)),
            Self::Text => Value::Str(match value {
                Value::Str(s) => s.clone(),
                Value::Bool(b) => b.to_string(),
                Value::Num(n) => format!("{n}"),
                _ => String::new(),
            }),
        }
    }
}

/// A value as a number: what a comparison and an arithmetic action read.
#[must_use]
pub fn as_num(value: &Value) -> f64 {
    match value {
        Value::Num(n) => *n,
        Value::Bool(true) => 1.0,
        Value::Str(s) => s.parse().unwrap_or(0.0),
        _ => 0.0,
    }
}

/// One declared variable.
#[derive(Clone, Debug)]
pub struct Variable {
    pub kind: VarType,
    pub value: Value,
    /// Whether the value survives through `save`.
    pub persist: bool,
}

/// The scene's variable table.
///
/// Ordered, so a digest over it is the same on every machine and a listing is
/// the same in every editor.
#[derive(Default)]
pub struct Variables {
    declared: BTreeMap<String, Variable>,
    /// Names changed this tick, for the dispatch at the end of it.
    pending: Vec<(String, Value)>,
}

impl Variables {
    /// Declare one, replacing what a previous scene declared under that name.
    pub fn declare(&mut self, name: &str, kind: VarType, value: &Value, persist: bool) {
        let value = kind.coerce(value);
        self.declared.insert(
            name.to_string(),
            Variable {
                kind,
                value,
                persist,
            },
        );
    }

    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.declared.get(name).map(|v| &v.value)
    }

    #[must_use]
    pub fn spec(&self, name: &str) -> Option<&Variable> {
        self.declared.get(name)
    }

    /// Every declared name, in order.
    #[must_use]
    pub fn names(&self) -> Vec<String> {
        self.declared.keys().cloned().collect()
    }

    /// Write one. A value equal to what is there is not a change, so a script
    /// writing the same number every tick dispatches nothing.
    ///
    /// # Errors
    /// If nothing declared `name`: a typo would otherwise be a variable
    /// nobody set and no error anywhere.
    pub fn set(&mut self, name: &str, value: &Value) -> Result<()> {
        let Some(entry) = self.declared.get_mut(name) else {
            bail!("no variable `{name}`; a scene declares one under `[variables]`");
        };
        let next = entry.kind.coerce(value);
        if next == entry.value {
            return Ok(());
        }
        entry.value = next.clone();
        self.pending.push((name.to_string(), next));
        Ok(())
    }

    /// Take what changed since this was last called.
    fn drain(&mut self) -> Vec<(String, Value)> {
        std::mem::take(&mut self.pending)
    }

    /// The names and values `save` should carry, in order.
    #[must_use]
    pub fn persisted(&self) -> Vec<(String, Value)> {
        self.declared
            .iter()
            .filter(|(_, v)| v.persist)
            .map(|(name, v)| (name.clone(), v.value.clone()))
            .collect()
    }
}

/// Read a scene's `[variables]` table into the engine.
///
/// Each row is `name = { type = "int", value = 0, persist = false }`, or a
/// bare value whose type is read off it — the short form a small scene wants.
///
/// # Errors
/// If a row names a type nothing knows.
pub fn declare_from_toml(eng: &Engine, table: &toml::Table) -> Result<()> {
    let variables = eng.resource::<Variables>();
    let mut variables = variables.borrow_mut();
    for (name, row) in table {
        let (kind, value, persist) = match row {
            toml::Value::Table(spec) => {
                let word = spec
                    .get("type")
                    .and_then(toml::Value::as_str)
                    .unwrap_or("float");
                let kind = VarType::of(word)
                    .ok_or_else(|| anyhow::anyhow!("variable `{name}`: no type `{word}`"))?;
                let value = spec.get("value").map_or(Value::Num(0.0), from_toml);
                let persist = spec
                    .get("persist")
                    .and_then(toml::Value::as_bool)
                    .unwrap_or(false);
                (kind, value, persist)
            }
            other => (kind_of(other), from_toml(other), false),
        };
        variables.declare(name, kind, &value, persist);
    }
    Ok(())
}

fn kind_of(value: &toml::Value) -> VarType {
    match value {
        toml::Value::Boolean(_) => VarType::Bool,
        toml::Value::Integer(_) => VarType::Int,
        toml::Value::String(_) => VarType::Text,
        _ => VarType::Float,
    }
}

fn from_toml(value: &toml::Value) -> Value {
    match value {
        toml::Value::Boolean(b) => Value::Bool(*b),
        toml::Value::String(s) => Value::Str(s.clone()),
        other => crate::components::as_f64(other).map_or(Value::Nil, Value::Num),
    }
}

/// Tell every node whose script declares it what changed this tick.
///
/// At the end of the tick rather than inside `set`, so a script writing three
/// variables in a row is not re-entered between them.
pub fn dispatch_changes_system(eng: &Engine, _dt: f32) {
    let changed = {
        let variables = eng.resource::<Variables>();
        let mut variables = variables.borrow_mut();
        variables.drain()
    };
    if changed.is_empty() {
        return;
    }
    let Some(host) = eng.script_host() else {
        return;
    };
    let listeners: Vec<crate::hecs::Entity> = {
        let world = eng.world();
        crate::scene::collect_subtree(&world, eng.root())
    };
    for (name, value) in changed {
        let args = [Value::Str(name.clone()), value.clone()];
        for entity in &listeners {
            let node = crate::node_id_of(*entity);
            if host.has_method(node, crate::hooks::ON_VARIABLE_CHANGED) {
                host.call_on(node, crate::hooks::ON_VARIABLE_CHANGED, &args);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_is_coerced_to_the_type_that_was_declared() {
        let mut variables = Variables::default();
        variables.declare("lives", VarType::Int, &Value::Num(3.7), false);
        assert_eq!(variables.get("lives"), Some(&Value::Num(3.0)));
        variables.set("lives", &Value::Str("2.9".into())).unwrap();
        assert_eq!(variables.get("lives"), Some(&Value::Num(2.0)));

        variables.declare("open", VarType::Bool, &Value::Num(1.0), false);
        assert_eq!(variables.get("open"), Some(&Value::Bool(true)));
    }

    /// Writing the value already held is not a change, so a script setting
    /// the same number every tick dispatches nothing.
    #[test]
    fn writing_the_same_value_is_not_a_change() {
        let mut variables = Variables::default();
        variables.declare("score", VarType::Int, &Value::Num(0.0), false);
        variables.set("score", &Value::Num(1.0)).unwrap();
        variables.set("score", &Value::Num(1.0)).unwrap();
        assert_eq!(variables.drain().len(), 1);
    }

    #[test]
    fn writing_a_variable_nothing_declared_says_so() {
        let mut variables = Variables::default();
        let err = variables.set("nope", &Value::Num(1.0)).unwrap_err();
        assert!(err.to_string().contains("no variable `nope`"));
    }

    #[test]
    fn a_scene_table_declares_the_long_form_and_the_short_one() {
        let table: toml::Table = toml::from_str(
            r#"
score = { type = "int", value = 0, persist = true }
name = "ana"
speed = 2.5
"#,
        )
        .unwrap();
        let mut variables = Variables::default();
        for (name, row) in &table {
            let (kind, value, persist) = match row {
                toml::Value::Table(spec) => (
                    VarType::of(spec.get("type").and_then(toml::Value::as_str).unwrap()).unwrap(),
                    spec.get("value").map(from_toml).unwrap(),
                    spec.get("persist").and_then(toml::Value::as_bool).unwrap(),
                ),
                other => (kind_of(other), from_toml(other), false),
            };
            variables.declare(name, kind, &value, persist);
        }
        assert_eq!(variables.spec("score").unwrap().kind, VarType::Int);
        assert!(variables.spec("score").unwrap().persist);
        assert_eq!(variables.spec("name").unwrap().kind, VarType::Text);
        assert_eq!(variables.spec("speed").unwrap().kind, VarType::Float);
        assert_eq!(variables.persisted().len(), 1);
    }
}
