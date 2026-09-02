//! Component registry end-to-end: plugin components are addable, readable,
//! editable, and removable through the generic node API.

use balaur::{standard_app, AppConfig};

#[test]
fn plugin_components_roundtrip_through_the_registry() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scenes")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "name = \"t\"\nmain_scene = \"scenes/main.toml\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("scenes/main.toml"), "").unwrap();
    let mut config = AppConfig::dev(dir.path().to_string_lossy().as_ref());
    config.watch = false;
    let app = standard_app(config).unwrap();
    let lua = balaur_script_luau::lua_of(&app.engine);
    lua.load(
        r#"
        -- The registry lists every plugin's components.
        local names = scene.component_types()
        local set = {}
        for _, n in names do set[n] = true end
        for _, expected in { "body3d", "collider3d", "shape3d", "widget" } do
            assert(set[expected], expected .. " not registered")
        end

        local n = scene.spawn("Thing")
        n:set_position(0, 3, 0)

        -- set_component adds with defaults when the node lacks the
        -- component, and merges when it has it; there is no add_component.
        n:set_component("body3d")
        n:set_component("collider3d", { kind = "ball", radius = 0.7 })
        n:set_component("shape3d", { kind = "ball", radius = 0.7 })
        n:set_component("widget", { text = "hi" })

        local body = n:get_component("body3d")
        assert(body.kind == "dynamic", "default body kind")
        n:set_component("body3d", { kind = "static" })
        assert(n:get_component("body3d").kind == "static", "edited body kind")

        local col = n:get_component("collider3d")
        assert(math.abs(col.radius - 0.7) < 1e-5, "collider radius roundtrip")

        -- Schema drives editors.
        local schema = scene.component_schema("collider3d")
        assert(schema.kind.type == "enum")
        assert(schema.radius.default == 0.5)

        -- Removal: body removed, collider survives as static geometry.
        n:remove_component("body3d")
        assert(n:get_component("body3d") == nil, "body removed")
        assert(n:get_component("collider3d") ~= nil, "collider survives body removal")
        n:remove_component("collider3d")
        assert(n:get_component("collider3d") == nil, "collider removed")

        local present = n:component_names()
        local have = {}
        for _, name in present do have[name] = true end
        assert(have.shape3d and have.widget and not have.body3d)
        "#,
    )
    .exec()
    .unwrap();
}

/// An app with every standard plugin's components registered, plus the
/// temporary project it was booted from (dropping that deletes it).
fn app_with_every_component() -> (tempfile::TempDir, balaur::App) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scenes")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "name = \"t\"\nmain_scene = \"scenes/main.toml\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("scenes/main.toml"), "").unwrap();
    let mut config = AppConfig::dev(dir.path().to_string_lossy().as_ref());
    config.watch = false;
    let app = standard_app(config).unwrap();
    (dir, app)
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
    assert!(registry.0.len() > 4, "the standard plugins registered none");

    for (name, def) in &registry.0 {
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
        kind = "ball"
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
    assert_eq!(
        got.get("clicked").and_then(toml::Value::as_bool),
        Some(false),
        "clicked is emitted, and now declared"
    );
}

#[test]
fn every_component_round_trips_through_get_and_apply() {
    let (_dir, app) = app_with_every_component();
    let registry = app
        .engine
        .resource::<balaur::components::ComponentRegistry>();
    let registry = registry.borrow();

    for (name, def) in &registry.0 {
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

    for (name, def) in &registry.0 {
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
                // A kind that needs an asset refuses to apply without one;
                // that refusal is tested elsewhere and is not a round-trip.
                if let Err(e) = balaur::components::add(&app.engine, entity, name, Some(&params)) {
                    let chain = format!("{e:#}");
                    if chain.contains("asset") || chain.contains("mesh") || chain.contains("field")
                    {
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
