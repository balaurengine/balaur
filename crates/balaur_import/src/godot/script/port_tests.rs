//! The translator's tests for what the Polyglot port found missing: records
//! the shim keeps, signals as values, and the calls Godot makes bare.

use super::convert;
use super::tests::{assert_compiles, flat, function};
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
        "gd::shader_material()",
        "gd::shader()",
        "gd::set_shader_parameter(m, \"tint\", 1.0)",
        "gd::circle_shape()",
        "m = gd::with_field(m, \"shader\", gd::shader());",
        "gd::theme_override(self.node, \"colors\", \"font_color\", gd::color(1.0, 0.0, 0.0, 1.0));",
        "gd::theme_override(self.node, \"constants\", \"separation\", 4);",
    ] {
        assert!(out.text.contains(want), "{want} in\n{}", out.text);
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
        "gd::image()",
        "gd::image_load(img, path)",
        "gd::image_texture(img)",
    ] {
        assert!(out.text.contains(want), "{want} in\n{}", out.text);
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
        "gd::regexp()",
        "let expression: any = gd::regexp();",
        "gd::call(expression, \"compile\", gd::list_of([\"^[a-z]+$\"]));",
        "return !gd::is_nil(gd::call(expression, \"search\", gd::list_of([name])));",
    ] {
        assert!(out.text.contains(want), "{want} in\n{}", out.text);
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
        "gd::add_child(self.node, self.my.btn);",
        "self._refresh(false);",
    ] {
        assert!(out.text.contains(want), "{want} in\n{}", out.text);
    }
    assert!(!out.text.contains("call_value"), "{}", out.text);
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
        out.text
            .contains("gd::call_value(self.my._is_idle, gd::list());"),
        "{}",
        out.text
    );
    assert!(out.text.contains("pub async fn init() {"), "{}", out.text);
    assert!(
        out.text.contains("await self._focus_later();"),
        "{}",
        out.text
    );
    assert!(
        flat(&out.text).contains(
            r#"let tmp1: any = gd::static_ref("scripts/later.gd:_provider", nil); gd::call_value(tmp1, gd::list());"#
        ) && !out.text.contains("_provider()"),
        "a static Callable is a value:\n{}",
        out.text
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
        out.text.contains(
            "return gd::same(gd::field(other, \"foldable_group\"), gd::field(self.node, \"foldable_group\"));"
        ),
        "{}",
        out.text
    );
    assert!(!out.text.contains("PORT("), "{}", out.text);
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
    assert!(!out.text.contains("AUTO_TRANSLATE"), "{}", out.text);
    assert!(
        out.text
            .contains("gd::set_field(self.node, \"auto_translate_mode\", 2);"),
        "{}",
        out.text
    );
    assert!(
        out.text.contains("let frames: any = gd::list();"),
        "{}",
        out.text
    );
    assert!(!out.text.contains("PORT("), "{}", out.text);
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
        "if !gd::truthy(gd::widget_connected(self.node, \"on_change\", \"_on_text_changed\")) {",
        "gd::patch_widget(self.node, map().with(\"on_change\", \"_on_text_changed\"));",
        "gd::connect(self.node, \"editing_toggled\", gd::callable(self.node, \"_on_toggled\").with(\"__takes\", 1));",
        "let own: any = true;",
        "let mine: any = self.node.has_component(\"widget\");",
        "let home: any = gd::environment(\"HOME\");",
    ] {
        assert!(out.text.contains(want), "{want} in\n{}", out.text);
    }
    // BUG: a static function is the module's, where there is no `self`: its
    // handler is the module function `_on_added`, not a callable on a node.
    let hook = function(&out.text, "\npub fn hook(tree: any) {");
    assert!(
        hook.contains("\"node_added\"") && hook.contains("_on_added") && !hook.contains("self."),
        "{hook}"
    );
    assert!(!out.text.contains("PORT("), "{}", out.text);
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
        "gd::multimesh()",
        "gd::array_mesh()",
        "t = gd::with_field(t, \"x\", gd::vec2(2.0, 0.0));",
        "t = gd::with_field(t, \"origin\",",
    ] {
        assert!(out.text.contains(want), "{want} in\n{}", out.text);
    }
    assert!(!out.text.contains("PORT("), "{}", out.text);
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
    // BUG: a node class has no `this`; Godot's `set`/`get` on itself go
    // through the node, whose `__set`/`__get` answer for its members.
    for want in [
        "gd::set_field(self.node, member, 0.0);",
        "return gd::field(self.node, member);",
    ] {
        assert!(out.text.contains(want), "{want} in\n{}", out.text);
    }
    assert!(!out.text.contains("PORT("), "{}", out.text);
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
        "gd::signal_value(self.my.start_button, \"pressed\", \"on_click\")",
        "await self._reached(gd::signal_value(self.my.anim, \"finished\", \"\"));",
        "let forward: any = gd::lambda(self.node, \"__lambda_reached_0\", map());",
        "gd::call(waited, \"connect\", gd::list_of([forward, 4]));",
        "let tmp1: any = await task::wait(events::next(\"gate_released\", self.node));",
        "let reached: bool = tmp1;",
        "pub fn __widget_on_click(node: Node) {",
    ] {
        assert!(out.text.contains(want), "{want} in\n{}", out.text);
    }
    assert!(
        function(&out.text, "pub fn __lambda_reached_0(cap: any) {")
            .contains("gd::emit_now(self.node, \"gate_released\", gd::list_of([true]));"),
        "the lambda is lifted into a method: {}",
        out.text
    );
    assert!(!out.text.contains("PORT("), "{}", out.text);
    // BUG: `if not await f(..)` awaits inside a condition, which Balaur
    // Script refuses; the await goes in a `let` first.
    assert!(!out.text.contains("gd::truthy(await "), "{}", out.text);
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
        "let top: any = gd::theme_constant(self.node, \"margin_top\");",
        "let content: any = gd::combined_min_size(self.node.get_node(\"Content\"));",
    ] {
        assert!(out.text.contains(want), "{want} in\n{}", out.text);
    }
    assert!(!out.text.contains("PORT("), "{}", out.text);
    // BUG: a write to a singleton property the engine has no answer for is a
    // bare `nil;` statement, which does not compile; it should be left out.
    assert!(!flat(&out.text).contains(" nil; "), "{}", out.text);
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
        out.text
            .contains(r#"this.set("_message", map().with("topic", topic));"#),
        "{}",
        out.text
    );
    assert!(
        flat(&out.text)
            .contains("pub fn __get__message(this: any) -> any { return to_dictionary(this); }"),
        "a read elsewhere goes through the getter: {}",
        out.text
    );
    assert!(!out.text.contains("__get__message(this) ="), "{}", out.text);
}

