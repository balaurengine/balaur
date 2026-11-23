use super::{Pointer, WidgetState, parse, parse_color};

const CARDS: &str = "type = \"widget_theme\"\n\n[colors]\nprimary = \"#994f7d\"\n\n[roles.card]\nfill = \"#ffffff\"\ngradient = { stops = [\"#000000\", \"#ffffff\"], angle = 0 }\nshadows = [{ y = 2, blur = 8, color = \"#00000040\" }]\n\n[roles.card.hover]\ngradient = { stops = [\"#ff0000\", \"#0000ff\"], angle = 90 }\n\n[roles.half]\ngradient = { stops = [\"#ffffff\"] }\n\n[roles.glow]\ngradient = { kind = \"radial\", stops = [\"primary\", \"#00ff00\", \"#000000\"], at = [0, 0.25, 1], center = [0.5, 0.25], radius = 0.75 }\n";

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
    let s = card.shadows.clone().expect("the role's shadows")[0];
    assert_eq!((s.x, s.y, s.blur, s.spread), (0.0, 2.0, 8.0, 0.0));
    let hovered = card.in_states(WidgetState {
        pointer: Pointer::Over,
        ..WidgetState::default()
    });
    assert_eq!(
        hovered.gradient.clone().expect("the hover table's").shape,
        GradientShape::Linear { angle: 90.0 }
    );
    assert!(
        hovered.shadows.is_some(),
        "a state keeps what it leaves out"
    );
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
        parse_color("#2020ff"),
        "the pointer over it and no hover table: still focused"
    );
}

#[test]
fn a_transition_is_read_on_the_kind_and_not_inside_a_state_table() {
    let text = "type = \"widget_theme\"\n\n[button]\ntransition = 0.2\n\n[button.hover]\ntransition = 0.05\nfill = \"#eeeeee\"\n\n[button.checked]\ntransition = 0.05\n";
    let theme = parse(&toml::from_str::<toml::Value>(text).unwrap());
    let button = theme.resolved("button", "");
    assert_eq!(button.transition, Some(0.2));
    let hovered = button.in_states(WidgetState {
        pointer: Pointer::Over,
        checked: true,
        ..WidgetState::default()
    });
    assert_eq!(
        hovered.transition,
        Some(0.2),
        "a state table's own is dropped, with a warning naming where it goes"
    );
}

#[test]
fn focus_and_the_pointer_apply_together_as_on_the_web() {
    let text = "type = \"widget_theme\"\n\n[button]\nfill = \"#ffffff\"\nstroke = \"#000000\"\n\n[button.hover]\nfill = \"#eeeeee\"\n\n[button.focus]\nfill = \"#2020ff\"\noutline = \"#ff8800\"\n\n[button.disabled]\nfill = \"#101010\"\n";
    let theme = parse(&toml::from_str::<toml::Value>(text).unwrap());
    let button = theme.resolved("button", "");
    let both = button.in_states(WidgetState {
        pointer: Pointer::Over,
        focused: true,
        ..WidgetState::default()
    });
    assert_eq!(
        both.fill,
        parse_color("#eeeeee"),
        "the pointer's table wins what both state"
    );
    assert_eq!(
        both.outline,
        parse_color("#ff8800"),
        "and the focus ring stays"
    );
    assert_eq!(
        both.stroke,
        parse_color("#000000"),
        "an outline leaves the edge as it is"
    );
    let off = button.in_states(WidgetState {
        pointer: Pointer::Over,
        focused: true,
        disabled: true,
        ..WidgetState::default()
    });
    assert_eq!(off.fill, parse_color("#101010"), "disabled is over neither");
    assert_eq!(off.outline, None);
}

