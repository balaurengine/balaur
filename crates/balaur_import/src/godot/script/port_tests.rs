//! The translator's tests for what the Polyglot port found missing: records
//! the shim keeps, signals as values, and the calls Godot makes bare.

use super::convert;
use crate::godot::exports::Classes;

#[test]
fn a_shader_material_a_shape_and_a_theme_override_go_through_the_shim() {
    let source = [
        "extends Control",
        "func _ready():",
        "\tvar m := ShaderMaterial.new()",
        "\tm.shader = Shader.new()",
        "\tm.set_shader_parameter(\"tint\", 1.0)",
        "\tvar c := CircleShape2D.new()",
        "\tadd_theme_color_override(\"font_color\", Color.RED)",
        "\tget_popup().add_theme_constant_override(\"separation\", 4)",
        "",
    ]
    .join("\n");
    let out = convert(&source, "scripts/fx.gd", &Classes::default());
    for want in [
        "(gd.shader_material)()",
        "(gd.shader)()",
        "(gd.set_shader_parameter)(m, \"tint\", 1.0)",
        "(gd.circle_shape)()",
        "(gd.theme_override)(this.node, \"colors\", \"font_color\",",
        "(gd.theme_override)(this.node, \"constants\", \"separation\", 4)",
    ] {
        assert!(out.rune.contains(want), "{want} in\n{}", out.rune);
    }
}

#[test]
fn an_image_loaded_at_run_time_is_the_texture_s_path() {
    let source = "extends Node\n\
func tex(path: String):\n\
\tvar img := Image.new()\n\
\tvar err := img.load(path)\n\
\treturn ImageTexture.create_from_image(img)\n";
    let out = convert(source, "scripts/pic.gd", &Classes::default());
    for want in [
        "(gd.image)()",
        "(gd.image_load)(img, path)",
        "(gd.image_texture)(img)",
    ] {
        assert!(out.rune.contains(want), "{want} in\n{}", out.rune);
    }
}

#[test]
fn a_regex_is_a_record_over_the_engine_s_module() {
    let source = "extends Node\n\
func valid(name: String) -> bool:\n\
\tvar expression := RegEx.new()\n\
\texpression.compile(\"^[a-z]+$\")\n\
\treturn expression.search(name) != null\n";
    let out = convert(source, "scripts/rules.gd", &Classes::default());
    for want in [
        "(gd.regexp)()",
        "(gd.invoke1)(expression, \"compile\", \"^[a-z]+$\")",
        "(gd.invoke1)(expression, \"search\", name)",
    ] {
        assert!(out.rune.contains(want), "{want} in\n{}", out.rune);
    }
}

#[test]
fn a_method_called_deferred_by_name_is_that_call_with_its_arguments() {
    let source = [
        "extends Node",
        "@onready var btn = $Btn",
        "func _ready():",
        "\tadd_child.call_deferred(btn)",
        "\t_refresh.call_deferred(false)",
        "func _refresh(skip: bool = false):",
        "\tpass",
        "",
    ]
    .join("\n");
    let out = convert(&source, "scripts/deferred.gd", &Classes::default());
    for want in [
        "(gd.add_child)(this.node, this.btn)",
        "_refresh(this, false)",
    ] {
        assert!(out.rune.contains(want), "{want} in\n{}", out.rune);
    }
    assert!(!out.rune.contains("call_value"), "{}", out.rune);
}

#[test]
fn a_callable_member_is_called_as_a_value_and_a_deferred_coroutine_makes_its_caller_async() {
    let source = [
        "extends Node",
        "var _is_idle: Callable",
        "static var _provider := Callable()",
        "func _ready():",
        "\t_is_idle.call()",
        "\t_provider.call()",
        "\t_focus_later.call_deferred()",
        "func _focus_later():",
        "\tawait get_tree().process_frame",
        "",
    ]
    .join("\n");
    let out = convert(&source, "scripts/later.gd", &Classes::default());
    assert!(
        out.rune.contains("(gd.call_value)(this._is_idle, [])"),
        "{}",
        out.rune
    );
    assert!(out.rune.contains("pub async fn init(this)"), "{}", out.rune);
    assert!(
        out.rune.contains("_focus_later(this).await"),
        "{}",
        out.rune
    );
    assert!(
        !out.rune.contains("_provider()"),
        "a static Callable is a value:\n{}",
        out.rune
    );
}

