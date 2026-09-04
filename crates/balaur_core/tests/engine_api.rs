//! The engine-level modules — engine, scene, log, rng, fs, toml, json —
//! called directly. Every language reaches these through the same
//! declarations.

use balaur_core::engine_api::ENGINE_OPS;
use balaur_core::{App, AppConfig, Engine};
use balaur_script::Value;

fn app_in(root: &std::path::Path) -> App {
    App::new(AppConfig {
        project_root: root.to_path_buf(),
        pack: None,
        watch: false,
        script_args: vec!["first".into(), "second".into()],
        script_backend: None,
    })
    .unwrap()
}

fn call(eng: &Engine, module: &str, name: &str, args: &[Value]) -> anyhow::Result<Value> {
    let decl = ENGINE_OPS
        .iter()
        .find(|d| d.module == module && d.name == name)
        .unwrap_or_else(|| panic!("`{module}.{name}` is not declared"));
    (decl.call)(eng, args)
}

#[test]
fn script_args_reach_scripts_as_a_list() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    assert_eq!(
        call(&app.engine, "engine", "args", &[]).unwrap(),
        Value::List(vec![
            Value::Str("first".into()),
            Value::Str("second".into())
        ])
    );
}

#[test]
fn the_scene_root_is_a_node_and_can_be_spawned_under() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let Value::Node(root) = call(&app.engine, "scene", "root", &[]).unwrap() else {
        panic!("root should be a node");
    };
    let made = call(
        &app.engine,
        "scene",
        "spawn",
        &[Value::Str("Made".into()), Value::Node(root)],
    )
    .unwrap();
    assert!(matches!(made, Value::Node(_)));
    assert_eq!(
        call(
            &app.engine,
            "scene",
            "get_node",
            &[Value::Str("Made".into())]
        )
        .unwrap(),
        made
    );
}

#[test]
fn an_unknown_path_is_nil_rather_than_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    assert_eq!(
        call(
            &app.engine,
            "scene",
            "get_node",
            &[Value::Str("Nope/Nowhere".into())]
        )
        .unwrap(),
        Value::Nil
    );
}

#[test]
fn the_rng_is_reproducible_from_a_seed() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let draw = |n: usize| {
        call(&app.engine, "rng", "seed", &[Value::Int(7)]).unwrap();
        (0..n)
            .map(|_| call(&app.engine, "rng", "random", &[]).unwrap())
            .collect::<Vec<_>>()
    };
    assert_eq!(draw(5), draw(5));
}

#[test]
fn rng_int_stays_inside_its_range() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    call(&app.engine, "rng", "seed", &[Value::Int(1)]).unwrap();
    for _ in 0..200 {
        let Value::Int(v) =
            call(&app.engine, "rng", "int", &[Value::Int(1), Value::Int(6)]).unwrap()
        else {
            panic!("int should return an integer");
        };
        assert!((1..=6).contains(&v), "{v} is outside 1..=6");
    }
}

#[test]
fn fs_is_rooted_at_the_project() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "name = \"t\"\nmain_scene = \"m.toml\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("m.toml"), "").unwrap();
    let mut app = app_in(dir.path());
    app.load_project().unwrap();

    call(
        &app.engine,
        "fs",
        "write",
        &[Value::Str("note.txt".into()), Value::Str("hi".into())],
    )
    .unwrap();
    assert_eq!(
        call(&app.engine, "fs", "read", &[Value::Str("note.txt".into())]).unwrap(),
        Value::Str("hi".into())
    );
    assert_eq!(
        call(
            &app.engine,
            "fs",
            "exists",
            &[Value::Str("note.txt".into())]
        )
        .unwrap(),
        Value::Bool(true)
    );
    assert!(
        dir.path().join("note.txt").exists(),
        "written outside the project root"
    );
}

#[test]
fn reading_a_missing_file_is_nil_rather_than_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    assert_eq!(
        call(&app.engine, "fs", "read", &[Value::Str("absent".into())]).unwrap(),
        Value::Nil
    );
}

