// What the translator knows of the engine: which `module::fn` names are the
// engine's (from `docs/generated/api.json`), the result types the docs
// spell out, and the node operations `balaur_script_bs` types by hand.

use crate::ty::Ty;
use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub struct Api {
    modules: HashSet<String>,
    results: HashMap<String, Ty>,
    params: HashMap<String, Vec<Ty>>,
    fns: HashSet<String>,
}

/// `balaur_script_bs/src/node_ops.rs`, results only: what a call on a node
/// hands back.
const NODE_OPS: &[(&str, &str)] = &[
    ("is_valid", "bool"),
    ("name", "string"),
    ("path", "string"),
    ("global_position", "vec3"),
    ("global_rotation_euler", "vec3"),
    ("global_scale", "vec3"),
    ("get_node", "node"),
    ("add_child", "node"),
    ("parent", "node"),
    ("children", "[node]"),
    ("sibling_index", "int"),
    ("get_component", "any"),
    ("state", "string"),
    ("has_component", "bool"),
    ("component_names", "[string]"),
    ("stable_id", "string"),
    ("descendants", "[node]"),
    ("script_path", "string"),
    ("has_method", "bool"),
    ("script_field", "any"),
    ("call", "any"),
    ("call_async", "int"),
    ("visible", "bool"),
    ("visible_in_tree", "bool"),
    ("tint", "color"),
    ("effective_tint", "color"),
    ("material", "string"),
    ("effective_material", "string"),
    ("z_index", "int"),
    ("effective_z_index", "int"),
    ("process", "string"),
    ("ticking", "bool"),
    ("interpolate", "bool"),
    ("tags", "[string]"),
    ("has_tag", "bool"),
];

/// The engine's hooks (`docs/generated/api.json`, `hooks`), whose
/// parameters the docs name but do not type: typed here, in order.
const HOOKS: &[(&str, &[&str])] = &[
    ("update", &["float"]),
    ("fixed_update", &["float"]),
    ("load_state", &["any"]),
    ("on_pointer_down", &["int"]),
    ("on_pointer_up", &["int"]),
    ("on_pointer_click", &["int"]),
    ("on_pointer_drag", &["float", "float"]),
    ("on_scroll", &["float", "float"]),
    ("on_pointer_drop", &["node"]),
    ("on_key_down", &["string"]),
    ("on_key_up", &["string"]),
    ("on_action", &["string"]),
    ("on_action_released", &["string"]),
    ("on_resize", &["float", "float"]),
    ("on_widget_event", &["string", "node", "any"]),
    ("on_variable_changed", &["string", "any"]),
    ("on_state_changed", &["string", "string"]),
    ("on_paused_changed", &["bool"]),
    ("on_focused_changed", &["bool"]),
    ("on_dark_mode_changed", &["bool"]),
    ("on_suspended_changed", &["bool"]),
    ("on_safe_area_changed", &["[float]"]),
    ("on_orientation_changed", &["string"]),
    ("on_gamepad_connected", &["int"]),
    ("on_gamepad_disconnected", &["int"]),
    ("on_gamepad_rumble_finished", &["int"]),
    ("on_setting_changed", &["any"]),
    ("on_locale_changed", &["string"]),
];

/// What the engine passes a hook, when `name` is one.
pub(crate) fn hook_params(name: &str) -> Option<Vec<Ty>> {
    HOOKS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, ps)| ps.iter().map(|t| doc_ty(t)).collect())
}

/// Calls the editor makes everywhere whose result the docs leave untyped.
const KNOWN: &[(&str, &str)] = &[
    ("scene::get_node", "node"),
    ("scene::root", "node"),
    ("scene::node_by_id", "node"),
    ("scene::tagged", "[node]"),
    ("engine::unix_time", "float"),
    ("engine::time", "float"),
    ("engine::delta", "float"),
    ("engine::tick", "int"),
    ("engine::dark_mode", "bool"),
    ("engine::target", "string"),
    ("engine::device_id", "string"),
    ("log::info", "void"),
    ("log::warn", "void"),
    ("log::error", "void"),
];

pub(crate) fn doc_ty(t: &str) -> Ty {
    let t = t.trim();
    if let Some(inner) = t.strip_suffix('?') {
        return Ty::opt(doc_ty(inner));
    }
    if let Some(inner) = t.strip_prefix('[').and_then(|t| t.strip_suffix(']')) {
        return Ty::Array(Box::new(doc_ty(inner)));
    }
    match t {
        "string" | "str" => Ty::Str,
        "int" => Ty::Int,
        "float" => Ty::Float,
        "bool" => Ty::Bool,
        "node" => Ty::Node,
        "vec3" => Ty::Named("Vec3"),
        "vec2" => Ty::Named("Vec2"),
        "color" => Ty::Named("Color"),
        "void" | "()" => Ty::Void,
        _ => Ty::Any,
    }
}