#[test]
fn a_base_class_property_read_bare_is_read_off_the_node() {
    let source = [
        "extends FoldableContainer",
        "func same_group(other) -> bool:",
        "\treturn other.foldable_group == foldable_group",
        "",
    ]
    .join("\n");
    let out = convert(&source, "scripts/fold.gd", &Classes::default());
    assert!(
        out.rune
            .contains("(gd.field)(this.node, \"foldable_group\")"),
        "{}",
        out.rune
    );
    assert!(!out.rune.contains("todo"), "{}", out.rune);
}

#[test]
fn a_node_s_auto_translate_mode_and_get_stack_translate() {
    let source = [
        "extends Control",
        "func _ready():",
        "\tauto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED",
        "\tvar frames = get_stack()",
        "",
    ]
    .join("\n");
    let out = convert(&source, "scripts/lang.gd", &Classes::default());
    assert!(!out.rune.contains("AUTO_TRANSLATE"), "{}", out.rune);
    assert!(out.rune.contains("let frames = [];"), "{}", out.rune);
    assert!(!out.rune.contains("todo"), "{}", out.rune);
}

#[test]
fn signals_named_as_strings_and_widget_is_connected_translate() {
    let source = [
        "extends LineEdit",
        "signal saved",
        "func _ready():",
        "\tif not text_changed.is_connected(_on_text_changed):",
        "\t\ttext_changed.connect(_on_text_changed)",
        "\tif has_signal(\"editing_toggled\") and not is_connected(\"editing_toggled\", _on_toggled):",
        "\t\tconnect(\"editing_toggled\", _on_toggled)",
        "\tvar own = has_signal(\"saved\")",
        "\tvar mine = has_signal(\"text_submitted\")",
        "\tvar home = OS.get_environment(\"HOME\")",
        "static func hook(tree):",
        "\ttree.node_added.connect(_on_added)",
        "static func _on_added(node):",
        "\tpass",
        "func _on_text_changed(t):",
        "\tpass",
        "func _on_toggled(on):",
        "\tpass",
        "",
    ]
    .join("\n");
    let out = convert(&source, "scripts/name_edit.gd", &Classes::default());
    for want in [
        "(gd.widget_connected)(this.node, \"on_change\", \"_on_text_changed\")",
        "let own = true;",
        "let mine = this.node.has_component(\"widget\");",
        "(gd.environment)(\"HOME\")",
        "|a0| _on_added(a0)",
    ] {
        assert!(out.rune.contains(want), "{want} in\n{}", out.rune);
    }
    assert!(!out.rune.contains("todo"), "{}", out.rune);
}

#[test]
fn a_multimesh_and_a_transform_built_field_by_field_translate() {
    let source = [
        "extends Node2D",
        "@export var waves: MultiMeshInstance2D",
        "func _ready():",
        "\tvar mm := MultiMesh.new()",
        "\tvar mesh := ArrayMesh.new()",
        "\tmm.mesh = mesh",
        "\twaves.multimesh = mm",
        "\tvar t := Transform2D()",
        "\tt.x = Vector2(2.0, 0.0)",
        "\tt.origin = Vector2(10.0, 20.0)",
        "\tmm.set_instance_transform_2d(0, t)",
        "",
    ]
    .join("\n");
    let out = convert(&source, "scripts/waves.gd", &Classes::default());
    for want in [
        "(gd.multimesh)()",
        "(gd.array_mesh)()",
        "t = (gd.with_field)(t, \"x\", (gd.vec2)(2.0, 0.0));",
        "t = (gd.with_field)(t, \"origin\",",
    ] {
        assert!(out.rune.contains(want), "{want} in\n{}", out.rune);
    }
    assert!(!out.rune.contains("todo"), "{}", out.rune);
}

