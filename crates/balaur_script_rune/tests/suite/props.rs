//! Exported script properties: what a script declares, what a scene sets on
//! one node, and what `init` finds already written on `this`.

use balaur_core::{App, AppConfig};

fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("project.toml"), "[project]\nname = \"t\"\n").unwrap();
    for (name, body) in files {
        let path = dir.path().join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, body).unwrap();
    }
    dir
}

fn app_in(dir: &std::path::Path) -> App {
    App::new(AppConfig {
        script_backend: Some(balaur_script_rune::factory()),
        ..AppConfig::bare(dir.to_path_buf())
    })
    .unwrap()
}

/// A script whose `init` copies every export onto a field the test can read
/// back through the host, so the assertions are about what `init` saw.
const ENEMY: &str = "pub fn exports() {\n\
     #{ speed: 2.0, jumps: 2, name: \"grunt\" }\n\
 }\n\
 pub fn init(this) {\n\
     this.seen_speed = this.speed;\n\
     this.seen_jumps = this.jumps;\n\
     this.seen_name = this.name;\n\
 }\n";

/// Read one field off a node's live instance. The concrete host owns the
/// readers, and the `Rc` has to outlive the borrow, so each is its own call.
fn number(app: &App, node: hecs::Entity, field: &str) -> Option<f64> {
    let host = app.engine.script_host().unwrap();
    rune(&host).number_field(node, field)
}

fn text(app: &App, node: hecs::Entity, field: &str) -> Option<String> {
    let host = app.engine.script_host().unwrap();
    rune(&host).text_field(node, field)
}

fn rune(
    host: &std::rc::Rc<dyn balaur_script::ScriptHost<balaur_core::Engine>>,
) -> &balaur_script_rune::RuneHost {
    host.as_any()
        .downcast_ref::<balaur_script_rune::RuneHost>()
        .expect("the app is running Rune")
}

fn node_named(app: &App, name: &str) -> hecs::Entity {
    balaur_core::scene::find_node(&app.engine.world(), app.engine.root(), name)
        .unwrap_or_else(|| panic!("no node named {name}"))
}

fn build(scene: &str, script: &str) -> (tempfile::TempDir, App) {
    let dir = project(&[("scripts/enemy.rn", script)]);
    let app = app_in(dir.path());
    let root = app.engine.root();
    balaur_core::project::instantiate_scene(&app.engine, scene, root, true).unwrap();
    (dir, app)
}

#[test]
fn a_scene_property_is_on_this_before_init_runs() {
    let (_dir, app) = build(
        "[[nodes]]\n\
         name = \"Enemy\"\n\
         script = { source = \"scripts/enemy.rn\", props = { speed = 3.5, name = \"brute\" } }\n",
        ENEMY,
    );
    let enemy = node_named(&app, "Enemy");
    assert_eq!(number(&app, enemy, "seen_speed"), Some(3.5));
    assert_eq!(text(&app, enemy, "seen_name"), Some(String::from("brute")));
    // Untouched by the scene, so the export's own default is what init read.
    assert_eq!(number(&app, enemy, "seen_jumps"), Some(2.0));
}

#[test]
fn the_string_form_of_the_script_key_still_attaches() {
    let (_dir, app) = build(
        "[[nodes]]\n\
         name = \"Enemy\"\n\
         script = { source = \"scripts/enemy.rn\" }\n",
        ENEMY,
    );
    let enemy = node_named(&app, "Enemy");
    assert_eq!(number(&app, enemy, "seen_speed"), Some(2.0));
}

#[test]
fn two_nodes_on_one_script_get_their_own_values() {
    let (_dir, app) = build(
        "[[nodes]]\n\
         name = \"Scene\"\n\
         \n\
         [[nodes]]\n\
         name = \"Fast\"\n\
         parent = \"Scene\"\n\
         script = { source = \"scripts/enemy.rn\", props = { speed = 9.0 } }\n\
         \n\
         [[nodes]]\n\
         name = \"Slow\"\n\
         parent = \"Scene\"\n\
         script = { source = \"scripts/enemy.rn\", props = { speed = 0.5 } }\n",
        ENEMY,
    );
    let (fast, slow) = (
        node_named(&app, "Scene/Fast"),
        node_named(&app, "Scene/Slow"),
    );
    assert_eq!(number(&app, fast, "seen_speed"), Some(9.0));
    assert_eq!(number(&app, slow, "seen_speed"), Some(0.5));
}

