//! The Godot API as engine calls.
//!
//! Everything here is a rewrite that has been checked against the port's
//! hand-written files. A call in neither this table nor the shim is emitted as
//! it was written and counted in the report, which is how the next rows get
//! chosen.

use super::emit::{quoted, safe};

/// What `get_tree()` becomes. Its verbs are the scene module's, so the
/// receiver is a marker the method table recognises rather than a value.
pub(crate) const TREE: &str = "scene::root()";

/// The local the importer binds `gd.rn` to at the top of a body that needs it.
pub(crate) const SHIM_MARK: &str = "(gd.";

/// A bare call Godot resolved on `self`: `hide()` is `self.hide()`. The node
/// is the receiver, and only names the node table knows are rewritten, so a
/// global this does not have still reports.
pub(crate) fn implicit_self(name: &str, args: &[String]) -> Option<String> {
    const NODE: &[&str] = &[
        "hide",
        "show",
        "set_visible",
        "is_visible",
        "is_visible_in_tree",
        "queue_free",
        "add_child",
        "remove_child",
        "get_parent",
        "get_children",
        "get_node",
        "get_node_or_null",
        "get_instance_id",
        "is_in_group",
        "add_to_group",
        "remove_from_group",
        "has_method",
        "get_meta",
        "set_meta",
        "has_meta",
        "get_index",
        "move_child",
        "find_child",
        "is_node_ready",
        "is_inside_tree",
        "move_to_front",
        "get_process_delta_time",
        "get_physics_process_delta_time",
    ];
    NODE.contains(&name)
        .then(|| method("this.node", name, args))?
}

/// A global function: `str(x)`, `range(n)`, `push_error(m)`.
pub(crate) fn global(name: &str, args: &[String]) -> Option<String> {
    let all = args.join(", ");
    let one = args.first().cloned().unwrap_or_default();
    Some(match name {
        // Variant conversions and queries live in the shim, which dispatches
        // on the value the way Godot's Variant did.
        "str" | "int" | "float" | "bool" => format!("(gd.{name})({all})"),
        "len" => format!("(gd.size)({one})"),
        "is_instance_valid" => format!("(gd.valid)({one})"),
        "typeof" => format!("(gd.type_of)({one})"),
        "is_zero_approx" => format!("(gd.is_zero_approx)({one})"),
        "is_equal_approx" => format!("(gd.is_equal_approx)({all})"),
        "weakref" => one,
        "range" => match args.len() {
            1 => format!("(0..{one})"),
            2 => format!("({}..{})", args[0], args[1]),
            _ => format!("(gd.range)({all})"),
        },
        "print" | "prints" | "printt" | "print_rich" => format!("log::info((gd.str_all)([{all}]))"),
        "push_error" | "printerr" => format!("log::error((gd.str_all)([{all}]))"),
        "push_warning" => format!("log::warn((gd.str_all)([{all}]))"),
        "min" | "minf" | "mini" => format!("math::min({all})"),
        "max" | "maxf" | "maxi" => format!("math::max({all})"),
        "clamp" | "clampf" | "clampi" => format!("math::clamp({all})"),
        "abs" | "absf" | "absi" => format!("math::abs({one})"),
        "floor" | "floorf" | "floori" => format!("math::floor({one})"),
        "ceil" | "ceilf" | "ceili" => format!("math::ceil({one})"),
        "round" | "roundf" | "roundi" => format!("math::round({one})"),
        "sqrt" => format!("math::sqrt({one})"),
        "pow" => format!("math::pow({all})"),
        "sin" | "cos" | "tan" | "asin" | "acos" | "atan" | "exp" | "log" => {
            format!("math::{name}({one})")
        }
        "atan2" => format!("math::atan2({all})"),
        "deg_to_rad" => format!("math::rad({one})"),
        "rad_to_deg" => format!("math::deg({one})"),
        "lerp" | "lerpf" => format!("(gd.lerp)({all})"),
        "sign" | "signf" | "signi" => format!("(gd.sign)({one})"),
        "snapped" | "snappedf" | "snappedi" => format!("(gd.snapped)({all})"),
        "fmod" | "fposmod" => format!("(gd.fmod)({all})"),
        "move_toward" => format!("(gd.move_toward)({all})"),
        "randf" => "rng::random()".into(),
        "randi" => "rng::int()".into(),
        "randf_range" | "randi_range" => format!("rng::range({all})"),
        "randomize" => "()".into(),
        "tr" => format!("strings::tr({all})"),
        "Vector2" | "Vector2i" => format!("(gd.vec2)({all})"),
        "Vector3" => format!("(gd.vec3)({all})"),
        "Color" => format!("(gd.color)({all})"),
        "Callable" => format!("(gd.callable)({all})"),
        "preload" | "load" => format!("(gd.load)({all})"),
        "instance_from_id" => format!("(gd.instance_from_id)({one})"),
        "get_tree" => TREE.into(),
        "get_viewport" => TREE.into(),
        "get_viewport_rect" => "(gd.viewport_rect)()".into(),
        "inverse_lerp" => format!("(gd.inverse_lerp)({all})"),
        "linear_to_db" => format!("(gd.linear_to_db)({one})"),
        "db_to_linear" => format!("(gd.db_to_linear)({one})"),
        "emit_signal" => match args.len() {
            0 => return None,
            1 => format!("this.node.emit({one}, ())"),
            2 => format!("this.node.emit({one}, {})", args[1]),
            _ => format!("this.node.emit({one}, [{}])", args[1..].join(", ")),
        },
        _ => return None,
    })
}

