//! The Godot API as engine calls.
//!
//! Everything here is a rewrite that has been checked against the port's
//! hand-written files. A call in neither this table nor the shim is emitted as
//! it was written and counted in the report, which is how the next rows get
//! chosen.

mod controls;
mod globals;

pub(crate) use globals::{global_constant, singleton_write};

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
        "remove_meta",
        "get_index",
        "move_child",
        "find_child",
        "find_children",
        "create_tween",
        "is_node_ready",
        "is_inside_tree",
        "move_to_front",
        "get_process_delta_time",
        "get_physics_process_delta_time",
        "get_canvas_transform",
        "get_global_transform",
        "get_size",
        "set_notify_transform",
        "set_notify_local_transform",
        "set_process_input",
        "set_process_unhandled_input",
        "set_process_unhandled_key_input",
        "release_focus",
        "has_focus",
        "accept_event",
        "get_viewport_transform",
        "get_global_transform_with_canvas",
        "get_child_count",
        "create_timer",
        "add_title_bar_control",
        "queue_redraw",
        "draw_circle",
        "draw_line",
        "draw_rect",
        "draw_arc",
        "draw_polyline",
        "draw_multiline",
        "draw_polygon",
        "draw_colored_polygon",
        "draw_texture_rect",
        "draw_texture_rect_region",
        "draw_set_transform",
        "draw_set_transform_matrix",
        "draw_string",
        "clear",
        "add_item",
        "add_icon_item",
        "select",
        "get_item_text",
        "get_item_count",
        "get_selected_id",
        "set_item_disabled",
        "is_item_disabled",
    ];
    // A method the shim maps is its verb; any other goes to the node's own.
    NODE.contains(&name)
        .then(|| method("this.node", name, args).unwrap_or_else(|| invoke("this.node", name, args)))
}

/// Godot's numeric globals: the `i` forms answer an int, and the plain ones
/// keep an int an int where the engine's would answer a float.
fn numeric(name: &str, args: &[String]) -> Option<String> {
    let all = args.join(", ");
    let one = args.first().cloned().unwrap_or_default();
    Some(match name {
        // The `i` forms answer an int whatever they were given, as Godot's do.
        "mini" => format!("(gd.int)(math::min({all}))"),
        "maxi" => format!("(gd.int)(math::max({all}))"),
        "clampi" => format!("(gd.int)(math::clamp({all}))"),
        "absi" => format!("(gd.int)(math::abs({one}))"),
        "floori" => format!("(gd.int)(math::floor({one}))"),
        "ceili" => format!("(gd.int)(math::ceil({one}))"),
        "roundi" => format!("(gd.int)(math::round({one}))"),
        // Godot's keep an int an int; the engine's answer a float.
        "min" if args.len() == 2 => format!("(gd.min)({all})"),
        "max" if args.len() == 2 => format!("(gd.max)({all})"),
        "clamp" => format!("(gd.clamp)({all})"),
        "abs" => format!("(gd.abs)({one})"),
        "min" | "minf" => format!("math::min({all})"),
        "max" | "maxf" => format!("math::max({all})"),
        "clampf" => format!("math::clamp({all})"),
        "absf" => format!("math::abs({one})"),
        "floor" | "floorf" => format!("math::floor({one})"),
        "ceil" | "ceilf" => format!("math::ceil({one})"),
        "round" | "roundf" => format!("math::round({one})"),
        _ => return None,
    })
}

