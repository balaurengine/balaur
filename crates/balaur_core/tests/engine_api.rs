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
        "[application]\nname = \"t\"\nmain_scene = \"m.toml\"\n",
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

#[test]
fn a_script_can_ask_a_plugins_version() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_in(dir.path());
    app.record_plugin(balaur_core::PluginInfo::new("weather", "2.1"));

    let version = |name: &str| {
        call(
            &app.engine,
            "engine",
            "plugin_version",
            &[Value::Str(name.into())],
        )
    };
    assert_eq!(version("weather").unwrap(), Value::Str("2.1".into()));
    assert_eq!(version("elsewhere").unwrap(), Value::Nil);
}

/// A project the editor opened is content, and a pack's bytecode is content:
/// what they name is checked, not trusted.
#[test]
fn fs_refuses_a_path_that_climbs_out_of_the_project() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("game");
    std::fs::create_dir(&project).unwrap();
    let app = app_in(&project);

    let err = call(
        &app.engine,
        "fs",
        "write",
        &[Value::Str("../stolen.txt".into()), Value::Str("hi".into())],
    )
    .expect_err("`..` walked out of the project");
    assert!(
        err.to_string().contains("stolen.txt"),
        "the error has to name the path, got {err}"
    );
    assert!(!dir.path().join("stolen.txt").exists());
}

#[test]
fn fs_refuses_an_absolute_path_outside_every_root() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("game");
    std::fs::create_dir(&project).unwrap();
    let outside = dir.path().join("elsewhere.txt");
    let app = app_in(&project);

    assert!(call(
        &app.engine,
        "fs",
        "write",
        &[
            Value::Str(outside.to_string_lossy().into_owned()),
            Value::Str("hi".into()),
        ],
    )
    .is_err());
    assert!(!outside.exists());
}

/// A link is a way out of a directory, so the path is followed to where it
/// really lands before it is checked.
#[cfg(unix)]
#[test]
fn fs_refuses_a_symlink_that_leaves_the_project() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("game");
    let outside = dir.path().join("outside");
    std::fs::create_dir(&project).unwrap();
    std::fs::create_dir(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, project.join("away")).unwrap();
    let app = app_in(&project);

    assert!(call(
        &app.engine,
        "fs",
        "write",
        &[
            Value::Str("away/stolen.txt".into()),
            Value::Str("hi".into()),
        ],
    )
    .is_err());
    assert!(!outside.join("stolen.txt").exists());
}

/// What `balaur edit <game>` needs: the editor's own root is the editor's
/// directory and the game it edits is a second one, named by the host.
#[test]
fn fs_reaches_a_second_root_the_host_declared() {
    let dir = tempfile::tempdir().unwrap();
    let editor = dir.path().join("editor");
    let game = dir.path().join("game");
    std::fs::create_dir(&editor).unwrap();
    std::fs::create_dir(&game).unwrap();
    let app = app_in(&editor);
    let target = game.join("scenes/main.toml");

    let write = |eng: &Engine| {
        call(
            eng,
            "fs",
            "write",
            &[
                Value::Str(target.to_string_lossy().into_owned()),
                Value::Str("hi".into()),
            ],
        )
    };
    assert!(write(&app.engine).is_err(), "undeclared, so out of reach");

    balaur_core::file_api::add_root(&app.engine, &game);
    write(&app.engine).unwrap();
    assert_eq!(
        std::fs::read_to_string(&target).unwrap(),
        "hi",
        "a declared root is writable, subdirectories included"
    );
}

#[test]
fn a_uuid_from_the_engine_stream_is_well_formed_and_repeats_from_a_seed() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    call(&app.engine, "rng", "seed", &[Value::Int(9)]).unwrap();
    let Value::Str(first) = call(&app.engine, "rng", "uuid", &[]).unwrap() else {
        panic!("a string")
    };
    assert_eq!(first.len(), 36);
    assert_eq!(&first[14..15], "4", "version nibble: {first}");
    assert!(
        matches!(&first[19..20], "8" | "9" | "a" | "b"),
        "variant: {first}"
    );
    call(&app.engine, "rng", "seed", &[Value::Int(9)]).unwrap();
    assert_eq!(
        call(&app.engine, "rng", "uuid", &[]).unwrap(),
        Value::Str(first)
    );
}