#[test]
fn a_member_read_and_written_by_name_goes_through_the_shim() {
    let source = [
        "extends Node",
        "var music_db := -6.0",
        "func level(member: String) -> float:",
        "\tset(member, 0.0)",
        "\treturn get(member)",
        "",
    ]
    .join("\n");
    let out = convert(&source, "scripts/sound.gd", &Classes::default());
    for want in [
        "(gd.set_field)(this, member, 0.0)",
        "(gd.field)(this, member)",
    ] {
        assert!(out.rune.contains(want), "{want} in\n{}", out.rune);
    }
    assert!(!out.rune.contains("todo"), "{}", out.rune);
}

#[test]
fn a_signal_handed_over_as_a_value_is_connected_where_it_is_waited_on() {
    let source = [
        "extends Control",
        "signal gate_released(reached: bool)",
        "@export var start_button: Button",
        "@export var anim: Node2D",
        "func play() -> void:",
        "\tif not await _reached(start_button.pressed):",
        "\t\treturn",
        "\tawait _reached(anim.finished)",
        "func _reached(waited: Signal) -> bool:",
        "\tvar forward := func() -> void: gate_released.emit(true)",
        "\twaited.connect(forward, CONNECT_ONE_SHOT)",
        "\tvar reached: bool = await gate_released",
        "\tif waited.is_connected(forward):",
        "\t\twaited.disconnect(forward)",
        "\treturn reached",
        "",
    ]
    .join("\n");
    // `finished` is another class's signal, as InitialAnimation declares it.
    let mut classes = Classes::default();
    classes.signal_arity.insert("finished".into(), 0);
    let out = convert(&source, "scripts/intro.gd", &classes);
    for want in [
        "(gd.signal_value)(this.start_button, \"pressed\", \"on_click\")",
        "(gd.signal_value)(this.anim, \"finished\", \"\")",
        "let forward = || { (gd.emit_now)(this.node, \"gate_released\", [true]) };",
        "pub fn __widget_on_click(this, node)",
    ] {
        assert!(out.rune.contains(want), "{want} in\n{}", out.rune);
    }
    assert!(!out.rune.contains("todo"), "{}", out.rune);
    assert!(!out.rune.contains("PORT("), "{}", out.rune);
}

#[test]
fn singleton_writes_and_theme_reads_translate() {
    let source = [
        "extends MarginContainer",
        "func _ready():",
        "\tEngine.max_fps = 0",
        "\tOS.low_processor_usage_mode = false",
        "\tvar top = get_theme_constant(\"margin_top\")",
        "\tvar content = $Content.get_combined_minimum_size()",
        "",
    ]
    .join("\n");
    let out = convert(&source, "scripts/perf.gd", &Classes::default());
    for want in [
        "settings::set(\"window/max_fps\", 0);",
        "(gd.theme_constant)(this.node, \"margin_top\")",
        "(gd.combined_min_size)(",
    ] {
        assert!(out.rune.contains(want), "{want} in\n{}", out.rune);
    }
    assert!(!out.rune.contains("todo"), "{}", out.rune);
}

#[test]
fn a_member_with_only_a_getter_is_written_where_it_is_kept() {
    let source = [
        "extends RefCounted",
        "var _message : Dictionary = {} : get = to_dictionary",
        "func _init(topic : String):",
        "\t_message = { topic = topic }",
        "func to_dictionary():",
        "\treturn _message",
        "",
    ]
    .join("\n");
    let out = convert(&source, "addons/message.gd", &Classes::default());
    assert!(
        out.rune.contains("this._message = #{ \"topic\": topic };"),
        "{}",
        out.rune
    );
    assert!(!out.rune.contains("__get__message(this) ="), "{}", out.rune);
}

#[test]
fn a_bitwise_not_is_rune_s_bang_on_an_integer() {
    let source = "extends RefCounted\nfunc unzig(n: int) -> int:\n\treturn ~(n >> 1)\n";
    let out = convert(source, "addons/packer.gd", &Classes::default());
    assert!(out.rune.contains("return !(n >> 1);"), "{}", out.rune);
}