/// A global function: `str(x)`, `range(n)`, `push_error(m)`.
pub(crate) fn global(name: &str, args: &[String]) -> Option<String> {
    if let Some(text) = numeric(name, args) {
        return Some(text);
    }
    let all = args.join(", ");
    let one = args.first().cloned().unwrap_or_default();
    if let Some(text) = globals::arithmetic(name, &all, &one) {
        return Some(text);
    }
    if let Some(text) = controls::own_control(name, &all) {
        return Some(text);
    }
    Some(match name {
        // Variant conversions and queries live in the shim, which dispatches
        // on the value the way Godot's Variant did.
        "str" | "int" | "float" | "bool" => format!("(gd.{name})({all})"),
        "String" if args.len() == 1 => format!("(gd.str)({one})"),
        "char" => format!("(gd.chr)({one})"),
        // A `SceneTree` script's own `quit`, which a tool run calls bare.
        "quit" => format!(
            "engine::quit({})",
            args.first().cloned().unwrap_or_else(|| "0".into())
        ),
        "len" => format!("(gd.size)({one})"),
        // Object's own `get` and `set`, called bare: a member by its name.
        "get" if args.len() == 1 => format!("(gd.field)(this, {one})"),
        "set" if args.len() == 2 => format!("(gd.set_field)(this, {all})"),
        "is_instance_valid" => format!("(gd.valid)({one})"),
        "typeof" => format!("(gd.type_of)({one})"),
        "is_zero_approx" => format!("(gd.is_zero_approx)({one})"),
        "hash" => format!("(gd.hash)({one})"),
        "is_equal_approx" => format!("(gd.is_equal_approx)({all})"),
        "weakref" => one,
        // A debugging aid with no stack to show: an empty list of frames.
        "get_stack" => "[]".into(),
        // A literal bound is an int already; anything else may be a float,
        // which Godot truncates and a Rune range refuses.
        "range" => {
            let int = |a: &String| a.parse::<i64>().is_ok();
            match args.len() {
                1 if int(&args[0]) => format!("(0..{one})"),
                2 if int(&args[0]) && int(&args[1]) => format!("({}..{})", args[0], args[1]),
                1 => format!("(gd.range)(0, {one}, 1)"),
                2 => format!("(gd.range)({all}, 1)"),
                _ => format!("(gd.range)({all})"),
            }
        }
        "print" | "prints" | "printt" | "print_rich" => format!("log::info((gd.str_all)([{all}]))"),
        "push_error" | "printerr" => format!("log::error((gd.str_all)([{all}]))"),
        "push_warning" => format!("log::warn((gd.str_all)([{all}]))"),
        "tr" => format!("strings::tr({all})"),
        "Vector2" | "Vector2i" if args.is_empty() => "(gd.vec2)(0.0, 0.0)".into(),
        "Vector2" | "Vector2i" if args.len() == 1 => format!("(gd.vec_of)({one})"),
        "Vector2" | "Vector2i" => format!("(gd.vec2)({all})"),
        "Vector3" | "Vector3i" if args.is_empty() => "(gd.vec3)(0.0, 0.0, 0.0)".into(),
        "Rect2" | "Rect2i" => match args.len() {
            0 => "(gd.rect)(0.0, 0.0, 0.0, 0.0)".into(),
            2 => format!("(gd.rect_of)({all})"),
            _ => format!("(gd.rect)({all})"),
        },
        "Vector3" => format!("(gd.vec3)({all})"),
        "Transform2D" => format!("(gd.transform2d)([{all}])"),
        "NodePath" | "StringName" => {
            if args.is_empty() {
                "\"\"".into()
            } else {
                one
            }
        }
        packed if packed.starts_with("Packed") && packed.ends_with("Array") => {
            if args.is_empty() {
                "[]".into()
            } else {
                one
            }
        }
        // Godot's `Color` takes a hex string or a colour, a colour and an
        // alpha, three channels, or four.
        "Color" => match args.len() {
            0 => "(gd.color)(0.0, 0.0, 0.0, 1.0)".into(),
            1 => format!("(gd.color_of)({one})"),
            2 => format!("(gd.color_alpha)({all})"),
            3 => format!("(gd.color)({all}, 1.0)"),
            _ => format!("(gd.color)({all})"),
        },
        "Callable" if args.is_empty() => "()".into(),
        "Callable" => format!("(gd.callable)({all})"),
        "preload" | "load" => loaded(&args[0]),
        "instance_from_id" => format!("(gd.instance_from_id)({one})"),
        "get_tree" | "get_viewport" => TREE.into(),
        "get_window" => "(gd.window)()".into(),
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

// Two singletons can answer alike without meaning alike, and merging the rows
// would hide which call each one carries.
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
        ("OS", "is_debug_build") => "engine::platform().dev".into(),
        // A button group is the widget's `group` name here.
        ("ButtonGroup" | "FoldableGroup", "new") => "(gd.button_group)()".into(),
        ("RandomNumberGenerator", "new") => "(gd.random_numbers)()".into(),
        ("ProjectSettings", "get" | "get_setting") => {
            let fallback = args.get(1).map_or("()", String::as_str);
            format!("(gd.project_setting)({one}, {fallback})")
        }
        ("ProjectSettings", "has_setting") => {
            format!("!(gd.is_nil)((gd.project_setting)({one}, ()))")
        }
        // A project path is the path here: `fs` resolves it against the project.
        ("ProjectSettings", "globalize_path" | "localize_path") => {
            format!("(gd.project_path)({one})")
        }
        ("OS", "has_feature") => format!("(gd.has_feature)({one})"),
        ("OS" | "DisplayServer", "get_name") => "(gd.os_name)()".into(),
        ("DisplayServer", "window_get_size" | "screen_get_size") => "(gd.screen_size)()".into(),
        ("OS", "get_user_data_dir") => "engine::user_data_dir()".into(),
        ("OS", "shell_open") => format!("engine::open_url({one})"),
        ("OS", "get_locale" | "get_locale_language") => "strings::locale()".into(),
        ("ConfigFile", "new") => "(gd.config)()".into(),
        ("OS", "get_cmdline_args" | "get_cmdline_user_args") => "engine::args()".into(),
        ("OS", "get_environment") => format!("(gd.environment)({one})"),
        ("OS", "has_environment") => format!("(gd.has_environment)({one})"),
        ("JSON", "stringify") => format!("json::encode({one})"),
        ("JSON", "parse_string") => format!("json::parse({one})"),
        ("JSON", "new") => "(gd.json_object)()".into(),
        ("Shader", "new") => "(gd.shader)()".into(),
        ("ArrayMesh", "new") => "(gd.array_mesh)()".into(),
        ("MultiMesh", "new") => "(gd.multimesh)()".into(),
        ("RegEx", "new") => "(gd.regexp)()".into(),
        ("RegEx", "create_from_string") => format!("(gd.regexp_of)({one})"),
        // A texture is a path here, so an image loaded from one is that path.
        ("Image", "new") => "(gd.image)()".into(),
        ("ImageTexture", "create_from_image") => format!("(gd.image_texture)({one})"),
        ("ShaderMaterial", "new") => "(gd.shader_material)()".into(),
        ("CircleShape2D", "new") => "(gd.circle_shape)()".into(),
        ("RectangleShape2D", "new") => "(gd.rect_shape)()".into(),
        ("FileAccess", "open") => format!("(gd.file_open)({all})"),
        ("FileAccess", "get_open_error") => "(gd.file_error)()".into(),
        ("FileAccess", "get_modified_time") => format!("fs::mtime((gd.project_path)({one}))"),
        ("FileAccess", "get_file_as_string") => {
            format!("(gd.or_text)(fs::read((gd.project_path)({one})))")
        }
        ("Engine", "get_main_loop") => TREE.into(),
        // Every bus is the master bus here; `-1` is Godot's "no such bus".
        ("AudioServer", "get_bus_index") => "0".into(),
        ("AudioServer", "get_bus_count") => "1".into(),
        // A browser callback is a closure already.
        ("JavaScriptBridge", "create_callback") => one.clone(),
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
        ("ProjectSettings", "set_setting") => format!("settings::set({all})"),
        ("Input", "is_action_pressed") => format!("input::action_down({one})"),
        ("Input", "is_action_just_pressed") => format!("input::action_just_pressed({one})"),
        ("Input", "is_action_just_released") => format!("input::action_just_released({one})"),
        ("Input", "is_key_pressed" | "is_physical_key_pressed") => {
            format!("input::key_down({one})")
        }
        _ => return service_call(class, name, args),
    })
}