#[test]
fn base64_round_trips_bytes_and_sha256_matches_the_known_vector() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let encoded = call(
        &app.engine,
        "encoding",
        "base64",
        &[Value::Str("hello".into())],
    )
    .unwrap();
    assert_eq!(encoded, Value::Str("aGVsbG8=".into()));
    let decoded = call(&app.engine, "encoding", "from_base64", &[encoded]).unwrap();
    assert_eq!(decoded, Value::Bytes(b"hello".to_vec()));
    assert!(call(
        &app.engine,
        "encoding",
        "from_base64",
        &[Value::Str("@@".into())]
    )
    .is_err());
    let digest = call(
        &app.engine,
        "hash",
        "sha256_text",
        &[Value::Str("abc".into())],
    )
    .unwrap();
    assert_eq!(
        digest,
        Value::Str("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".into())
    );
    std::fs::write(dir.path().join("pack.bin"), b"abc").unwrap();
    let from_file = call(
        &app.engine,
        "hash",
        "sha256",
        &[Value::Str("pack.bin".into())],
    )
    .unwrap();
    assert_eq!(from_file, digest);
}

#[test]
fn the_platform_and_device_id_are_stable_facts() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let Value::Map(facts) = call(&app.engine, "engine", "platform", &[]).unwrap() else {
        panic!("a map")
    };
    let keys: Vec<&str> = facts.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(keys, ["os", "web", "mobile", "editor"]);
    let first = call(&app.engine, "engine", "device_id", &[]).unwrap();
    let again = call(&app.engine, "engine", "device_id", &[]).unwrap();
    assert_eq!(first, again, "one id per install");
    assert!(matches!(&first, Value::Str(s) if s.len() >= 8));
}

#[test]
fn the_wall_clock_is_read_at_the_top_of_a_tick() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_in(dir.path());
    assert_eq!(
        call(&app.engine, "engine", "unix_time", &[]).unwrap(),
        Value::Num(0.0)
    );
    app.tick(1.0 / 60.0);
    let Value::Num(now) = call(&app.engine, "engine", "unix_time", &[]).unwrap() else {
        panic!("a number")
    };
    assert!(now > 1.7e9, "seconds since 1970, got {now}");
}

#[test]
fn a_tagged_node_is_found_and_an_untagged_one_is_not() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let root = app.engine.root();
    let (door, wall) = {
        let mut world = app.engine.world_mut();
        (
            balaur_core::scene::spawn_node(&mut world, "Door", root),
            balaur_core::scene::spawn_node(&mut world, "Wall", root),
        )
    };
    let node = |e| Value::Node(balaur_core::node_id_of(e).0);
    let op = |name: &str| {
        balaur_core::node_api::NODE_OPS
            .iter()
            .find(|d| d.name == name)
            .unwrap()
            .call
    };
    op("add_tag")(&app.engine, &[node(door), Value::Str("door".into())]).unwrap();
    op("add_tag")(&app.engine, &[node(door), Value::Str("wood".into())]).unwrap();
    assert_eq!(
        op("tags")(&app.engine, &[node(door)]).unwrap(),
        Value::List(vec![Value::Str("door".into()), Value::Str("wood".into())])
    );
    assert_eq!(
        call(&app.engine, "scene", "tagged", &[Value::Str("door".into())]).unwrap(),
        Value::List(vec![node(door)])
    );
    assert_eq!(
        op("has_tag")(&app.engine, &[node(wall), Value::Str("door".into())]).unwrap(),
        Value::Bool(false)
    );
    op("remove_tag")(&app.engine, &[node(door), Value::Str("door".into())]).unwrap();
    assert_eq!(
        call(&app.engine, "scene", "tagged", &[Value::Str("door".into())]).unwrap(),
        Value::List(vec![])
    );
}

#[test]
fn the_device_facts_default_and_take_a_backends_report() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    assert_eq!(
        call(&app.engine, "engine", "focused", &[]).unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        call(&app.engine, "engine", "dark_mode", &[]).unwrap(),
        Value::Bool(false)
    );
    balaur_core::facts::update_device(&app.engine, |facts| {
        facts.focused = false;
        facts.dark_mode = true;
        facts.safe_area = [0.0, 44.0, 0.0, 34.0];
    });
    assert_eq!(
        call(&app.engine, "engine", "focused", &[]).unwrap(),
        Value::Bool(false)
    );
    assert_eq!(
        call(&app.engine, "engine", "dark_mode", &[]).unwrap(),
        Value::Bool(true)
    );
    assert!((balaur_core::facts::device(&app.engine).safe_area[1] - 44.0).abs() < f32::EPSILON);
}
