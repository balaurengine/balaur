use std::collections::BTreeMap;

use super::convert;
use crate::godot::exports::Classes;

/// Every error compiling `text`, the conversion of `godot`, against the
/// engine: beside the shim, an empty dispatcher, the settings and the
/// callback module, as a converted project links it, and any `extra` files
/// it uses. Boots an engine, so a handful of tests call it, not all.
pub(super) fn compile_errors(godot: &str, text: &str, extra: &[(&str, String)]) -> Vec<String> {
    let path = crate::godot::scene::script_path(godot);
    let mut files: Vec<(&str, String)> = vec![
        ("gd.bs", crate::godot::gdscript::SHIM.to_string()),
        (
            "classes.bs",
            crate::godot::exports::classes_module(&Classes::default(), &[]),
        ),
        (
            "godot_settings.bs",
            "pub fn setting(path: str) -> any {\n    nil\n}\n".into(),
        ),
        (
            "gd_callback.bs",
            crate::godot::gdscript::CALLBACK.to_string(),
        ),
        (path.as_str(), text.to_string()),
    ];
    files.extend(extra.iter().cloned());
    crate::godot::gdscript::check::errors(&files, &path)
}

/// `godot`'s conversion compiles against the engine.
#[track_caller]
pub(super) fn assert_compiles(godot: &str, text: &str, extra: &[(&str, String)]) {
    let errors = compile_errors(godot, text, extra);
    assert!(errors.is_empty(), "{errors:#?}\n{text}");
}

/// The text with every run of whitespace one space, so an assertion over
/// lines does not depend on the formatter's indent.
pub(super) fn flat(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The function that `signature` opens, through its closing brace, flat.
pub(super) fn function(text: &str, signature: &str) -> String {
    let start = text
        .find(signature)
        .unwrap_or_else(|| panic!("no `{signature}` in:\n{text}"));
    let mut depth = 0;
    let mut end = text.len();
    for (at, c) in text[start..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = start + at + 1;
                    break;
                }
            }
            _ => {}
        }
    }
    flat(&text[start..end])
}

const SHIP: &str = "extends Node\n\
class_name Ship\n\
\n\
signal sunk(depth)\n\
@export var speed := 2.0\n\
@export var label: String\n\
@export_range(0, 10) var crew: int = 4\n\
var hidden := 1\n\
const LIMIT = 9\n\
\n\
func _ready() -> void:\n\
\tprint(\"ahoy\")\n\
\n\
func _process(delta: float) -> void:\n\
\tposition.x += speed * delta\n\
\n\
func _on_go_pressed(\n\
\t\tforce: float,\n\
\t\tloud := true) -> void:\n\
\tsunk.emit(3)\n\
\n\
static func knots(v):\n\
\treturn v * 1.94\n\
\n\
func match(a):\n\
\tpass\n";

#[test]
fn hooks_are_renamed_and_every_other_function_keeps_its_name() {
    let out = convert(SHIP, "scripts/ship.gd", &Classes::default());
    assert!(out.text.contains("pub fn init() {"), "{}", out.text);
    assert!(
        out.text.contains("pub fn update(delta: float) {"),
        "the hook keeps the name its body reads: {}",
        out.text
    );
    assert!(
        out.text
            .contains("pub fn _on_go_pressed(force: float, loud_arg: any) {"),
        "a signature over three lines, and the handler name a scene points at: {}",
        out.text
    );
    assert!(
        out.text.contains("\npub fn knots(v: any) -> any {"),
        "a static function is the module's, with no `self`: {}",
        out.text
    );
    assert!(
        out.text.contains("pub fn match(a: any) {"),
        "`match` is no Balaur Script keyword: {}",
        out.text
    );
    assert!(
        !out.notes.iter().any(|n| n.contains("`match`")),
        "{:?}",
        out.notes
    );
}

/// A node class with exports, members, a signal, hooks, a defaulted
/// parameter and a static function compiles against the engine.
#[test]
fn a_node_class_compiles() {
    let out = convert(SHIP, "scripts/ship.gd", &Classes::default());
    assert_compiles("scripts/ship.gd", &out.text, &[]);
}

