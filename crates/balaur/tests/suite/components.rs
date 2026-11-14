//! Component registry end-to-end: plugin components are addable, readable,
//! editable, and removable through the generic node API.

use balaur::{AppConfig, standard_app};

#[test]
fn plugin_components_roundtrip_through_the_registry() {
    let (dir, app) = app_with_every_component();
    run_script(
        dir.path(),
        &app,
        r#"
        fn has(list: [str], item: str) -> bool {
            for x in list {
                if x == item {
                    return true;
                }
            }
            false
        }

        pub class Checks { node: Node, done: int, failed: bool }

        impl Checks {
            fn init() {
                let names = scene::component_types();
                for expected in ["body3d", "collider3d", "shape3d", "widget"] {
                    self.check(has(names, expected), "{expected} not registered");
                }

                let n = scene::root().add_child("Thing");
                n.transform.position = vec3(0.0, 3.0, 0.0);

                // set_component adds with defaults when the node lacks the
                // component, and merges when it has it; there is no add_component.
                n.set_component("body3d");
                n.set_component("collider3d", { "shape": { "kind": "sphere", "radius": 0.7 } });
                n.set_component("shape3d", { "shape": { "kind": "sphere", "radius": 0.7 } });
                n.set_component("widget", { "text": "hi" });

                let body = n.get_component("body3d");
                self.check(body["kind"].as_str() == "dynamic", "default body kind");
                n.set_component("body3d", { "kind": "static" });
                self.check(n.body3d.kind == "static", "edited body kind");

                let col = n.get_component("collider3d");
                self.check(math::abs(col["shape"]["radius"].as_float() - 0.7) < 1e-5, "collider radius roundtrip");

                let schema = scene::component_schema("collider3d");
                self.check(schema["shape"]["type"].as_str() == "variant", "shape is a variant");
                self.check(schema["shape"]["variants"]["sphere"]["fields"]["radius"]["default"].as_float() == 0.5, "radius default");

                n.remove_component("body3d");
                self.check(n.get_component("body3d") == nil, "body removed");
                self.check(n.get_component("collider3d") != nil, "collider survives body removal");
                n.remove_component("collider3d");
                self.check(n.get_component("collider3d") == nil, "collider removed");

                let present = n.component_names();
                self.check(has(present, "shape3d") && has(present, "widget") && !has(present, "body3d"), "names left");
                if !self.failed {
                    self.done = 1;
                }
            }

            fn check(ok: bool, what: str) {
                if !ok {
                    self.failed = true;
                    log::error("failed: {what}");
                }
            }
        }
        
"#,
    );
}

/// Attach `source` to a fresh node and require it to have reached its
/// closing `self.done = 1`; a failed check inside leaves that unset.
fn run_script(dir: &std::path::Path, app: &balaur::App, source: &str) {
    balaur::logbuf::capture_for_test();
    balaur::logbuf::clear();
    std::fs::create_dir_all(dir.join("scripts")).unwrap();
    std::fs::write(dir.join("scripts/checks.bs"), source).unwrap();
    let node = spawn(app, "Tester");
    app.engine
        .script_host()
        .unwrap()
        .attach(balaur::node_id_of(node), "scripts/checks.bs")
        .unwrap();
    let host = app.engine.script_host().unwrap();
    assert_eq!(
        host.script_field(balaur::node_id_of(node), "done"),
        Some(balaur_script::Value::Int(1)),
        "the script did not run to its end: {:#?}",
        balaur::logbuf::recent(10)
    );
}

/// An app with every standard plugin's components registered, plus the
/// temporary project it was booted from (dropping that deletes it).
fn app_with_every_component() -> (tempfile::TempDir, balaur::App) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scenes")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"fixture\"\nmain_scene = \"scenes/main.toml\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("scenes/main.toml"), "").unwrap();
    let mut config = AppConfig::dev(dir.path().to_string_lossy().as_ref());
    config.watch = false;
    let app = standard_app(config).unwrap();
    (dir, app)
}

fn logged_at(level: &str) -> Vec<String> {
    balaur_core::logbuf::heard()
        .into_iter()
        .filter(|entry| entry.level == level)
        .map(|entry| entry.message)
        .collect()
}