// Several rows share a value without sharing a meaning: `CONNECT_ONE_SHOT` is
// not a mouse button, and merging them would hide what each row is for.
#[allow(clippy::match_same_arms)]
/// A static call on one of Godot's built-in singletons: `Time.get_ticks_msec()`,
/// `OS.has_feature(..)`. Returns `None` for a class this does not carry, which
/// the report then names.
pub(crate) fn static_call(class: &str, name: &str, args: &[String]) -> Option<String> {
    let all = args.join(", ");
    let one = args.first().cloned().unwrap_or_default();
    Some(match (class, name) {
        ("Time", "get_unix_time_from_system") => "engine::unix_time()".into(),
        ("Time", "get_ticks_msec") => "(engine::time() * 1000.0)".into(),
        ("Time", "get_ticks_usec") => "(engine::time() * 1000000.0)".into(),
        ("Time", _) => format!("engine::unix_time() /* Time.{name} */"),
        ("Engine", "get_frames_per_second" | "get_process_frames" | "get_physics_frames") => {
            "engine::tick()".into()
        }
        ("Engine", "is_editor_hint") => "(gd.has_feature)(\"editor\")".into(),
        ("Engine", "is_debug_build") => "false".into(),
        ("Engine", "has_singleton") => "engine::has_plugin({one})".replace("{one}", &one),
        ("OS", "get_unique_id") => "engine::device_id()".into(),
        ("OS", "has_feature") => format!("(gd.has_feature)({one})"),
        ("OS" | "DisplayServer", "get_name") => "(gd.os_name)()".into(),
        ("OS", "get_user_data_dir") => "engine::user_data_dir()".into(),
        ("OS", "shell_open") => format!("engine::open_url({one})"),
        ("OS", "get_locale" | "get_locale_language") => "strings::locale()".into(),
        ("ConfigFile", "new") => "(gd.config)()".into(),
        ("OS", "get_cmdline_args" | "get_cmdline_user_args") => "engine::args()".into(),
        ("JSON", "stringify") => format!("json::encode({one})"),
        ("JSON", "parse_string") => format!("json::parse({one})"),
        ("FileAccess", "file_exists") | ("DirAccess", "dir_exists") => {
            format!("fs::exists({one})")
        }
        ("DirAccess", "make_dir_recursive_absolute" | "make_dir_absolute") => {
            format!("fs::mkdir({one})")
        }
        ("TranslationServer", "translate") => format!("strings::tr({one})"),
        ("TranslationServer", "get_locale") => "strings::locale()".into(),
        ("Vector2", "ZERO") => "(gd.vec2)(0.0, 0.0)".into(),
        ("Vector2", "ONE") => "(gd.vec2)(1.0, 1.0)".into(),
        ("TranslationServer", "set_locale") => format!("strings::set_locale({one})"),
        ("TranslationServer", "get_loaded_locales") => "strings::locales()".into(),
        ("ProjectSettings", "get_setting") => format!("settings::get({all})"),
        ("ProjectSettings", "set_setting") => format!("settings::set({all})"),
        ("Input", "is_action_pressed") => format!("input::is_action_pressed({one})"),
        ("Input", "is_action_just_pressed") => format!("input::is_action_just_pressed({one})"),
        ("Input", "is_key_pressed") => format!("input::is_down({one})"),
        _ => return None,
    })
}

