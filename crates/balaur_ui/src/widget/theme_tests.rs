use super::{Pointer, WidgetState, parse, parse_color};

const CARDS: &str = "type = \"widget_theme\"\n\n[colors]\nprimary = \"#994f7d\"\n\n[roles.card]\nfill = \"#ffffff\"\ngradient = { from = \"#000000\", to = \"#ffffff\", angle = 0 }\nshadow = { y = 2, blur = 8, color = \"#00000040\" }\n\n[roles.card.hover]\ngradient = { from = \"#ff0000\", to = \"#0000ff\", angle = 90 }\n\n[roles.half]\ngradient = { to = \"#ffffff\" }\n";

#[test]
fn a_role_reads_its_gradient_and_shadow_and_a_state_its_own() {
    let theme = parse(&toml::from_str::<toml::Value>(CARDS).unwrap());
    let card = theme.resolved("panel", "card");
    let g = card.gradient.expect("the role's gradient");
    assert_eq!(
        (g.from, g.to, g.angle),
        (
            parse_color("#000000").unwrap(),
            parse_color("#ffffff").unwrap(),
            0.0
        )
    );
    let s = card.shadow.expect("the role's shadow");
    assert_eq!((s.x, s.y, s.blur, s.spread), (0.0, 2.0, 8.0, 0.0));
    let hovered = card.in_states(WidgetState {
        pointer: Pointer::Over,
        ..WidgetState::default()
    });
    assert!((hovered.gradient.expect("the hover table's").angle - 90.0).abs() < f32::EPSILON);
    assert!(hovered.shadow.is_some(), "a state keeps what it leaves out");
    assert!(
        theme.resolved("panel", "half").gradient.is_none(),
        "a gradient with one colour is none"
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