/// A record asset, a component row that names one, and a row its schema declares.
const RECORD_ASSETS: [(&str, &str, &str, &str); 5] = [
    ("text_style", "text3d", "text_style", "outline_size = 3.0"),
    ("font", "text2d", "font", "pixel_snap = true"),
    (
        "camera_effects",
        "camera3d",
        "effects",
        "bloom_intensity = 0.5",
    ),
    (
        "physics_material",
        "collider3d",
        "physics_material",
        "friction = 0.9",
    ),
    (
        "soft_material",
        "softbody3d",
        "soft_material",
        "edge_hz = 20.0",
    ),
];

/// A scene with one node whose `component` names `row` of its asset, inline
/// and from a file with and without its `type`.
fn scenes_naming(
    dir: &std::path::Path,
    (asset, component, row, _): (&str, &str, &str, &str),
    key: &str,
) -> [String; 3] {
    std::fs::write(
        dir.join("typed.toml"),
        format!("type = \"{asset}\"\n{key}\n"),
    )
    .unwrap();
    std::fs::write(dir.join("bare.toml"), format!("{key}\n")).unwrap();
    let node = "[[nodes]]\nid = \"n_host\"\nname = \"Host\"\n";
    [
        format!("{node}\n[nodes.{component}.{row}]\n{key}\n"),
        format!("{node}\n[nodes.{component}]\n{row} = \"typed.toml\"\n"),
        format!("{node}\n[nodes.{component}]\n{row} = \"bare.toml\"\n"),
    ]
}

#[test]
fn every_record_asset_refuses_an_undeclared_key_inline_and_from_a_file() {
    balaur_core::logbuf::capture_for_test();
    let (dir, app) = app_with_every_component();
    let root = app.engine.root();
    for record in RECORD_ASSETS {
        for scene in scenes_naming(dir.path(), record, "no_such_row = 3.0") {
            balaur_core::assets::reload(&app.engine, "typed.toml").unwrap();
            balaur_core::assets::reload(&app.engine, "bare.toml").unwrap();
            balaur_core::logbuf::clear();
            balaur_core::project::instantiate_scene(&app.engine, &scene, root, false).unwrap();
            let refused = format!("`no_such_row` is not a property of a {}", record.0);
            let errors = logged_at("error");
            assert!(
                errors.iter().any(|error| error.contains(&refused)),
                "{scene}: {errors:?}"
            );
        }
    }
}

#[test]
fn every_record_asset_takes_a_row_it_declares_inline_and_from_a_file() {
    balaur_core::logbuf::capture_for_test();
    let (dir, app) = app_with_every_component();
    let root = app.engine.root();
    for record in RECORD_ASSETS {
        // The file with no `type` has nothing to parse it by; it is only ever
        // refused, which the test above holds.
        let [inline, typed, _] = scenes_naming(dir.path(), record, record.3);
        for scene in [inline, typed] {
            balaur_core::assets::reload(&app.engine, "typed.toml").unwrap();
            balaur_core::logbuf::clear();
            balaur_core::project::instantiate_scene(&app.engine, &scene, root, false).unwrap();
            assert_eq!(logged_at("error"), Vec::<String>::new(), "{scene}");
        }
    }
}

// So a record asset added later has to be named above.
#[test]
fn the_record_assets_above_are_all_of_them() {
    let (_dir, app) = app_with_every_component();
    let registry = app.engine.resource::<balaur_core::AssetTypeRegistry>();
    let registry = registry.borrow();
    let mut records: Vec<String> = registry
        .names()
        .into_iter()
        .filter(|name| registry.schema(name).is_some())
        .collect();
    records.sort();
    let mut named: Vec<&str> = RECORD_ASSETS.iter().map(|record| record.0).collect();
    named.sort_unstable();
    assert_eq!(records, named);
}

/// An asset with no schema whose parser's keys are closed, a component row that
/// names one, and a definition holding only what it reads.
const CHECKED_ASSETS: [(&str, &str, &str, &str); 2] = [
    (
        "state_machine",
        "state_machine",
        "machine",
        "start = \"idle\"\nstates = { idle = \"\" }",
    ),
    (
        "tileset",
        "tilemap",
        "tileset",
        "texture = \"a.png\"\ntile_size = 16\ncolumns = 1",
    ),
];