// Rows share values without sharing meaning, as `static_call`'s do.
#[allow(clippy::match_same_arms)]
/// The rest of Godot's singletons: the services a game reaches for less
/// often, each onto the engine module that answers it.
fn service_call(class: &str, name: &str, args: &[String]) -> Option<String> {
    let all = args.join(", ");
    let one = args.first().cloned().unwrap_or_default();
    Some(match (class, name) {
        // Godot's buttons count from one; the engine's from zero.
        ("Input", "is_mouse_button_pressed") => format!("input::mouse_down({one} - 1)"),
        ("Performance", "get_monitor") => format!("(gd.monitor)({one})"),
        ("ResourceLoader", "exists") => format!("(gd.resource_exists)({one})"),
        ("ResourceLoader", "load") if !args.is_empty() => loaded(&args[0]),
        ("Marshalls", "base64_to_raw" | "base64_to_utf8") => {
            format!("encoding::from_base64({one})")
        }
        ("Marshalls", "raw_to_base64" | "utf8_to_base64") => format!("encoding::base64({one})"),
        ("FileAccess", "get_sha256") => format!("hash::sha256((gd.project_path)({one}))"),
        ("DirAccess", "rename_absolute") => format!("fs::rename({all})"),
        ("DirAccess", "remove_absolute") => format!("fs::remove({one})"),
        ("DirAccess", "open") => format!("(gd.dir_open)({one})"),
        ("DisplayServer", "screen_set_keep_on") => format!("window::set_keep_awake({one})"),
        // A platform has the display verbs this table carries, and no other.
        ("DisplayServer", "has_method") => {
            let known = [
                "screen_set_keep_on",
                "virtual_keyboard_get_height",
                "window_get_size",
            ]
            .iter()
            .any(|verb| one.trim_matches('"') == *verb);
            known.to_string()
        }
        ("DisplayServer", "virtual_keyboard_get_height") => "input::keyboard_height()".into(),
        // The page answers the one question the game asks of the browser;
        // a reload and a heap probe get nothing. Decided on the literal,
        // since the shim compiles in builds that carry no `web` module.
        ("JavaScriptBridge", "eval") if one.contains("document.hidden") => "!web::visible()".into(),
        ("JavaScriptBridge", "eval") if one.contains("reload") => "()".into(),
        ("JavaScriptBridge", "eval") => "-1".into(),
        ("JavaScriptBridge", "get_interface") => "()".into(),
        ("Vector2", "from_angle") => format!("(gd.vec_from_angle)({one})"),
        // Every bus is the master bus here, as `get_bus_index` says.
        ("AudioServer", "set_bus_volume_db") if args.len() == 2 => {
            format!(
                "audio::set_bus_volume(\"master\", (gd.db_to_linear)({}))",
                args[1]
            )
        }
        ("AudioServer", "get_bus_volume_db") => {
            "(gd.linear_to_db)(audio::bus_volume(\"master\"))".into()
        }
        ("AudioServer", "set_bus_mute") if args.len() == 2 => format!(
            "audio::set_bus_volume(\"master\", if {} {{ 0.0 }} else {{ 1.0 }})",
            args[1]
        ),
        ("AudioServer", "is_bus_mute") => "(audio::bus_volume(\"master\") <= 0.0)".into(),
        ("Geometry2D", "is_point_in_polygon") if args.len() == 2 => {
            format!("geometry2d::contains({}, {})", args[1], args[0])
        }
        // Godot's y runs down and the engine's up, so the winding flips.
        ("Geometry2D", "is_polygon_clockwise") => format!("!geometry2d::is_clockwise({one})"),
        ("Geometry2D", "merge_polygons") => format!("(gd.merge_polygons)({all})"),
        ("Geometry2D", "segment_intersects_segment") => {
            format!("geometry2d::segments_intersect({all})")
        }
        ("Geometry2D", "triangulate_polygon") => format!("(gd.triangulate)({one})"),
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
        ("Tween", trans) if trans.starts_with("TRANS_") => {
            quoted(&trans["TRANS_".len()..].to_lowercase())
        }
        ("Tween", mode) if mode.starts_with("EASE_") => {
            quoted(&mode["EASE_".len()..].to_lowercase())
        }
        ("Color", "WHITE") => "(gd.color)(1.0, 1.0, 1.0, 1.0)".into(),
        ("Color", "BLACK") => "(gd.color)(0.0, 0.0, 0.0, 1.0)".into(),
        ("Color", "TRANSPARENT") => "(gd.color)(0.0, 0.0, 0.0, 0.0)".into(),
        ("Color", "RED") => "(gd.color)(1.0, 0.0, 0.0, 1.0)".into(),
        ("Color", "GREEN") => "(gd.color)(0.0, 1.0, 0.0, 1.0)".into(),
        ("Color", "BLUE") => "(gd.color)(0.0, 0.0, 1.0, 1.0)".into(),
        ("Color", "YELLOW") => "(gd.color)(1.0, 1.0, 0.0, 1.0)".into(),
        ("Color", "ORANGE") => "(gd.color)(1.0, 0.647, 0.0, 1.0)".into(),
        ("Color", "GRAY") => "(gd.color)(0.75, 0.75, 0.75, 1.0)".into(),
        ("Vector2", "INF") => "(gd.vec2)(1.0 / 0.0, 1.0 / 0.0)".into(),
        ("Transform2D", "IDENTITY") => "balaur::Transform2d::IDENTITY".into(),
        (class, name) => class_constant(class, name)?.to_string(),
    })
}