#[test]
fn shadows_are_a_list_inset_ones_included_and_an_empty_one_takes_them_away() {
    let text = "type = \"widget_theme\"\n\n[button]\nshadows = [{ y = 3, blur = 2, spread = -2, color = \"#994f7d4d\" }, { inset = true, y = 0.5, spread = 0.5, color = \"#ffffff0f\" }]\n\n[button.active]\nshadows = []\n";
    let theme = parse(&toml::from_str::<toml::Value>(text).unwrap());
    let button = theme.resolved("button", "");
    let list = button.shadows.clone().expect("the kind's shadows");
    assert_eq!(list.len(), 2);
    assert_eq!(
        (list[0].y, list[0].spread, list[0].inset),
        (3.0, -2.0, false)
    );
    assert_eq!((list[1].y, list[1].spread, list[1].inset), (0.5, 0.5, true));
    let held = button.in_states(WidgetState {
        pointer: Pointer::Held,
        ..WidgetState::default()
    });
    assert!(
        held.shadows.as_deref().is_some_and(<[_]>::is_empty),
        "an empty list states none, rather than leaving the kind's"
    );
    assert!(!held.paints());
}

#[test]
fn shadows_written_as_one_table_are_not_read() {
    let text = "type = \"widget_theme\"\n\n[button]\nshadows = { y = 2, color = \"#000000\" }\n";
    let theme = parse(&toml::from_str::<toml::Value>(text).unwrap());
    assert!(theme.resolved("button", "").shadows.is_none());
}

#[test]
fn shadow_lists_ease_shadow_by_shadow() {
    use super::{Shadow, Style};
    let lift = |y: f32, inset: bool| Shadow {
        x: 0.0,
        y,
        blur: 0.0,
        spread: 0.0,
        color: egui::Color32::BLACK,
        inset,
    };
    let rest = Style {
        shadows: Some(vec![lift(4.0, false), lift(1.0, true)].into()),
        ..Style::default()
    };
    let pressed = Style {
        shadows: Some(Vec::new().into()),
        ..Style::default()
    };
    let half = rest.toward(&pressed, 0.5).shadows.expect("on its way");
    assert_eq!(half.len(), 2, "each runs to clear");
    assert!((half[0].y - 4.0).abs() < f32::EPSILON);
    assert!(half[0].color.a() > 0 && half[0].color.a() < 255);
    assert!(half[1].inset, "and stays inside");
    let swapped = Style {
        shadows: Some(vec![lift(1.0, true)].into()),
        ..Style::default()
    };
    let early = rest.toward(&swapped, 0.25).shadows.expect("a list");
    assert_eq!(
        early.len(),
        2,
        "inset at different places steps, at halfway"
    );
    assert_eq!(
        rest.toward(&swapped, 0.75).shadows.expect("a list").len(),
        1
    );
}

#[test]
fn a_role_spaces_and_cases_its_letters_and_a_state_table_cannot() {
    let text = "type = \"widget_theme\"\n\n[roles.eyebrow]\nletter_spacing = 2.4\ntext_transform = \"uppercase\"\n\n[roles.eyebrow.hover]\nletter_spacing = 9\ntext_transform = \"lowercase\"\n\n[roles.shout]\ntext_transform = \"loud\"\n";
    let theme = parse(&toml::from_str::<toml::Value>(text).unwrap());
    let eyebrow = theme.resolved("label", "eyebrow");
    assert_eq!(eyebrow.letter_spacing, Some(2.4));
    assert_eq!(eyebrow.text_transform, Some(balaur_text::Case::Upper));
    let hovered = eyebrow.in_states(WidgetState {
        pointer: Pointer::Over,
        ..WidgetState::default()
    });
    assert_eq!(
        (hovered.letter_spacing, hovered.text_transform),
        (Some(2.4), Some(balaur_text::Case::Upper)),
        "a state table never resizes what sits beside the widget"
    );
    assert_eq!(theme.resolved("label", "shout").text_transform, None);
}

#[test]
fn a_focus_ring_s_width_and_gap_and_a_spinner_s_turn_are_the_theme_s() {
    let text = "type = \"widget_theme\"\n\n[button.focus]\noutline_width = 3\noutline_offset = 4\n\n[spinner]\nduration = 1.5\n";
    let theme = parse(&toml::from_str::<toml::Value>(text).unwrap());
    let focused = theme.resolved("button", "").in_states(WidgetState {
        focused: true,
        ..WidgetState::default()
    });
    assert_eq!(focused.ring(), (4.0, 3.0));
    assert_eq!(
        theme.resolved("label", "").ring(),
        (2.0, 2.0),
        "2 and 2 unset"
    );
    assert_eq!(theme.resolved("spinner", "").duration, Some(1.5));
}