/// A property set on a node the script does not export is a typo, and the
/// warning is the only thing that says so — but dropping the value would
/// lose an edit, so it is still written.
#[test]
fn a_property_the_script_does_not_export_is_still_written() {
    let (_dir, app) = build(
        "[[nodes]]\n\
         name = \"Enemy\"\n\
         script = { source = \"scripts/enemy.rn\", props = { speeed = 3.5 } }\n",
        "pub fn exports() { #{ speed: 2.0 } }\n\
         pub fn init(this) { this.seen = this.speeed; }\n",
    );
    let enemy = node_named(&app, "Enemy");
    assert_eq!(number(&app, enemy, "seen"), Some(3.5));
}

#[test]
fn a_script_without_exports_takes_properties_anyway() {
    let (_dir, app) = build(
        "[[nodes]]\n\
         name = \"Enemy\"\n\
         script = { source = \"scripts/enemy.rn\", props = { speed = 7.0 } }\n",
        "pub fn init(this) { this.seen = this.speed; }\n",
    );
    let enemy = node_named(&app, "Enemy");
    assert_eq!(number(&app, enemy, "seen"), Some(7.0));
}

/// A bare default is lifted into a spec, so every reader of `exports` sees
/// one shape — and the type it was inferred at is the one an inspector draws.
#[test]
fn exports_reports_the_declared_defaults_at_their_own_types() {
    let dir = project(&[("scripts/enemy.rn", ENEMY)]);
    let app = app_in(dir.path());
    let host = app.engine.script_host().unwrap();
    let declared = host.exports("scripts/enemy.rn").unwrap();
    // Sorted by key: a spec round-trips through TOML, whose tables are
    // ordered, so every reader sees the same one twice running.
    let spec = |kind: &str, default: balaur_script::Value| {
        balaur_script::Value::Map(vec![
            (String::from("default"), default),
            (String::from("type"), balaur_script::Value::Str(kind.into())),
        ])
    };
    assert_eq!(
        declared,
        vec![
            (
                String::from("jumps"),
                spec("int", balaur_script::Value::Int(2))
            ),
            (
                String::from("name"),
                spec("string", balaur_script::Value::Str("grunt".into()))
            ),
            (
                String::from("speed"),
                spec("float", balaur_script::Value::Num(2.0))
            ),
        ],
        "sorted by name, with an int default staying an int"
    );
}

/// The other half: a table carrying `type` is taken as written, so a script
/// can declare a range, an enum or a node reference a bare default cannot.
#[test]
fn a_written_spec_is_taken_as_it_stands_and_orders_the_rows() {
    let dir = project(&[(
        "scripts/tuned.rn",
        "pub fn exports() {\n\
         \x20   #{\n\
         \x20       speed: #{ \"type\": \"float\", \"default\": 2.0, min: 0.5, max: 8.0, order: 2 },\n\
         \x20       mode: #{ \"type\": \"enum\", \"default\": \"spin\", options: [\"spin\", \"wobble\"], order: 1 },\n\
         \x20   }\n\
         }\n\
         pub fn init(this) {}\n",
    )]);
    let app = app_in(dir.path());
    let host = app.engine.script_host().unwrap();
    let declared = host.exports("scripts/tuned.rn").unwrap();
    let names: Vec<&str> = declared.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["mode", "speed"], "`order` decides, not the name");
    let balaur_script::Value::Map(speed) = &declared[1].1 else {
        panic!("a spec is a map: {:?}", declared[1].1);
    };
    assert!(
        speed
            .iter()
            .any(|(k, v)| k == "max" && *v == balaur_script::Value::Num(8.0)),
        "the range the script wrote survives: {speed:?}"
    );
}

/// A spec is held to the same rules a component schema is, and the failure
/// names the script and the property rather than appearing at the first row.
#[test]
fn a_spec_that_breaks_the_schema_rules_is_refused() {
    let dir = project(&[(
        "scripts/bad.rn",
        "pub fn exports() { #{ mode: #{ \"type\": \"enum\", \"default\": \"spin\" } } }\n\
         pub fn init(this) {}\n",
    )]);
    let app = app_in(dir.path());
    let host = app.engine.script_host().unwrap();
    let err = host.exports("scripts/bad.rn").unwrap_err().to_string();
    assert!(
        err.contains("mode") && err.contains("options"),
        "the error should name the property and what is missing: {err}"
    );
}