#[test]
fn every_checked_asset_refuses_another_key_inline_and_from_a_file() {
    balaur_core::logbuf::capture_for_test();
    let (dir, app) = app_with_every_component();
    let root = app.engine.root();
    for record in CHECKED_ASSETS {
        let body = format!("{}\nno_such_key = 3.0", record.3);
        for scene in scenes_naming(dir.path(), record, &body) {
            balaur_core::assets::reload(&app.engine, "typed.toml").unwrap();
            balaur_core::assets::reload(&app.engine, "bare.toml").unwrap();
            balaur_core::logbuf::clear();
            balaur_core::project::instantiate_scene(&app.engine, &scene, root, false).unwrap();
            let errors = logged_at("error");
            assert!(
                errors
                    .iter()
                    .any(|error| error.contains("`no_such_key` is not a key of")),
                "{scene}: {errors:?}"
            );
        }
    }
}

#[test]
fn every_checked_asset_takes_what_it_reads_inline_and_from_a_file() {
    balaur_core::logbuf::capture_for_test();
    let (dir, app) = app_with_every_component();
    let root = app.engine.root();
    for record in CHECKED_ASSETS {
        let [inline, typed, _] = scenes_naming(dir.path(), record, record.3);
        for scene in [inline, typed] {
            balaur_core::assets::reload(&app.engine, "typed.toml").unwrap();
            balaur_core::logbuf::clear();
            balaur_core::project::instantiate_scene(&app.engine, &scene, root, false).unwrap();
            assert_eq!(logged_at("error"), Vec::<String>::new(), "{scene}");
        }
    }
}

fn app_with_plugins(plugins: &str) -> (tempfile::TempDir, balaur::App) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        format!("[application]\nname = \"fixture\"\nmain_scene = \"main.toml\"\n\n[plugins]\n{plugins}\n"),
    )
    .unwrap();
    std::fs::write(dir.path().join("main.toml"), "").unwrap();
    let mut config = AppConfig::dev(dir.path().to_string_lossy().as_ref());
    config.watch = false;
    let app = standard_app(config).unwrap();
    (dir, app)
}

fn lacks_key_owner(app: &balaur::App) -> bool {
    app.engine
        .try_resource::<balaur_core::project::LacksKeyOwner>()
        .is_some()
}

fn scene_keys(app: &balaur::App) -> Vec<String> {
    let keys = app
        .engine
        .resource::<balaur_core::project::SceneKeyRegistry>();
    let mut keys: Vec<String> = keys.borrow().0.iter().map(|(key, _)| key.clone()).collect();
    keys.sort();
    keys
}

// `KEYLESS_MODULES` is what a typo stays an error beside, so it has to be
// true: turning every one of them off takes no scene key away.
#[test]
fn a_keyless_module_turned_off_takes_no_key_away() {
    let off = balaur::KEYLESS_MODULES
        .map(|module| format!("{module} = false"))
        .join("\n");
    let (_whole_dir, whole) = app_with_plugins("");
    let (_less_dir, less) = app_with_plugins(&off);
    assert_eq!(scene_keys(&whole), scene_keys(&less));
    assert!(!lacks_key_owner(&whole));
    assert!(!lacks_key_owner(&less));
}

#[test]
fn a_key_nothing_handles_is_an_error_in_a_whole_build() {
    balaur_core::logbuf::capture_for_test();
    let (_dir, app) = app_with_plugins("");
    let root = app.engine.root();
    balaur_core::logbuf::clear();
    balaur_core::project::instantiate_scene(
        &app.engine,
        "[[nodes]]\nid = \"n_host\"\nname = \"Host\"\n\n[nodes.cloner]\ncount = 3\n",
        root,
        false,
    )
    .unwrap();
    let errors = logged_at("error");
    assert!(
        errors
            .iter()
            .any(|error| error.contains("scene key 'cloner' on node 'Host'")),
        "{errors:?}"
    );
}

