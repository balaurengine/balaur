//! Edge semantics: `just_pressed` is true for exactly one frame, `is_down`
//! for as long as the key is held. Getting this wrong makes a game feel
//! broken in ways that are hard to trace back.

use balaur_input::{InputSnapshot, KEY_NAMES, MOUSE_BUTTON_CONSTANTS};

#[test]
fn a_press_is_just_pressed_for_one_frame_only() {
    let mut input = InputSnapshot::default();
    input.key_event("Space", true);
    assert!(input.just_pressed("Space"));
    assert!(input.is_down("Space"));

    input.begin_frame();
    assert!(
        !input.just_pressed("Space"),
        "still just-pressed a frame later"
    );
    assert!(input.is_down("Space"), "the key is still held");
}

#[test]
fn a_release_is_just_released_for_one_frame_only() {
    let mut input = InputSnapshot::default();
    input.key_event("Space", true);
    input.begin_frame();
    input.key_event("Space", false);
    assert!(input.just_released("Space"));
    assert!(!input.is_down("Space"));

    input.begin_frame();
    assert!(!input.just_released("Space"));
}

#[test]
fn holding_a_key_does_not_re_fire() {
    let mut input = InputSnapshot::default();
    input.key_event("A", true);
    for frame in 0..5 {
        input.begin_frame();
        input.key_event("A", true); // the OS repeats while held
        assert!(!input.just_pressed("A"), "re-fired on frame {frame}");
        assert!(input.is_down("A"));
    }
}

#[test]
fn keys_are_independent() {
    let mut input = InputSnapshot::default();
    input.key_event("A", true);
    input.key_event("B", true);
    input.begin_frame();
    input.key_event("A", false);
    assert!(!input.is_down("A"));
    assert!(input.is_down("B"));
}

#[test]
fn an_unknown_key_is_simply_not_down() {
    let input = InputSnapshot::default();
    assert!(!input.is_down("Spcae"));
    assert!(!input.just_pressed(""));
}

#[test]
fn mouse_buttons_follow_the_same_edge_rules() {
    let mut input = InputSnapshot::default();
    input.mouse_button_event(0, true);
    assert!(input.is_mouse_down(0));
    assert!(input.mouse_just_pressed(0));
    assert!(
        !input.is_mouse_down(1),
        "the right button is not the left one"
    );

    input.begin_frame();
    assert!(input.is_mouse_down(0));
    assert!(!input.mouse_just_pressed(0));
}

#[test]
fn a_released_mouse_button_reports_one_frame_of_release() {
    let mut input = InputSnapshot::default();
    input.mouse_button_event(0, true);
    assert!(
        !input.mouse_just_released(0),
        "it was pressed, not released"
    );

    input.begin_frame();
    input.mouse_button_event(0, false);
    assert!(input.mouse_just_released(0));
    assert!(!input.is_mouse_down(0));

    input.begin_frame();
    assert!(!input.mouse_just_released(0), "the edge did not reset");
    assert!(!input.mouse_just_released(999), "out of range is quiet");
}

#[test]
fn an_out_of_range_button_does_not_panic() {
    let mut input = InputSnapshot::default();
    input.mouse_button_event(999, true);
    assert!(!input.is_mouse_down(999));
}

#[test]
fn mouse_delta_is_per_frame_and_position_is_absolute() {
    let mut input = InputSnapshot::default();
    input.set_mouse_pos(10.0, 10.0);
    input.begin_frame();
    input.set_mouse_pos(13.0, 14.0);
    assert_eq!(input.mouse_pos(), (13.0, 14.0));
    assert_eq!(input.mouse_delta(), (3.0, 4.0));

    input.begin_frame();
    assert_eq!(input.mouse_delta(), (0.0, 0.0), "delta did not reset");
    assert_eq!(input.mouse_pos(), (13.0, 14.0), "position is not per frame");
}

#[test]
fn the_mouse_constants_address_real_buttons() {
    let mut input = InputSnapshot::default();
    for (name, index) in MOUSE_BUTTON_CONSTANTS {
        let i = usize::try_from(*index).unwrap();
        input.mouse_button_event(i, true);
        assert!(
            input.is_mouse_down(i),
            "{name} does not address a tracked button"
        );
        input.mouse_button_event(i, false);
    }
}

#[test]
fn every_named_key_can_actually_be_pressed() {
    let mut input = InputSnapshot::default();
    for key in KEY_NAMES {
        input.key_event(key, true);
        assert!(input.is_down(key), "{key} is named but does not register");
        input.key_event(key, false);
    }
}

#[test]
fn scroll_accumulates_within_a_frame_and_resets_between() {
    let mut input = InputSnapshot::default();
    input.add_scroll(1.0, 2.0);
    input.add_scroll(0.5, 0.5);
    assert_eq!(
        input.scroll_delta(),
        (1.5, 2.5),
        "scroll did not accumulate"
    );
    input.begin_frame();
    assert_eq!(input.scroll_delta(), (0.0, 0.0), "scroll did not reset");
}