#[test]
fn a_script_without_exports_declares_nothing() {
    let dir = project(&[("scripts/bare.rn", "pub fn init(this) { }\n")]);
    let app = app_in(dir.path());
    let host = app.engine.script_host().unwrap();
    assert!(host.exports("scripts/bare.rn").unwrap().is_empty());
}

#[test]
fn exports_returning_something_other_than_an_object_is_an_error() {
    let dir = project(&[("scripts/bad.rn", "pub fn exports() { 3 }\n")]);
    let app = app_in(dir.path());
    let host = app.engine.script_host().unwrap();
    let err = host.exports("scripts/bad.rn").unwrap_err().to_string();
    assert!(err.contains("object of defaults"), "{err}");
}

/// A shipped game reads its properties out of the pack, not off disk. Scenes
/// travel as their own source, so this is really asking whether the packed
/// boot path takes the same `script` key the dev one does.
#[test]
fn a_packed_game_boots_with_the_properties_its_scene_set() {
    let dir = project(&[
        (
            "project.toml",
            "[application]\nname = \"t\"\nmain_scene = \"main.toml\"\n",
        ),
        ("scripts/enemy.rn", ENEMY),
        (
            "main.toml",
            "[[nodes]]\n\
             name = \"Enemy\"\n\
             script = { source = \"scripts/enemy.rn\", props = { speed = 4.25 } }\n",
        ),
    ]);
    // The host is the compiler: a script is compiled against the modules the
    // game will actually have.
    let built = app_in(dir.path());
    let compiler = built.engine.script_host().unwrap();
    let pack = balaur_core::Pack::build(dir.path(), rune(&compiler)).unwrap();
    let pack = balaur_core::Pack::decode(&pack.encode()).unwrap();
    drop(built);

    let mut app = App::new(AppConfig {
        pack: Some(pack),
        script_backend: Some(balaur_script_rune::factory()),
        ..AppConfig::bare(dir.path().to_path_buf())
    })
    .unwrap();
    app.load_project().unwrap();

    let enemy = node_named(&app, "Enemy");
    assert_eq!(number(&app, enemy, "seen_speed"), Some(4.25));
    assert_eq!(number(&app, enemy, "seen_jumps"), Some(2.0));
}

/// A prefab's scripts attach once the whole outer tree exists, with the
/// properties the prefab set — and an override on the instance reaches the
/// node inside it.
#[test]
fn scripts_inside_a_prefab_attach_with_their_properties() {
    let dir = project(&[
        ("scripts/enemy.rn", ENEMY),
        (
            "scenes/enemy.toml",
            "[[nodes]]\n\
             id = \"n_body\"\n\
             name = \"Body\"\n\
             script = { source = \"scripts/enemy.rn\", props = { speed = 1.5 } }\n",
        ),
    ]);
    let app = app_in(dir.path());
    let root = app.engine.root();
    balaur_core::project::instantiate_scene(
        &app.engine,
        "[[nodes]]\n\
         id = \"n_scene\"\n\
         name = \"Scene\"\n\
         \n\
         [[nodes]]\n\
         id = \"n_left\"\n\
         name = \"Left\"\n\
         parent = \"n_scene\"\n\
         instance = \"scenes/enemy.toml\"\n\
         \n\
         [[nodes]]\n\
         id = \"n_right\"\n\
         name = \"Right\"\n\
         parent = \"n_scene\"\n\
         instance = \"scenes/enemy.toml\"\n",
        root,
        true,
    )
    .unwrap();

    for name in ["Scene/Left", "Scene/Right"] {
        let node = node_named(&app, name);
        assert_eq!(number(&app, node, "seen_speed"), Some(1.5), "{name}");
        assert_eq!(number(&app, node, "seen_jumps"), Some(2.0), "{name}");
    }
}