/// A constant on one of Godot's built-in types, read as a value rather than
/// called.
pub(crate) fn static_value(class: &str, name: &str) -> Option<String> {
    Some(match (class, name) {
        ("Vector2" | "Vector2i", "ZERO") => "(gd.vec2)(0.0, 0.0)".into(),
        ("Vector2" | "Vector2i", "ONE") => "(gd.vec2)(1.0, 1.0)".into(),
        ("Vector2" | "Vector2i", "UP") => "(gd.vec2)(0.0, -1.0)".into(),
        ("Vector2" | "Vector2i", "DOWN") => "(gd.vec2)(0.0, 1.0)".into(),
        ("Vector2" | "Vector2i", "LEFT") => "(gd.vec2)(-1.0, 0.0)".into(),
        ("Vector2" | "Vector2i", "RIGHT") => "(gd.vec2)(1.0, 0.0)".into(),
        ("Color", "WHITE") => "(gd.color)(1.0, 1.0, 1.0, 1.0)".into(),
        ("Color", "BLACK") => "(gd.color)(0.0, 0.0, 0.0, 1.0)".into(),
        ("Color", "TRANSPARENT") => "(gd.color)(0.0, 0.0, 0.0, 0.0)".into(),
        _ => return None,
    })
}

/// A bare constructor for one of Godot's value types.
pub(crate) fn value_type(name: &str) -> Option<&'static str> {
    Some(match name {
        "Rect2" | "Rect2i" => "(gd.rect)",
        "PackedStringArray" | "PackedFloat32Array" | "PackedInt32Array" | "PackedVector2Array"
        | "PackedByteArray" | "Array" | "Dictionary" => "(gd.empty_of)",
        _ => return None,
    })
}

/// A name used as a value: Godot's enums and singletons that have a constant
/// counterpart here.
pub(crate) fn constant(name: &str) -> Option<String> {
    Some(match name {
        "PI" => "math::PI".into(),
        "TAU" => "math::TAU".into(),
        "INF" => "math::INF".into(),
        "NAN" => "(gd.nan)()".into(),
        _ => return None,
    })
}

/// `x is Ship`. A project class is a script, which the engine tests by name;
/// the built-in types test by Rune's own kinds.
pub(crate) fn type_test(value: &str, name: &str) -> String {
    match name {
        "int" => format!("{value} is i64"),
        "float" => format!("{value} is f64"),
        "bool" => format!("{value} is bool"),
        "String" | "StringName" => format!("{value} is String"),
        "Array" => format!("{value} is Vec"),
        "Dictionary" => format!("{value} is Object"),
        _ => format!("(gd.is_a)({value}, {})", quoted(name)),
    }
}

/// `x as float`. Rune never mixes ints and floats, so a numeric cast is real
/// work rather than the assertion it was in GDScript.
pub(crate) fn cast(value: &str, name: &str) -> String {
    match name {
        "float" => format!("(gd.float)({value})"),
        "int" => format!("(gd.int)({value})"),
        "bool" => format!("(gd.bool)({value})"),
        "String" | "StringName" => format!("(gd.str)({value})"),
        _ => value.to_string(),
    }
}

/// A property read that is a method call here: `node.visible` is
/// `node.visible()`.
pub(crate) fn property(receiver: &str, field: &str) -> Option<String> {
    // `this.<export>` and every other script field stay fields.
    if receiver == "this" {
        return None;
    }
    Some(match field {
        "visible" => format!("{receiver}.visible()"),
        "global_position" => format!("{receiver}.global_position()"),
        "position" => format!("{receiver}.position()"),
        "scale" => format!("{receiver}.scale()"),
        "modulate" | "self_modulate" => format!("{receiver}.tint()"),
        "z_index" => format!("{receiver}.z_index()"),
        "name" => format!("{receiver}.name()"),
        "rotation_degrees" => format!("{receiver}.rotation_degrees()"),
        "rotation" => format!("math::rad({receiver}.rotation_degrees())"),
        "current_scene" | "root" => "scene::root()".into(),
        "text" | "disabled" | "pressed" | "button_pressed" | "editable" | "selected"
        | "placeholder_text" | "tooltip_text" | "value" | "max_value" | "min_value" | "icon" => {
            let key = if field == "button_pressed" {
                "pressed"
            } else {
                field
            };
            format!("(gd.get)({receiver}.get_component(\"widget\"), \"{key}\", ())")
        }
        _ => return None,
    })
}

