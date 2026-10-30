//! A derived component: the struct is the schema (PLAN-property-types §5).

use balaur_core::app::{App, AppConfig};
use balaur_core::components::derived::{Component, Enum, Fields, Record};
use balaur_core::components::{self, ComponentDef};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Enum)]
enum Gait {
    Walk,
    #[option(name = "run_fast")]
    Run,
}

#[derive(Debug, PartialEq, Record)]
struct Wave {
    /// How many come
    #[property(default = 3, min = 0)]
    count: i64,
    name: String,
}

/// A thing that walks in waves.
#[derive(Debug, Component)]
#[component(name = "walker")]
struct Walker {
    /// How fast it goes
    #[property(default = 2.5, min = 0.0, unit = "degrees", group = "motion")]
    speed: f32,
    #[property(default = Gait::Walk)]
    gait: Gait,
    /// Its tint
    #[property(color, default = [1.0, 0.5, 0.0, 1.0])]
    tint: [f32; 4],
    #[property(default = "lead")]
    label: String,
    waves: Vec<Wave>,
    /// The gaits it may switch to
    #[property(flags, default = vec![Gait::Walk])]
    may: Vec<Gait>,
    /// Its sprite
    #[property(asset = "texture")]
    sprite: String,
    /// Who it follows
    #[property(node)]
    leader: String,
}

fn app_with_walker() -> App {
    let mut app = App::new(AppConfig::bare(".")).unwrap();
    app.register_component(Walker::NAME, ComponentDef::of::<Walker>(None));
    app
}

#[test]
fn the_schema_is_the_fields_in_field_order() {
    let app = app_with_walker();
    let schemas = components::schemas(&app.engine);
    let (_, schema) = schemas.iter().find(|(name, _)| name == "walker").unwrap();
    assert_eq!(
        schema.order(),
        [
            "speed", "gait", "tint", "label", "waves", "may", "sprite", "leader"
        ]
    );
    assert_eq!(schema["may"]["type"].as_str(), Some("flags"));
    assert_eq!(schema["may"]["options"].as_array().map(Vec::len), Some(2));
    assert_eq!(schema["may"]["default"].as_array().map(Vec::len), Some(1));
    assert_eq!(schema["sprite"]["type"].as_str(), Some("asset"));
    assert_eq!(schema["sprite"]["asset"].as_str(), Some("texture"));
    assert_eq!(schema["leader"]["type"].as_str(), Some("node"));
    assert_eq!(schema["speed"]["type"].as_str(), Some("float"));
    assert_eq!(schema["speed"]["default"].as_float(), Some(2.5));
    assert_eq!(schema["speed"]["unit"].as_str(), Some("degrees"));
    assert_eq!(schema["speed"]["group"].as_str(), Some("motion"));
    assert_eq!(
        schema["speed"]["description"].as_str(),
        Some("How fast it goes")
    );
    assert_eq!(schema["gait"]["type"].as_str(), Some("enum"));
    assert_eq!(schema["gait"]["default"].as_str(), Some("walk"));
    assert_eq!(
        schema["gait"]["options"].as_array().unwrap().len(),
        2,
        "{}",
        schema["gait"]
    );
    assert_eq!(schema["tint"]["type"].as_str(), Some("color"));
    assert_eq!(schema["label"]["default"].as_str(), Some("lead"));
    assert_eq!(schema["waves"]["type"].as_str(), Some("list"));
    assert_eq!(schema["waves"]["of"]["type"].as_str(), Some("record"));
    assert_eq!(
        schema["waves"]["of"]["fields"]["count"]["default"].as_integer(),
        Some(3)
    );
}

#[test]
fn a_table_goes_in_and_comes_back_out() {
    let app = app_with_walker();
    let root = app.engine.root();
    let e = balaur_core::scene::spawn_node(&mut app.engine.world_mut(), "W", root);
    let asked: toml::Value = toml::from_str(
        r##"speed = 4
gait = "run_fast"
tint = "#ff0000"
waves = [{ count = 5, name = "first" }, { name = "second" }]"##,
    )
    .unwrap();
    components::add(&app.engine, e, "walker", Some(&asked)).unwrap();
    {
        let world = app.engine.world();
        let walker = world.get::<&Walker>(e).unwrap();
        assert!((walker.speed - 4.0).abs() < f32::EPSILON);
        assert_eq!(walker.gait, Gait::Run);
        assert_eq!(
            walker.tint.map(f32::to_bits),
            [1.0f32, 0.0, 0.0, 1.0].map(f32::to_bits)
        );
        assert_eq!(walker.label, "lead");
        assert_eq!(
            walker.waves,
            [
                Wave {
                    count: 5,
                    name: "first".into()
                },
                Wave {
                    count: 3,
                    name: "second".into()
                }
            ]
        );
    }
    let table = components::get(&app.engine, e, "walker").unwrap();
    assert_eq!(table["gait"].as_str(), Some("run_fast"));
    assert_eq!(table["waves"][1]["count"].as_integer(), Some(3));
}

#[test]
fn one_property_is_written_by_its_place_and_a_wrong_shape_is_refused() {
    let mut walker = Walker::default();
    let gait = Walker::NAMES
        .iter()
        .position(|name| *name == "gait")
        .unwrap();
    walker
        .write(gait, &toml::Value::String("run_fast".into()))
        .unwrap();
    assert_eq!(walker.gait, Gait::Run);
    let refused = walker
        .write(gait, &toml::Value::String("crawl".into()))
        .unwrap_err();
    assert!(
        format!("{refused:#}").contains("walk, run_fast"),
        "{refused:#}"
    );
    assert_eq!(
        walker.gait,
        Gait::Run,
        "a refused write keeps what was held"
    );
}