// A tab's look is the theme's alone: its parts stacked, its icon sized apart
// from its caption, and its edge on the sides the role names.
#[test]
fn a_role_stacks_its_face_sizes_its_icon_and_names_its_edge_s_sides() {
    let theme = parse(
        &toml::from_str::<toml::Value>(
            "[roles.tab]\nflex_direction = \"column\"\nicon_size = 24\n\n[roles.bar]\nstroke = \"#000000\"\nborder = [0, 1, 0, 0]\n",
        )
        .unwrap(),
    );
    let tab = theme.resolved("button", "tab");
    assert!(tab.stacked());
    assert_eq!(tab.icon_size, Some(24.0));
    assert!(!theme.resolved("button", "").stacked(), "a row unset");
    assert_eq!(
        theme.resolved("row", "bar").border,
        Some([0.0, 1.0, 0.0, 0.0])
    );
    assert_eq!(theme.resolved("row", "").border, None);
}

#[test]
fn a_role_s_fill_of_none_is_transparent_and_its_dashed_outline_stays() {
    balaur_core::logbuf::capture_for_test();
    let doc = "type = \"widget_theme\"\n\n[roles.outline]\nfill = \"none\"\nstroke = \"#336699\"\ndashed = true\n";
    let theme = parse(&toml::from_str::<toml::Value>(doc).unwrap());
    let outline = theme.resolved("panel", "outline");
    assert_eq!(outline.fill, Some(egui::Color32::TRANSPARENT));
    assert_eq!(outline.stroke, parse_color("#336699"));
    assert_eq!(
        outline.dashed,
        Some(true),
        "control: the rest of the role was read"
    );
    let warned: Vec<String> = balaur_core::logbuf::heard()
        .into_iter()
        .filter(|e| e.level == "warn" && e.message.contains("none"))
        .map(|e| e.message)
        .collect();
    assert!(
        warned.is_empty(),
        "`none` was reported as no colour: {warned:?}"
    );
}

#[test]
fn a_project_word_s_table_folds_over_the_screen_s_and_its_own_nested_ones_last() {
    use crate::widget::class_words::{Words, declare, word};
    declare(&["tt_small".to_owned()]);
    let small = word("tt_small").expect("declared");
    let theme = parse(
        &toml::from_str::<toml::Value>(
            "type = \"widget_theme\"\n\n[roles.title]\nfont_size = 30\n\n[roles.title.wide]\nfont_size = 36\n\n[roles.title.tt_small]\nfont_size = 24\nshow_text = false\n\n[roles.title.tt_small.wide]\nfont_size = 30\n",
        )
        .unwrap(),
    );
    let narrow = Words::of([balaur_core::facts::NARROW, small]);
    let wide = Words::of([balaur_core::facts::WIDE, small]);
    assert_eq!(
        theme.resolved_in("label", "title", narrow).font_size,
        Some(24.0)
    );
    assert_eq!(
        theme.resolved_in("label", "title", wide).font_size,
        Some(30.0)
    );
    assert_eq!(
        theme.resolved_in("label", "title", wide).show_text,
        Some(false)
    );
    let plain = Words::of([balaur_core::facts::WIDE]);
    assert_eq!(
        theme.resolved_in("label", "title", plain).font_size,
        Some(36.0)
    );
    assert_eq!(theme.resolved_in("label", "title", plain).show_text, None);
}

#[test]
fn a_theme_names_the_icons_a_mirrored_ui_turns_round() {
    let theme = parse(
        &toml::from_str::<toml::Value>(
            "type = \"widget_theme\"\n\n[icons]\nmirrored = [\"chevron-right\", \"arrow-left\"]\n",
        )
        .unwrap(),
    );
    assert!(theme.mirrors_icon("chevron-right") && theme.mirrors_icon("arrow-left"));
    assert!(
        !theme.mirrors_icon("magnifying-glass"),
        "a glyph it does not list keeps its way"
    );
    assert!(!theme.mirrors_icon(""), "no icon is none to turn");
    assert!(
        theme.style("icons").fill.is_none(),
        "the table is no widget kind's"
    );
}
