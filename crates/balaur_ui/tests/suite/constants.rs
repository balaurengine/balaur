//! `ui.ANCHOR_*` and friends must name the same strings the widget layer and
//! the scene schema accept. Written in two places, so check they agree.

use balaur_core::components::ComponentRegistry;
use balaur_core::{App, AppConfig};
use balaur_ui::{ALIGNS, ANCHORS, FONT_STYLES, FONTS, MODIFIERS, PILL_ALIGNS, WIDGET_KINDS};

fn registered_options(field: &str) -> Vec<String> {
    let mut app = App::new(AppConfig::bare(".")).unwrap();
    balaur_plugin::load(&mut app, &mut balaur_ui::UiPlugin::default()).unwrap();
    let registry = app.engine.resource::<ComponentRegistry>();
    let registry = registry.borrow();
    registry
        .def("widget")
        .expect("`widget` is registered")
        .schema
        .get(field)
        .and_then(|f| f.get("options"))
        .and_then(toml::Value::as_array)
        .unwrap_or_else(|| panic!("`widget.{field}` declares enum options"))
        .iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect()
}

#[test]
fn anchor_constants_match_the_registered_schema() {
    let declared: Vec<&str> = ANCHORS.iter().map(|(_, v)| *v).collect();
    assert_eq!(declared, registered_options("anchor"));
}

#[test]
fn widget_kind_constants_match_the_registered_schema() {
    let declared: Vec<&str> = WIDGET_KINDS.iter().map(|(_, v)| *v).collect();
    assert_eq!(declared, registered_options("kind"));
}

#[test]
fn align_and_font_style_constants_match_the_registered_schema() {
    let aligns: Vec<&str> = ALIGNS.iter().map(|(_, v)| *v).collect();
    assert_eq!(aligns, registered_options("align"));
    assert_eq!(aligns, registered_options("text_align"));
    let styles: Vec<&str> = FONT_STYLES.iter().map(|(_, v)| *v).collect();
    assert_eq!(styles, registered_options("font_style"));
}

#[test]
fn every_constant_is_screaming_snake_and_unique() {
    let mut seen = std::collections::BTreeSet::new();
    for (name, value) in ANCHORS
        .iter()
        .chain(WIDGET_KINDS)
        .chain(ALIGNS)
        .chain(PILL_ALIGNS)
        .chain(FONT_STYLES)
        .chain(FONTS)
        .chain(MODIFIERS)
    {
        assert!(
            name.chars().all(|c| c.is_ascii_uppercase() || c == '_'),
            "{name} is not SCREAMING_SNAKE_CASE"
        );
        assert!(seen.insert(*name), "{name} is declared twice");
        assert!(!value.is_empty(), "{name} has no value");
    }
}