/// Prefabs travel in the pack as their own scene files, so a shipped game
/// resolves `instance` the way a dev run does.
#[test]
fn a_packed_game_builds_its_prefabs() {
    let dir = project(&[
        (
            "project.toml",
            "[application]\nname = \"t\"\nmain_scene = \"main.toml\"\n",
        ),
        ("scripts/enemy.rn", ENEMY),
        (
            "scenes/enemy.toml",
            "[[nodes]]\n\
             id = \"n_body\"\n\
             name = \"Body\"\n\
             script = { source = \"scripts/enemy.rn\", props = { speed = 6.5 } }\n",
        ),
        (
            "main.toml",
            "[[nodes]]\n\
             id = \"n_enemy\"\n\
             name = \"Enemy\"\n\
             instance = \"scenes/enemy.toml\"\n",
        ),
    ]);
    let built = app_in(dir.path());
    let compiler = built.engine.script_host().unwrap();
    let pack = balaur_core::Pack::build(dir.path(), rune(&compiler)).unwrap();
    let pack = balaur_core::Pack::decode(&pack.encode()).unwrap();
    drop(built);

    let mut app = App::new(AppConfig {
        pack: Some(pack),
        script_backend: Some(balaur_script_rune::factory()),
        ..AppConfig::bare(dir.path().to_path_buf())
    })
    .unwrap();
    app.load_project().unwrap();

    let body = node_named(&app, "Enemy");
    assert_eq!(number(&app, body, "seen_speed"), Some(6.5));
}

/// Retuning a prefab from the instance that named it: two enemies from one
/// file, differing only in a property the scene overrode.
#[test]
fn an_override_retunes_a_prefabs_script() {
    let dir = project(&[
        ("scripts/enemy.rn", ENEMY),
        (
            "scenes/enemy.toml",
            "[[nodes]]\n\
             id = \"n_body\"\n\
             name = \"Body\"\n\
             script = { source = \"scripts/enemy.rn\", props = { speed = 1.5 } }\n",
        ),
    ]);
    let app = app_in(dir.path());
    let root = app.engine.root();
    balaur_core::project::instantiate_scene(
        &app.engine,
        "[[nodes]]\n\
         id = \"n_scene\"\n\
         name = \"Scene\"\n\
         \n\
         [[nodes]]\n\
         id = \"n_slow\"\n\
         name = \"Slow\"\n\
         parent = \"n_scene\"\n\
         instance = \"scenes/enemy.toml\"\n\
         \n\
         [[nodes]]\n\
         id = \"n_fast\"\n\
         name = \"Fast\"\n\
         parent = \"n_scene\"\n\
         instance = \"scenes/enemy.toml\"\n\
         \n\
         [nodes.overrides.\".\".script.props]\n\
         speed = 12.0\n",
        root,
        true,
    )
    .unwrap();

    assert_eq!(
        number(&app, node_named(&app, "Scene/Slow"), "seen_speed"),
        Some(1.5)
    );
    assert_eq!(
        number(&app, node_named(&app, "Scene/Fast"), "seen_speed"),
        Some(12.0)
    );
    // Untouched by the override, so both still take the export's default.
    assert_eq!(
        number(&app, node_named(&app, "Scene/Fast"), "seen_jumps"),
        Some(2.0)
    );
}

/// `#[export]` on a constant is the other way to declare a property: the
/// compiler checks the name, so a typo is a build error rather than a row
/// that never appears.
#[test]
fn an_exported_constant_is_a_property() {
    let dir = project(&[(
        "scripts/ship.rn",
        "#[export] pub const SPEED = 2.0;\n\
         #[export(node)] pub const TARGET = \"\";\n\
         pub const PRIVATE = 7;\n\
         pub fn init(this) {}\n",
    )]);
    let app = app_in(dir.path());
    let host = app.engine.script_host().unwrap();
    let declared = host.exports("scripts/ship.rn").unwrap();

    let named: Vec<(&str, &str)> = declared
        .iter()
        .map(|(name, spec)| {
            let balaur_script::Value::Map(fields) = spec else {
                panic!("a spec is a map");
            };
            let kind = fields
                .iter()
                .find(|(k, _)| k == "type")
                .map(|(_, v)| match v {
                    balaur_script::Value::Str(text) => text.as_ref(),
                    _ => panic!("a type is a string"),
                })
                .expect("a spec names a type");
            (name.as_str(), kind)
        })
        .collect();

    let mut named = named;
    named.sort_unstable();
    assert_eq!(
        named,
        [("SPEED", "float"), ("TARGET", "node")],
        "the kind names what a default cannot say; a plain constant is typed by its value, \
         and a constant without the attribute is not a property"
    );
}

