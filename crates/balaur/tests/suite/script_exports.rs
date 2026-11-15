//! Every `pub` field of a script is a property, whatever its type: listed with
//! a spec in the component schema's words, and set from a scene.

use balaur::{App, AppConfig, standard_app};
use balaur_script::Value;

const TUNED: &str = r#"
pub enum Mode { Calm, Angry }
pub enum Shape { Circle(float), Rect(float, float), Empty }

pub struct Spot {
    pub x: int,
    pub label: str,
    pub tally: {str: int},
}

pub class Tuned {
    node: Node,
    /// How fast it goes.
    pub speed: float = 2.0,
    pub count: int,
    pub name: str,
    pub on: bool,
    pub target: Node,
    pub marks: [Node],
    pub at: Vec3,
    pub tint: Color,
    pub steps: [int],
    pub scores: {str: int},
    pub spot: Spot,
    pub mode: Mode,
    pub shape: Shape,
    pub maybe: float?,
    pub moves: Transform2d,
    pub rows: [{int: str}],
    internal hidden: int,
    kept: int,
}

impl Tuned {
    pub fn report() -> str {
        let mode = match self.mode {
            Mode::Calm => "calm",
            Mode::Angry => "angry",
        };
        let shape = match self.shape {
            Shape::Circle(r) => "circle {r as int}",
            Shape::Rect(w, h) => "rect {w as int}x{h as int}",
            Shape::Empty => "empty",
        };
        let maybe = if self.maybe == nil { -1 } else { self.maybe.unwrap() as int };
        return "{self.count} {self.name} {self.on} {self.target.name()} {self.marks.len()} {self.at.y as int} {(self.tint.g * 10.0) as int} {self.steps[2]} {self.scores["bo"]} {self.scores.len()} {self.spot.x} {self.spot.label} {mode} {shape} {maybe}";
    }

    pub fn nested() -> str {
        let tally = self.spot.tally;
        let row = self.rows[1];
        return "{tally.len()} {tally["k"]} {tally.has("a")} {tally.has("z")} {row[2]} {row.len()} {self.moves.origin.x as int} {self.moves.y.y as int} {self.kept}";
    }

    pub fn unmarked() -> int {
        return self.kept + self.hidden;
    }

    // A table a scene made takes a key as a literal's does.
    pub fn grown() -> str {
        let tally = self.spot.tally;
        tally["z"] = 26;
        tally["a"] = 100;
        return "{tally.len()} {tally["z"]} {tally["a"]} {tally.keys()[0]}";
    }
}
"#;

fn booted(props: &str) -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    let scene = format!(
        "[[nodes]]\nid = \"world\"\nname = \"World\"\n\n\
         [[nodes]]\nid = \"enemy\"\nname = \"Enemy\"\nparent = \"world\"\n\n\
         [[nodes]]\nid = \"flag\"\nname = \"Flag\"\nparent = \"world\"\n\n\
         [[nodes]]\nid = \"tuned\"\nname = \"Tuned\"\nparent = \"world\"\n\n\
         [nodes.script]\nsource = \"scripts/tuned.bs\"\n\n\
         [nodes.script.props]\n{props}\n"
    );
    for (path, text) in [
        (
            "project.toml",
            "[application]\nname = \"script-exports\"\nmain_scene = \"main.toml\"\n",
        ),
        ("main.toml", scene.as_str()),
        ("scripts/tuned.bs", TUNED),
    ] {
        let at = dir.path().join(path);
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::write(at, text).unwrap();
    }
    let mut config = AppConfig::dev(dir.path().to_string_lossy().as_ref());
    config.watch = false;
    let mut app = standard_app(config).unwrap();
    app.load_project().unwrap();
    (dir, app)
}

fn at<'a>(map: &'a Value, key: &str) -> &'a Value {
    match map {
        Value::Map(pairs) => pairs
            .iter()
            .find(|(k, _)| k == key)
            .map_or_else(|| panic!("no `{key}` in {map:?}"), |(_, v)| v),
        other => panic!("not a map: {other:?}"),
    }
}

fn text(value: &Value) -> &str {
    match value {
        Value::Str(text) => text,
        other => panic!("not a string: {other:?}"),
    }
}

