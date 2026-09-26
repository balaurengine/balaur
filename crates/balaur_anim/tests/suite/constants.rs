//! The script constants and the parsers describe the same closed sets: every
//! word a script can name as `animation::…` is one a clip, a state machine or
//! a modifier actually reads.

use balaur_anim::ease::Easing;
use balaur_anim::{
    ADVANCE_MODES, AnimationPlugin, CONSTANTS, INTERPOLATIONS, LOOP_MODES, MODIFIER_KINDS,
    PROPERTIES, SWITCH_MODES, clip, ease_constants, machine,
};
use balaur_core::components::ComponentRegistry;
use balaur_core::{App, AppConfig};

fn parses_as_clip(body: &str) -> anyhow::Result<clip::Clip> {
    clip::parse(&toml::from_str(body).unwrap())
}

#[test]
fn every_loop_mode_interp_and_property_parses_in_a_clip() {
    for (name, word) in LOOP_MODES {
        let body = format!(
            "length = 1.0\nloop_mode = \"{word}\"\n[[tracks]]\nkeys = [{{ time = 0.0, call = \"on_tick\" }}]\n"
        );
        assert!(parses_as_clip(&body).is_ok(), "{name}");
    }
    for (name, word) in INTERPOLATIONS {
        let body = format!(
            "[[tracks]]\nproperty = \"position\"\ninterpolation = \"{word}\"\nkeys = [{{ time = 1.0, value = [0.0, 0.0, 0.0] }}]\n"
        );
        assert!(parses_as_clip(&body).is_ok(), "{name}");
    }
    for (name, word) in PROPERTIES {
        let value = match *word {
            "visible" => "[1.0]",
            "rotation" | "tint" => "[0.0, 0.0, 0.0, 1.0]",
            clip::DEFORM => "[0.0, 0.0]",
            _ => "[0.0, 0.0, 0.0]",
        };
        let body = format!(
            "[[tracks]]\nproperty = \"{word}\"\nkeys = [{{ time = 1.0, value = {value} }}]\n"
        );
        assert!(
            parses_as_clip(&body).is_ok(),
            "{name}: {:?}",
            parses_as_clip(&body).err()
        );
    }
}

#[test]
fn every_advance_and_switch_mode_parses_in_a_machine() {
    for (advance, switch) in ADVANCE_MODES.iter().zip(SWITCH_MODES.iter().cycle()) {
        let body = format!(
            "start = \"a\"\n[states]\na = \"\"\nb = \"\"\n[[transitions]]\nfrom = \"a\"\nto = \"b\"\nadvance_mode = \"{}\"\nswitch_mode = \"{}\"\n",
            advance.1, switch.1
        );
        let parsed = machine::parse(&toml::from_str(&body).unwrap());
        assert!(parsed.is_ok(), "{} with {}", advance.0, switch.0);
    }
}

#[test]
fn every_ease_constant_names_a_curve_by_its_own_name() {
    let constants = ease_constants();
    assert_eq!(constants.len(), balaur_anim::ease::names().len());
    for (name, word) in constants {
        let curve = Easing::parse(word).unwrap();
        assert_eq!(format!("EASE_{}", curve.name().to_ascii_uppercase()), name);
    }
}

#[test]
fn the_modifier_kinds_are_the_ones_both_modifier_schemas_declare() {
    let mut app = App::new(AppConfig::bare(".")).unwrap();
    balaur_plugin::load(&mut app, &mut AnimationPlugin::default()).unwrap();
    let registry = app.engine.resource::<ComponentRegistry>();
    let registry = registry.borrow();
    let declared: Vec<&str> = MODIFIER_KINDS.iter().map(|(_, word)| *word).collect();
    for component in ["modifier2d", "modifier3d"] {
        let options: Vec<String> = registry
            .def(component)
            .and_then(|def| def.schema.get("kind")?.get("options")?.as_array().cloned())
            .unwrap_or_default()
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect();
        assert_eq!(declared, options, "`{component}.kind`");
    }
}

#[test]
fn every_constant_is_screaming_snake_and_named_once() {
    let mut seen = std::collections::BTreeSet::new();
    let tables = CONSTANTS
        .iter()
        .flat_map(|table| table.iter())
        .map(|(name, word)| ((*name).to_string(), *word));
    for (name, word) in tables.chain(ease_constants()) {
        assert!(
            name.chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'),
            "animation::{name} is not SCREAMING_SNAKE_CASE"
        );
        assert!(!word.is_empty(), "animation::{name} has no value");
        assert!(
            seen.insert(name.clone()),
            "animation::{name} is declared twice"
        );
    }
}