/// The two ways of declaring a property are not meant to be combined on one
/// name: whichever the reader trusted, the other would be silently ignored.
#[test]
fn declaring_a_property_both_ways_is_refused() {
    let dir = project(&[(
        "scripts/clash.rn",
        "#[export] pub const SPEED = 2.0;\n\
         pub fn exports() { #{ SPEED: 1.0 } }\n",
    )]);
    let app = app_in(dir.path());
    let host = app.engine.script_host().unwrap();
    let err = host.exports("scripts/clash.rn").unwrap_err().to_string();
    assert!(err.contains("declared twice"), "{err}");
}

/// An asset property names the asset type it takes, and the attribute has
/// nowhere to put that, so it says so rather than building a spec the schema
/// would refuse further down.
#[test]
fn an_exported_asset_says_where_to_declare_it() {
    let dir = project(&[(
        "scripts/icon.rn",
        "#[export(asset)] pub const ICON = \"\";\n",
    )]);
    let app = app_in(dir.path());
    let host = app.engine.script_host().unwrap();
    let err = host.exports("scripts/icon.rn").unwrap_err().to_string();
    assert!(err.contains("asset type it takes"), "{err}");
    assert!(
        err.contains("exports()"),
        "it points at the form that works: {err}"
    );
}

/// A `node` export is the node its path names, relative to the scripted one,
/// even one declared later in the file; a path to nothing is nil.
#[test]
fn a_node_export_arrives_as_the_node_it_names() {
    let script = "pub fn exports() {\n\
         #{ target: #{ \"type\": \"node\", \"default\": \"\" }, lost: #{ \"type\": \"node\", \"default\": \"\" } }\n\
     }\n\
     pub fn init(this) {\n\
         this.seen_target = this.target.name();\n\
         this.seen_lost = if this.lost is Tuple { \"nil\" } else { \"node\" };\n\
     }\n";
    let (_dir, app) = build(
        "[[nodes]]\n\
         name = \"Scene\"\n\
         \n\
         [[nodes]]\n\
         name = \"Hunter\"\n\
         parent = \"Scene\"\n\
         script = { source = \"scripts/enemy.rn\", props = { target = \"../Prey\", lost = \"../Nobody\" } }\n\
         \n\
         [[nodes]]\n\
         name = \"Prey\"\n\
         parent = \"Scene\"\n",
        script,
    );
    let hunter = node_named(&app, "Scene/Hunter");
    assert_eq!(
        text(&app, hunter, "seen_target"),
        Some(String::from("Prey"))
    );
    assert_eq!(text(&app, hunter, "seen_lost"), Some(String::from("nil")));
}

/// A list of nodes is Godot's `Array[Node]`, each path resolved the way one
/// is.
#[test]
fn a_list_of_nodes_arrives_as_the_nodes_it_names() {
    let script = "pub fn exports() {\n\
         #{ crew: #{ \"type\": \"list\", \"of\": #{ \"type\": \"node\" }, \"default\": [] } }\n\
     }\n\
     pub fn init(this) {\n\
         this.seen = this.crew.len() as f64;\n\
         this.first = this.crew[0].name();\n\
         this.lost = if this.crew[2] is Tuple { \"nil\" } else { \"node\" };\n\
     }\n";
    let (_dir, app) = build(
        "[[nodes]]\n\
         name = \"Ship\"\n\
         script = { source = \"scripts/enemy.rn\", props = { crew = [\"Cook\", \"Bosun\", \"Ghost\"] } }\n\
         \n\
         [[nodes]]\n\
         name = \"Cook\"\n\
         parent = \"Ship\"\n\
         \n\
         [[nodes]]\n\
         name = \"Bosun\"\n\
         parent = \"Ship\"\n",
        script,
    );
    let ship = node_named(&app, "Ship");
    assert_eq!(number(&app, ship, "seen"), Some(3.0));
    assert_eq!(text(&app, ship, "first"), Some(String::from("Cook")));
    assert_eq!(text(&app, ship, "lost"), Some(String::from("nil")));
}