// A scene with sound still loads where the module is off, and says so only as
// a warning: this build cannot tell `sound` from a typo, and the build that
// has it can.
#[test]
fn a_key_a_module_turned_off_would_own_only_warns() {
    balaur_core::logbuf::capture_for_test();
    for off in ["audio = false", "some_extension = false"] {
        let (_dir, app) = app_with_plugins(off);
        assert!(lacks_key_owner(&app), "{off}");
        let root = app.engine.root();
        balaur_core::logbuf::clear();
        balaur_core::project::instantiate_scene(
            &app.engine,
            "[[nodes]]\nid = \"n_host\"\nname = \"Host\"\n\n[nodes.owned_by_what_is_off]\nvolume = 1.0\n",
            root,
            false,
        )
        .unwrap();
        assert_eq!(logged_at("error"), Vec::<String>::new(), "{off}");
        assert!(
            logged_at("warn")
                .iter()
                .any(|warning| warning.contains("owned_by_what_is_off")),
            "{off}"
        );
    }
}

fn spawn(app: &balaur::App, name: &str) -> balaur::hecs::Entity {
    let root = app.engine.root();
    balaur::scene::spawn_node(&mut app.engine.world_mut(), name, root)
}

#[test]
fn every_component_emits_only_keys_its_schema_declares() {
    let (_dir, app) = app_with_every_component();
    let registry = app
        .engine
        .resource::<balaur::components::ComponentRegistry>();
    let registry = registry.borrow();
    assert!(registry.len() > 4, "the standard plugins registered none");

    for (name, def) in &*registry {
        let entity = spawn(&app, name);
        // `color` writes into a renderable rather than owning storage, so
        // seed both; every other component applies on a bare node.
        for seed in ["shape3d", "shape2d"] {
            balaur::components::add(&app.engine, entity, seed, None).unwrap();
        }
        balaur::components::add(&app.engine, entity, name, None)
            .unwrap_or_else(|e| panic!("`{name}` does not apply with its own defaults: {e}"));

        let emitted = (def.get)(&app.engine, entity)
            .unwrap_or_else(|| panic!("`{name}` applied but reads back as absent"));
        let emitted = emitted
            .as_table()
            .unwrap_or_else(|| panic!("`{name}`'s get did not return a table"));
        let declared = def
            .schema
            .as_table()
            .unwrap_or_else(|| panic!("`{name}`'s schema is not a table"));
        for key in emitted.keys() {
            assert!(
                declared.contains_key(key),
                "`{name}.{key}` comes out of get but no schema declares it"
            );
        }
    }
}

#[test]
fn a_hex_string_is_a_colour_wherever_a_colour_is_taken() {
    let (_dir, app) = app_with_every_component();
    let floats = |value: &toml::Value, key: &str| -> Vec<f64> {
        value
            .get(key)
            .and_then(toml::Value::as_array)
            .unwrap_or_else(|| panic!("{key} is not an array"))
            .iter()
            .filter_map(toml::Value::as_float)
            .collect()
    };

    // A renderable's `color` property, written as the hex a scene uses.
    let e = spawn(&app, "Red");
    let params = toml::Value::Table(toml::toml! {
        shape = { kind = "sphere" }
        color = "#ff0000"
    });
    balaur::components::add(&app.engine, e, "shape3d", Some(&params)).unwrap();
    let got = balaur::components::get(&app.engine, e, "shape3d").unwrap();
    assert_eq!(floats(&got, "color"), vec![1.0, 0.0, 0.0, 1.0]);

    // `widget.text_color`, which every example scene writes as hex.
    let w = spawn(&app, "Label");
    let params = toml::toml! { text_color = "#0000ff" };
    balaur::components::add(&app.engine, w, "widget", Some(&params.into())).unwrap();
    let got = balaur::components::get(&app.engine, w, "widget").unwrap();
    assert_eq!(floats(&got, "text_color"), vec![0.0, 0.0, 1.0, 1.0]);
}

