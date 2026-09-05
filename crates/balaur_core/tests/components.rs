//! The component registry: how a plugin adds vocabulary that scene files and
//! scripts both speak.

use balaur_core::components::{self, ComponentDef, ComponentRegistry};
use balaur_core::{App, AppConfig, Engine};

fn app_with_marker() -> App {
    let mut app = App::new(AppConfig::bare("."))
    .unwrap();
    app.register_component(
        "marker",
        ComponentDef {
            doc: "",
            schema: ComponentDef::parse_schema(
                "marker",
                r#"label = { type = "string", default = "none" }
size = { type = "float", default = 1.0 }"#,
            ),
            tags: &[],
            expects: &[],
            apply: Box::new(|eng: &Engine, entity, params| {
                eng.world_mut()
                    .insert_one(entity, Marker(params.clone()))
                    .map_err(|_| anyhow::anyhow!("dead node"))?;
                Ok(())
            }),
            remove: Box::new(|eng: &Engine, entity| {
                let _ = eng.world_mut().remove_one::<Marker>(entity);
                Ok(())
            }),
            get: Box::new(|eng: &Engine, entity| {
                eng.world().get::<&Marker>(entity).ok().map(|m| m.0.clone())
            }),
        },
    );
    app
}

struct Marker(toml::Value);

fn spawn(app: &App) -> hecs::Entity {
    let root = app.engine.root();
    balaur_core::scene::spawn_node(&mut app.engine.world_mut(), "N", root)
}

#[test]
fn a_registered_component_is_listed_and_has_a_schema() {
    let app = app_with_marker();
    assert!(components::names(&app.engine).contains(&"marker".to_string()));
    let registry = app.engine.resource::<ComponentRegistry>();
    assert!(registry.borrow().def("marker").is_some());
    assert!(registry.borrow().def("nope").is_none());
}

#[test]
fn add_then_get_returns_what_was_set() {
    let app = app_with_marker();
    let e = spawn(&app);
    let params = toml::toml! { label = "hello" };
    components::add(&app.engine, e, "marker", Some(&params.into())).unwrap();

    let got = components::get(&app.engine, e, "marker").expect("component reads back");
    assert_eq!(
        got.get("label").and_then(toml::Value::as_str),
        Some("hello")
    );
}

#[test]
fn defaults_fill_in_what_was_not_given() {
    let app = app_with_marker();
    let e = spawn(&app);
    let params = toml::toml! { label = "x" };
    components::add(&app.engine, e, "marker", Some(&params.into())).unwrap();

    let got = components::get(&app.engine, e, "marker").unwrap();
    assert!(
        (got.get("size")
            .and_then(toml::Value::as_float)
            .unwrap_or_default()
            - 1.0)
            .abs()
            < 1e-9,
        "the schema default did not fill in"
    );
}

#[test]
fn adding_twice_updates_rather_than_duplicating() {
    let app = app_with_marker();
    let e = spawn(&app);
    for label in ["first", "second"] {
        let params = toml::toml! { label = label };
        components::add(&app.engine, e, "marker", Some(&params.into())).unwrap();
    }
    let got = components::get(&app.engine, e, "marker").unwrap();
    assert_eq!(
        got.get("label").and_then(toml::Value::as_str),
        Some("second")
    );
    assert_eq!(
        components::present_on(&app.engine, e),
        vec!["marker".to_string()]
    );
}

#[test]
fn remove_takes_it_off_the_node() {
    let app = app_with_marker();
    let e = spawn(&app);
    components::add(&app.engine, e, "marker", None).unwrap();
    assert!(!components::present_on(&app.engine, e).is_empty());

    components::remove(&app.engine, e, "marker").unwrap();
    assert!(components::get(&app.engine, e, "marker").is_none());
    assert!(components::present_on(&app.engine, e).is_empty());
}

#[test]
fn an_unregistered_name_is_an_error_that_says_so() {
    let app = app_with_marker();
    let e = spawn(&app);
    let err = components::add(&app.engine, e, "nonesuch", None).unwrap_err();
    assert!(err.to_string().contains("nonesuch"), "unhelpful: {err}");
    assert!(components::remove(&app.engine, e, "nonesuch").is_err());
}

#[test]
fn a_node_starts_with_no_components() {
    let app = app_with_marker();
    let e = spawn(&app);
    assert!(components::present_on(&app.engine, e).is_empty());
    assert!(components::get(&app.engine, e, "marker").is_none());
}