#[test]
fn a_bitwise_not_and_a_shift_on_an_integer_go_through_the_shim() {
    let source = "extends RefCounted\nfunc unzig(n: int) -> int:\n\treturn ~(n >> 1)\n";
    let out = convert(source, "addons/packer.gd", &Classes::default());
    let unzig = function(&out.text, "pub fn unzig(this: any, n: int) -> int {");
    assert!(
        unzig.contains("return gd::bit_not(") && unzig.contains("gd::shift_right(n, 1)"),
        "{}",
        out.text
    );
}

#[test]
fn the_javascript_bridge_as_a_value_is_absent() {
    let source = "extends Node\n\
func probe() -> bool:\n\
\tif not JavaScriptBridge:\n\
\t\treturn false\n\
\treturn JavaScriptBridge != null\n";
    let out = convert(source, "scripts/probe.gd", &Classes::default());
    assert!(!out.text.contains("PORT("), "{}", out.text);
    assert!(out.text.contains("if !gd::truthy(nil) {"), "{}", out.text);
    assert!(
        out.text.contains("return !gd::is_nil(nil);"),
        "{}",
        out.text
    );
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
    assert!(!out.text.contains("PORT("), "{}", out.text);
    for want in [
        "gd::same(what, 2017)",
        "pub fn on_focused_changed(focused: bool) {",
        "let what: int = if focused { 2016 } else { 2017 };",
        "pub fn on_suspended_changed(suspended: bool) {",
        "let what: int = if suspended { 2015 } else { 2014 };",
        "pub fn on_locale_changed(locale: str) {",
        "let what: int = 2010;",
        "pub fn on_quit_requested() {",
        "let what: int = 1006;",
    ] {
        assert!(out.text.contains(want), "{want} in\n{}", out.text);
    }
    assert_eq!(
        out.text.matches("self._notification(what);").count(),
        4,
        "{}",
        out.text
    );
    assert_compiles("scripts/pause.gd", &out.text, &[]);
}

