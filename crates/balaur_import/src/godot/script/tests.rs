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
fn comparing_a_missing_value_with_a_string_answers_false() {
    let source = "extends Node\n\nfunc check(id):\n\treturn id == \"ann\"\n";
    let out = convert(source, "scripts/a.gd", &Classes::default());
    assert!(out.rune.contains("(gd.same)(id, \"ann\")"), "{}", out.rune);
}

#[test]
fn godots_hidden_signal_is_the_engines_visibility_event() {
    let source = "extends Control\n\nfunc _ready():\n\thidden.connect(_on_hidden)\n\nfunc _on_hidden():\n\tprint(\"gone\")\n";
    let out = convert(source, "scripts/panel.gd", &Classes::default());
    assert!(
        out.rune
            .contains("(gd.listen)(this.node, \"visibility_changed\""),
        "{}",
        out.rune
    );
    assert!(
        out.rune
            .contains("pub fn on_visibility_changed(this, payload) {\n    if payload {"),
        "{}",
        out.rune
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
    assert!(
        out.rune
            .contains("script::require(\"scripts/codec.rn\").decode"),
        "{}",
        out.rune
    );
    assert!(!out.rune.contains("gd.constant"), "{}", out.rune);
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
        methods: BTreeMap::default(),
        signal_arity: BTreeMap::default(),
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
        out.rune.contains(r#"(gd.emit_engine)(box, "pressed", [])"#),
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
        r#"(gd.set_field)(v, "x", 3)"#,
        r#"(gd.set_field)(this.velocity, "y", (gd.field)(this.velocity, "y") + 1)"#,
    ] {
        assert!(out.rune.contains(expected), "{expected}\n{}", out.rune);
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
        r#"let _ = (gd.set)((gd.field)(other, "state"), "id", "ro");"#,
        r#"(gd.emit_engine)(other, "state_changed", [(gd.field)(other, "state")])"#,
    ] {
        assert!(out.rune.contains(expected), "{expected}\n{}", out.rune);
    }
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
    assert!(
        out.rune
            .contains(r#"pub const PB_ERR = #{ "NO_ERRORS": 0, "TRUNCATED": 1 };"#),
        "{}",
        out.rune
    );
    assert!(out.rune.contains("pub const LIMIT = 3;"), "{}", out.rune);
    assert!(out.rune.contains("proto/pb__Field.rn"), "{}", out.rune);
    assert!(!out.rune.contains("gd.todo"), "{}", out.rune);
}

#[test]
fn an_inner_class_is_reached_through_a_preload_of_its_file() {
    let mut classes = Classes::default();
    classes.inner.insert(
        "proto/pb.rn.Field".to_string(),
        "proto/pb__Field.gd".to_string(),
    );
    let source = "extends Node\n\
const PB = preload(\"res://proto/pb.gd\")\n\
func make():\n\
\treturn PB.Field.new()\n";
    let out = convert(source, "scripts/u.gd", &classes);
    assert!(
        out.rune
            .contains(r#"script::require("proto/pb__Field.rn").new"#),
        "{}",
        out.rune
    );
    assert!(!out.rune.contains("gd.invoke"), "{}", out.rune);
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
        .insert("proto/pb.rn.Msg".into(), "proto/pb__Msg.gd".into());
    classes.inner.insert(
        "proto/pb__Msg.rn.Part".into(),
        "proto/pb__Msg__Part.gd".into(),
    );
    let part = r#"script::require("proto/pb__Msg__Part.rn").new"#;
    let out = convert(msg, "proto/pb__Msg.gd", &classes);
    assert!(out.rune.contains(part), "{}", out.rune);
    let user = "extends Node\n\
const PB = preload(\"res://proto/pb.gd\")\n\
func make():\n\
\treturn PB.Msg.Part.new()\n";
    let out = convert(user, "scripts/u.gd", &classes);
    assert!(out.rune.contains(part), "{}", out.rune);
}

#[test]
fn a_hex_colour_at_the_top_level_hides_none_of_the_declarations_below_it() {
    let source = "extends Node\n\
var tint: Color = Color(\"#ff8a7a\")\n\
var found := false\n\
func mark():\n\
\tfound = true\n";
    let out = convert(source, "scripts/cell.gd", &Classes::default());
    assert!(out.rune.contains("this.found = true;"), "{}", out.rune);
    assert!(!out.rune.contains("gd.todo"), "{}", out.rune);
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
    let bound = r#"#{ "__bound": this.bar, "__method": "refresh" }"#;
    assert_eq!(out.rune.matches(bound).count(), 2, "{}", out.rune);
    assert!(
        !out.rune.contains(r#"(gd.field)(this.bar, "refresh")"#),
        "{}",
        out.rune
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
    let record = r#"#{ "__call": { |arg0, arg1| { _on_state(this, arg0, arg1) } }, "__takes": 2 }"#;
    assert!(out.rune.contains(record), "{}", out.rune);
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
        "pub fn on_key_down(this, key) {",
        "let event = (gd.key_event)(key, true);",
        r#"let _ = (gd.invoke1)(this.node, "_input", event);"#,
        "if !(gd.input_handled)() && !ui::wants_keyboard() {",
        "pub fn on_pointer_down(this, button) {",
        "!ui::wants_pointer()",
        "return (gd.take_input_handled)();",
        "(gd.set_input_handled)()",
        "this.input_enabled = false;",
    ] {
        assert!(out.rune.contains(want), "no `{want}` in:\n{}", out.rune);
    }
    assert_eq!(out.rune.matches("pub fn on_").count(), 6, "{}", out.rune);
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
        "pub fn update(this, dt) {\n    let _ = (script::require(\"gd.rn\").draw_frame)(this.node);\n}",
        "(gd.draw_circle)(this.node, (gd.vec2)(i * 12, 0), 4.0, (gd.color)(1.0, 0.0, 0.0, 1.0), (), (), ())",
    ] {
        assert!(out.rune.contains(want), "no `{want}` in:\n{}", out.rune);
    }
    assert!(
        out.rune.contains("(gd.queue_redraw)(this.node)"),
        "{}",
        out.rune
    );
    assert!(!out.rune.contains("gd.todo"), "{}", out.rune);
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
        out.rune.contains(
            "pub fn update(this, delta) {\n    let _ = (script::require(\"gd.rn\").draw_frame)(this.node);"
        ),
        "{}",
        out.rune
    );
    assert_eq!(
        out.rune.matches("pub fn update(").count(),
        1,
        "{}",
        out.rune
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
        out.rune.contains(r#"(gd.widget_bind)(this.btn, "on_click", #{ "__call": { let tmp1 = "a"; || { _on_letter(this, tmp1) } }, "__takes": 0 })"#),
        "{}",
        out.rune
    );
    assert!(
        out.rune.contains("pub fn __widget_on_click(this, node) {\n    let _ = (script::require(\"gd.rn\").widget_fire)(node, \"on_click\", []);\n}"),
        "{}",
        out.rune
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
        out.rune
            .contains(r#"(gd.connect)(this.popup, "closed", this.on_closed)"#),
        "{}",
        out.rune
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
    assert!(out.rune.contains("(gd.at)(alphabet, 0)"), "{}", out.rune);
    assert!(out.rune.contains("letters[0]"), "{}", out.rune);
    assert!(out.rune.contains("(gd.at)(this.title, 0)"), "{}", out.rune);
    assert!(out.rune.contains("(gd.at)(state, 1)"), "{}", out.rune);
    assert!(out.rune.contains("state[\"packet\"] = 1;"), "{}", out.rune);
    assert!(out.rune.contains("state[2] = 3;"), "{}", out.rune);
}

#[test]
fn a_cursor_shape_set_on_a_control_names_the_widget_s_cursor() {
    let source = "extends Control\n\
@onready var name_label = $Name\n\
func _ready():\n\
\tname_label.mouse_default_cursor_shape = Control.CURSOR_POINTING_HAND\n";
    let out = convert(source, "scripts/entry.gd", &Classes::default());
    assert!(
        out.rune
            .contains("patch_component(\"widget\", #{ \"cursor\": (gd.cursor_word)(2) })"),
        "{}",
        out.rune
    );
}

#[test]
fn every_cursor_shape_constant_keeps_its_number_under_both_spellings() {
    let source = "extends Control\n\
func _ready():\n\
\tmouse_default_cursor_shape = Control.CURSOR_VSPLIT\n\
\tmouse_default_cursor_shape = CursorShape.CURSOR_HELP\n";
    let out = convert(source, "scripts/seam.gd", &Classes::default());
    assert!(out.rune.contains("(gd.cursor_word)(14)"), "{}", out.rune);
    assert!(out.rune.contains("(gd.cursor_word)(16)"), "{}", out.rune);
}

#[test]
fn a_mouse_filter_set_from_a_script_says_whether_the_pointer_passes() {
    let source = "extends Control\n\
func _ready():\n\
\tmouse_filter = Control.MOUSE_FILTER_IGNORE\n";
    let out = convert(source, "scripts/veil.gd", &Classes::default());
    assert!(
        out.rune
            .contains("patch_component(\"widget\", #{ \"pointer_through\": 2 == 2 })"),
        "{}",
        out.rune
    );
}

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
        "func _ready():",
        "\t_is_idle.call()",
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
}
