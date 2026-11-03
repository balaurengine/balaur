//! Reading a component's params table and a script's options table, which
//! every physics component and call goes through.

use balaur_core::components::as_f64;
use balaur_script::Value;

/// A property of a component's TOML params table, as f32.
pub(crate) fn f(params: &toml::Value, key: &str, default: f32) -> f32 {
    params
        .get(key)
        .and_then(as_f64)
        .map_or(default, |v| v as f32)
}

pub(crate) fn boolean(params: &toml::Value, key: &str, default: bool) -> bool {
    params
        .get(key)
        .and_then(toml::Value::as_bool)
        .unwrap_or(default)
}

pub(crate) fn text<'a>(params: &'a toml::Value, key: &str, default: &'a str) -> &'a str {
    params
        .get(key)
        .and_then(toml::Value::as_str)
        .unwrap_or(default)
}

/// A trailing options table, as scripts write it (`{ "max": 10.0 }`).
///
/// Every query, joint and character call takes one: they have more parameters
/// than a positional list can carry legibly, and most of them are optional
/// (N9's options-table idiom).
pub(crate) struct Opts<'a>(pub Option<&'a Value>);

impl<'a> Opts<'a> {
    pub(crate) fn get(&self, name: &str) -> Option<&'a Value> {
        match self.0 {
            Some(Value::Map(fields)) => fields.iter().find(|(k, _)| k == name).map(|(_, v)| v),
            _ => None,
        }
    }

    pub(crate) fn boolean(&self, name: &str, default: bool) -> bool {
        match self.get(name) {
            Some(Value::Bool(b)) => *b,
            _ => default,
        }
    }

    pub(crate) fn f32(&self, name: &str, default: f32) -> f32 {
        match self.get(name) {
            Some(Value::Num(n)) => *n as f32,
            Some(Value::Int(i)) => *i as f32,
            _ => default,
        }
    }

    pub(crate) fn text(&self, name: &str) -> Option<&'a str> {
        match self.get(name) {
            Some(Value::Str(s)) => Some(s.as_str()),
            _ => None,
        }
    }

    /// A node argument, as `Value::Node` or as the node id a script holds.
    pub(crate) fn node(&self, name: &str) -> Option<u64> {
        match self.get(name) {
            Some(Value::Node(bits)) => Some(*bits),
            _ => None,
        }
    }

    pub(crate) fn list(&self, name: &str) -> Option<&'a [Value]> {
        match self.get(name) {
            Some(Value::List(items)) => Some(items.as_slice()),
            _ => None,
        }
    }

    /// A vector, or the default when absent.
    pub(crate) fn vec3(&self, name: &str, default: [f32; 3]) -> [f32; 3] {
        match self.get(name) {
            Some(Value::Vec3(v)) => *v,
            _ => default,
        }
    }

    pub(crate) fn vec2(&self, name: &str, default: [f32; 2]) -> [f32; 2] {
        match self.get(name) {
            Some(Value::Vec2(v)) => *v,
            _ => default,
        }
    }
}

/// A `Value::Map` from pairs, which is what every query and character call
/// returns.
pub(crate) fn map<const N: usize>(pairs: [(&str, Value); N]) -> Value {
    Value::Map(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}