#[test]
fn another_node_s_method_bound_with_arguments_is_a_record_not_a_read() {
    let source = "extends Node\n\
var progress: Node\n\
var client\n\
func _ready():\n\
\tclient.rejoined.connect(progress.refresh.bind(true, 2))\n";
    let out = convert(source, "scripts/sync.gd", &Classes::default());
    let want = r#"gd::connect(self.my.client, "rejoined", gd::callable(self.my.progress, "refresh").with("__args", gd::list_of([true, 2])));"#;
    assert!(out.text.contains(want), "{want} in\n{}", out.text);
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
        function(&out.text, "pub fn set_complete(is_complete: bool) {")
            == "pub fn set_complete(is_complete: bool) { self.my.complete = is_complete; }",
        "the setter writes the field, not itself: {}",
        out.text
    );
    assert!(
        function(&out.text, "pub fn get_label() -> str {")
            == "pub fn get_label() -> str { return self.my.label; }",
        "the getter reads the field, not itself: {}",
        out.text
    );
    assert!(
        flat(&out.text).contains("pub fn __set_complete(value: any) { self.set_complete(value); }"),
        "another write goes through the setter: {}",
        out.text
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
    assert!(!out.text.contains("PORT("), "{}", out.text);
    assert!(!out.text.contains("\"body_entered\""), "{}", out.text);
    // BUG: the engine's own name for the signal is no `ENGINE_SIGNALS` entry,
    // so the connect is a script signal's `gd::connect`, which the engine's
    // event never reaches, and no `on_<event>` forwarder is written.
    for want in [
        "gd::listen(self.node, \"collision_enter\", self.node.get_node(\"Hatch\"), ",
        "pub fn on_collision_enter(",
        "gd::listen(self.node, \"screen_exit\", self.node.get_node(\"Coin\"), ",
        "pub fn on_screen_exit(",
    ] {
        assert!(out.text.contains(want), "{want} in\n{}", out.text);
    }
}

/// The engine hands a collision a record; a Godot handler is handed the
/// other node out of it, whether a script or a scene connected it.
#[test]
fn a_collision_handler_takes_the_other_node_out_of_the_record() {
    let source = "extends Node\n\
func _ready():\n\
\t$Dock.body_entered.connect(_on_hit)\n\
func _on_hit(body):\n\
\tprint(body)\n\
func _on_dock(ship):\n\
\tprint(ship)\n";
    let mut classes = Classes::default();
    classes.collision_handlers.insert("_on_dock".into());
    let out = convert(source, "scripts/dock.gd", &classes);
    assert!(
        function(&out.text, "pub fn _on_dock(ship: any) {").contains("ship = gd::collided(ship);"),
        "a handler a scene connects takes the node out first: {}",
        out.text
    );
    assert!(
        !out.text.contains("body = gd::collided(body)"),
        "a handler no scene connects is left alone: {}",
        out.text
    );
    // BUG: as in `a_signal_the_engine_names_its_own_way_is_heard_by_that_name`,
    // a script's connect to the collision is no forwarder that hands the
    // handler the node.
    assert!(
        out.text.contains("self._on_hit(gd::collided(payload));"),
        "{}",
        out.text
    );
}

#[test]
fn a_scene_names_the_methods_it_aims_a_collision_at() {
    let scene = "[node name=\"Dock\" type=\"Node\"]\n\
[connection signal=\"body_entered\" from=\"Dock\" to=\".\" method=\"_on_dock\"]\n\
[connection signal=\"area_exited\" from=\"Dock\" to=\".\" method=\"_on_leave\"]\n\
[connection signal=\"pressed\" from=\"Go\" to=\".\" method=\"_on_go\"]\n";
    assert_eq!(
        crate::godot::exports::collision_handlers(scene),
        ["_on_dock", "_on_leave"]
    );
}