/// An enum value on one of Godot's classes, as the integer Godot gives it.
// Rows share values without sharing meaning, as `global_constant`'s do.
#[allow(clippy::match_same_arms)]
fn class_constant(class: &str, name: &str) -> Option<i64> {
    if let Some(value) = controls::control_constant(class, name) {
        return Some(value);
    }
    Some(match (class, name) {
        // Godot 4 also spells a Control enum by its own name: `MouseFilter.X`.
        ("Control" | "MouseFilter", "MOUSE_FILTER_STOP") => 0,
        ("Control" | "MouseFilter", "MOUSE_FILTER_PASS") => 1,
        ("Control" | "MouseFilter", "MOUSE_FILTER_IGNORE") => 2,
        ("Control" | "FocusMode", "FOCUS_NONE") => 0,
        ("Control" | "FocusMode", "FOCUS_CLICK") => 1,
        ("Control" | "FocusMode", "FOCUS_ALL") => 2,
        ("Control", "SIZE_SHRINK_BEGIN") => 0,
        ("Control", "SIZE_FILL") => 1,
        ("Control", "SIZE_EXPAND") => 2,
        ("Control", "SIZE_EXPAND_FILL") => 3,
        ("Control", "SIZE_SHRINK_CENTER") => 4,
        ("Control", "SIZE_SHRINK_END") => 8,
        ("Control", "PRESET_TOP_LEFT") => 0,
        ("Control", "PRESET_CENTER") => 8,
        ("Control", "PRESET_FULL_RECT") => 15,
        ("Control" | "CursorShape", "CURSOR_ARROW") => 0,
        ("Control" | "CursorShape", "CURSOR_IBEAM") => 1,
        ("Control" | "CursorShape", "CURSOR_POINTING_HAND") => 2,
        ("Control" | "CursorShape", "CURSOR_CROSS") => 3,
        ("Control" | "CursorShape", "CURSOR_WAIT") => 4,
        ("Control" | "CursorShape", "CURSOR_BUSY") => 5,
        ("Control" | "CursorShape", "CURSOR_DRAG") => 6,
        ("Control" | "CursorShape", "CURSOR_CAN_DROP") => 7,
        ("Control" | "CursorShape", "CURSOR_FORBIDDEN") => 8,
        ("Control" | "CursorShape", "CURSOR_VSIZE") => 9,
        ("Control" | "CursorShape", "CURSOR_HSIZE") => 10,
        ("Control" | "CursorShape", "CURSOR_BDIAGSIZE") => 11,
        ("Control" | "CursorShape", "CURSOR_FDIAGSIZE") => 12,
        ("Control" | "CursorShape", "CURSOR_MOVE") => 13,
        ("Control" | "CursorShape", "CURSOR_VSPLIT") => 14,
        ("Control" | "CursorShape", "CURSOR_HSPLIT") => 15,
        ("Control" | "CursorShape", "CURSOR_HELP") => 16,
        ("BoxContainer" | "FlowContainer", "ALIGNMENT_BEGIN") => 0,
        ("BoxContainer" | "FlowContainer", "ALIGNMENT_CENTER") => 1,
        ("BoxContainer" | "FlowContainer", "ALIGNMENT_END") => 2,
        ("ScrollContainer", "SCROLL_MODE_DISABLED") => 0,
        ("ScrollContainer", "SCROLL_MODE_AUTO") => 1,
        ("ScrollContainer", "SCROLL_MODE_SHOW_ALWAYS") => 2,
        ("ScrollContainer", "SCROLL_MODE_SHOW_NEVER") => 3,
        ("Mesh", "ARRAY_VERTEX") => 0,
        ("Mesh", "PRIMITIVE_TRIANGLES") => 3,
        ("Mesh", "ARRAY_MAX") => 13,
        // The monitor ids `gd.monitor` answers; the two only have to agree.
        ("Performance", "TIME_FPS") => 0,
        ("Performance", "TIME_PROCESS") => 1,
        ("Performance", "TIME_PHYSICS_PROCESS") => 2,
        ("Performance", "MEMORY_STATIC") => 4,
        ("Performance", "MEMORY_STATIC_MAX") => 5,
        ("Performance", "OBJECT_COUNT") => 7,
        ("Performance", "OBJECT_NODE_COUNT") => 9,
        ("Performance", "OBJECT_ORPHAN_NODE_COUNT") => 10,
        ("Performance", "RENDER_TOTAL_OBJECTS_IN_FRAME") => 11,
        ("Performance", "RENDER_TOTAL_DRAW_CALLS_IN_FRAME") => 13,
        ("Performance", "RENDER_VIDEO_MEM_USED") => 14,
        ("TextureRect", "EXPAND_KEEP_SIZE") => 0,
        ("TextureRect", "EXPAND_IGNORE_SIZE") => 1,
        ("TextureRect", "EXPAND_FIT_WIDTH") => 2,
        ("TextureRect", "EXPAND_FIT_WIDTH_PROPORTIONAL") => 3,
        ("TextureRect", "EXPAND_FIT_HEIGHT") => 4,
        ("TextureRect", "EXPAND_FIT_HEIGHT_PROPORTIONAL") => 5,
        ("TextureRect", "STRETCH_SCALE") => 0,
        ("TextureRect", "STRETCH_TILE") => 1,
        ("TextureRect", "STRETCH_KEEP") => 2,
        ("TextureRect", "STRETCH_KEEP_CENTERED") => 3,
        ("TextureRect", "STRETCH_KEEP_ASPECT") => 4,
        ("TextureRect", "STRETCH_KEEP_ASPECT_CENTERED") => 5,
        ("TextureRect", "STRETCH_KEEP_ASPECT_COVERED") => 6,
        ("Line2D", "LINE_JOINT_SHARP" | "LINE_CAP_NONE") => 0,
        ("Line2D", "LINE_JOINT_BEVEL" | "LINE_CAP_BOX") => 1,
        ("Line2D", "LINE_JOINT_ROUND" | "LINE_CAP_ROUND") => 2,
        ("Node", "PROCESS_MODE_INHERIT") => 0,
        ("Node", "PROCESS_MODE_PAUSABLE") => 1,
        ("Node", "PROCESS_MODE_WHEN_PAUSED") => 2,
        ("Node", "PROCESS_MODE_ALWAYS") => 3,
        ("Node", "PROCESS_MODE_DISABLED") => 4,
        ("MultiMesh", "TRANSFORM_2D") => 0,
        ("MultiMesh", "TRANSFORM_3D") => 1,
        ("TextServer", "AUTOWRAP_OFF") => 0,
        ("TextServer", "AUTOWRAP_ARBITRARY") => 1,
        ("TextServer", "AUTOWRAP_WORD") => 2,
        ("TextServer", "AUTOWRAP_WORD_SMART") => 3,
        ("Label" | "RichTextLabel", "AUTOWRAP_OFF") => 0,
        ("Label" | "RichTextLabel", "AUTOWRAP_WORD") => 2,
        ("Label" | "RichTextLabel", "AUTOWRAP_WORD_SMART") => 3,
        ("FileAccess", "READ") => 1,
        ("FileAccess", "WRITE") => 2,
        ("FileAccess", "READ_WRITE") => 3,
        ("BaseButton", "ACTION_MODE_BUTTON_PRESS") => 0,
        ("BaseButton", "ACTION_MODE_BUTTON_RELEASE") => 1,
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
    // A key is the engine's constant for it, which holds the code a key hook
    // hands over and `input::key_down` takes.
    if let Some(key) = name
        .strip_prefix("KEY_")
        .and_then(crate::godot::keys::key_constant)
    {
        return Some(format!("input::{key}"));
    }
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
    let simple = value
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '.');
    let value = if simple {
        value.to_string()
    } else {
        format!("({value})")
    };
    match name {
        "int" => format!("{value} is i64"),
        "float" => format!("{value} is f64"),
        "bool" => format!("{value} is bool"),
        "String" | "StringName" => format!("{value} is String"),
        "Array" => format!("{value} is Vec"),
        "Dictionary" => format!("(gd.is_dict)({value})"),
        _ => format!("(gd.is_a)({value}, {})", quoted(name)),
    }
}

/// `x as float`: a numeric cast is real work here, where it was an assertion
/// in GDScript.
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
/// Signals every node or control carries, which a script names bare.
pub(crate) const BUILTIN_SIGNALS: &[&str] = &[
    "visibility_changed",
    "resized",
    "ready",
    "tree_entered",
    "tree_exiting",
    "tree_exited",
    "draw",
    "mouse_entered",
    "mouse_exited",
    "focus_entered",
    "focus_exited",
    "item_rect_changed",
    "child_entered_tree",
    "child_exiting_tree",
];

/// The node properties `field` in the shim reads off a node at run time.
const NODE_PROPERTIES: &[&str] = &[
    "zoom",
    "foldable_group",
    "position",
    "global_position",
    "scale",
    "rotation",
    "rotation_degrees",
    "size",
    "visible",
    "modulate",
    "self_modulate",
    "z_index",
    "name",
];

pub(crate) fn property(receiver: &str, field: &str) -> Option<String> {
    // `this.<export>` and every other script field stay fields.
    if receiver == "this" {
        return None;
    }
    // A receiver only known at run time may be a rect or a table as well as
    // a node: the shim's `field` asks which.
    let node = receiver == "this.node";
    if !node && NODE_PROPERTIES.contains(&field) {
        return None;
    }
    Some(match field {
        "visible" => format!("{receiver}.visible()"),
        // Kept beside the node by the shim: the engine has no fold groups.
        "foldable_group" => format!("(gd.field)({receiver}, \"foldable_group\")"),
        "global_position" => format!("(gd.global_position_of)({receiver})"),
        "position" => format!("(gd.position_of)({receiver})"),
        "scale" => format!("(gd.vec_of)({receiver}.transform.scale)"),
        "modulate" | "self_modulate" => format!("{receiver}.tint()"),
        "z_index" => format!("{receiver}.z_index()"),
        "name" => format!("{receiver}.name()"),
        "rotation_degrees" => format!("math::deg((gd.rotation_of)({receiver}))"),
        "custom_minimum_size" => format!("(gd.min_size)({receiver})"),
        "size" => format!("(gd.size_of)({receiver})"),
        "theme" => format!("(gd.theme_of)({receiver})"),
        "rotation" => format!("(gd.rotation_of)({receiver})"),
        "current_scene" | "root" => "scene::root()".into(),
        "selected" => format!("(gd.option_index)({receiver})"),
        "item_count" => format!("(gd.option_count)({receiver})"),
        "text" | "disabled" | "pressed" | "button_pressed" | "editable" | "placeholder_text"
        | "tooltip_text" | "value" | "max_value" | "min_value" | "icon" => {
            let key = widget_key(field);
            format!("(gd.get)({receiver}.get_component(\"widget\"), \"{key}\", ())")
        }
        _ => return None,
    })
}

/// The widget component's key for a Godot button or range property: a
/// toggle's state is `checked` here.
fn widget_key(field: &str) -> &str {
    match field {
        "button_pressed" | "pressed" => "checked",
        "min_value" => "min",
        "max_value" => "max",
        other => other,
    }
}

/// An argument Godot lets a call leave out, as nil when it did.
fn or_nil(arg: &str) -> &str {
    if arg.is_empty() { "()" } else { arg }
}

/// Godot's `Tween`, whose builder the shim runs over `animation::tween`.
fn tween_verb(receiver: &str, name: &str, args: &[String]) -> Option<String> {
    let all = args.join(", ");
    let one = args.first().cloned().unwrap_or_default();
    Some(match name {
        "create_tween" => format!("(gd.create_tween)({receiver})"),
        "set_trans" => format!("(gd.set_trans)({receiver}, {one})"),
        "set_ease" => format!("(gd.set_ease)({receiver}, {one})"),
        "tween_property" if args.len() == 4 => format!("(gd.tween_property)({receiver}, {all})"),
        "tween_interval" => format!("(gd.tween_interval)({receiver}, {one})"),
        "tween_callback" => format!("(gd.tween_callback)({receiver}, {one})"),
        "kill" => format!("(gd.kill_tween)({receiver})"),
        "is_running" => format!("(gd.tween_running)({receiver})"),
        "parallel" => format!("(gd.tween_parallel)({receiver})"),
        "chain" => format!("(gd.tween_chain)({receiver})"),
        "set_parallel" => format!("(gd.tween_set_parallel)({receiver}, {})", or_nil(&one)),
        "set_loops" => format!("(gd.tween_loops)({receiver}, {})", or_nil(&one)),
        // A step here starts as it is declared, and runs at the node's speed.
        "set_speed_scale" | "bind_node" | "set_process_mode" => {
            format!("(gd.tween_same)({receiver}, \"{name}\", [{all}])")
        }
        _ => return None,
    })
}

/// A Godot global constant with a value here. Its enums are plain integers.
/// `load("res://a/b.png")` is the project path here, so a component naming
/// the asset gets what it expects. A path only known at run time goes to the
/// shim, which does the same rewrite then.
fn loaded(arg: &str) -> String {
    let Some(literal) = arg.strip_prefix('"').and_then(|a| a.strip_suffix('"')) else {
        return format!("(gd.load)({arg})");
    };
    let path = crate::godot::relative_path(literal);
    if crate::godot::files::has_extension(path, "tres") {
        return format!("(gd.resource)({})", quoted(&format!("{path}.rn")));
    }
    if crate::godot::files::has_extension(path, "csv") {
        return format!("(gd.csv_resource)({})", quoted(path));
    }
    match path.strip_suffix(".tscn") {
        Some(stem) => quoted(&format!("{stem}.toml")),
        None => quoted(path),
    }
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
        "value",
        "max_value",
        "min_value",
        "tooltip_text",
        "icon",
    ];
    if field == "selected" {
        return Some(format!("(gd.option_select)({receiver}, {value})"));
    }
    // The window's content scale is the UI's global scale here.
    if field == "content_scale_factor" {
        return Some(format!("ui::set_scale({value})"));
    }
    if WIDGET.contains(&field) {
        let key = widget_key(field);
        return Some(format!(
            "{receiver}.patch_component(\"widget\", #{{ \"{key}\": {value} }})"
        ));
    }
    if receiver != "this.node" && NODE_PROPERTIES.contains(&field) {
        return Some(format!("(gd.set_field)({receiver}, \"{field}\", {value})"));
    }
    Some(match field {
        "visible" => format!("{receiver}.set_visible({value})"),
        // Godot numbers its cursor shapes; the widget names its `cursor`.
        "mouse_default_cursor_shape" => format!(
            "{receiver}.patch_component(\"widget\", #{{ \"cursor\": (gd.cursor_word)({value}) }})"
        ),
        // `MOUSE_FILTER_IGNORE` is 2; the other two keep the pointer.
        "mouse_filter" => format!(
            "{receiver}.patch_component(\"widget\", #{{ \"pointer_through\": {value} == 2 }})"
        ),
        "position" => format!("(gd.set_position)({receiver}, {value})"),
        "global_position" => format!("(gd.set_global_position)({receiver}, {value})"),
        "scale" => format!("(gd.set_scale)({receiver}, {value})"),
        "modulate" | "self_modulate" => format!("(gd.set_tint)({receiver}, {value})"),
        "z_index" => format!("{receiver}.set_z_index({value})"),
        "name" => format!("{receiver}.set_name({value})"),
        "rotation_degrees" => format!("(gd.set_rotation)({receiver}, math::rad({value}))"),
        "rotation" => format!("(gd.set_rotation)({receiver}, {value})"),
        "custom_minimum_size" => format!("(gd.set_min_size)({receiver}, {value})"),
        // A control's own size is the widget panel's width and height here.
        "size" => format!("(gd.set_size)({receiver}, {value})"),
        "button_group" => format!(
            "{receiver}.patch_component(\"widget\", #{{ \"group\": {value}, \"toggle\": true }})"
        ),
        _ => return None,
    })
}

