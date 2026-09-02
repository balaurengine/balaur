//! Script state in a rollback snapshot, under the hybrid contract: a script
//! that defines `save_state`/`load_state` decides what matters; one that does
//! not gets its plain fields captured -- minus anything a snapshot cannot
//! carry, and minus `node`, which the host owns.

use balaur_core::{App, AppConfig};
use balaur_script::Value;

fn make_app(dir: &std::path::Path) -> App {
    std::fs::create_dir_all(dir.join("scripts")).unwrap();
    std::fs::write(
        dir.join("project.toml"),
        "name = \"t\"\nmain_scene = \"scenes/main.toml\"\n",
    )
    .unwrap();
    App::new(AppConfig {
        project_root: dir.to_path_buf(),
        pack: None,
        watch: false,
        script_args: Vec::new(),
        script_backend: Some(balaur_script_luau::factory()),
    })
    .unwrap()
}

fn attach(app: &App, name: &str, file: &str, source: &str) {
    let dir = app.engine.resource::<balaur_core::project::ProjectRoot>();
    let path = dir.borrow().0.join("scripts").join(file);
    std::fs::write(path, source).unwrap();
    let root = app.engine.root();
    let entity = balaur_core::scene::spawn_node(&mut app.engine.world_mut(), name, root);
    app.engine
        .script_host()
        .unwrap()
        .attach(balaur_core::node_id_of(entity), &format!("scripts/{file}"))
        .unwrap();
}

fn field<'a>(map: &'a [(String, Value)], key: &str) -> Option<&'a Value> {
    map.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

#[test]
fn a_script_with_save_and_load_state_is_asked() {
    let dir = tempfile::tempdir().unwrap();
    let app = make_app(dir.path());
    attach(
        &app,
        "Counter",
        "counter.luau",
        r"
local C = {}
function C:init() self.hidden = 1; self.count = 5 end
function C:save_state() return { count = self.count } end
function C:load_state(s) self.count = s.count * 10 end
return C
",
    );
    let host = app.engine.script_host().unwrap();
    let states = host.save_state();
    assert_eq!(states.len(), 1);
    let Value::Map(map) = &states[0].1 else {
        panic!(
            "save_state should return the script's table: {:?}",
            states[0].1
        )
    };
    assert_eq!(field(map, "count"), Some(&Value::Int(5)));
    assert!(
        field(map, "hidden").is_none(),
        "the script chose what to save; `hidden` was not chosen"
    );

    host.load_state(&states);
    let again = host.save_state();
    let Value::Map(map) = &again[0].1 else {
        panic!()
    };
    assert_eq!(
        field(map, "count"),
        Some(&Value::Int(50)),
        "load_state ran the script's own loader"
    );
}

#[test]
fn a_script_without_them_has_its_plain_fields_captured_and_restored() {
    let dir = tempfile::tempdir().unwrap();
    let app = make_app(dir.path());
    attach(
        &app,
        "Plain",
        "plain.luau",
        r#"
local P = {}
function P:init()
    self.hp = 3
    self.label = "ok"
    self.on_hit = function() end
end
return P
"#,
    );
    let host = app.engine.script_host().unwrap();
    let states = host.save_state();
    let Value::Map(map) = &states[0].1 else {
        panic!("plain fields become a map")
    };
    assert_eq!(field(map, "hp"), Some(&Value::Int(3)));
    assert_eq!(field(map, "label"), Some(&Value::Str("ok".into())));
    assert!(
        field(map, "on_hit").is_none(),
        "a function is skipped, not an error: {map:?}"
    );
    assert!(
        field(map, "node").is_none(),
        "`node` is the host's, never the snapshot's"
    );

    let perturbed = vec![(
        states[0].0,
        Value::Map(vec![("hp".to_string(), Value::Int(99))]),
    )];
    host.load_state(&perturbed);
    let after = host.save_state();
    let Value::Map(map) = &after[0].1 else {
        panic!()
    };
    assert_eq!(field(map, "hp"), Some(&Value::Int(99)));
    assert_eq!(
        field(map, "label"),
        Some(&Value::Str("ok".into())),
        "fields the snapshot did not mention are left alone"
    );
}
