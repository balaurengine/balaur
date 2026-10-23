//! The verbs a node answers for itself: drawing, what it reports about
//! itself, and the scene tree's own.

use super::{TREE, node_text};
use crate::godot::gdscript::{INPUT_FLAG, MY};

/// Godot's `_draw` verbs on a node, through the shim, which takes the
/// node's transform into account and draws for this frame. `queue_redraw`
/// asks for nothing: every frame draws.
pub(super) fn draw_verb(receiver: &str, name: &str, args: &[String]) -> Option<String> {
    // Each verb with how many arguments Godot's takes, defaults included:
    // a Rune function takes every one, so a shorter call is padded with nil.
    const VERBS: &[(&str, usize)] = &[
        ("draw_circle", 6),
        ("draw_line", 5),
        ("draw_rect", 5),
        ("draw_arc", 8),
        ("draw_polyline", 4),
        ("draw_multiline", 4),
        ("draw_polygon", 4),
        ("draw_colored_polygon", 4),
        ("draw_texture_rect", 5),
        ("draw_texture_rect_region", 6),
        ("draw_set_transform", 3),
        ("draw_set_transform_matrix", 1),
        ("draw_string", 7),
    ];
    if name == "queue_redraw" {
        return Some(format!("gd::queue_redraw({receiver})"));
    }
    let (_, takes) = VERBS.iter().find(|(verb, _)| *verb == name)?;
    let mut parts: Vec<String> = args.iter().take(*takes).cloned().collect();
    parts.resize(*takes, "nil".to_string());
    Some(format!("gd::{name}({receiver}, {})", parts.join(", ")))
}

/// What a node answers about itself: its hash, its children, its focus and
/// where the canvas put it.
pub(super) fn node_query(receiver: &str, name: &str, args: &[String]) -> Option<String> {
    let node = node_text(receiver);
    let one = args.first().cloned().unwrap_or_default();
    Some(match name {
        // `name.hash()`: the global `hash` on any value, as Godot answers it.
        "hash" if args.is_empty() => format!("gd::hash({receiver})"),
        "get_canvas_transform" | "get_viewport_transform" => {
            format!("gd::canvas_transform({receiver})")
        }
        "get_child_count" => format!("{node}.children().len()"),
        "has_focus" => format!("gd::same(ui::focused_widget(), {receiver})"),
        // Focus is the widget layer's to give.
        "release_focus" => "nil".into(),
        "add_title_bar_control" => format!("gd::add_title_bar_control({receiver}, {one})"),
        // The hook that handed the event over answers `true` for it.
        "accept_event" | "set_input_as_handled" => "gd::set_input_handled()".into(),
        // A per-node switch the hooks the translator writes read first.
        "set_process_input" | "set_process_unhandled_input" | "set_process_unhandled_key_input" => {
            if receiver == "self.node" {
                format!("self.{MY}.{INPUT_FLAG} = {one}")
            } else {
                format!("gd::set_field({receiver}, \"{INPUT_FLAG}\", {one})")
            }
        }
        _ => return None,
    })
}

/// The scene tree's own verbs, which Godot reached through `get_tree()`. The
/// receiver is the tree and carries nothing here.
pub(super) fn tree_verb(receiver: &str, name: &str, args: &[String]) -> Option<String> {
    let one = args.first().cloned().unwrap_or_default();
    Some(match name {
        "get_nodes_in_group" => format!("scene::tagged({one})"),
        "create_timer" => format!("gd::timer({one})"),
        // A path known only as it runs: `load` makes it the converted scene's.
        "change_scene_to_file" | "change_scene_to_packed" => {
            format!("scene::switch(gd::text(gd::load({one})))")
        }
        "reload_current_scene" => "gd::todo(\"reload_current_scene\")".into(),
        "quit" if receiver == TREE => format!(
            "engine::quit({})",
            args.first().cloned().unwrap_or("0".into())
        ),
        "get_root" => "scene::root()".into(),
        "get_first_node_in_group" => format!("gd::front(scene::tagged({one}))"),
        _ => return None,
    })
}