/// A string method as the shim verb that answers it on any value.
fn string_verb(name: &str) -> Option<&'static str> {
    Some(match name {
        "to_lower" => "to_lower",
        "to_upper" => "to_upper",
        "strip_edges" => "strip_edges",
        "begins_with" => "begins_with",
        "ends_with" => "ends_with",
        "contains" => "contains",
        "split" => "split",
        "join" => "join",
        "substr" => "substr",
        "length" => "size",
        "format" => "format_map",
        "to_int" => "int",
        "to_float" => "float",
        "is_valid_int" => "is_valid_int",
        "is_valid_float" => "is_valid_float",
        _ => return None,
    })
}

/// `ConfigFile`'s verbs as the shim's. These names are not the config's
/// alone, so the shim checks the receiver and hands any other value on.
fn config_verb(name: &str) -> Option<&'static str> {
    Some(match name {
        "load" => "config_load",
        "save" => "config_save",
        "get_value" => "config_get",
        "set_value" => "config_set",
        "has_section" => "config_has_section",
        "has_section_key" => "config_has_key",
        "erase_section" => "config_erase_section",
        "erase_section_key" => "config_erase_key",
        "get_sections" => "config_sections",
        "get_section_keys" => "config_keys",
        _ => return None,
    })
}

/// Godot's `_draw` verbs on a node, through the shim, which takes the
/// node's transform into account and draws for this frame. `queue_redraw`
/// asks for nothing: every frame draws.
fn draw_verb(receiver: &str, name: &str, args: &[String]) -> Option<String> {
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
        return Some(format!("(gd.queue_redraw)({receiver})"));
    }
    let (_, takes) = VERBS.iter().find(|(verb, _)| *verb == name)?;
    let mut parts: Vec<String> = args.iter().take(*takes).cloned().collect();
    parts.resize(*takes, "()".to_string());
    Some(format!("(gd.{name})({receiver}, {})", parts.join(", ")))
}