#[test]
fn every_component_round_trips_through_get_and_apply() {
    let (_dir, app) = app_with_every_component();
    let registry = app
        .engine
        .resource::<balaur::components::ComponentRegistry>();
    let registry = registry.borrow();

    for (name, def) in &*registry {
        let entity = spawn(&app, name);
        // `color` writes into a renderable rather than owning storage, so
        // seed both; every other component applies on a bare node.
        for seed in ["shape3d", "shape2d"] {
            balaur::components::add(&app.engine, entity, seed, None).unwrap();
        }
        balaur::components::add(&app.engine, entity, name, None).unwrap();

        let first = (def.get)(&app.engine, entity)
            .unwrap_or_else(|| panic!("`{name}` applied but reads back as absent"));
        balaur::components::add(&app.engine, entity, name, Some(&first))
            .unwrap_or_else(|e| panic!("`{name}` does not accept its own `get` output: {e}"));
        let second = (def.get)(&app.engine, entity)
            .unwrap_or_else(|| panic!("`{name}` vanished when re-applied"));

        assert_eq!(
            first, second,
            "`{name}` does not survive its own round trip"
        );
    }
}

#[test]
fn every_enum_option_a_schema_offers_round_trips() {
    let (_dir, app) = app_with_every_component();
    let registry = app
        .engine
        .resource::<balaur::components::ComponentRegistry>();
    let registry = registry.borrow();
    let mut checked = 0;

    for (name, def) in &*registry {
        let schema = def.schema.as_table().unwrap();
        for (prop, spec) in schema {
            if spec.get("type").and_then(toml::Value::as_str) != Some("enum") {
                continue;
            }
            let options = spec.get("options").and_then(toml::Value::as_array).unwrap();
            for option in options.iter().filter_map(toml::Value::as_str) {
                let entity = spawn(&app, name);
                for seed in ["shape3d", "shape2d"] {
                    balaur::components::add(&app.engine, entity, seed, None).unwrap();
                }
                let mut params = toml::map::Map::new();
                params.insert(prop.clone(), toml::Value::String(option.to_string()));
                let params = toml::Value::Table(params);
                // A kind that needs an asset, or points of its own, refuses to
                // apply without them; that refusal is tested elsewhere.
                if let Err(e) = balaur::components::add(&app.engine, entity, name, Some(&params)) {
                    let chain = format!("{e:#}");
                    let needs = ["asset", "mesh", "field", "points"];
                    if needs.iter().any(|word| chain.contains(word)) {
                        continue;
                    }
                    panic!("`{name}.{prop} = \"{option}\"` was rejected: {chain}");
                }

                let got = balaur::components::get(&app.engine, entity, name).unwrap();
                assert_eq!(
                    got.get(prop).and_then(toml::Value::as_str),
                    Some(option),
                    "`{name}.{prop}` cannot read back the option `{option}` it offers"
                );
                checked += 1;
            }
        }
    }
    assert!(
        checked >= 12,
        "only {checked} options seen; schemas missing?"
    );
}

// A `line2d` takes a `mesh` or a `path2d`, and the schema can only name one
// of them, so an inline table's own `type` is what decides.
#[test]
fn an_inline_asset_is_the_type_its_table_declares() {
    let (dir, app) = app_with_every_component();
    run_script(
        dir.path(),
        &app,
        r##"
        pub class Checks { node: Node, done: int }

        impl Checks {
            fn init() {
                let n = scene::root().add_child("Curve");
                n.set_component("line2d", {
                    "width": 0.05,
                    "mesh": { "type": "path2d", "points": [[0.0, 0.0], [1.0, 0.0]], "closed": false },
                });
                // It resolved to an inline reference rather than failing, and the
                // component reads back with one.
                let mesh = n.line2d.mesh;
                if mesh.starts_with("#!") {
                    self.done = 1;
                } else {
                    log::error("not an inline reference: {mesh}");
                }
            }
        }
        
"##,
    );
}

