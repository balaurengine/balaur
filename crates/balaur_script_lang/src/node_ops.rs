// The engine's node operations as a script types them: `balaur_core` documents
// their arguments for Rune and no return type, so the types live here.

use crate::api::Registry;
use balaur_lang::{HostSignature, Ty};

/// What a key names when it is a node operation: `node.get_node`, or
/// `node.transform.translate` for one its component drives.
pub(crate) const PREFIX: &str = "node.";
/// The binding group the engine installs the operations into, docs and all.
const MODULE: &str = "node";

#[derive(Clone, Copy)]
enum T {
    Void,
    Bool,
    Int,
    Float,
    Str,
    Node,
    Vec3,
    Color,
    Strs,
    Nodes,
}

use T::{Bool, Color, Float, Int, Node, Nodes, Str, Strs, Vec3, Void};

/// Each operation a script can call on a node, after the node itself. One
/// that takes or answers what the language has no type for is left out:
/// `set_component`, `patch_component`, `get_component`, `script_field`.
#[rustfmt::skip]
const OPS: &[(&str, &[T], T)] = &[
    ("is_valid", &[], Bool),
    ("name", &[], Str),
    ("set_name", &[Str], Void),
    ("path", &[], Str),
    ("translate", &[Float, Float, Float], Void),
    ("global_position", &[], Vec3),
    ("global_rotation_euler", &[], Vec3),
    ("global_scale", &[], Vec3),
    ("get_node", &[Str], Node),
    ("add_child", &[Str], Node),
    ("parent", &[], Node),
    ("children", &[], Nodes),
    ("set_parent", &[Node], Void),
    ("sibling_index", &[], Int),
    ("set_sibling_index", &[Int], Void),
    ("set_state", &[Str], Void),
    ("state", &[], Str),
    ("remove_component", &[Str], Void),
    ("has_component", &[Str], Bool),
    ("component_names", &[], Strs),
    ("stable_id", &[], Str),
    ("descendants", &[], Nodes),
    ("script_path", &[], Str),
    ("has_method", &[Str], Bool),
    // What the method answers has no one type, so it is dropped.
    ("call", &[Str], Void),
    // A token for `task::wait`.
    ("call_async", &[Str], Int),
    ("emit", &[Str], Void),
    ("attach_script", &[Str], Void),
    ("detach_script", &[], Void),
    ("queue_free", &[], Void),
    ("visible", &[], Bool),
    ("set_visible", &[Bool], Void),
    ("visible_in_tree", &[], Bool),
    ("tint", &[], Color),
    ("set_tint", &[Float, Float, Float, Float], Void),
    ("effective_tint", &[], Color),
    ("material", &[], Str),
    ("set_material", &[Str], Void),
    ("effective_material", &[], Str),
    ("z_index", &[], Int),
    ("set_z_index", &[Int], Void),
    ("effective_z_index", &[], Int),
    ("process", &[], Str),
    ("set_process", &[Str], Void),
    ("ticking", &[], Bool),
    ("interpolate", &[], Bool),
    ("set_interpolate", &[Bool], Void),
    ("reset_interpolation", &[], Void),
    ("tags", &[], Strs),
    ("has_tag", &[Str], Bool),
    ("add_tag", &[Str], Void),
    ("remove_tag", &[Str], Void),
];

fn ty(t: T) -> Ty {
    match t {
        Void => Ty::Void,
        Bool => Ty::Bool,
        Int => Ty::Int,
        Float => Ty::Float,
        Str => Ty::Str,
        Node => Ty::Node,
        Vec3 => Ty::Struct("Vec3".into()),
        Color => Ty::Struct("Color".into()),
        Strs => Ty::Array(Box::new(Ty::Str)),
        Nodes => Ty::Array(Box::new(Ty::Node)),
    }
}

/// The operation a key calls: its last segment.
pub(crate) fn op_name(key: &str) -> Option<&str> {
    key.strip_prefix(PREFIX)?.rsplit('.').next()
}

pub(crate) fn signatures(registry: &Registry) -> Vec<HostSignature> {
    let docs = registry.modules.get(MODULE);
    OPS.iter()
        .map(|(name, params, ret)| HostSignature {
            key: match docs.and_then(|d| d.get(*name)).and_then(|e| e.acts_on) {
                Some(comp) => format!("{PREFIX}{comp}.{name}"),
                None => format!("{PREFIX}{name}"),
            },
            params: std::iter::once(Ty::Node)
                .chain(params.iter().map(|p| ty(*p)))
                .collect(),
            ret: ty(*ret),
            is_async: false,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_row_names_an_operation_the_engine_has() {
        let engine: Vec<&str> = balaur_core::node_api::NODE_OPS
            .iter()
            .map(|o| o.name)
            .collect();
        for (name, _, _) in super::OPS {
            assert!(
                engine.contains(name),
                "the engine has no node operation `{name}`"
            );
        }
    }
}