/// A `node` export that names a `component` hands the script that node's
/// handle for it, so it calls the component straight off the export.
#[test]
fn a_node_export_naming_a_component_arrives_as_that_handle() {
    let script = "pub fn exports() {\n\
         #{ wheel: #{ \"type\": \"node\", \"component\": \"transform\", \"default\": \"\" } }\n\
     }\n\
     pub fn init(this) {\n\
         this.wheel.position = [1.0, 2.0, 0.0];\n\
         this.seen_x = this.wheel.position.x;\n\
     }\n";
    let (_dir, app) = build(
        "[[nodes]]\n\
         name = \"Scene\"\n\
         \n\
         [[nodes]]\n\
         name = \"Cart\"\n\
         parent = \"Scene\"\n\
         script = { source = \"scripts/enemy.rn\", props = { wheel = \"../Wheel\" } }\n\
         \n\
         [[nodes]]\n\
         name = \"Wheel\"\n\
         parent = \"Scene\"\n\
         transform = { position = [0.0, 0.0, 0.0] }\n",
        script,
    );
    let cart = node_named(&app, "Scene/Cart");
    assert_eq!(number(&app, cart, "seen_x"), Some(1.0));
}

/// A child's `init` runs before its parent's, as Godot runs `_ready`: a
/// parent composes children that have already set themselves up.
#[test]
fn a_child_inits_before_its_parent() {
    let script = "pub fn init(this) {\n\
         let order = scene::variable(\"order\");\n\
         scene::set_variable(\"order\", order + this.node.name());\n\
     }\n";
    let scene = "[variables]\n\
         order = { type = \"string\", value = \"\" }\n\
         \n\
         [[nodes]]\n\
         id = \"p\"\n\
         name = \"P\"\n\
         script = { source = \"scripts/enemy.rn\" }\n\
         \n\
         [[nodes]]\n\
         name = \"C\"\n\
         parent = \"p\"\n\
         script = { source = \"scripts/enemy.rn\" }\n";
    let dir = project(&[("scripts/enemy.rn", script)]);
    let app = app_in(dir.path());
    let root = app.engine.root();
    balaur_core::project::instantiate_scene(&app.engine, scene, root, true).unwrap();
    let variables = app.engine.resource::<balaur_core::variables::Variables>();
    let order = variables.borrow().get("order").cloned();
    let order = match order {
        Some(balaur_script::Value::Str(text)) => Some(String::from(&*text)),
        _ => None,
    };
    assert_eq!(order, Some(String::from("CP")));
}

/// A bare list is exported as a list of whatever its entries are, and a scene
/// writing its own list reaches the script as that list.
#[test]
fn a_bare_list_export_takes_the_type_of_its_entries() {
    let script = "pub fn exports() {\n\
         #{ waves: [2, 4, 8] }\n\
     }\n\
     pub fn init(this) {\n\
         this.seen = this.waves[1] as f64;\n\
         this.count = this.waves.len() as f64;\n\
     }\n";
    let (dir, app) = build(
        "[[nodes]]\n\
         name = \"Ship\"\n\
         script = { source = \"scripts/enemy.rn\", props = { waves = [1, 2] } }\n",
        script,
    );
    let ship = node_named(&app, "Ship");
    assert_eq!(number(&app, ship, "seen"), Some(2.0));
    assert_eq!(number(&app, ship, "count"), Some(2.0));

    let host = app.engine.script_host().unwrap();
    let declared = host.exports("scripts/enemy.rn").unwrap();
    let (name, spec) = &declared[0];
    assert_eq!(name, "waves");
    let balaur_script::Value::Map(fields) = spec else {
        panic!("a spec is a map: {spec:?}");
    };
    let key = |wanted: &str| {
        fields
            .iter()
            .find(|(k, _)| k == wanted)
            .map(|(_, v)| v.clone())
    };
    assert_eq!(
        key("type"),
        Some(balaur_script::Value::Str("list".to_string()))
    );
    let Some(balaur_script::Value::Map(of)) = key("of") else {
        panic!("a list declares what it holds: {spec:?}");
    };
    assert!(
        of.iter()
            .any(|(k, v)| k == "type" && *v == balaur_script::Value::Str("int".to_string())),
        "{of:?}"
    );
    drop(dir);
}