#[test]
fn fs_list_is_sorted_and_hides_dotfiles() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["b.txt", "a.txt", ".hidden"] {
        std::fs::write(dir.path().join(name), "").unwrap();
    }
    std::fs::create_dir(dir.path().join("sub")).unwrap();
    let app = app_in(dir.path());
    let Value::List(items) = call(&app.engine, "fs", "list", &[Value::Str(".".into())]).unwrap()
    else {
        panic!("list should return a list");
    };
    let names: Vec<String> = items
        .iter()
        .filter_map(|v| match v {
            Value::Map(pairs) => pairs
                .iter()
                .find(|(k, _)| k == "name")
                .map(|(_, v)| match v {
                    Value::Str(s) => s.clone(),
                    _ => String::new(),
                }),
            _ => None,
        })
        .collect();
    assert_eq!(names, vec!["a.txt", "b.txt", "sub"]);
}

#[test]
fn toml_round_trips_through_neutral_values() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let parsed = call(
        &app.engine,
        "toml",
        "parse",
        &[Value::Str("name = \"x\"\nvalues = [1, 2, 3]\n".into())],
    )
    .unwrap();
    let encoded = call(&app.engine, "toml", "encode", std::slice::from_ref(&parsed)).unwrap();
    let Value::Str(text) = &encoded else {
        panic!("encode should return a string");
    };
    let again = call(&app.engine, "toml", "parse", &[Value::Str(text.clone())]).unwrap();
    assert_eq!(parsed, again, "a round trip changed the document");
}

#[test]
fn json_round_trips_through_neutral_values() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let parsed = call(
        &app.engine,
        "json",
        "parse",
        &[Value::Str(
            "{\"name\": \"x\", \"values\": [1, 2.5], \"nested\": {\"flag\": true}}".into(),
        )],
    )
    .unwrap();
    let encoded = call(&app.engine, "json", "encode", std::slice::from_ref(&parsed)).unwrap();
    let Value::Str(text) = &encoded else {
        panic!("encode should return a string");
    };
    let again = call(&app.engine, "json", "parse", &[Value::Str(text.clone())]).unwrap();
    assert_eq!(parsed, again, "a round trip changed the document");
}

#[test]
fn json_keeps_integers_and_floats_apart() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let parsed = call(
        &app.engine,
        "json",
        "parse",
        &[Value::Str("[1, 1.5]".into())],
    )
    .unwrap();
    assert_eq!(
        parsed,
        Value::List(vec![Value::Int(1), Value::Num(1.5)]),
        "1 should stay an integer and 1.5 a float"
    );
}

#[test]
fn json_null_and_nil_are_the_same_value() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    assert_eq!(
        call(&app.engine, "json", "parse", &[Value::Str("null".into())]).unwrap(),
        Value::Nil
    );
    assert_eq!(
        call(&app.engine, "json", "encode", &[Value::Nil]).unwrap(),
        Value::Str("null".into())
    );
}

#[test]
fn a_node_is_not_json_data() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    assert!(call(&app.engine, "json", "encode", &[Value::Node(1)]).is_err());
}

#[test]
fn malformed_json_is_an_error_rather_than_a_crash() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    assert!(call(&app.engine, "json", "parse", &[Value::Str("{".into())]).is_err());
}

#[test]
fn vectors_encode_as_json_number_arrays() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    assert_eq!(
        call(&app.engine, "json", "encode", &[Value::Vec2([1.0, 2.0])]).unwrap(),
        Value::Str("[1.0,2.0]".into())
    );
}

#[test]
fn declarations_are_uniquely_named_within_a_module() {
    let mut seen = std::collections::BTreeSet::new();
    for decl in ENGINE_OPS {
        assert!(
            seen.insert((decl.module, decl.name)),
            "`{}.{}` is declared twice",
            decl.module,
            decl.name
        );
    }
}

