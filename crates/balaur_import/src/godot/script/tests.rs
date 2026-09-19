use std::collections::BTreeMap;

use super::convert;
use crate::godot::exports::Classes;

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
    assert!(out.rune.contains("pub fn init(this) {"), "{}", out.rune);
    assert!(
        out.rune.contains("pub fn update(this, delta) {"),
        "the hook keeps the name its body reads: {}",
        out.rune
    );
    assert!(
        out.rune
            .contains("pub fn _on_go_pressed(this, force, loud) {"),
        "a signature over three lines, and the handler name a scene points at: {}",
        out.rune
    );
    assert!(
        out.rune.contains("pub fn knots(v) {"),
        "a static function takes no `this`"
    );
    assert!(out.rune.contains("pub fn match_(this, a) {"));
    assert!(out.notes.iter().any(|n| n.contains("`match`")));
}

#[test]
fn exports_carry_their_defaults_and_their_types_fill_the_rest() {
    let out = convert(SHIP, "scripts/ship.gd", &Classes::default());
    assert!(
        out.rune
            .contains("#{ \"speed\": 2.0, \"label\": \"\", \"crew\": 4 }"),
        "{}",
        out.rune
    );
    assert!(
        !out.rune.contains("\"hidden\""),
        "a plain var is not an export"
    );
}