/// A Godot global constant with a value here. Its enums are plain integers.
#[allow(clippy::match_same_arms)]
pub(crate) fn global_constant(name: &str) -> Option<&'static str> {
    Some(match name {
        "MOUSE_BUTTON_LEFT" => "1",
        "MOUSE_BUTTON_RIGHT" => "2",
        "MOUSE_BUTTON_MIDDLE" => "3",
        "MOUSE_BUTTON_WHEEL_UP" => "4",
        "MOUSE_BUTTON_WHEEL_DOWN" => "5",
        "CONNECT_ONE_SHOT" => "4",
        "CONNECT_DEFERRED" => "1",
        "HORIZONTAL" => "0",
        "VERTICAL" => "1",
        _ => return None,
    })
}

/// What the importer could not translate, as something that compiles and says
/// so at run time. A file that does not compile stops the whole project, and
/// the port needs one that boots.
pub(crate) fn todo(what: &str) -> String {
    format!("(gd.todo)({})", quoted(what))
}

/// Writing a Godot property: `node.visible = false` is a call here. The
/// vector and colour setters take their components apart, so the value goes
/// through the shim and is evaluated once.
pub(crate) fn setter(receiver: &str, field: &str, value: &str) -> Option<String> {
    const WIDGET: &[&str] = &[
        "text",
        "disabled",
        "pressed",
        "button_pressed",
        "editable",
        "placeholder_text",
        "selected",
        "value",
        "max_value",
        "min_value",
        "tooltip_text",
        "icon",
    ];
    if WIDGET.contains(&field) {
        let key = if field == "button_pressed" {
            "pressed"
        } else {
            field
        };
        return Some(format!(
            "{receiver}.patch_component(\"widget\", #{{ \"{key}\": {value} }})"
        ));
    }
    Some(match field {
        "visible" => format!("{receiver}.set_visible({value})"),
        "position" => format!("(gd.set_position)({receiver}, {value})"),
        "global_position" => format!("(gd.set_global_position)({receiver}, {value})"),
        "scale" => format!("(gd.set_scale)({receiver}, {value})"),
        "modulate" | "self_modulate" => format!("(gd.set_tint)({receiver}, {value})"),
        "z_index" => format!("{receiver}.set_z_index({value})"),
        "name" => format!("{receiver}.set_name({value})"),
        "rotation_degrees" => format!("{receiver}.set_rotation_degrees({value})"),
        "rotation" => format!("{receiver}.set_rotation_degrees(math::deg({value}))"),
        _ => return None,
    })
}