/// What a node answers about itself: its hash, its children, its focus and
/// where the canvas put it.
fn node_query(receiver: &str, name: &str, args: &[String]) -> Option<String> {
    let one = args.first().cloned().unwrap_or_default();
    Some(match name {
        // `name.hash()`: the global `hash` on any value, as Godot answers it.
        "hash" if args.is_empty() => format!("(gd.hash)({receiver})"),
        "get_canvas_transform" | "get_viewport_transform" => {
            format!("(gd.canvas_transform)({receiver})")
        }
        "get_child_count" => format!("{receiver}.children().len()"),
        "has_focus" => format!("(gd.same)(ui::focused_widget(), {receiver})"),
        // Focus is the widget layer's to give, and a foldable's title bar is
        // its `fold` widget's header.
        "release_focus" | "add_title_bar_control" => "()".into(),
        // The hook that handed the event over answers `true` for it.
        "accept_event" | "set_input_as_handled" => "(gd.set_input_handled)()".into(),
        // A per-node switch the hooks the translator writes read first.
        "set_process_input" | "set_process_unhandled_input" | "set_process_unhandled_key_input" => {
            if receiver == "this.node" {
                format!("this.input_enabled = {one}")
            } else {
                format!("(gd.set_field)({receiver}, \"input_enabled\", {one})")
            }
        }
        _ => return None,
    })
}

