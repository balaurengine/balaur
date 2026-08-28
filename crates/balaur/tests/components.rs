//! Component registry end-to-end: plugin components are addable, readable,
//! editable, and removable through the generic node API.

use balaur::{AppConfig, standard_app};

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
    let lua = app.engine.scripts().unwrap().lua();
    lua.load(
        r#"
        -- The registry lists every plugin's components.
        local names = scene.components()
        local set = {}
        for _, n in names do set[n] = true end
        for _, expected in { "body", "collider", "shape", "color", "widget" } do
            assert(set[expected], expected .. " not registered")
        end

        local n = scene.spawn("Thing")
        n:set_position(0, 3, 0)

        -- Add with defaults, then edit a property.
        n:add_component("body")
        n:add_component("collider", { shape = "ball", radius = 0.7 })
        n:add_component("shape", { kind = "ball", radius = 0.7 })
        n:add_component("widget", { text = "hi" })

        local body = n:get_component("body")
        assert(body.kind == "dynamic", "default body kind")
        n:set_component("body", { kind = "fixed" })
        assert(n:get_component("body").kind == "fixed", "edited body kind")

        local col = n:get_component("collider")
        assert(math.abs(col.radius - 0.7) < 1e-5, "collider radius roundtrip")

        -- Schema drives editors.
        local schema = scene.component_schema("collider")
        assert(schema.shape.kind == "enum")
        assert(schema.radius.default == 0.5)

        -- Removal: body removed, collider survives as static geometry.
        n:remove_component("body")
        assert(n:get_component("body") == nil, "body removed")
        assert(n:get_component("collider") ~= nil, "collider survives body removal")
        n:remove_component("collider")
        assert(n:get_component("collider") == nil, "collider removed")

        local present = n:component_names()
        local have = {}
        for _, name in present do have[name] = true end
        assert(have.shape and have.widget and not have.body)
        "#,
    )
    .exec()
    .unwrap();
}