#[test]
fn a_script_can_write_to_the_log_it_reads_back() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    balaur_core::logbuf::capture_for_test();
    balaur_core::logbuf::clear();

    for level in ["info", "warn", "error"] {
        call(
            &app.engine,
            "log",
            level,
            &[Value::Str(format!("hello from {level}"))],
        )
        .unwrap();
    }

    let recent = balaur_core::logbuf::recent(10);
    for level in ["info", "warn", "error"] {
        let entry = recent
            .iter()
            .find(|e| e.message.contains(&format!("hello from {level}")))
            .unwrap_or_else(|| panic!("log.{level} did not reach the buffer"));
        assert_eq!(entry.level, level);
    }
}

/// The verbs the editor's asset browser needs: it could read and write, but
/// not create, rename or delete.
#[test]
fn a_file_can_be_made_moved_and_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let eng = &app.engine;
    let s = |t: &str| Value::Str(t.into());

    call(eng, "fs", "mkdir", &[s("sprites/enemies")]).unwrap();
    assert!(dir.path().join("sprites/enemies").is_dir());

    call(
        eng,
        "fs",
        "write",
        &[s("sprites/enemies/pig.toml"), s("a = 1\n")],
    )
    .unwrap();
    call(
        eng,
        "fs",
        "rename",
        &[s("sprites/enemies/pig.toml"), s("sprites/boar.toml")],
    )
    .unwrap();
    assert!(!dir.path().join("sprites/enemies/pig.toml").exists());
    assert_eq!(
        std::fs::read_to_string(dir.path().join("sprites/boar.toml")).unwrap(),
        "a = 1\n"
    );

    // Deleting says whether there was anything there, so deleting twice is
    // not an error a tool has to guard against.
    assert_eq!(
        call(eng, "fs", "remove", &[s("sprites/boar.toml")]).unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        call(eng, "fs", "remove", &[s("sprites/boar.toml")]).unwrap(),
        Value::Bool(false)
    );

    // A directory goes with everything under it.
    call(
        eng,
        "fs",
        "write",
        &[s("sprites/enemies/a.toml"), s("x = 1\n")],
    )
    .unwrap();
    assert_eq!(
        call(eng, "fs", "remove", &[s("sprites/enemies")]).unwrap(),
        Value::Bool(true)
    );
    assert!(!dir.path().join("sprites/enemies").exists());
}

/// A tool polls this instead of re-reading a file to see whether it changed.
#[test]
fn a_files_modification_time_is_readable_and_absent_for_one_that_is_not_there() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let eng = &app.engine;
    let s = |t: &str| Value::Str(t.into());

    assert_eq!(
        call(eng, "fs", "mtime", &[s("nothing.toml")]).unwrap(),
        Value::Nil
    );
    call(eng, "fs", "write", &[s("thing.toml"), s("a = 1\n")]).unwrap();
    let Value::Num(at) = call(eng, "fs", "mtime", &[s("thing.toml")]).unwrap() else {
        panic!("a written file has a modification time");
    };
    assert!(at > 1_700_000_000.0, "seconds since the epoch, got {at}");
}

#[test]
fn a_script_can_ask_which_plugins_loaded() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_in(dir.path());
    app.record_plugin(balaur_core::PluginInfo::new("weather", "1"));

    assert_eq!(
        call(&app.engine, "engine", "plugins", &[]).unwrap(),
        Value::List(vec![Value::Str("weather".into())])
    );
}

#[test]
fn a_script_can_ask_whether_one_plugin_loaded() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_in(dir.path());
    app.record_plugin(balaur_core::PluginInfo::new("weather", "1"));

    let has = |name: &str| {
        call(
            &app.engine,
            "engine",
            "has_plugin",
            &[Value::Str(name.into())],
        )
    };
    assert_eq!(has("weather").unwrap(), Value::Bool(true));
    assert_eq!(has("elsewhere").unwrap(), Value::Bool(false));
}