#[test]
fn bodies_are_translated_rather_than_commented() {
    let out = convert(SHIP, "scripts/ship.gd", &Classes::default());
    assert!(
        out.rune.contains(r#"log::info((gd.str_all)(["ahoy"]))"#),
        "{}",
        out.rune
    );
    assert!(
        out.rune
            .contains(r#"(gd.emit_now)(this.node, "sunk", [3]);"#),
        "{}",
        out.rune
    );
    assert!(
        out.rune.contains("return v * 1.94;"),
        "a static body too: {}",
        out.rune
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
        out.rune.contains("this.process_enabled = false;"),
        "{}",
        out.rune
    );
    assert!(
        out.rune
            .contains("    if !this.process_enabled {\n        return;\n    }"),
        "{}",
        out.rune
    );
    assert!(
        out.rune.contains("this.process_enabled = true;"),
        "on by default: {}",
        out.rune
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
        "(gd.create_tween)(panel)",
        "(gd.set_trans)(tween, \"sine\")",
        "(gd.tween_property)(tween, panel, \"modulate:a\", 0.0, 0.2)",
        "(gd.tween_callback)(tween, ",
        "(gd.when_finished)(tween, ",
    ] {
        assert!(out.rune.contains(want), "no `{want}` in:\n{}", out.rune);
    }
    assert!(!out.rune.contains("PORT(gdscript)"), "{}", out.rune);
}

#[test]
fn the_window_scale_and_a_debug_build_have_engine_answers() {
    let source = "extends Node\n\n\
func grow():\n\
\tget_window().content_scale_factor = 2.0\n\
\treturn OS.is_debug_build()\n";
    let out = convert(source, "scripts/a.gd", &Classes::default());
    assert!(out.rune.contains("ui::set_scale(2.0)"), "{}", out.rune);
    assert!(out.rune.contains("engine::platform().dev"), "{}", out.rune);
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
        out.rune.contains("send(this, \"a\", NO_REF, 3)"),
        "{}",
        out.rune
    );
    assert!(
        out.rune.contains("send(this, \"b\", \"r\", 3)"),
        "{}",
        out.rune
    );
}

#[test]
fn an_object_class_new_is_a_table_its_functions_take() {
    let source = "extends RefCounted\nclass_name Machine\n\nvar state = \"idle\"\n\n\
func _init(owner_name, states):\n\
\tstate = owner_name\n\n\
func me():\n\
\treturn self\n";
    let out = convert(source, "scripts/machine.gd", &Classes::default());
    assert!(
        out.rune.contains("return this;"),
        "an object's `self` is its table: {}",
        out.rune
    );
    assert!(
        out.rune.contains("pub fn new(owner_name, states) {")
            && out
                .rune
                .contains("let this = #{ \"__class\": \"scripts/machine.rn\" };")
            && out.rune.contains("_init(this, owner_name, states);"),
        "{}",
        out.rune
    );
}

#[test]
fn a_node_class_new_builds_its_node_and_init_runs_its_init() {
    let source = "extends Label\nclass_name Caption\n\nfunc _init():\n\tpass\n";
    let out = convert(source, "scripts/caption.gd", &Classes::default());
    assert!(
        out.rune.contains("pub fn new() {") && out.rune.contains("(gd.new_node)("),
        "{}",
        out.rune
    );
    assert!(out.rune.contains("\"scripts/caption.rn\")"), "{}", out.rune);
    assert!(
        out.rune.contains("    _init(this);\n"),
        "init runs `_init`: {}",
        out.rune
    );
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
        out.rune
            .contains("(gd.static_ref)(\"scripts/settings.gd:debug_mode\", false)"),
        "{}",
        out.rune
    );
    assert!(
        out.rune
            .contains("(gd.static_set)(\"scripts/settings.gd:debug_mode\", false)"),
        "{}",
        out.rune
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
    };
    let source = "extends Fish\n\nfunc swim(speed):\n\treturn super(speed) * 2\n";
    let out = convert(source, "scripts/shark.gd", &classes);
    std::fs::remove_dir_all(&dir).ok();
    assert!(
        out.rune.contains("pub fn swim__base(this, speed)"),
        "{}",
        out.rune
    );
    assert!(out.rune.contains("swim__base(this, speed)"), "{}", out.rune);
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
        out.rune
            .contains(r#"(gd.static_ref)("scripts/a.gd:_cache", (gd.dict)([]))"#),
        "{}",
        out.rune
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
    assert!(
        out.rune
            .contains(r#"(gd.set_field)(tmp1, "default_font_size", size)"#),
        "{}",
        out.rune
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
        "(gd.clear)(this.node)",
        r#"(gd.invoke2)(this.node, "add_icon_item", icon, label)"#,
        "(gd.option_select)(this.node, 0)",
    ] {
        assert!(out.rune.contains(expected), "{expected}\n{}", out.rune);
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
        out.rune
            .contains(r#"(gd.emit_engine)(box, "pressed", [])"#),
        "{}",
        out.rune
    );
}

#[test]
fn visible_in_tree_asks_the_ancestors_too() {
    let source = "extends Node\n\
\n\
func shown(box):\n\
\treturn box.is_visible_in_tree()\n";
    let out = convert(source, "scripts/a.gd", &Classes::default());
    assert!(out.rune.contains("box.global_visible()"), "{}", out.rune);
}

#[test]
fn a_body_that_calls_the_shim_binds_it_first() {
    let out = convert(SHIP, "scripts/ship.gd", &Classes::default());
    assert!(
        out.rune
            .contains(r#"    let gd = script::require("gd.rn");"#),
        "{}",
        out.rune
    );
}

#[test]
fn a_class_constant_becomes_a_module_constant() {
    let out = convert(SHIP, "scripts/ship.gd", &Classes::default());
    assert!(out.rune.contains("pub const LIMIT = 9;"), "{}", out.rune);
}

#[test]
fn a_member_with_a_value_lands_in_defaults() {
    let out = convert(SHIP, "scripts/ship.gd", &Classes::default());
    assert!(out.rune.contains("this.hidden = 1;"), "{}", out.rune);
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
    assert!(
        out.rune.contains("pub async fn inner(this)"),
        "{}",
        out.rune
    );
    assert!(
        out.rune.contains("pub async fn outer(this)"),
        "{}",
        out.rune
    );
    assert!(out.rune.contains("inner(this).await;"), "{}", out.rune);
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
    assert!(!out.rune.contains("PORT(gdscript)"), "{}", out.rune);
}

#[test]
fn parentheses_the_operators_would_regroup_are_kept() {
    let source = "extends Node\n\
\n\
func skew(a, b):\n\
\treturn (a - b) * 2.0 - (a - (b - 1))\n";
    let out = convert(source, "scripts/skew.gd", &Classes::default());
    assert!(
        out.rune.contains("(a - b) * 2.0 - (a - (b - 1))"),
        "{}",
        out.rune
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
    assert!(!out.rune.contains("PORT(gdscript)"), "{}", out.rune);
    assert!(out.rune.contains("20.0"), "{}", out.rune);
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
        out.rune
            .contains("__set_hp(this, __get_current(this) + 1);"),
        "{}",
        out.rune
    );
    assert!(
        out.rune.contains("pub fn __get_current(this)"),
        "{}",
        out.rune
    );
    assert!(
        out.rune
            .contains("this.hp = (gd.int)(math::clamp(value, 0, 9));"),
        "{}",
        out.rune
    );
    assert!(out.rune.contains("this.hp = 3;"), "{}", out.rune);
    assert!(!out.rune.contains("PORT(gdscript)"), "{}", out.rune);
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
    assert!(out.rune.contains("return __get__data();"), "{}", out.rune);
    assert!(out.rune.contains("pub fn __get__data()"), "{}", out.rune);
    assert!(
        out.rune
            .contains(r#"(gd.static_set)("scripts/meta.gd:_data""#),
        "{}",
        out.rune
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
    assert!(
        out.rune
            .contains(r#"pub const SessionState = #{ "IDLE": 0, "STARTING": 1, "PLAYING": 2 };"#),
        "{}",
        out.rune
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
    assert!(out.rune.contains("scripts/r__Tracker.rn"), "{}", out.rune);
}