/// The scene tree's own verbs, which Godot reached through `get_tree()`. The
/// receiver is the tree and carries nothing here.
fn tree_verb(receiver: &str, name: &str, args: &[String]) -> Option<String> {
    let one = args.first().cloned().unwrap_or_default();
    Some(match name {
        "get_nodes_in_group" => format!("scene::tagged({one})"),
        "create_timer" => format!("(gd.timer)({one})"),
        "change_scene_to_file" | "change_scene_to_packed" => format!("scene::switch({one})"),
        "reload_current_scene" => "scene::switch(scene::source())".into(),
        "quit" if receiver == TREE => format!(
            "engine::quit({})",
            args.first().cloned().unwrap_or("0".into())
        ),
        "get_root" => "scene::root()".into(),
        "get_first_node_in_group" => format!("(gd.front)(scene::tagged({one}))"),
        _ => return None,
    })
}

/// A method on a value. The receiver's text is passed so a rewrite can put it
/// where the engine call wants it.
#[allow(clippy::match_same_arms)]
pub(crate) fn method(receiver: &str, name: &str, args: &[String]) -> Option<String> {
    let all = args.join(", ");
    let one = args.first().cloned().unwrap_or_default();
    if let Some(text) = tween_verb(receiver, name, args) {
        return Some(text);
    }
    if let Some(text) = node_query(receiver, name, args) {
        return Some(text);
    }
    if let Some(text) = draw_verb(receiver, name, args) {
        return Some(text);
    }
    if let Some(text) = tree_verb(receiver, name, args) {
        return Some(text);
    }
    if let Some(text) = controls::control_verb(receiver, name, args) {
        return Some(text);
    }
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
        "duplicate" if args.first().is_some_and(|deep| deep == "true") => {
            format!("(gd.duplicate_deep)({receiver})")
        }
        "duplicate" => format!("(gd.duplicate)({receiver})"),
        "size" | "is_empty" | "keys" | "values" | "clear" | "front" | "back" | "pop_front"
        | "pop_back" | "sort" | "reverse" => with_receiver(name),
        // The shim's `get` always takes a fallback; Godot's defaults to null.
        "get" if args.len() == 1 => format!("(gd.get)({receiver}, {one}, ())"),
        "get" => with_receiver("get"),
        "set" if args.len() == 2 => with_receiver("set"),
        "has" | "has_key" => with_receiver("has"),
        "is_valid" if args.is_empty() => format!("(gd.valid)({receiver})"),
        "append" | "push_back" => with_receiver("append"),
        "append_array" => with_receiver("append_array"),
        "erase" => with_receiver("erase"),
        "find" => with_receiver("find"),
        "merge" => with_receiver("merge"),
        "slice" => with_receiver("slice"),
        _ if string_verb(name).is_some() => with_receiver(string_verb(name)?),
        // Nodes.
        "queue_free" => format!("{receiver}.queue_free()"),
        "add_child" => format!("(gd.add_child)({receiver}, {one})"),
        "get_parent" => format!("{receiver}.parent()"),
        "get_children" => format!("{receiver}.children()"),
        // An unset `NodePath` export is nothing, and Godot answered null.
        "get_node" | "get_node_or_null" => format!("(gd.node_at)({receiver}, {one})"),
        "instantiate" => format!("(gd.instantiate)({receiver})"),
        "find_child" => format!(
            "(gd.front)((gd.find_children)({receiver}, {one}, \"\", {}))",
            args.get(1).map_or("true", String::as_str)
        ),
        "find_children" => format!(
            "(gd.find_children)({receiver}, {one}, {}, {})",
            args.get(1).map_or("\"\"", String::as_str),
            args.get(2).map_or("true", String::as_str)
        ),
        "hide" | "show" => format!("{receiver}.set_visible({})", name == "show"),
        "set_visible" => format!("{receiver}.set_visible({one})"),
        "is_visible" => format!("{receiver}.visible()"),
        "is_visible_in_tree" => format!("{receiver}.global_visible()"),
        "get_instance_id" => format!("{receiver}.stable_id()"),
        "is_in_group" => format!("{receiver}.has_tag({one})"),
        "add_to_group" => format!("{receiver}.add_tag({one})"),
        "remove_from_group" => format!("{receiver}.remove_tag({one})"),
        "has_method" => format!("{receiver}.has_method({one})"),
        "get_meta" => format!(
            "(gd.get_meta)({receiver}, {one}, {})",
            args.get(1).map_or("()", String::as_str)
        ),
        "set_meta" => format!(
            "(gd.set_meta)({receiver}, {}, {})",
            args.first()?,
            args.get(1)?
        ),
        "has_meta" => format!("(gd.has_meta)({receiver}, {one})"),
        "remove_meta" => format!("(gd.remove_meta)({receiver}, {one})"),
        _ if config_verb(name).is_some() => with_receiver(config_verb(name)?),
        // `call` on a node is the engine's own verb already.
        "call" | "call_deferred" => format!("(gd.call_value)({receiver}, [{all}])"),
        // Frame ordering and drawing, which the engine states differently.
        "is_node_ready" | "is_inside_tree" => format!("{receiver}.is_valid()"),
        // The viewport, which Godot reached through the node and the engine
        // reports as the screen.
        "get_visible_rect" | "get_viewport_rect" => "(gd.viewport_rect)()".into(),
        "get_global_transform" | "get_global_transform_with_canvas" => {
            format!("(gd.global_transform)({receiver})")
        }
        "get_size" if receiver != TREE => format!("(gd.size_of)({receiver})"),
        "get_size" | "get_screen_size" => "(gd.screen_size)()".into(),
        "set_notify_transform" | "set_notify_local_transform" => "()".into(),
        "move_to_front" => format!("{receiver}.set_sibling_index(-1)"),
        "get_index" => format!("{receiver}.sibling_index()"),
        "remove_child" => format!("(gd.remove_child)({one})"),
        "get_process_delta_time" | "get_physics_process_delta_time" => "engine::delta()".into(),
        "set_pressed_no_signal" | "set_pressed" => {
            format!("{receiver}.patch_component(\"widget\", #{{ \"checked\": {one} }})")
        }
        _ => return None,
    })
}

