use super::{Pointer, WidgetState, parse, parse_color};

const CARDS: &str = "type = \"widget_theme\"\n\n[colors]\nprimary = \"#994f7d\"\n\n[roles.card]\nfill = \"#ffffff\"\ngradient = { stops = [\"#000000\", \"#ffffff\"], angle = 0 }\nshadow = { y = 2, blur = 8, color = \"#00000040\" }\n\n[roles.card.hover]\ngradient = { stops = [\"#ff0000\", \"#0000ff\"], angle = 90 }\n\n[roles.half]\ngradient = { stops = [\"#ffffff\"] }\n\n[roles.glow]\ngradient = { kind = \"radial\", stops = [\"primary\", \"#00ff00\", \"#000000\"], at = [0, 0.25, 1], center = [0.5, 0.25], radius = 0.75 }\n";

#[test]
fn a_role_reads_its_gradient_and_shadow_and_a_state_its_own() {
    use super::GradientShape;
    let theme = parse(&toml::from_str::<toml::Value>(CARDS).unwrap());
    let card = theme.resolved("panel", "card");
    let g = card.gradient.clone().expect("the role's gradient");
    assert_eq!(g.shape, GradientShape::Linear { angle: 0.0 });
    assert_eq!(
        g.stops,
        vec![
            (0.0, parse_color("#000000").unwrap()),
            (1.0, parse_color("#ffffff").unwrap())
        ]
    );
    let s = card.shadow.expect("the role's shadow");
    assert_eq!((s.x, s.y, s.blur, s.spread), (0.0, 2.0, 8.0, 0.0));
    let hovered = card.in_states(WidgetState {
        pointer: Pointer::Over,
        ..WidgetState::default()
    });
    assert_eq!(
        hovered.gradient.clone().expect("the hover table's").shape,
        GradientShape::Linear { angle: 90.0 }
    );
    assert!(hovered.shadow.is_some(), "a state keeps what it leaves out");
    assert!(
        theme.resolved("panel", "half").gradient.is_none(),
        "a gradient with one colour is none"
    );
}

#[test]
fn a_radial_gradient_places_its_stops_where_at_says() {
    use super::GradientShape;
    let theme = parse(&toml::from_str::<toml::Value>(CARDS).unwrap());
    let glow = theme
        .resolved("panel", "glow")
        .gradient
        .clone()
        .expect("the radial gradient");
    assert_eq!(
        glow.shape,
        GradientShape::Radial {
            center: [0.5, 0.25],
            radius: 0.75
        }
    );
    let places: Vec<f32> = glow.stops.iter().map(|(at, _)| *at).collect();
    assert_eq!(places, vec![0.0, 0.25, 1.0]);
    assert_eq!(
        glow.at(0.0),
        parse_color("#994f7d").unwrap(),
        "a token names a stop"
    );
    assert_eq!(glow.at(0.25), parse_color("#00ff00").unwrap());
    assert_eq!(
        glow.at(2.0),
        parse_color("#000000").unwrap(),
        "past the last is the last"
    );
}

#[test]
fn a_disabled_or_focused_widget_takes_the_state_table_over_its_kind() {
    let text = "type = \"widget_theme\"\n\n[button]\nfill = \"#ffffff\"\n\n[button.disabled]\nfill = \"#101010\"\n\n[button.focus]\nfill = \"#2020ff\"\n";
    let theme = parse(&toml::from_str::<toml::Value>(text).unwrap());
    let button = theme.resolved("button", "");
    assert_eq!(button.fill, parse_color("#ffffff"));
    assert_eq!(
        button
            .in_states(WidgetState {
                disabled: true,
                ..WidgetState::default()
            })
            .fill,
        parse_color("#101010"),
        "disabled wins"
    );
    assert_eq!(
        button
            .in_states(WidgetState {
                focused: true,
                ..WidgetState::default()
            })
            .fill,
        parse_color("#2020ff"),
        "focus with the pointer elsewhere"
    );
    assert_eq!(
        button
            .in_states(WidgetState {
                pointer: Pointer::Over,
                focused: true,
                ..WidgetState::default()
            })
            .fill,
        parse_color("#ffffff"),
        "the pointer over it and no hover table: the kind's own look"
    );
}