#[test]
fn exports_carry_their_defaults_and_their_types_fill_the_rest() {
    let out = convert(SHIP, "scripts/ship.gd", &Classes::default());
    for want in [
        "pub crew: int = 4,",
        "pub speed: float = 2.0,",
        "label: str,",
        "self.my.crew = self.crew;",
        "self.my.speed = self.speed;",
        r#"self.my.label = gd::export_value(self.node, "label", self.my.label);"#,
    ] {
        assert!(out.text.contains(want), "no `{want}` in:\n{}", out.text);
    }
    assert!(
        !out.text.contains("pub hidden")
            && !out.text.contains(r#"export_value(self.node, "hidden""#),
        "a plain var is not an export: {}",
        out.text
    );
}

#[test]
fn bodies_are_translated_rather_than_commented() {
    let out = convert(SHIP, "scripts/ship.gd", &Classes::default());
    assert!(
        out.text
            .contains(r#"log::print(gd::str_all(gd::list_of(["ahoy"])))"#),
        "{}",
        out.text
    );
    assert!(
        out.text
            .contains(r#"gd::emit_now(self.node, "sunk", gd::list_of([3]));"#),
        "{}",
        out.text
    );
    assert!(
        out.text.contains("return gd::mul(v, 1.94);"),
        "a static body too: {}",
        out.text
    );
}

#[test]
fn set_process_becomes_a_flag_the_frame_hook_reads() {
    let source = "extends Node\n\
func _process(delta):\n\
\tif delta > 1.0:\n\
\t\tset_process(false)\n";
    let out = convert(source, "scripts/a.gd", &Classes::default());
    assert!(
        out.text.contains("self.my.process_enabled = false;"),
        "{}",
        out.text
    );
    assert!(
        flat(&out.text)
            .contains("pub fn update(delta: float) { if !self.my.process_enabled { return; }"),
        "{}",
        out.text
    );
    assert!(
        out.text.contains("process_enabled: bool = true,"),
        "on by default: {}",
        out.text
    );
}

#[test]
fn a_tween_chain_and_its_finished_lambda_translate() {
    let source = "extends Node\n\n\
func hide(panel):\n\
\tvar tween := panel.create_tween()\n\
\ttween.set_trans(Tween.TRANS_SINE)\n\
\ttween.tween_property(panel, \"modulate:a\", 0.0, 0.2)\n\
\ttween.tween_callback(_done)\n\
\ttween.finished.connect(\n\
\t\tfunc() -> void:\n\
\t\t\tpanel.visible = false\n\
\t)\n\n\
func _done():\n\
\tpass\n";
    let out = convert(source, "scripts/a.gd", &Classes::default());
    for want in [
        "gd::create_tween(panel)",
        "gd::set_trans(tween, \"sine\")",
        "gd::tween_property(tween, panel, \"modulate:a\", 0.0, 0.2)",
        "gd::tween_callback(tween, ",
        "gd::when_finished(tween, ",
    ] {
        assert!(out.text.contains(want), "no `{want}` in:\n{}", out.text);
    }
    assert!(!out.text.contains("PORT(gdscript)"), "{}", out.text);
}

#[test]
fn a_connect_flag_named_on_object_is_its_number() {
    let source = "extends Node\n\n\
func go(button):\n\
\tbutton.ready.connect(_finish.bind(button), Object.CONNECT_ONE_SHOT)\n\n\
func _finish(button):\n\
\tpass\n";
    let out = convert(source, "scripts/a.gd", &Classes::default());
    assert!(!out.text.contains("PORT("), "{}", out.text);
    assert!(
        out.text.contains(
            r#"gd::list_of([gd::callable(self.node, "_finish").with("__args", gd::list_of([button])).with("__takes", 0), 4])"#
        ),
        "{}",
        out.text
    );
}

#[test]
fn a_widget_signal_connected_uses_the_shim_it_patches_through() {
    let source = "extends Button\n\n\
func _ready() -> void:\n\
\tpressed.connect(_on_pressed)\n\n\
func _on_pressed() -> void:\n\
\tpass\n";
    let out = convert(source, "scripts/a.gd", &Classes::default());
    let at = out
        .text
        .find(r#"gd::patch_widget(self.node, map().with("on_click", "_on_pressed"));"#)
        .unwrap_or_else(|| panic!("the connect patches the widget: {}", out.text));
    assert!(out.text[..at].contains("\nuse gd;\n"), "{}", out.text);
}

#[test]
fn the_system_s_dark_mode_is_the_engine_s() {
    let source = "extends Node\n\n\
func dark() -> bool:\n\
\tif DisplayServer.has_method(\"is_dark_mode\"):\n\
\t\treturn DisplayServer.is_dark_mode()\n\
\treturn false\n";
    let out = convert(source, "scripts/a.gd", &Classes::default());
    assert!(out.text.contains("if gd::truthy(true) {"), "{}", out.text);
    assert!(out.text.contains("engine::dark_mode()"), "{}", out.text);
}

#[test]
fn the_window_scale_and_a_debug_build_have_engine_answers() {
    let source = "extends Node\n\n\
func grow():\n\
\tget_window().content_scale_factor = 2.0\n\
\treturn OS.is_debug_build()\n";
    let out = convert(source, "scripts/a.gd", &Classes::default());
    assert!(out.text.contains("ui::set_scale(2.0)"), "{}", out.text);
    assert!(out.text.contains("engine::target().dev"), "{}", out.text);
}

#[test]
fn a_call_leaving_out_a_default_passes_it() {
    let source = "extends Node\n\nconst NO_REF := \"none\"\n\n\
func send(topic, ref := NO_REF, tries: int = 3):\n\
\tpass\n\n\
func go():\n\
\tsend(\"a\")\n\
\tsend(\"b\", \"r\")\n";
    let out = convert(source, "scripts/a.gd", &Classes::default());
    assert!(
        out.text.contains("self.send(\"a\", NO_REF, 3);"),
        "{}",
        out.text
    );
    assert!(
        out.text.contains("self.send(\"b\", \"r\", 3);"),
        "{}",
        out.text
    );
}

#[test]
fn an_object_class_new_is_a_map_its_functions_take() {
    let source = "extends RefCounted\nclass_name Machine\n\nvar state = \"idle\"\n\n\
func _init(owner_name, states):\n\
\tstate = owner_name\n\n\
func me():\n\
\treturn self\n";
    let out = convert(source, "scripts/machine.gd", &Classes::default());
    let text = flat(&out.text);
    assert!(
        text.contains("pub fn me(this: any) -> any { return this; }"),
        "an object's `self` is its map: {}",
        out.text
    );
    assert!(
        text.contains(
            r#"pub fn new(owner_name: any, states: any) -> any { let this: any = map().with("__class", "scripts/machine.bs"); this.set("state", "idle"); _init(this, owner_name, states); return this; }"#
        ),
        "{}",
        out.text
    );
    assert!(
        text.contains(r#"pub fn _init(this: any, owner_name: any, states: any) { this.set("state", owner_name); }"#),
        "a member is the map's key: {}",
        out.text
    );
    assert!(
        text.contains(r#"pub fn __call(this: any, method: str, args: any) -> any {"#)
            && text.contains(r#"if method == "me" { return me(this); }"#),
        "the dispatcher reaches each function by name: {}",
        out.text
    );
    assert_compiles("scripts/machine.gd", &out.text, &[]);
}

#[test]
fn a_node_class_new_builds_its_node_and_init_runs_its_init() {
    let source = "extends Label\nclass_name Caption\n\nfunc _init():\n\tpass\n";
    let out = convert(source, "scripts/caption.gd", &Classes::default());
    assert!(
        out.text.contains("pub fn new() -> Node {") && out.text.contains("gd::new_node("),
        "{}",
        out.text
    );
    assert!(out.text.contains("\"scripts/caption.bs\")"), "{}", out.text);
    assert!(
        flat(&out.text).contains("pub fn init() { self._init(); }"),
        "init runs `_init`: {}",
        out.text
    );
}

#[test]
fn comparing_a_missing_value_with_a_string_answers_false() {
    let source = "extends Node\n\nfunc check(id):\n\treturn id == \"ann\"\n";
    let out = convert(source, "scripts/a.gd", &Classes::default());
    assert!(out.text.contains("gd::same(id, \"ann\")"), "{}", out.text);
}

#[test]
fn godots_hidden_signal_is_the_engines_visibility_event() {
    let source = "extends Control\n\nfunc _ready():\n\thidden.connect(_on_hidden)\n\nfunc _on_hidden():\n\tprint(\"gone\")\n";
    let out = convert(source, "scripts/panel.gd", &Classes::default());
    assert!(
        out.text
            .contains("gd::listen(self.node, \"visibility_changed\", self.node, "),
        "{}",
        out.text
    );
    assert!(
        flat(&out.text).contains(
            "pub fn on_visibility_changed(payload: any) { if gd::truthy(payload) { return; } self._on_hidden(); }"
        ),
        "{}",
        out.text
    );
}

#[test]
fn another_classs_function_handed_over_is_the_function_itself() {
    let classes = Classes {
        files: [("Codec".to_string(), "scripts/codec.gd".to_string())]
            .into_iter()
            .collect(),
        methods: [(
            "Codec".to_string(),
            ["decode".to_string()].into_iter().collect(),
        )]
        .into_iter()
        .collect(),
        ..Classes::default()
    };
    let source = "extends Node\n\nfunc wire():\n\tvar f = Codec.decode\n\treturn f\n";
    let out = convert(source, "scripts/a.gd", &classes);
    assert!(out.text.contains("\nuse scripts::codec;\n"), "{}", out.text);
    assert!(
        out.text.contains("let f: any = codec::decode;"),
        "{}",
        out.text
    );
    assert!(!out.text.contains("gd::field("), "{}", out.text);
}

#[test]
fn another_class_static_var_reads_and_writes_its_store() {
    let classes = Classes {
        files: [("Settings".to_string(), "scripts/settings.gd".to_string())]
            .into_iter()
            .collect(),
        statics: [(
            "Settings".to_string(),
            [("debug_mode".to_string(), "false".to_string())]
                .into_iter()
                .collect(),
        )]
        .into_iter()
        .collect(),
        ..Classes::default()
    };
    let source = "extends Node\n\nfunc toggle():\n\tif Settings.debug_mode:\n\t\tSettings.debug_mode = false\n";
    let out = convert(source, "scripts/a.gd", &classes);
    assert!(
        out.text
            .contains("gd::static_ref(\"scripts/settings.gd:debug_mode\", false)"),
        "{}",
        out.text
    );
    assert!(
        out.text
            .contains("gd::static_set(\"scripts/settings.gd:debug_mode\", false)"),
        "{}",
        out.text
    );
}

#[test]
fn super_reaches_the_base_copy_of_an_overridden_function() {
    let base = "extends Node\nclass_name Fish\n\nfunc swim(speed):\n\treturn speed\n";
    let dir = std::env::temp_dir().join(format!("gdsuper{}", std::process::id()));
    std::fs::create_dir_all(dir.join("scripts")).unwrap();
    std::fs::write(dir.join("scripts/fish.gd"), base).unwrap();
    let classes = Classes {
        bases: BTreeMap::default(),
        files: [("Fish".to_string(), "scripts/fish.gd".to_string())]
            .into_iter()
            .collect(),
        root: dir.clone(),
        statics: BTreeMap::default(),
        inner: BTreeMap::default(),
        defaulted: BTreeMap::default(),
        methods: BTreeMap::default(),
        signal_arity: BTreeMap::default(),
        members: std::collections::BTreeSet::default(),
        autoload_nodes: std::collections::BTreeSet::default(),
        collision_handlers: std::collections::BTreeSet::default(),
    };
    let source = "extends Fish\n\nfunc swim(speed):\n\treturn super(speed) * 2\n";
    let out = convert(source, "scripts/shark.gd", &classes);
    std::fs::remove_dir_all(&dir).ok();
    assert!(
        out.text.contains("pub fn swim__base(speed: any) -> any {"),
        "{}",
        out.text
    );
    assert!(
        out.text
            .contains("return gd::mul(self.swim__base(speed), 2);"),
        "{}",
        out.text
    );
}

#[test]
fn a_static_var_lives_on_the_scene_root() {
    let source = "extends Node\n\
static var _cache := {}\n\
\n\
func seen():\n\
\treturn _cache.size()\n\
\n\
func note(key):\n\
\t_cache[key] = true\n";
    let out = convert(source, "scripts/a.gd", &Classes::default());
    assert!(
        out.text
            .contains(r#"let tmp1: any = gd::static_ref("scripts/a.gd:_cache", map());"#),
        "{}",
        out.text
    );
    // BUG: the store hands out a copy, so a write into the static must go
    // back with `static_set`, and not after the `return` that ends the
    // function: `seen` writes back unreachably, `note` never does.
    let seen = function(&out.text, "pub fn seen() -> any {");
    assert!(
        !seen.contains("return gd::size(tmp1); gd::static_set"),
        "{seen}"
    );
    let note = function(&out.text, "pub fn note(key: any) {");
    assert!(
        note.contains(r#"gd::static_set("scripts/a.gd:_cache", "#),
        "{note}"
    );
}

#[test]
fn a_property_of_a_static_object_is_written_through_the_shim() {
    let source = "extends Node\n\
static var _theme: Theme\n\
\n\
static func grow(size):\n\
\t_theme.default_font_size = size\n";
    let out = convert(source, "scripts/a.gd", &Classes::default());
    let text = flat(&out.text);
    assert!(
        text.contains(
            r#"pub fn grow(size: any) { let tmp1: any = gd::static_ref("scripts/a.gd:_theme", nil); tmp1 = gd::with_field(tmp1, "default_font_size", size); gd::static_set("scripts/a.gd:_theme", tmp1); }"#
        ),
        "{}",
        out.text
    );
}

#[test]
fn an_option_buttons_own_item_verbs_reach_its_node() {
    let source = "extends OptionButton\n\
\n\
func fill(icon, label):\n\
\tclear()\n\
\tadd_icon_item(icon, label)\n\
\tselected = 0\n";
    let out = convert(source, "scripts/a.gd", &Classes::default());
    for expected in [
        "gd::clear(self.node);",
        r#"gd::call(self.node, "add_icon_item", gd::list_of([icon, label]));"#,
        "gd::option_select(self.node, 0);",
    ] {
        assert!(out.text.contains(expected), "{expected}\n{}", out.text);
    }
}

#[test]
fn emitting_another_buttons_pressed_goes_through_the_shim() {
    let source = "extends Node\n\
\n\
func poke(box):\n\
\tbox.button_pressed = true\n\
\tbox.pressed.emit()\n";
    let out = convert(source, "scripts/a.gd", &Classes::default());
    assert!(
        out.text
            .contains(r#"gd::emit_engine(box, "pressed", gd::list());"#),
        "{}",
        out.text
    );
}

#[test]
fn visible_in_tree_asks_the_ancestors_too() {
    let source = "extends Node\n\
\n\
func shown(box):\n\
\treturn box.is_visible_in_tree()\n";
    let out = convert(source, "scripts/a.gd", &Classes::default());
    assert!(
        out.text
            .contains("return gd::node_of(box).visible_in_tree();"),
        "{}",
        out.text
    );
}

#[test]
fn writing_a_vector_lane_writes_the_vector() {
    let source = "extends Node\n\
var velocity := Vector2.ZERO\n\
\n\
func push(other):\n\
\tvar v := Vector2(1, 2)\n\
\tv.x = 3\n\
\tvelocity.y += 1\n";
    let out = convert(source, "scripts/a.gd", &Classes::default());
    for expected in [
        r#"v = gd::with_field(v, "x", 3);"#,
        r#"gd::set_field(self.my.velocity, "y", gd::field(self.my.velocity, "y") + 1);"#,
    ] {
        assert!(out.text.contains(expected), "{expected}\n{}", out.text);
    }
}

#[test]
fn writing_into_another_objects_table_and_emitting_its_signal_go_through_the_shim() {
    let source = "extends Node\n\
\n\
func seed(other):\n\
\tif other.state.is_empty():\n\
\t\tother.state[\"id\"] = \"ro\"\n\
\tother.state_changed.emit(other.state)\n";
    let out = convert(source, "scripts/a.gd", &Classes::default());
    for expected in [
        r#"gd::set(gd::field(other, "state"), "id", "ro");"#,
        r#"gd::emit_engine(other, "state_changed", gd::list_of([gd::field(other, "state")]));"#,
    ] {
        assert!(out.text.contains(expected), "{expected}\n{}", out.text);
    }
}

#[test]
fn a_script_that_calls_the_shim_uses_it_first() {
    let out = convert(SHIP, "scripts/ship.gd", &Classes::default());
    let used = out.text.find("\nuse gd;\n").expect("the shim's `use`");
    let called = out.text.find("gd::").expect("a call into the shim");
    assert!(used < called, "{}", out.text);
}

#[test]
fn a_class_constant_becomes_a_module_constant() {
    let out = convert(SHIP, "scripts/ship.gd", &Classes::default());
    assert!(
        out.text.contains("\npub const LIMIT: int = 9;\n"),
        "{}",
        out.text
    );
}

#[test]
fn a_member_with_a_value_lands_in_defaults() {
    let out = convert(SHIP, "scripts/ship.gd", &Classes::default());
    assert!(
        flat(&out.text)
            .contains("pub class ShipState { process_enabled: bool = true, hidden: int = 1,"),
        "{}",
        out.text
    );
}

#[test]
fn awaiting_makes_a_function_async_and_its_callers_too() {
    let source = "extends Node\n\
func outer():\n\
\tinner()\n\
\n\
func inner():\n\
\tawait ready\n";
    let out = convert(source, "scripts/a.gd", &Classes::default());
    assert!(out.text.contains("pub async fn inner() {"), "{}", out.text);
    assert!(out.text.contains("pub async fn outer() {"), "{}", out.text);
    assert!(out.text.contains("await self.inner();"), "{}", out.text);
    assert!(
        out.text
            .contains(r#"await task::wait(events::next("ready", self.node));"#),
        "{}",
        out.text
    );
    assert_compiles("scripts/a.gd", &out.text, &[]);
}

#[test]
fn a_typed_lambda_bound_to_a_local_is_a_closure() {
    let source = "extends Control\n\
\n\
func fit_to(overlay: Control, heart: Control, margin: float) -> void:\n\
\tvar fit := func() -> void:\n\
\t\tvar half := minf(heart.size.x, heart.size.y) / 2.0 + margin\n\
\t\toverlay.set_anchors_preset(Control.PRESET_CENTER)\n\
\t\toverlay.offset_left = -half\n\
\tfit.call()\n\
\theart.resized.connect(fit)\n";
    let out = convert(source, "scripts/fit.gd", &Classes::default());
    assert!(!out.text.contains("PORT(gdscript)"), "{}", out.text);
}

#[test]
fn parentheses_the_operators_would_regroup_are_kept() {
    let source = "extends Node\n\
\n\
func skew(a: float, b: float):\n\
\treturn (a - b) * 2.0 - (a - (b - 1))\n\
\n\
func loose(a, b):\n\
\treturn (a - b) * 2.0 - (a - (b - 1))\n";
    let out = convert(source, "scripts/skew.gd", &Classes::default());
    assert!(
        out.text.contains("return (a - b) * 2.0 - (a - (b - 1.0));"),
        "typed operands keep the operators: {}",
        out.text
    );
    assert!(
        out.text
            .contains("return gd::sub(gd::mul(gd::sub(a, b), 2.0), gd::sub(a, gd::sub(b, 1)));"),
        "untyped ones nest the shim's calls the same way: {}",
        out.text
    );
}

#[test]
fn a_block_lambda_followed_by_another_argument_translates() {
    let source = "extends Node\n\
\n\
func landed(runner):\n\
\treturn await runner.wait_until(\n\
\t\t\"landed\",\n\
\t\tfunc() -> bool:\n\
\t\t\tvar menu = runner.menu()\n\
\t\t\treturn menu != null,\n\
\t\t20.0\n\
\t)\n";
    let out = convert(source, "scripts/landed.gd", &Classes::default());
    assert!(!out.text.contains("PORT(gdscript)"), "{}", out.text);
    assert!(out.text.contains("20.0"), "{}", out.text);
    // BUG: Balaur Script takes no `return await ..`: the value is awaited
    // into a `let` and that returned.
    assert!(!out.text.contains("return await "), "{}", out.text);
}

#[test]
fn a_property_read_outside_its_getter_calls_the_getter() {
    let source = "extends RefCounted\n\
\n\
var current: int:\n\
\tget:\n\
\t\treturn _current\n\
var hp := 3:\n\
\tset(value):\n\
\t\thp = clampi(value, 0, 9)\n\
var _current: int = 0\n\
\n\
func bump():\n\
\thp = current + 1\n\
\tif 2 not in [1, 3]:\n\
\t\tpass\n";
    let out = convert(source, "scripts/prop.gd", &Classes::default());
    assert!(
        out.text
            .contains("__set_hp(this, gd::add(__get_current(this), 1));"),
        "{}",
        out.text
    );
    assert!(
        out.text
            .contains("pub fn __get_current(this: any) -> any {"),
        "{}",
        out.text
    );
    assert!(
        out.text
            .contains(r#"this.set("hp", gd::int(math::clamp(value, 0, 9)));"#),
        "{}",
        out.text
    );
    assert!(out.text.contains(r#"this.set("hp", 3);"#), "{}", out.text);
    let text = flat(&out.text);
    assert!(
        text.contains(r#"if name == "current" { return __get_current(this); }"#)
            && text.contains(r#"if name == "hp" { __set_hp(this, value); return; }"#),
        "another script's read and write go through the accessors: {}",
        out.text
    );
    assert!(!out.text.contains("PORT(gdscript)"), "{}", out.text);
}

#[test]
fn a_static_property_reads_through_its_getter() {
    let source = "extends Node\n\
static var _data: Dictionary:\n\
\tget():\n\
\t\tif _data.is_empty():\n\
\t\t\t_data = {\"a\": 1}\n\
\t\treturn _data\n\
\n\
static func get_data() -> Dictionary:\n\
\treturn _data\n";
    let out = convert(source, "scripts/meta.gd", &Classes::default());
    assert!(out.text.contains("return __get__data();"), "{}", out.text);
    assert!(out.text.contains("pub fn __get__data()"), "{}", out.text);
    assert!(
        out.text
            .contains(r#"gd::static_set("scripts/meta.gd:_data""#),
        "{}",
        out.text
    );
}

#[test]
fn an_enum_member_documented_with_commas_keeps_its_place() {
    let source = "extends Node\n\
enum SessionState {\n\
\tIDLE,  ## Shown, no game.\n\
\tSTARTING,  ## Pressed start, loading.\n\
\tPLAYING,\n\
}\n";
    let out = convert(source, "scripts/s.gd", &Classes::default());
    for want in [
        "pub const SessionState__IDLE: int = 0;",
        "pub const SessionState__STARTING: int = 1;",
        "pub const SessionState__PLAYING: int = 2;",
        r#"map().with("IDLE", 0).with("STARTING", 1).with("PLAYING", 2)"#,
    ] {
        assert!(out.text.contains(want), "no `{want}` in:\n{}", out.text);
    }
    assert!(
        out.text.contains("pub fn SessionState() -> any {"),
        "{}",
        out.text
    );
}

#[test]
fn an_inner_class_is_a_script_of_its_own() {
    let source = "extends Node\n\
class Tracker:\n\
\tvar seen := 0\n\
\tfunc bump():\n\
\t\tseen += 1\n\
\n\
func make():\n\
\treturn Tracker.new()\n";
    let inner = super::inner_classes(source);
    assert_eq!(inner.len(), 1);
    assert_eq!(inner[0].0, "Tracker");
    assert!(
        inner[0]
            .1
            .starts_with("extends RefCounted\nvar seen := 0\nfunc bump():\n\tseen += 1\n"),
        "{}",
        inner[0].1
    );
    let out = convert(source, "scripts/r.gd", &Classes::default());
    assert!(
        out.text.contains("\nuse scripts::r__Tracker;\n"),
        "{}",
        out.text
    );
    assert!(
        out.text.contains("return r__Tracker::new();"),
        "{}",
        out.text
    );
    // The inner class is an object class of its own, and both compile.
    let file = super::inner_file("scripts/r.gd", "Tracker");
    let (_, text) = &super::inner_scripts(source, "scripts/r.gd")[0];
    let tracker = convert(text, &file, &Classes::default());
    let tracker_path = crate::godot::scene::script_path(&file);
    assert!(
        flat(&tracker.text).contains(
            r#"pub fn bump(this: any) { this.set("seen", gd::add(this.get("seen"), 1)); }"#
        ),
        "{}",
        tracker.text
    );
    assert_compiles(&file, &tracker.text, &[]);
    assert_compiles(
        "scripts/r.gd",
        &out.text,
        &[(tracker_path.as_str(), tracker.text.clone())],
    );
}

#[test]
fn an_inner_class_sees_the_outers_constants_and_its_siblings() {
    let source = "extends RefCounted\n\
enum PB_ERR { NO_ERRORS = 0, TRUNCATED = 1 }\n\
const LIMIT = 9\n\
class Field:\n\
\tvar state := 0\n\
class Packer:\n\
\tconst LIMIT = 3\n\
\tstatic func check(value):\n\
\t\tvar field = Field.new()\n\
\t\treturn value == PB_ERR.NO_ERRORS and field.state < LIMIT\n";
    let inners = super::inner_scripts(source, "proto/pb.gd");
    assert_eq!(inners.len(), 2);
    let packer = &inners[1].1;
    assert!(
        packer.contains("const Field = preload(\"res://proto/pb__Field.gd\")"),
        "{packer}"
    );
    assert!(
        packer.contains("enum PB_ERR { NO_ERRORS = 0, TRUNCATED = 1 }"),
        "{packer}"
    );
    assert_eq!(packer.matches("const LIMIT").count(), 1, "{packer}");
    let out = convert(packer, "proto/pb__Packer.gd", &Classes::default());
    for want in [
        "pub const PB_ERR__NO_ERRORS: int = 0;",
        "pub const PB_ERR__TRUNCATED: int = 1;",
        "pub fn PB_ERR() -> any {",
        "pub const LIMIT: int = 3;",
        "\nuse proto::pb__Field;\n",
        "let field: any = pb__Field::new();",
        "gd::same(value, PB_ERR__NO_ERRORS)",
        r#"gd::lt(gd::field(field, "state"), LIMIT)"#,
    ] {
        assert!(out.text.contains(want), "no `{want}` in:\n{}", out.text);
    }
    assert!(!out.text.contains("PORT("), "{}", out.text);
}

#[test]
fn an_inner_class_is_reached_through_a_preload_of_its_file() {
    let mut classes = Classes::default();
    classes.inner.insert(
        "proto/pb.bs.Field".to_string(),
        "proto/pb__Field.gd".to_string(),
    );
    let source = "extends Node\n\
const PB = preload(\"res://proto/pb.gd\")\n\
func make():\n\
\treturn PB.Field.new()\n";
    let out = convert(source, "scripts/u.gd", &classes);
    assert!(
        out.text.contains("\nuse proto::pb__Field;\n"),
        "{}",
        out.text
    );
    assert!(
        out.text.contains("return pb__Field::new();"),
        "{}",
        out.text
    );
    assert!(!out.text.contains("gd::call("), "{}", out.text);
}

#[test]
fn a_class_inside_an_inner_class_is_reached_from_inside_and_outside() {
    let source = "extends RefCounted\n\
class Msg:\n\
\tclass Part:\n\
\t\tvar n := 0\n\
\tfunc make():\n\
\t\treturn Msg.Part.new()\n";
    let inners = super::inner_scripts(source, "proto/pb.gd");
    let msg = &inners[0].1;
    assert!(
        msg.starts_with("extends RefCounted\nclass_name Msg\n"),
        "{msg}"
    );
    let nested = super::inner_scripts(msg, "proto/pb__Msg.gd");
    assert_eq!(nested[0].0, "Part");
    let mut classes = Classes::default();
    classes
        .inner
        .insert("proto/pb.bs.Msg".into(), "proto/pb__Msg.gd".into());
    classes.inner.insert(
        "proto/pb__Msg.bs.Part".into(),
        "proto/pb__Msg__Part.gd".into(),
    );
    let part = "return pb__Msg__Part::new();";
    let out = convert(msg, "proto/pb__Msg.gd", &classes);
    assert!(out.text.contains(part), "{}", out.text);
    assert!(
        out.text.contains("\nuse proto::pb__Msg__Part;\n"),
        "{}",
        out.text
    );
    let user = "extends Node\n\
const PB = preload(\"res://proto/pb.gd\")\n\
func make():\n\
\treturn PB.Msg.Part.new()\n";
    let out = convert(user, "scripts/u.gd", &classes);
    assert!(out.text.contains(part), "{}", out.text);
    assert!(
        out.text.contains("\nuse proto::pb__Msg__Part;\n"),
        "{}",
        out.text
    );
}

#[test]
fn a_hex_colour_at_the_top_level_hides_none_of_the_declarations_below_it() {
    let source = "extends Node\n\
var tint: Color = Color(\"#ff8a7a\")\n\
var found := false\n\
func mark():\n\
\tfound = true\n";
    let out = convert(source, "scripts/cell.gd", &Classes::default());
    assert!(out.text.contains("self.my.found = true;"), "{}", out.text);
    assert!(
        out.text
            .contains(r##"self.my.tint = gd::color_of("#ff8a7a");"##),
        "{}",
        out.text
    );
    assert!(!out.text.contains("PORT("), "{}", out.text);
}

#[test]
fn another_nodes_method_handed_to_connect_is_bound_rather_than_called() {
    let source = "extends Node\n\
signal changed\n\
var bar\n\
var server\n\
func _ready():\n\
\tchanged.connect(bar.refresh)\n\
\tserver.done.connect(bar.refresh)\n";
    let out = convert(source, "scripts/lobby.gd", &Classes::default());
    let bound = r#"gd::callable(self.my.bar, "refresh")"#;
    assert_eq!(out.text.matches(bound).count(), 2, "{}", out.text);
    assert!(
        out.text.contains(
            r#"gd::connect(self.my.server, "done", gd::callable(self.my.bar, "refresh"));"#
        ),
        "{}",
        out.text
    );
    assert!(
        !out.text.contains(r#"gd::field(self.my.bar, "refresh")"#),
        "{}",
        out.text
    );
}

#[test]
fn an_own_handler_of_a_foreign_signal_takes_its_own_arguments() {
    let source = "extends Node\n\
var machine\n\
func _ready():\n\
\tmachine.state_changed.connect(_on_state)\n\
func _on_state(old, new):\n\
\tpass\n";
    let out = convert(source, "scripts/flow.gd", &Classes::default());
    let record = r#"gd::connect(self.my.machine, "state_changed", gd::callable(self.node, "_on_state").with("__takes", 2));"#;
    assert!(out.text.contains(record), "{}", out.text);
}

#[test]
fn an_input_handler_hangs_off_the_engines_hooks_and_answers_handled() {
    let source = "extends Control\n\
func _input(event: InputEvent) -> void:\n\
\tif event.is_action_pressed(\"ui_cancel\"):\n\
\t\tget_viewport().set_input_as_handled()\n\
func _unhandled_input(event):\n\
\tset_process_input(false)\n";
    let out = convert(source, "scripts/popup.gd", &Classes::default());
    for want in [
        "pub fn on_key_down(key: str) -> bool {",
        "let event: any = gd::key_event(key, true);",
        "self._input(event);",
        "if !gd::input_handled() && !ui::wants_keyboard() {",
        "self._unhandled_input(event);",
        "pub fn on_pointer_down(button: any) -> bool {",
        "!ui::wants_pointer()",
        "return gd::take_input_handled();",
        "gd::set_input_handled()",
        "self.my.input_enabled = false;",
    ] {
        assert!(out.text.contains(want), "no `{want}` in:\n{}", out.text);
    }
    assert_eq!(out.text.matches("pub fn on_").count(), 6, "{}", out.text);
    assert_compiles("scripts/popup.gd", &out.text, &[]);
}

#[test]
fn a_class_that_draws_draws_every_frame_through_the_shim() {
    let pips = [
        "extends Node2D",
        "var count := 3",
        "func _draw() -> void:",
        "\tfor i in count:",
        "\t\tdraw_circle(Vector2(i * 12, 0), 4.0, Color.RED)",
        "func set_count(n):",
        "\tcount = n",
        "\tqueue_redraw()",
        "",
    ]
    .join("\n");
    let out = convert(&pips, "scripts/pips.gd", &Classes::default());
    for want in [
        "pub fn update(dt: float) { gd::draw_frame(self.node); }",
        "gd::draw_circle(self.node, gd::vec2(i * 12, 0), 4.0, gd::color(1.0, 0.0, 0.0, 1.0), nil, nil, nil)",
    ] {
        assert!(
            flat(&out.text).contains(want),
            "no `{want}` in:\n{}",
            out.text
        );
    }
    assert!(
        out.text.contains("gd::queue_redraw(self.node)"),
        "{}",
        out.text
    );
    assert!(!out.text.contains("PORT("), "{}", out.text);
    let lines = [
        "extends Node2D",
        "func _process(delta):",
        "\tpass",
        "func _draw():",
        "\tdraw_line(Vector2.ZERO, Vector2(1, 1), Color.WHITE, 2.0)",
        "",
    ]
    .join("\n");
    let out = convert(&lines, "scripts/lines.gd", &Classes::default());
    assert!(
        flat(&out.text).contains("pub fn update(delta: float) { gd::draw_frame(self.node);"),
        "{}",
        out.text
    );
    assert_eq!(
        out.text.matches("pub fn update(").count(),
        1,
        "{}",
        out.text
    );
}

#[test]
fn a_widget_signal_with_a_bound_handler_goes_through_a_forwarder() {
    let source = "extends Control\n\
var btn\n\
func _ready():\n\
\tbtn.pressed.connect(_on_letter.bind(\"a\"))\n\
func _on_letter(letter):\n\
\tpass\n";
    let out = convert(source, "scripts/pad.gd", &Classes::default());
    assert!(
        out.text.contains(r#"gd::widget_bind(self.my.btn, "on_click", gd::callable(self.node, "_on_letter").with("__args", gd::list_of(["a"])).with("__takes", 0));"#),
        "{}",
        out.text
    );
    assert!(
        flat(&out.text).contains(r#"pub fn __widget_on_click(node: Node) { gd::widget_fire(node, "on_click", gd::list()); }"#),
        "{}",
        out.text
    );
}

#[test]
fn a_callable_held_in_a_variable_connects_as_a_value() {
    let source = "extends Node\n\
var popup\n\
var on_closed: Callable\n\
func _ready():\n\
\tpopup.closed.connect(on_closed)\n";
    let out = convert(source, "scripts/intro.gd", &Classes::default());
    assert!(
        out.text
            .contains(r#"gd::connect(self.my.popup, "closed", self.my.on_closed);"#),
        "{}",
        out.text
    );
}

#[test]
fn a_string_parameter_indexes_by_character() {
    let source = "extends Node\n\
var title: String = \"\"\n\
static func shown(alphabet: String, letters: Array) -> String:\n\
\treturn alphabet[0] + letters[0]\n\
func first():\n\
\treturn title[0]\n\
func label(state: String):\n\
\treturn state[1]\n\
func save(state):\n\
\tstate[\"packet\"] = 1\n\
\tstate[2] = 3\n";
    let out = convert(source, "scripts/words.gd", &Classes::default());
    for want in [
        "return gd::add(gd::at(alphabet, 0), gd::at(letters, 0));",
        "return gd::at(self.my.title, 0);",
        "pub fn label(state: str) -> any {",
        "return gd::at(state, 1);",
        r#"gd::set(state, "packet", 1);"#,
        "gd::set(state, 2, 3);",
    ] {
        assert!(out.text.contains(want), "no `{want}` in:\n{}", out.text);
    }
}

#[test]
fn a_cursor_shape_set_on_a_control_names_the_widget_s_cursor() {
    let source = "extends Control\n\
@onready var name_label = $Name\n\
func _ready():\n\
\tname_label.mouse_default_cursor_shape = Control.CURSOR_POINTING_HAND\n";
    let out = convert(source, "scripts/entry.gd", &Classes::default());
    assert!(
        out.text.contains(
            "gd::patch_widget(self.my.name_label, map().with(\"cursor\", gd::cursor_word(2)));"
        ),
        "{}",
        out.text
    );
}

#[test]
fn a_field_s_placeholder_and_tooltip_reach_the_widget_keys_of_those_names() {
    let source = "extends LineEdit\n\
func _ready():\n\
\tplaceholder_text = \"Name\"\n\
\ttooltip_text = \"Who\"\n\
\tprint(placeholder_text, tooltip_text)\n";
    let out = convert(source, "scripts/field.gd", &Classes::default());
    for wanted in [
        "gd::patch_widget(self.node, map().with(\"placeholder\", \"Name\"));",
        "gd::patch_widget(self.node, map().with(\"tooltip\", \"Who\"));",
        "gd::get(self.node.get_component(\"widget\"), \"placeholder\", nil)",
        "gd::get(self.node.get_component(\"widget\"), \"tooltip\", nil)",
    ] {
        assert!(out.text.contains(wanted), "{wanted} in {}", out.text);
    }
    assert!(!out.text.contains("_text\""), "{}", out.text);
}

#[test]
fn every_cursor_shape_constant_keeps_its_number_under_both_spellings() {
    let source = "extends Control\n\
func _ready():\n\
\tmouse_default_cursor_shape = Control.CURSOR_VSPLIT\n\
\tmouse_default_cursor_shape = CursorShape.CURSOR_HELP\n";
    let out = convert(source, "scripts/seam.gd", &Classes::default());
    assert!(out.text.contains("gd::cursor_word(14)"), "{}", out.text);
    assert!(out.text.contains("gd::cursor_word(16)"), "{}", out.text);
}

#[test]
fn a_mouse_filter_set_from_a_script_says_whether_the_pointer_passes() {
    let source = "extends Control\n\
func _ready():\n\
\tmouse_filter = Control.MOUSE_FILTER_IGNORE\n";
    let out = convert(source, "scripts/veil.gd", &Classes::default());
    assert!(
        out.text
            .contains("gd::patch_widget(self.node, map().with(\"interactive\", 2 != 2));"),
        "{}",
        out.text
    );
}

#[test]
fn a_constant_holding_a_triple_quoted_string_over_lines_is_one_declaration() {
    let source = [
        "extends Node",
        "class Plugin:",
        "\tconst HEADER: String = \"\"\"<?xml version=\"1.0\"?>",
        "\t<plist version=\"1.0\">",
        "\t<array>\\n\"\"\"",
        "\tconst SUFFIX: String = \".ipa\"",
        "\tfunc write(file):",
        "\t\tfile.store_string(HEADER + SUFFIX)",
        "",
    ]
    .join("\n");
    let inner = super::inner_scripts(&source, "addons/deeplink.gd");
    let (name, text) = inner
        .iter()
        .find(|(name, _)| name == "Plugin")
        .expect("the inner class's file");
    let file = super::inner_file("addons/deeplink.gd", name);
    let out = convert(text, &file, &Classes::default()).text;
    assert_eq!(out.matches("HEADER() -> any").count(), 1, "{out}");
    assert!(
        flat(&out).contains(
            r#"pub fn HEADER() -> any { "<?xml version=\"1.0\"?>\n<plist version=\"1.0\">\n<array>\n" }"#
        ),
        "{out}"
    );
    assert!(out.contains("pub const SUFFIX: str = \".ipa\";"), "{out}");
    assert!(out.contains("gd::add(HEADER(), SUFFIX)"), "{out}");
    assert!(!out.contains("PORT("), "{out}");
}