/// `sig.emit(..)` and `sig.connect(..)`, where `sig` is a signal this class
/// declares. The engine names a signal with a string.
/// The widget key a built-in Godot signal is spelled by here. A clicked
/// widget calls `on_click` on the first ancestor whose script has the method,
/// which is what `button.pressed.connect(self._on_pressed)` meant.
pub(crate) fn widget_signal(signal: &str) -> Option<&'static str> {
    WIDGET_SIGNALS
        .iter()
        .find(|(godot, _)| *godot == signal)
        .map(|(_, key)| *key)
}

/// The widget keys a control signal lands on, which `balaur_ui` spells.
pub(crate) const ON_CLICK: &str = "on_click";
pub(crate) const ON_CHANGE: &str = "on_change";
pub(crate) const ON_SUBMIT: &str = "on_submit";
pub(crate) const ON_FOCUS: &str = "on_focus";

/// Each Godot control signal and the widget key that hears it. The scene
/// importer and the translator read it here; the shim keeps a copy a test
/// holds to it.
pub(crate) const WIDGET_SIGNALS: &[(&str, &str)] = &[
    ("pressed", ON_CLICK),
    ("button_up", ON_CLICK),
    ("toggled", ON_CHANGE),
    ("value_changed", ON_CHANGE),
    ("text_changed", ON_CHANGE),
    ("item_selected", ON_CHANGE),
    ("color_changed", ON_CHANGE),
    ("tab_changed", ON_CHANGE),
    ("tab_selected", ON_CHANGE),
    ("folding_changed", ON_CHANGE),
    ("close_requested", ON_CHANGE),
    ("text_submitted", ON_SUBMIT),
    ("focus_entered", ON_FOCUS),
];

/// The keys of the records the shim's `call_value` calls: another object's
/// method, with what `bind` fixed, and a handler with how many it takes. The
/// shim spells them too, and a test holds the two together.
pub(crate) const BOUND_OWNER: &str = "__bound";
pub(crate) const BOUND_METHOD: &str = "__method";
pub(crate) const BOUND_ARGS: &str = "__args";
pub(crate) const CALL_KEY: &str = "__call";
pub(crate) const CALL_TAKES: &str = "__takes";

/// Godot's `Callable.bind`.
pub(crate) const BIND: &str = "bind";

/// Whether the engine sends a signal itself: a widget's, or one of the rest.
pub(crate) fn engine_signal(signal: &str) -> bool {
    widget_signal(signal).is_some() || ENGINE_SIGNALS.contains(&signal)
}

/// Connecting one: the handler's name goes on the widget, and disconnecting
/// takes it off again.
pub(crate) fn widget_connect(receiver: &str, key: &str, handler: Option<&str>) -> String {
    let name = quoted(handler.unwrap_or(""));
    format!("{receiver}.patch_component(\"widget\", #{{ \"{key}\": {name} }})")
}

/// Hearing another node's signal. The engine calls the subscriber's
/// `on_<name>`, which §Signals emits as a forwarder to Godot's handler.
/// One script calling another's method. Godot reached it off the node; here
/// the node is asked for it, and an object answers its own field. A shim per
/// arity, because a Rune function takes the arguments it declares.
pub(crate) fn invoke(receiver: &str, method: &str, args: &[String]) -> String {
    if args.len() > MOST_ARGS {
        return format!(
            "(gd.invoke_many)({receiver}, {}, [{}])",
            quoted(method),
            args.join(", ")
        );
    }
    let name = if args.is_empty() {
        "invoke".to_string()
    } else {
        format!("invoke{}", args.len())
    };
    let list = if args.is_empty() {
        String::new()
    } else {
        format!(", {}", args.join(", "))
    };
    format!("(gd.{name})({receiver}, {}{list})", quoted(method))
}

/// How many arguments the `invoke` shims spell out; past this, `invoke_many`.
pub(crate) const MOST_ARGS: usize = 3;

/// A node emitter is heard as an event, through the module's forwarder; a
/// class table keeps the handler and calls it itself.
pub(crate) fn signal_subscribe(receiver: &str, signal: &str, handler: &str) -> String {
    if engine_signal(signal) {
        return format!(
            "(gd.listen)(this.node, {}, {receiver}, {handler})",
            quoted(signal)
        );
    }
    format!("(gd.connect)({receiver}, {}, {handler})", quoted(signal))
}

pub(crate) fn signal_unsubscribe(receiver: &str, signal: &str) -> String {
    if engine_signal(signal) {
        return format!("(gd.unlisten)(this.node, {}, {receiver})", quoted(signal));
    }
    format!("(gd.disconnect)({receiver}, {})", quoted(signal))
}

pub(crate) fn signal_verb(signal: &str, verb: &str, args: &[String]) -> Option<String> {
    let name = quoted(signal);
    Some(match verb {
        // Godot calls a signal's handlers as it is emitted; the engine's
        // event goes out too, for a scene's rows and the engine's listeners.
        "emit" => format!("(gd.emit_now)(this.node, {name}, [{}])", args.join(", ")),
        "connect" if !args.is_empty() => format!("(gd.connect)(this.node, {name}, {})", args[0]),
        "connect" => format!("events::subscribe(this.node, {name}, this.node)"),
        "disconnect" => format!("(gd.disconnect)(this.node, {name})"),
        "is_connected" => format!("(gd.is_connected)(this.node, {name})"),
        "get_connections" => format!("/* connections of {} */ []", safe(signal)),
        _ => return None,
    })
}

/// Signals the engine itself sends besides a widget's, heard as events; a
/// script's own are called as they are emitted.
/// Godot's "it went away", which the engine reports as the visibility event.
pub(crate) const HIDDEN_SIGNAL: &str = "hidden";

/// The engine's own name for it, carrying the new value.
pub(crate) const VISIBILITY_SIGNAL: &str = "visibility_changed";

pub(crate) const ENGINE_SIGNALS: &[&str] = &[
    "timeout",
    "animation_finished",
    "finished",
    "body_entered",
    "body_exited",
    "area_entered",
    "area_exited",
    "button_down",
    "visibility_changed",
    "resized",
    "tree_entered",
    "tree_exiting",
    "tree_exited",
    "ready",
    "mouse_entered",
    "mouse_exited",
    "focus_exited",
    "gui_input",
    "input_event",
    "screen_entered",
    "screen_exited",
    "request_completed",
    "draw",
    "changed",
];