/// A row has to know which editor to draw in it, so a list holds one type and
/// the error names the entry that broke it.
#[test]
fn a_bare_list_of_two_types_is_an_error() {
    let dir = project(&[(
        "scripts/mixed.rn",
        "pub fn exports() { #{ waves: [2, \"four\"] } }\n",
    )]);
    let app = app_in(dir.path());
    let host = app.engine.script_host().unwrap();
    let err = host.exports("scripts/mixed.rn").unwrap_err().to_string();
    assert!(err.contains("entry 1"), "{err}");
    assert!(err.contains("holds one type"), "{err}");
}

/// A bare object is a record whose fields keep their own types, which is how
/// a script exports the data a class of its own would hold.
#[test]
fn a_bare_object_export_is_a_record_of_its_fields() {
    let script = "pub fn exports() {\n\
         #{ wave: #{ hp: 3, name: \"boss\" } }\n\
     }\n\
     pub fn init(this) {\n\
         this.seen_hp = this.wave.hp as f64;\n\
         this.seen_name = this.wave.name;\n\
     }\n";
    let (_dir, app) = build(
        "[[nodes]]\n\
         name = \"Ship\"\n\
         script = { source = \"scripts/enemy.rn\", props = { wave = { hp = 9, name = \"brute\" } } }\n",
        script,
    );
    let ship = node_named(&app, "Ship");
    assert_eq!(number(&app, ship, "seen_hp"), Some(9.0));
    assert_eq!(text(&app, ship, "seen_name"), Some(String::from("brute")));
}

/// A node path is resolved wherever the spec puts one, a record's field
/// included.
#[test]
fn a_node_inside_a_record_arrives_as_the_node_it_names() {
    let script = "pub fn exports() {\n\
         #{ crew: #{ \"type\": \"record\", \"fields\": #{ \"cook\": #{ \"type\": \"node\" } }, \"default\": #{ } } }\n\
     }\n\
     pub fn init(this) {\n\
         this.seen = this.crew.cook.name();\n\
     }\n";
    let (_dir, app) = build(
        "[[nodes]]\n\
         name = \"Ship\"\n\
         script = { source = \"scripts/enemy.rn\", props = { crew = { cook = \"Cook\" } } }\n\
         \n\
         [[nodes]]\n\
         name = \"Cook\"\n\
         parent = \"Ship\"\n",
        script,
    );
    let ship = node_named(&app, "Ship");
    assert_eq!(text(&app, ship, "seen"), Some(String::from("Cook")));
}

/// A map keyed by whole numbers reaches the script keyed by numbers: TOML
/// spells `7` as `"7"`, and the spec is what says to read it back.
#[test]
fn a_map_keyed_by_numbers_arrives_keyed_by_numbers() {
    let script = "pub fn exports() {\n\
         #{ spawns: #{ \"type\": \"map\", \"key\": \"int\", \"of\": #{ \"type\": \"float\" }, \"default\": #{ } } }\n\
     }\n\
     pub fn init(this) {\n\
         this.seen = this.spawns[7];\n\
         this.count = this.spawns.len() as f64;\n\
     }\n";
    let (_dir, app) = build(
        "[[nodes]]\n\
         name = \"Ship\"\n\
         script = { source = \"scripts/enemy.rn\", props = { spawns = { 7 = 2.5 } } }\n",
        script,
    );
    let ship = node_named(&app, "Ship");
    assert_eq!(number(&app, ship, "seen"), Some(2.5));
    assert_eq!(number(&app, ship, "count"), Some(1.0));
}

/// A map keyed by whole numbers refuses a key that is not one, where the
/// scene wrote it.
#[test]
fn a_map_keyed_by_numbers_refuses_a_word() {
    let dir = project(&[(
        "scripts/bad.rn",
        "pub fn exports() {\n\
             #{ spawns: #{ \"type\": \"map\", \"key\": \"int\", \"of\": #{ \"type\": \"float\" }, \"default\": #{ \"x\": 1.0 } } }\n\
         }\n",
    )]);
    let app = app_in(dir.path());
    let host = app.engine.script_host().unwrap();
    let err = host.exports("scripts/bad.rn").unwrap_err().to_string();
    assert!(err.contains("not a whole number"), "{err}");
}