#[test]
fn every_pub_field_is_listed_with_the_spec_its_type_draws() {
    let (_dir, app) = booted("");
    let exports = app
        .engine
        .script_host()
        .unwrap()
        .exports("scripts/tuned.bs")
        .unwrap();
    let names: Vec<&str> = exports.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        names,
        [
            "speed", "count", "name", "on", "target", "marks", "at", "tint", "steps", "scores",
            "spot", "mode", "shape", "maybe", "moves", "rows"
        ],
        "every pub field in its order, the hidden ones left out"
    );
    let spec = |name: &str| &exports.iter().find(|(n, _)| n == name).unwrap().1;
    let kind = |name: &str| text(at(spec(name), "type")).to_string();
    for (name, wanted) in [
        ("speed", "float"),
        ("count", "int"),
        ("name", "string"),
        ("on", "bool"),
        ("target", "node"),
        ("marks", "list"),
        ("at", "vec3"),
        ("tint", "color"),
        ("steps", "list"),
        ("scores", "map"),
        ("spot", "record"),
        ("mode", "enum"),
        ("shape", "variant"),
        ("maybe", "float"),
        ("moves", "record"),
        ("rows", "list"),
    ] {
        assert_eq!(kind(name), wanted, "{name}");
    }
    assert_eq!(at(spec("speed"), "default"), &Value::Num(2.0));
    assert_eq!(text(at(spec("speed"), "description")), "How fast it goes.");
    assert_eq!(at(spec("count"), "default"), &Value::Int(0));
    assert_eq!(text(at(at(spec("marks"), "of"), "type")), "node");
    assert_eq!(text(at(at(spec("scores"), "of"), "type")), "int");
    assert_eq!(
        text(at(at(at(spec("spot"), "fields"), "label"), "type")),
        "string"
    );
    assert_eq!(
        at(spec("mode"), "options"),
        &Value::List(vec![Value::Str("Calm".into()), Value::Str("Angry".into())])
    );
    let rect = at(at(spec("shape"), "variants"), "Rect");
    assert_eq!(text(at(at(at(rect, "fields"), "1"), "type")), "float");
    let moves = at(spec("moves"), "fields");
    for axis in ["x", "y", "origin"] {
        assert_eq!(text(at(at(moves, axis), "type")), "vec2", "{axis}");
    }
    let row = at(spec("rows"), "of");
    assert_eq!(text(at(row, "type")), "map");
    assert_eq!(text(at(row, "key")), "int");
    assert_eq!(
        text(at(at(at(spec("spot"), "fields"), "tally"), "type")),
        "map"
    );
}

#[test]
fn a_scene_sets_a_pub_field_of_every_kind() {
    balaur_core::logbuf::capture_for_test();
    let props = r#"count = 3
name = "ana"
on = true
target = "../Enemy"
marks = ["../Enemy", "../Flag"]
at = [1.0, 2.0, 3.0]
tint = [1.0, 0.5, 0.25, 1.0]
steps = [1, 2, 3]
scores = { ana = 3, bo = 5 }
spot = { x = 4, label = "here" }
mode = "Angry"
shape = { kind = "Rect", "0" = 2.0, "1" = 3.0 }
maybe = 1.5"#;
    let (_dir, app) = booted(props);
    let world = app.engine.world();
    let tuned = balaur_core::scene::find_node(&world, app.engine.root(), "World/Tuned").unwrap();
    drop(world);
    let report =
        app.engine
            .script_host()
            .unwrap()
            .call_on(balaur::node_id_of(tuned), "report", &[]);
    assert_eq!(
        report,
        Some(Value::Str(
            "3 ana true Enemy 2 2 5 3 5 2 4 here angry rect 2x3 1".into()
        ))
    );
    assert!(warned().is_empty(), "{:#?}", warned());
}

fn warned() -> Vec<String> {
    balaur_core::logbuf::heard()
        .into_iter()
        .filter(|e| e.level == "warn" || e.level == "error")
        .filter(|e| e.message.contains("property") || e.message.contains("pub"))
        .map(|e| e.message)
        .collect()
}

fn called(app: &App, method: &str) -> Option<Value> {
    let world = app.engine.world();
    let tuned = balaur_core::scene::find_node(&world, app.engine.root(), "World/Tuned").unwrap();
    drop(world);
    app.engine
        .script_host()
        .unwrap()
        .call_on(balaur::node_id_of(tuned), method, &[])
}

#[test]
fn a_map_inside_a_record_or_a_list_and_a_transform_come_from_a_scene() {
    balaur_core::logbuf::capture_for_test();
    let props = r#"spot = { x = 1, label = "s", tally = { a = 1, b = 2, c = 3, d = 4, e = 5, f = 6, g = 7, h = 8, i = 9, j = 10, k = 11 } }
rows = [{ 1 = "one" }, { 1 = "uno", 2 = "dos", 3 = "tres" }]
moves = { x = [1.0, 0.0], y = [0.0, 2.0], origin = [5.0, 6.0] }"#;
    let (_dir, app) = booted(props);
    assert_eq!(
        called(&app, "nested"),
        Some(Value::Str("11 11 true false dos 3 5 2 0".into()))
    );
    assert_eq!(
        called(&app, "grown"),
        Some(Value::Str("12 26 100 a".into()))
    );
    assert!(warned().is_empty(), "{:#?}", warned());
}

#[test]
fn a_scene_cannot_set_a_field_that_is_not_pub() {
    balaur_core::logbuf::capture_for_test();
    let (_dir, app) = booted("kept = 9\nhidden = 4");
    assert_eq!(
        called(&app, "unmarked"),
        Some(Value::Int(0)),
        "the script ran, both fields left at their zero"
    );
    let warned = warned();
    assert_eq!(warned.len(), 2, "{warned:#?}");
    for field in ["kept", "hidden"] {
        assert!(
            warned
                .iter()
                .any(|line| line.contains(&format!("'{field}', which is no `pub` field"))),
            "{warned:#?}"
        );
    }
}