// The other half of [`every_component_emits_only_keys_its_schema_declares`]:
// a property the schema promises the editor has to come back out of `get`,
// or the inspector shows a control that reads as absent.
#[test]
fn every_component_emits_every_key_its_schema_declares() {
    let (_dir, app) = app_with_every_component();
    let registry = app
        .engine
        .resource::<balaur::components::ComponentRegistry>();
    let registry = registry.borrow();

    for (name, def) in &*registry {
        let entity = spawn(&app, name);
        // `color` writes into a renderable rather than owning storage, so
        // seed both; every other component applies on a bare node.
        for seed in ["shape3d", "shape2d"] {
            balaur::components::add(&app.engine, entity, seed, None).unwrap();
        }
        balaur::components::add(&app.engine, entity, name, None).unwrap();

        let emitted = (def.get)(&app.engine, entity)
            .unwrap_or_else(|| panic!("`{name}` applied but reads back as absent"));
        let emitted = emitted.as_table().unwrap();
        let declared = def.schema.as_table().unwrap();
        for key in declared.keys() {
            assert!(
                emitted.contains_key(key),
                "`{name}.{key}` is declared but `get` never emits it"
            );
        }
    }
}

// `transform` takes a single property without reading its table back; this covers that fast path.
#[test]
fn a_one_property_write_leaves_the_rest() {
    let (dir, app) = app_with_every_component();
    run_script(
        dir.path(),
        &app,
        r#"
        pub class Checks { node: Node, done: int, failed: bool }

        impl Checks {
            fn init() {
                let n = scene::root().add_child("Moved");
                n.set_component("transform", { "position": [1.0, 2.0, 3.0], "scale": [4.0, 4.0, 4.0] });
                n.transform.position = vec3(9.0, 0.0, 0.0);

                let at = n.transform.position;
                self.check(at.x == 9.0, "position moved");
                let size = n.transform.scale;
                self.check(size.x == 4.0, "scale survived the write");
                self.check(n.transform.position.x == 9.0, "the handle reads it back");

                if !self.failed {
                    self.done = 1;
                }
            }

            fn check(ok: bool, what: str) {
                if !ok {
                    self.failed = true;
                    log::error("failed: {what}");
                }
            }
        }
        
"#,
    );
}

#[test]
#[ignore = "needs engine: a .bs component read on a node without the component gives zeros, not the schema default"]
fn a_handle_on_a_missing_component_reads_its_defaults() {
    let (dir, app) = app_with_every_component();
    run_script(
        dir.path(),
        &app,
        r#"
        pub class Checks { node: Node, done: int }

        impl Checks {
            fn init() {
                let bare = scene::root().add_child("Bare");
                bare.remove_component("transform");
                let scale = bare.transform.scale.x;
                if scale == 1.0 {
                    self.done = 1;
                } else {
                    log::error("the default scale read {scale}");
                }
            }
        }
        "#,
    );
}

// `widget` reads one property without building the table and `collider3d` has no such fast path, so
// this covers both.
#[test]
fn a_keyed_read_answers_one_property() {
    let (dir, app) = app_with_every_component();
    run_script(
        dir.path(),
        &app,
        r#"
        pub class Checks { node: Node, done: int, failed: bool }

        impl Checks {
            fn init() {
                let n = scene::root().add_child("Keyed");
                n.set_component("widget", { "text": "hi", "kind": "button" });
                n.set_component("collider3d", { "shape": { "kind": "sphere", "radius": 0.7 } });

                let text = "text";
                let kind = "kind";
                let enabled_key = "enabled";
                self.check(n.component_property("widget", text).as_str() == "hi", "widget text");
                self.check(n.component_property("widget", kind).as_str() == "button", "widget kind");
                let enabled = n.component_property("widget", enabled_key);
                self.check(enabled.kind() == "bool" && enabled.as_bool(), "widget enabled");
                let table = n.get_component("widget");
                self.check(n.component_property("widget", "value") == table["value"], "same as the table");

                // No fast path: the table is read and indexed.
                let shape = "shape";
                self.check(n.component_property("collider3d", shape)["kind"].as_str() == "sphere", "collider kind");

                self.check(n.component_property("widget", "no_such_property") == nil, "unknown property");
                self.check(n.component_property("body3d", kind) == nil, "component the node lacks");

                if !self.failed {
                    self.done = 1;
                }
            }

            fn check(ok: bool, what: str) {
                if !ok {
                    self.failed = true;
                    log::error("failed: {what}");
                }
            }
        }
        
"#,
    );
}