/// A scene naming one of a record's fields still hands the script both: the
/// record's shape is what the spec declares, not what the node wrote.
#[test]
fn a_record_field_the_scene_left_out_takes_its_default() {
    let script = "pub fn exports() {\n\
         #{ wave: #{ hp: 3, name: \"grunt\" } }\n\
     }\n\
     pub fn init(this) {\n\
         this.seen_hp = this.wave.hp as f64;\n\
         this.seen_name = this.wave.name;\n\
     }\n";
    let (_dir, app) = build(
        "[[nodes]]\n\
         name = \"Ship\"\n\
         script = { source = \"scripts/enemy.rn\", props = { wave = { hp = 9 } } }\n",
        script,
    );
    let ship = node_named(&app, "Ship");
    assert_eq!(number(&app, ship, "seen_hp"), Some(9.0));
    assert_eq!(text(&app, ship, "seen_name"), Some(String::from("grunt")));
}

/// A `struct` the script declares is exported as itself: the scene writes its
/// fields, and `init` gets the class back, methods and all.
#[test]
fn a_class_export_arrives_as_the_class_it_was_declared_as() {
    let script = "struct Wave { hp, name }\n\
     impl Wave {\n\
         fn power(self) { self.hp * 2 }\n\
     }\n\
     pub fn exports() {\n\
         #{ wave: Wave { hp: 3, name: \"grunt\" } }\n\
     }\n\
     pub fn init(this) {\n\
         this.seen = this.wave.power() as f64;\n\
         this.seen_name = this.wave.name;\n\
     }\n";
    let (_dir, app) = build(
        "[[nodes]]\n\
         name = \"Ship\"\n\
         script = { source = \"scripts/enemy.rn\", props = { wave = { hp = 21 } } }\n",
        script,
    );
    let ship = node_named(&app, "Ship");
    assert_eq!(number(&app, ship, "seen"), Some(42.0));
    assert_eq!(text(&app, ship, "seen_name"), Some(String::from("grunt")));
}

/// The class reaches the inspector as a `record` naming it, so the rows are
/// the fields and the file holds a table.
#[test]
fn a_class_export_is_a_record_naming_its_class() {
    let dir = project(&[(
        "scripts/enemy.rn",
        "struct Wave { hp }\n\
         pub fn exports() { #{ wave: Wave { hp: 3 } } }\n",
    )]);
    let app = app_in(dir.path());
    let host = app.engine.script_host().unwrap();
    let declared = host.exports("scripts/enemy.rn").unwrap();
    let balaur_script::Value::Map(spec) = &declared[0].1 else {
        panic!("a spec is a map: {declared:?}");
    };
    let key = |wanted: &str| {
        spec.iter()
            .find(|(k, _)| k == wanted)
            .map(|(_, v)| v.clone())
    };
    assert_eq!(
        key("type"),
        Some(balaur_script::Value::Str("record".to_string()))
    );
    assert_eq!(
        key("class"),
        Some(balaur_script::Value::Str("Wave".to_string()))
    );
}

/// Godot set a member where it was declared, before any `_ready` ran, so a
/// script whose method another script's `init` calls still has them.
#[test]
fn a_scripts_declared_members_are_set_before_anything_calls_it() {
    let dir = project(&[
        (
            "slots.rn",
            "pub fn defaults(this) { this.slots = [\"\", \"\", \"\"]; }\n\
             pub fn take(this, name) { this.slots[0] = name; this.slots.len() as f64 }\n",
        ),
        (
            "caller.rn",
            "pub fn init(this) { this.out = scene::get_node(\"Slots\").call(\"take\", \"ann\"); }\n",
        ),
    ]);
    let app = app_in(dir.path());
    let spawn = |name: &str| {
        let root = app.engine.root();
        balaur_core::scene::spawn_node(&mut app.engine.world_mut(), name, root)
    };
    let slots = spawn("Slots");
    let caller = spawn("Caller");
    let host = app.engine.script_host().unwrap();
    host.attach(balaur_core::node_id_of(slots), "slots.rn")
        .unwrap();
    host.attach(balaur_core::node_id_of(caller), "caller.rn")
        .unwrap();
    let rune = host
        .as_any()
        .downcast_ref::<balaur_script_rune::RuneHost>()
        .unwrap();
    assert_eq!(
        rune.number_field(caller, "out"),
        Some(3.0),
        "the members were set when the instance was made"
    );
}