#[test]
fn the_javascript_bridge_as_a_value_is_absent() {
    let source = "extends Node\n\
func probe() -> bool:\n\
\tif not JavaScriptBridge:\n\
\t\treturn false\n\
\treturn JavaScriptBridge != null\n";
    let out = convert(source, "scripts/probe.gd", &Classes::default());
    assert!(!out.rune.contains("todo"), "{}", out.rune);
    assert!(out.rune.contains("(gd.truthy)(())"), "{}", out.rune);
}

#[test]
fn a_notification_handler_hears_the_engine_s_focus_suspend_locale_and_quit_hooks() {
    let source = "extends Node\n\
func _notification(what: int) -> void:\n\
\tif what == NOTIFICATION_APPLICATION_FOCUS_OUT:\n\
\t\tprint(\"away\")\n\
\telif what == NOTIFICATION_WM_CLOSE_REQUEST:\n\
\t\tprint(\"closing\")\n";
    let out = convert(source, "scripts/pause.gd", &Classes::default());
    assert!(!out.rune.contains("todo"), "{}", out.rune);
    for want in [
        "(gd.same)(what, 2017)",
        "pub fn on_focused_changed(this, focused) {",
        "let what = if focused { 2016 } else { 2017 };",
        "pub fn on_suspended_changed(this, suspended) {",
        "let what = if suspended { 2015 } else { 2014 };",
        "pub fn on_locale_changed(this, locale) {",
        "let what = 2010;",
        "pub fn on_quit_requested(this) {",
        "let what = 1006;",
    ] {
        assert!(out.rune.contains(want), "{want} in\n{}", out.rune);
    }
}

#[test]
fn another_node_s_method_bound_with_arguments_is_a_record_not_a_read() {
    let source = "extends Node\n\
var progress: Node\n\
var client\n\
func _ready():\n\
\tclient.rejoined.connect(progress.refresh.bind(true, 2))\n";
    let out = convert(source, "scripts/sync.gd", &Classes::default());
    let want = "#{ \"__bound\": this.progress, \"__method\": \"refresh\", \"__args\": [true, 2] }";
    assert!(out.rune.contains(want), "{want} in\n{}", out.rune);
}

#[test]
fn a_named_accessor_reads_and_writes_the_property_s_own_storage() {
    let source = "extends Node\n\
@export var complete := false:\n\
\tset = set_complete\n\
var label := \"\":\n\
\tget = get_label\n\
func set_complete(is_complete: bool) -> void:\n\
\tcomplete = is_complete\n\
func get_label() -> String:\n\
\treturn label\n";
    let out = convert(source, "scripts/city.gd", &Classes::default());
    assert!(
        out.rune.contains("this.complete = is_complete;"),
        "the setter writes the field, not itself: {}",
        out.rune
    );
    assert!(
        !out.rune.contains("__set_complete(this, is_complete)"),
        "{}",
        out.rune
    );
    assert!(
        !out.rune.contains("return __get_label(this)"),
        "the getter reads the field, not itself: {}",
        out.rune
    );
}

#[test]
fn a_signal_the_engine_names_its_own_way_is_heard_by_that_name() {
    let source = "extends Node\n\
func _ready():\n\
\t$Hatch.body_entered.connect(_on_hit)\n\
\t$Coin.screen_exited.connect(_on_gone)\n\
func _on_hit(body):\n\
\tprint(body)\n\
func _on_gone():\n\
\tprint(\"gone\")\n";
    let out = convert(source, "scripts/deck.gd", &Classes::default());
    assert!(!out.rune.contains("todo"), "{}", out.rune);
    for want in [
        "\"collision_enter\"",
        "pub fn on_collision_enter(",
        "\"screen_exit\"",
        "pub fn on_screen_exit(",
    ] {
        assert!(out.rune.contains(want), "{want} in\n{}", out.rune);
    }
    assert!(!out.rune.contains("\"body_entered\""), "{}", out.rune);
}