// A type as the compiler writes it in `api.json`'s `bs` section: `[str]`,
// `int?`, `Vec3`. A record or a `fn` is an `any` to the translator, which
// reads its fields by name either way.
fn bs_ty(t: &str) -> Ty {
    let t = t.trim();
    if let Some(inner) = t.strip_suffix('?') {
        return Ty::opt(bs_ty(inner));
    }
    if let Some(inner) = t.strip_prefix('[').and_then(|t| t.strip_suffix(']')) {
        return Ty::Array(Box::new(bs_ty(inner)));
    }
    match t {
        "str" => Ty::Str,
        "int" => Ty::Int,
        "float" => Ty::Float,
        "bool" => Ty::Bool,
        "Node" => Ty::Node,
        "Vec3" => Ty::Named("Vec3"),
        "Vec2" => Ty::Named("Vec2"),
        "Color" => Ty::Named("Color"),
        "nothing" => Ty::Void,
        _ => Ty::Any,
    }
}

// The top-level pieces of `a, [b, c], d`.
fn split_top(s: &str) -> Vec<&str> {
    let (mut out, mut depth, mut start) = (vec![], 0i32, 0);
    for (i, c) in s.char_indices() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            ',' if depth == 0 => {
                out.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&s[start..]);
    out.into_iter().filter(|p| !p.trim().is_empty()).collect()
}

impl Api {
    /// Parses `api.json`; an unreadable one is an empty API, and every
    /// `module::fn` is then taken as the engine's. Its `bs` section, what a
    /// `.bs` file compiles against, wins over the docs where both say.
    pub fn from_json(text: &str) -> Self {
        let mut api = Api::default();
        let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
            return api;
        };
        for m in v["modules"].as_array().into_iter().flatten() {
            let Some(name) = m["name"].as_str() else {
                continue;
            };
            api.modules.insert(name.to_string());
            for f in m["functions"].as_array().into_iter().flatten() {
                if let Some(f) = f.as_str() {
                    api.fns.insert(format!("{name}::{f}"));
                }
            }
            let Some(sigs) = m["signatures"].as_object() else {
                continue;
            };
            for (f, sig) in sigs {
                let Some(sig) = sig.as_str() else { continue };
                let key = format!("{name}::{f}");
                if let Some((_, ret)) = sig.rsplit_once("->") {
                    api.results.insert(key.clone(), doc_ty(ret));
                }
                let args = sig.split("->").next().unwrap_or("");
                let args = args.trim().trim_start_matches('(').trim_end_matches(')');
                let params = args
                    .split(',')
                    .filter(|a| !a.trim().is_empty())
                    .map(|a| a.split_once(':').map_or(Ty::Any, |(_, t)| doc_ty(t)))
                    .collect();
                api.params.insert(key, params);
            }
        }
        let compiled = v["bs"]["functions"].as_object();
        for (key, row) in compiled.into_iter().flatten() {
            let Some(sig) = row["signature"].as_str() else {
                continue;
            };
            let (args, ret) = match sig.split_once(") -> ") {
                Some((a, r)) => (a.trim_start_matches('('), bs_ty(r)),
                None => (sig.trim_start_matches('(').trim_end_matches(')'), Ty::Void),
            };
            api.params.insert(
                key.clone(),
                split_top(args).into_iter().map(bs_ty).collect(),
            );
            api.results.insert(key.clone(), ret);
        }
        if compiled.is_none() {
            for (k, t) in KNOWN {
                api.results.insert((*k).to_string(), doc_ty(t));
            }
        }
        api
    }

    /// Whether the docs list this `module::fn`.
    pub(crate) fn knows(&self, path: &str) -> bool {
        self.params.contains_key(path) || self.results.contains_key(path) || self.fns.contains(path)
    }

    /// The type the docs give the `i`th argument, `any` when they give none.
    pub(crate) fn param(&self, path: &str, i: usize) -> Ty {
        self.params
            .get(path)
            .and_then(|p| p.get(i))
            .cloned()
            .unwrap_or(Ty::Any)
    }

    pub(crate) fn result(&self, path: &str) -> Ty {
        self.results.get(path).cloned().unwrap_or(Ty::Any)
    }

    pub(crate) fn node_op(name: &str) -> Option<Ty> {
        NODE_OPS
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, t)| doc_ty(t))
    }
}