/// A method on a value. The receiver's text is passed so a rewrite can put it
/// where the engine call wants it.
#[allow(clippy::match_same_arms)]
pub(crate) fn method(receiver: &str, name: &str, args: &[String]) -> Option<String> {
    let all = args.join(", ");
    let one = args.first().cloned().unwrap_or_default();
    // A shim verb takes the value Godot called it on as its first argument.
    let with_receiver = |verb: &str| {
        if args.is_empty() {
            format!("(gd.{verb})({receiver})")
        } else {
            format!("(gd.{verb})({receiver}, {all})")
        }
    };
    Some(match name {
        // Collections and strings, through the shim: Godot answered all of
        // these on every Variant and Rune's types do not.
        "size" | "is_empty" | "keys" | "values" | "clear" | "duplicate" | "front" | "back"
        | "pop_front" | "pop_back" | "sort" | "reverse" => with_receiver(name),
        "get" => with_receiver("get"),
        "has" | "has_key" => with_receiver("has"),
        "append" | "push_back" => with_receiver("append"),
        "append_array" => with_receiver("append_array"),
        "erase" => with_receiver("erase"),
        "find" => with_receiver("find"),
        "merge" => with_receiver("merge"),
        "slice" => with_receiver("slice"),
        "to_lower" => with_receiver("to_lower"),
        "to_upper" => with_receiver("to_upper"),
        "strip_edges" => with_receiver("strip_edges"),
        "begins_with" => with_receiver("begins_with"),
        "ends_with" => with_receiver("ends_with"),
        "contains" => with_receiver("contains"),
        "split" => with_receiver("split"),
        "join" => with_receiver("join"),
        "substr" => with_receiver("substr"),
        "length" => format!("(gd.size)({receiver})"),
        "format" => with_receiver("format_map"),
        "to_int" => format!("(gd.int)({receiver})"),
        "to_float" => format!("(gd.float)({receiver})"),
        "is_valid_int" | "is_valid_float" => with_receiver(name),
        // Nodes.
        "queue_free" => format!("{receiver}.queue_free()"),
        "add_child" => format!("{receiver}.add_child({one})"),
        "get_parent" => format!("{receiver}.parent()"),
        "get_children" => format!("{receiver}.children()"),
        "get_node" | "get_node_or_null" | "find_child" => format!("{receiver}.get_node({one})"),
        "hide" => format!("{receiver}.set_visible(false)"),
        "show" => format!("{receiver}.set_visible(true)"),
        "set_visible" => format!("{receiver}.set_visible({one})"),
        "is_visible" | "is_visible_in_tree" => format!("{receiver}.visible()"),
        "get_instance_id" => format!("{receiver}.stable_id()"),
        "is_in_group" => format!("{receiver}.has_tag({one})"),
        "add_to_group" => format!("{receiver}.add_tag({one})"),
        "remove_from_group" => format!("{receiver}.remove_tag({one})"),
        "has_method" => format!("{receiver}.has_method({one})"),
        "get_meta" => format!("(gd.get)({receiver}.meta, {all})"),
        "set_meta" => format!("{receiver}.meta[{}] = {}", args.first()?, args.get(1)?),
        "has_meta" => format!("(gd.has)({receiver}.meta, {one})"),
        // `ConfigFile`'s verbs. The shim checks the receiver and hands any
        // other value back to its own method, since these names are not the
        // config's alone.
        // `load` and `save` are not the config's alone, which is why the shim
        // dispatches rather than this table.
        "load" => with_receiver("config_load"),
        "save" => with_receiver("config_save"),
        "get_value" => with_receiver("config_get"),
        "set_value" => with_receiver("config_set"),
        "has_section" => with_receiver("config_has_section"),
        "has_section_key" => with_receiver("config_has_key"),
        "erase_section" => with_receiver("config_erase_section"),
        "get_sections" => with_receiver("config_sections"),
        "get_section_keys" => with_receiver("config_keys"),
        // `call` on a node is the engine's own verb already.
        "call" | "call_deferred" => format!("{receiver}.call({all})"),
        // The scene tree's own verbs, which Godot reached through
        // `get_tree()`. The receiver is the tree and carries nothing here.
        "get_nodes_in_group" => format!("scene::tagged({one})"),
        "create_timer" => format!("task::wait({one})"),
        "change_scene_to_file" | "change_scene_to_packed" => format!("scene::switch({one})"),
        "reload_current_scene" => "scene::switch(scene::source())".into(),
        "quit" if receiver == TREE => format!(
            "engine::quit({})",
            args.first().cloned().unwrap_or("0".into())
        ),
        "get_root" => "scene::root()".into(),
        "get_first_node_in_group" => format!("(gd.front)(scene::tagged({one}))"),
        // Frame ordering and drawing, which the engine states differently.
        "is_node_ready" | "is_inside_tree" => format!("{receiver}.is_valid()"),
        // The viewport, which Godot reached through the node and the engine
        // reports as the screen.
        "get_visible_rect" | "get_viewport_rect" => "(gd.viewport_rect)()".into(),
        "get_size" | "get_screen_size" => "(gd.screen_size)()".into(),
        "move_to_front" => format!("{receiver}.set_sibling_index(-1)"),
        "get_index" => format!("{receiver}.sibling_index()"),
        "remove_child" => format!("{one}.set_parent(())"),
        "get_process_delta_time" | "get_physics_process_delta_time" => "engine::delta()".into(),
        "set_pressed_no_signal" | "set_pressed" => {
            format!("{receiver}.set_component(\"widget\", #{{ \"pressed\": {one} }})")
        }
        _ => return None,
    })
}

/// `sig.emit(..)` and `sig.connect(..)`, where `sig` is a signal this class
/// declares. The engine names a signal with a string.
pub(crate) fn signal_verb(signal: &str, verb: &str, args: &[String]) -> Option<String> {
    let name = quoted(signal);
    Some(match verb {
        "emit" => match args.len() {
            0 => format!("this.node.emit({name}, ())"),
            1 => format!("this.node.emit({name}, {})", args[0]),
            _ => format!("this.node.emit({name}, [{}])", args.join(", ")),
        },
        // A connection is a subscription on the emitting node; the handler is
        // named by convention rather than passed, so the target is reported.
        "connect" => format!("events::subscribe(this.node, {name}, this.node)"),
        "disconnect" => format!("events::unsubscribe(this.node, {name}, this.node)"),
        "is_connected" => format!("events::emitted({name})"),
        "get_connections" => format!("/* connections of {} */ []", safe(signal)),
        _ => return None,
    })
}