#[test]
fn a_colour_property_accepts_hex_as_well_as_floats() {
    let schema = ComponentDef::parse_schema(
        "swatch",
        r##"rgba = { type = "color", default = [0.8, 0.8, 0.8, 1.0] }
label = { type = "string", default = "#notacolour" }"##,
    );
    let channels = |value: &toml::Value| -> Vec<f64> {
        value
            .get("rgba")
            .and_then(toml::Value::as_array)
            .expect("rgba is an array after merging")
            .iter()
            .filter_map(toml::Value::as_float)
            .collect()
    };

    let given = toml::toml! { rgba = "#ff0000" };
    assert_eq!(
        channels(&components::merge_defaults(&schema, Some(&given.into()))),
        vec![1.0, 0.0, 0.0, 1.0]
    );

    let with_alpha = toml::toml! { rgba = "#00ff0080" };
    let merged = components::merge_defaults(&schema, Some(&with_alpha.into()));
    let rgba = channels(&merged);
    assert_eq!(rgba[0..3], [0.0, 1.0, 0.0]);
    assert!((rgba[3] - 128.0 / 255.0).abs() < 1e-9, "alpha: {rgba:?}");

    // Floats pass through untouched, and a string-typed property is not
    // touched even when it looks like a colour.
    let floats = toml::toml! { rgba = [0.25, 0.5, 0.75, 0.5] };
    assert_eq!(
        channels(&components::merge_defaults(&schema, Some(&floats.into()))),
        vec![0.25, 0.5, 0.75, 0.5]
    );
    let defaults = components::merge_defaults(&schema, None);
    assert_eq!(
        defaults.get("label").and_then(toml::Value::as_str),
        Some("#notacolour")
    );
}

#[test]
fn an_unparseable_colour_is_left_alone() {
    let schema = ComponentDef::parse_schema(
        "swatch",
        r#"rgba = { type = "color", default = [1.0, 1.0, 1.0, 1.0] }"#,
    );
    let given = toml::toml! { rgba = "not a colour" };
    let merged = components::merge_defaults(&schema, Some(&given.into()));
    assert_eq!(
        merged.get("rgba").and_then(toml::Value::as_str),
        Some("not a colour")
    );
}

#[test]
fn merge_defaults_prefers_what_was_given() {
    let schema = ComponentDef::parse_schema(
        "pair",
        r#"a = { type = "float", default = 1.0 }
b = { type = "string", default = "d" }"#,
    );
    let given = toml::toml! { a = 9.0 };
    let merged = components::merge_defaults(&schema, Some(&given.into()));
    assert!((merged.get("a").and_then(toml::Value::as_float).unwrap() - 9.0).abs() < 1e-9);
    assert_eq!(merged.get("b").and_then(toml::Value::as_str), Some("d"));

    let all_defaults = components::merge_defaults(&schema, None);
    assert!(
        (all_defaults
            .get("a")
            .and_then(toml::Value::as_float)
            .unwrap()
            - 1.0)
            .abs()
            < 1e-9
    );
}

/// What a plugin author gets back for a schema that departs from the
/// vocabulary: a panic at registration naming the component, the property and
/// the key at fault, rather than an inspector row that silently never appears.
#[test]
fn a_bad_schema_is_rejected_at_registration() {
    // (schema, what is wrong with it, words the message owes the author)
    let cases = [
        ("label = 3", "the spec is not a table", "not a table"),
        (
            r#"label = { default = "x" }"#,
            "no datatype",
            "no `type` key",
        ),
        (
            r#"label = { type = "str", default = "x" }"#,
            "an abbreviated datatype",
            "is not one of",
        ),
        (
            r#"label = { type = "string" }"#,
            "no default",
            "no `default`",
        ),
        (
            r#"label = { type = "float", default = "x" }"#,
            "a default of the wrong type",
            "wants a number",
        ),
        (
            r#"label = { type = "string", default = "x", options = ["x"] }"#,
            "options on a non-enum",
            "belongs to `type = \"enum\"`",
        ),
        (
            r#"label = { type = "enum", default = "x" }"#,
            "an enum with no options",
            "needs an `options` list",
        ),
        (
            r#"label = { type = "enum", default = "z", options = ["x", "y"] }"#,
            "a default the dropdown cannot show",
            "is not in `options`",
        ),
        (
            r#"label = { type = "vec2", default = [1.0, 2.0, 3.0] }"#,
            "a vec2 of three numbers",
            "has 3 entries",
        ),
        (
            r#"label = { type = "color", default = "reddish" }"#,
            "a colour that is not a colour",
            "not #rrggbb",
        ),
    ];

    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let messages: Vec<(&str, &str, String)> = cases
        .iter()
        .map(|(schema, what, expected)| {
            let caught = std::panic::catch_unwind(|| ComponentDef::parse_schema("thing", schema));
            let payload = caught.err().unwrap_or_else(|| panic!("accepted {what}"));
            (*what, *expected, panic_message(&*payload))
        })
        .collect();
    std::panic::set_hook(hook);

    for (what, expected, message) in messages {
        assert!(
            message.contains(expected),
            "{what}: message does not say {expected:?}: {message}"
        );
        assert!(
            message.contains("thing") && message.contains("label"),
            "{what}: message names neither the component nor the property: {message}"
        );
    }
}

/// The text out of a `catch_unwind` payload, whichever way it was raised.
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_string()))
        .unwrap_or_else(|| "panicked with no message".to_string())
}
