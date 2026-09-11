//! Touch: the emulation between pointer and finger, the gesture recognisers,
//! and the two controls that feed actions.
//!
//! Driven from Rust rather than a script so the assertions can read the
//! components and the action table directly. Every test here runs headless,
//! which is the point of the controls being components: nothing below needs a
//! window, and none of it would run at all if a control were a widget kind.

use balaur_core::{App, AppConfig, components, facts, scene};
use balaur_input::{
    Gestures, InputActions, InputConfig, InputPlugin, InputSnapshot, TouchButton, TouchStick,
};

const MANIFEST: &str = r#"
[application]
name = "touch test"
main_scene = "main.toml"

[input.actions]
jump = ["Space"]
move_x = ["keys:A,D"]
"#;

/// A booted app with a screen to place controls against: a 1000 x 600 window
/// at one physical pixel per design pixel, and no notch.
fn app(manifest: &str) -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("project.toml"), manifest).unwrap();
    std::fs::write(dir.path().join("main.toml"), "").unwrap();
    let mut app = App::new(AppConfig::bare(dir.path().to_path_buf())).unwrap();
    balaur_plugin::load(&mut app, &mut InputPlugin::default()).unwrap();
    app.load_project().unwrap();
    facts::update_device(&app.engine, |f| {
        f.screen_size = [1000.0, 600.0];
        f.ui_scale = 1.0;
    });
    (dir, app)
}

/// One frame: the backend's `begin_frame`, the test's own events, then the
/// tick that derives the gestures and the actions from them.
fn frame(app: &mut App, events: impl FnOnce(&mut InputSnapshot)) {
    {
        let input = app.engine.resource::<InputSnapshot>();
        let mut input = input.borrow_mut();
        input.begin_frame();
        events(&mut input);
    }
    app.tick(1.0 / 60.0);
}

fn finger(id: u64, x: f32, y: f32, phase: balaur_input::TouchPhase) -> impl Fn(&mut InputSnapshot) {
    move |input: &mut InputSnapshot| input.touch_event(id, x, y, phase)
}

fn value(app: &App, name: &str) -> f32 {
    app.engine.resource::<InputActions>().borrow().value(name)
}

/// A node carrying one component built from TOML.
fn control(app: &App, name: &str, component: &str, params: &str) -> balaur_core::hecs::Entity {
    let root = app.engine.root();
    let entity = scene::spawn_node(&mut app.engine.world_mut(), name, root);
    let params: toml::Value = toml::from_str(params).unwrap();
    components::add(&app.engine, entity, component, Some(&params)).unwrap();
    entity
}

use balaur_input::TouchPhase::{Cancel, End, Move, Start};

/// The default, and the reason every existing widget kind works on a phone:
/// a finger is also a left click at the same place.
#[test]
fn a_finger_moves_the_mouse_by_default() {
    let (_dir, mut app) = app(MANIFEST);
    frame(&mut app, finger(1, 300.0, 200.0, Start));
    {
        let input = app.engine.resource::<InputSnapshot>();
        let input = input.borrow();
        assert_eq!(input.mouse_pos(), (300.0, 200.0));
        assert!(input.is_mouse_down(0), "a finger down is the button down");
        assert!(input.mouse_just_pressed(0), "and the edge fires once");
    }
    frame(&mut app, finger(1, 320.0, 200.0, Move));
    assert_eq!(
        app.engine.resource::<InputSnapshot>().borrow().mouse_pos(),
        (320.0, 200.0)
    );
    frame(&mut app, finger(1, 320.0, 200.0, End));
    let input = app.engine.resource::<InputSnapshot>();
    let input = input.borrow();
    assert!(!input.is_mouse_down(0));
    assert!(input.mouse_just_released(0));
}

/// One cursor, so the second finger is a touch and nothing more. Without this
/// a two-finger gesture would drag the pointer between the fingers.
#[test]
fn only_the_first_finger_drives_the_cursor() {
    let (_dir, mut app) = app(MANIFEST);
    frame(&mut app, finger(1, 100.0, 100.0, Start));
    frame(&mut app, |input| {
        input.touch_event(2, 800.0, 500.0, Start);
        input.touch_event(1, 110.0, 100.0, Move);
    });
    let input = app.engine.resource::<InputSnapshot>();
    let input = input.borrow();
    assert_eq!(input.mouse_pos(), (110.0, 100.0), "the first finger");
    assert_eq!(input.touches().len(), 2);
}

/// A cancelled finger releases the button too: a gesture the system took over
/// must not leave a game holding a click forever.
#[test]
fn a_cancelled_finger_releases_the_button() {
    let (_dir, mut app) = app(MANIFEST);
    frame(&mut app, finger(1, 100.0, 100.0, Start));
    frame(&mut app, finger(1, 100.0, 100.0, Cancel));
    assert!(
        !app.engine
            .resource::<InputSnapshot>()
            .borrow()
            .is_mouse_down(0)
    );
}

/// Off by default, and what lets a desktop drive touch code. A hover is not a
/// touch: only a held button reports a finger.
#[test]
fn a_mouse_can_stand_in_for_a_finger() {
    let manifest = format!("{MANIFEST}\n[input]\nemulate_touch_from_mouse = true\n");
    let (_dir, mut app) = app(&manifest);
    frame(&mut app, |input| input.set_mouse_pos(400.0, 300.0));
    assert!(
        app.engine
            .resource::<InputSnapshot>()
            .borrow()
            .touches()
            .is_empty(),
        "a hovering cursor is not a finger"
    );
    frame(&mut app, |input| input.mouse_button_event(0, true));
    {
        let input = app.engine.resource::<InputSnapshot>();
        let input = input.borrow();
        assert_eq!(input.touches().len(), 1);
        assert_eq!(input.touches()[0].0, balaur_input::EMULATED_TOUCH_ID);
    }
    frame(&mut app, |input| input.set_mouse_pos(450.0, 300.0));
    assert_eq!(
        app.engine.resource::<InputSnapshot>().borrow().touches()[0],
        (balaur_input::EMULATED_TOUCH_ID, 450.0, 300.0)
    );
    frame(&mut app, |input| input.mouse_button_event(0, false));
    assert!(
        app.engine
            .resource::<InputSnapshot>()
            .borrow()
            .touches()
            .is_empty()
    );
}

/// The two conversions must not feed each other: with both on, one press is
/// still one finger and one button.
#[test]
fn the_two_conversions_do_not_loop() {
    let manifest = format!(
        "{MANIFEST}\n[input]\nemulate_touch_from_mouse = true\nemulate_mouse_from_touch = true\n"
    );
    let (_dir, mut app) = app(&manifest);
    frame(&mut app, finger(1, 200.0, 200.0, Start));
    let input = app.engine.resource::<InputSnapshot>();
    let input = input.borrow();
    assert_eq!(input.touches().len(), 1, "the real finger, and no echo");
    assert!(input.is_mouse_down(0));
}

/// A project may turn the default off, for a game that reads both and would
/// otherwise see one press twice.
#[test]
fn emulation_can_be_turned_off() {
    let manifest = format!("{MANIFEST}\n[input]\nemulate_mouse_from_touch = false\n");
    let (_dir, mut app) = app(&manifest);
    frame(&mut app, finger(1, 300.0, 200.0, Start));
    assert!(
        !app.engine
            .resource::<InputSnapshot>()
            .borrow()
            .is_mouse_down(0)
    );
}

const BUTTON: &str = r#"
action = "jump"
anchor = "bottom_right"
offset = [-100.0, -100.0]
width = 120.0
height = 120.0
shape = "circle"
visibility = "always"
"#;

#[test]
fn a_button_presses_its_action() {
    let (_dir, mut app) = app(MANIFEST);
    let entity = control(&app, "Jump", "touch_button", BUTTON);
    // Anchored to the bottom right of 1000 x 600, offset back up and left.
    frame(&mut app, finger(1, 900.0, 500.0, Start));
    assert!((value(&app, "jump") - 1.0).abs() < 1e-6);
    assert!(
        app.engine
            .world()
            .get::<&TouchButton>(entity)
            .unwrap()
            .pressed
    );
    frame(&mut app, finger(1, 900.0, 500.0, End));
    assert!((value(&app, "jump") - 0.0).abs() < 1e-6);
}

/// A finger outside the circle is not on the button, even inside the square
/// the circle is inscribed in.
#[test]
fn a_circle_button_ignores_its_corners() {
    let (_dir, mut app) = app(MANIFEST);
    control(&app, "Jump", "touch_button", BUTTON);
    // The box's corner: inside a rect of the same size, outside the circle.
    frame(&mut app, finger(1, 960.0, 560.0, Start));
    assert!((value(&app, "jump") - 0.0).abs() < 1e-6);
}

/// The finger that pressed it keeps it wherever it goes. A thumb sliding off
/// the edge mid-jump should not drop the jump.
#[test]
fn a_thumb_that_slides_off_keeps_the_button() {
    let (_dir, mut app) = app(MANIFEST);
    control(&app, "Jump", "touch_button", BUTTON);
    frame(&mut app, finger(1, 900.0, 500.0, Start));
    frame(&mut app, finger(1, 400.0, 200.0, Move));
    assert!((value(&app, "jump") - 1.0).abs() < 1e-6, "still held");
    frame(&mut app, finger(1, 400.0, 200.0, End));
    assert!((value(&app, "jump") - 0.0).abs() < 1e-6);
}

/// A control the platform hides takes no fingers either, the way a hidden
/// widget does. This is a desktop, so `touchscreen` is off.
#[test]
fn a_touchscreen_only_button_is_dead_on_a_desktop() {
    let (_dir, mut app) = app(MANIFEST);
    let params = BUTTON.replace("visibility = \"always\"", "visibility = \"touchscreen\"");
    control(&app, "Jump", "touch_button", &params);
    frame(&mut app, finger(1, 900.0, 500.0, Start));
    assert!((value(&app, "jump") - 0.0).abs() < 1e-6);
}

/// The whole point of feeding rather than binding: a key and a button drive
/// one action, and the game reads one name.
#[test]
fn a_key_and_a_button_drive_one_action() {
    let (_dir, mut app) = app(MANIFEST);
    control(&app, "Jump", "touch_button", BUTTON);
    frame(&mut app, |input| input.key_event("Space", true));
    assert!((value(&app, "jump") - 1.0).abs() < 1e-6, "the key alone");
    frame(&mut app, |input| {
        input.key_event("Space", false);
        input.touch_event(1, 900.0, 500.0, Start);
    });
    assert!((value(&app, "jump") - 1.0).abs() < 1e-6, "the button alone");
}

/// An action no manifest declared still answers, because a scene may carry
/// its own controls into a project that never named them.
#[test]
fn a_button_can_feed_an_undeclared_action() {
    let (_dir, mut app) = app(MANIFEST);
    let params = BUTTON.replace("action = \"jump\"", "action = \"crouch\"");
    control(&app, "Crouch", "touch_button", &params);
    frame(&mut app, finger(1, 900.0, 500.0, Start));
    assert!((value(&app, "crouch") - 1.0).abs() < 1e-6);
}

/// The safe area moves a control, so a HUD does not sit under a home bar.
#[test]
fn a_safe_area_moves_a_button() {
    let (_dir, mut app) = app(MANIFEST);
    control(&app, "Jump", "touch_button", BUTTON);
    facts::update_device(&app.engine, |f| f.safe_area = [0.0, 0.0, 0.0, 80.0]);
    // Where it used to be: now 80 pixels below the button's new place, which
    // is still inside a 120-wide circle, so aim at the old centre's old edge.
    frame(&mut app, finger(1, 900.0, 500.0, Start));
    assert!(
        (value(&app, "jump") - 0.0).abs() < 1e-6,
        "the old spot is past the rim now"
    );
    frame(&mut app, |input| {
        input.touch_event(1, 900.0, 500.0, End);
        input.touch_event(2, 900.0, 420.0, Start);
    });
    assert!(
        (value(&app, "jump") - 1.0).abs() < 1e-6,
        "and the new spot answers"
    );
}

/// Design pixels, not physical ones: the same scene on a dense screen puts
/// the control in the same place relative to the glass.
#[test]
fn the_ui_scale_sizes_a_control() {
    let (_dir, mut app) = app(MANIFEST);
    control(&app, "Jump", "touch_button", BUTTON);
    facts::update_device(&app.engine, |f| {
        f.screen_size = [2000.0, 1200.0];
        f.ui_scale = 2.0;
    });
    frame(&mut app, finger(1, 1800.0, 1000.0, Start));
    assert!((value(&app, "jump") - 1.0).abs() < 1e-6);
}

const STICK: &str = r#"
action_x = "move_x"
action_y = "move_y"
anchor = "bottom_left"
offset = [150.0, -150.0]
radius = 100.0
deadzone = 0.0
visibility = "always"
"#;

#[test]
fn a_stick_pushes_its_actions() {
    let (_dir, mut app) = app(MANIFEST);
    control(&app, "Move", "touch_stick", STICK);
    // Centre is (150, 450). Half a throw to the right.
    frame(&mut app, finger(1, 150.0, 450.0, Start));
    frame(&mut app, finger(1, 200.0, 450.0, Move));
    assert!((value(&app, "move_x") - 0.5).abs() < 0.01, "half right");
    assert!(value(&app, "move_y").abs() < 0.01);
}

/// Screen y counts down and a stick does not: a thumb pushed away from the
/// player reads positive, the way `axis:LeftStickY` does.
#[test]
fn a_stick_reads_up_as_positive() {
    let (_dir, mut app) = app(MANIFEST);
    control(&app, "Move", "touch_stick", STICK);
    frame(&mut app, finger(1, 150.0, 450.0, Start));
    frame(&mut app, finger(1, 150.0, 350.0, Move));
    assert!((value(&app, "move_y") - 1.0).abs() < 0.01, "up is +1");
}

/// Past the rim the reading saturates rather than growing without bound.
#[test]
fn a_stick_saturates_at_its_throw() {
    let (_dir, mut app) = app(MANIFEST);
    control(&app, "Move", "touch_stick", STICK);
    frame(&mut app, finger(1, 150.0, 450.0, Start));
    frame(&mut app, finger(1, 900.0, 450.0, Move));
    assert!((value(&app, "move_x") - 1.0).abs() < 0.01);
}

/// A resting thumb reads zero, and the first live reading is near zero rather
/// than jumping to the deadzone's own size.
#[test]
fn a_deadzone_holds_a_resting_thumb_at_rest() {
    let (_dir, mut app) = app(MANIFEST);
    let params = STICK.replace("deadzone = 0.0", "deadzone = 0.5");
    control(&app, "Move", "touch_stick", &params);
    frame(&mut app, finger(1, 150.0, 450.0, Start));
    frame(&mut app, finger(1, 180.0, 450.0, Move));
    assert!(
        (value(&app, "move_x") - 0.0).abs() < 1e-6,
        "inside the deadzone"
    );
    frame(&mut app, finger(1, 206.0, 450.0, Move));
    let just_live = value(&app, "move_x");
    assert!(
        just_live > 0.0 && just_live < 0.2,
        "the first live reading is near zero, not 0.5: {just_live}"
    );
}

/// A stick that recentres puts itself under the thumb that found it, so an
/// off-centre grab does not read as a push.
#[test]
fn a_recentring_stick_starts_where_the_thumb_landed() {
    let (_dir, mut app) = app(MANIFEST);
    let params = format!("{STICK}recenter = true\n");
    control(&app, "Move", "touch_stick", &params);
    // Inside the circle, well off its centre.
    frame(&mut app, finger(1, 220.0, 450.0, Start));
    assert!(
        value(&app, "move_x").abs() < 0.01,
        "the grab itself is not a push"
    );
    frame(&mut app, finger(1, 270.0, 450.0, Move));
    assert!((value(&app, "move_x") - 0.5).abs() < 0.01, "and then it is");
}

#[test]
fn a_stick_lets_go_when_the_thumb_lifts() {
    let (_dir, mut app) = app(MANIFEST);
    let entity = control(&app, "Move", "touch_stick", STICK);
    frame(&mut app, finger(1, 150.0, 450.0, Start));
    frame(&mut app, finger(1, 250.0, 450.0, Move));
    assert!(value(&app, "move_x") > 0.9);
    frame(&mut app, finger(1, 250.0, 450.0, End));
    assert!((value(&app, "move_x") - 0.0).abs() < 1e-6);
    let world = app.engine.world();
    let stick = world.get::<&TouchStick>(entity).unwrap();
    assert!(stick.finger.is_none());
    assert!(
        stick.value.iter().all(|v| v.abs() < 1e-6),
        "at rest: {:?}",
        stick.value
    );
}

/// A finger that lands outside the circle is somebody else's: the stick is
/// not a whole-screen drag target.
#[test]
fn a_stick_ignores_a_finger_outside_it() {
    let (_dir, mut app) = app(MANIFEST);
    control(&app, "Move", "touch_stick", STICK);
    frame(&mut app, finger(1, 700.0, 200.0, Start));
    frame(&mut app, finger(1, 750.0, 200.0, Move));
    assert!((value(&app, "move_x") - 0.0).abs() < 1e-6);
}

/// The keys the project bound and the stick both drive `move_x`, and the
/// furthest from rest wins.
#[test]
fn a_stick_and_a_key_pair_share_an_action() {
    let (_dir, mut app) = app(MANIFEST);
    control(&app, "Move", "touch_stick", STICK);
    frame(&mut app, |input| input.key_event("D", true));
    assert!((value(&app, "move_x") - 1.0).abs() < 1e-6, "the key alone");
    frame(&mut app, |input| {
        input.key_event("D", false);
        input.touch_event(1, 150.0, 450.0, Start);
    });
    frame(&mut app, finger(1, 100.0, 450.0, Move));
    assert!(
        value(&app, "move_x") < -0.4,
        "and the thumb alone, the other way"
    );
}

fn gestures<T>(app: &App, read: impl FnOnce(&Gestures) -> T) -> T {
    read(&app.engine.resource::<Gestures>().borrow())
}

#[test]
fn two_fingers_spreading_are_a_pinch() {
    let (_dir, mut app) = app(MANIFEST);
    frame(&mut app, |input| {
        input.touch_event(1, 400.0, 300.0, Start);
        input.touch_event(2, 600.0, 300.0, Start);
    });
    assert!(gestures(&app, |g| g.pinch().is_none()), "no history yet");
    frame(&mut app, |input| {
        input.touch_event(1, 300.0, 300.0, Move);
        input.touch_event(2, 700.0, 300.0, Move);
    });
    let pinch = gestures(&app, Gestures::pinch).expect("two fingers moved apart");
    assert!((pinch.scale - 2.0).abs() < 0.01, "200 apart became 400");
    assert!((pinch.center.0 - 500.0).abs() < 0.01);
}

#[test]
fn one_finger_is_never_a_pinch() {
    let (_dir, mut app) = app(MANIFEST);
    frame(&mut app, finger(1, 400.0, 300.0, Start));
    frame(&mut app, finger(1, 500.0, 300.0, Move));
    assert!(gestures(&app, |g| g.pinch().is_none()));
    assert_eq!(gestures(&app, Gestures::pan), (0.0, 0.0));
}

#[test]
fn two_fingers_moving_together_are_a_pan() {
    let (_dir, mut app) = app(MANIFEST);
    frame(&mut app, |input| {
        input.touch_event(1, 400.0, 300.0, Start);
        input.touch_event(2, 500.0, 300.0, Start);
    });
    frame(&mut app, |input| {
        input.touch_event(1, 400.0, 260.0, Move);
        input.touch_event(2, 500.0, 260.0, Move);
    });
    let (dx, dy) = gestures(&app, Gestures::pan);
    assert!(dx.abs() < 0.01);
    assert!((dy + 40.0).abs() < 0.01, "both fingers went up 40");
}

/// Reported on the frame the finger lifts, and never again.
#[test]
fn a_travelled_finger_is_a_swipe_when_it_lifts() {
    let (_dir, mut app) = app(MANIFEST);
    frame(&mut app, finger(1, 200.0, 300.0, Start));
    frame(&mut app, finger(1, 600.0, 300.0, Move));
    assert!(gestures(&app, |g| g.swipe().is_none()), "still down");
    frame(&mut app, finger(1, 600.0, 300.0, End));
    let swipe = gestures(&app, Gestures::swipe).expect("400 pixels is a swipe");
    assert!((swipe.direction.0 - 1.0).abs() < 0.01, "to the right");
    assert!(swipe.speed > 0.0);
    frame(&mut app, |_| {});
    assert!(gestures(&app, |g| g.swipe().is_none()), "once only");
}

#[test]
fn a_tap_is_not_a_swipe() {
    let (_dir, mut app) = app(MANIFEST);
    frame(&mut app, finger(1, 200.0, 300.0, Start));
    frame(&mut app, finger(1, 203.0, 302.0, Move));
    frame(&mut app, finger(1, 203.0, 302.0, End));
    assert!(gestures(&app, |g| g.swipe().is_none()));
}

/// Once per finger, on the frame the hold passes its threshold.
#[test]
fn a_still_finger_becomes_a_long_press() {
    let (_dir, mut app) = app(MANIFEST);
    let seconds = app
        .engine
        .resource::<InputConfig>()
        .borrow()
        .long_press_seconds;
    frame(&mut app, finger(1, 400.0, 300.0, Start));
    let mut held = 0.0;
    let mut fired = 0;
    while held < seconds + 0.2 {
        frame(&mut app, finger(1, 400.0, 300.0, Move));
        held += 1.0 / 60.0;
        if gestures(&app, |g| g.long_press().is_some()) {
            fired += 1;
        }
    }
    assert_eq!(fired, 1, "a hold is announced once, not every frame after");
}

/// A finger that wandered was dragging, and a drag that pauses is still a
/// drag rather than a hold.
#[test]
fn a_wandering_finger_is_not_a_long_press() {
    let (_dir, mut app) = app(MANIFEST);
    frame(&mut app, finger(1, 400.0, 300.0, Start));
    frame(&mut app, finger(1, 500.0, 300.0, Move));
    for _ in 0..60 {
        frame(&mut app, finger(1, 500.0, 300.0, Move));
        assert!(gestures(&app, |g| g.long_press().is_none()));
    }
}

/// Every reading is neutral with nothing happening, so a desktop and a
/// headless run answer the same as a still screen.
#[test]
fn a_screen_nobody_touched_reads_neutral() {
    let (_dir, mut app) = app(MANIFEST);
    frame(&mut app, |_| {});
    gestures(&app, |g| {
        assert!(g.pinch().is_none());
        assert!(g.swipe().is_none());
        assert!(g.long_press().is_none());
        assert_eq!(g.pan(), (0.0, 0.0));
    });
}

/// A control with no screen behind it is neutral rather than placed at the
/// origin, where every finger would land on it.
#[test]
fn a_control_with_no_screen_takes_nothing() {
    let (_dir, mut app) = app(MANIFEST);
    control(&app, "Jump", "touch_button", BUTTON);
    facts::update_device(&app.engine, |f| f.screen_size = [0.0, 0.0]);
    frame(&mut app, finger(1, 0.0, 0.0, Start));
    assert!((value(&app, "jump") - 0.0).abs() < 1e-6);
}

/// In the editor a game is confined to the viewport, and its controls go with
/// it rather than to the window's corners.
#[test]
fn a_control_sits_in_the_game_area_the_host_confines_it_to() {
    let (_dir, mut app) = app(MANIFEST);
    control(&app, "Jump", "touch_button", BUTTON);
    // A 600 x 400 viewport at (200, 100): its bottom right is (800, 500).
    facts::update_device(&app.engine, |f| f.game_area = Some([200.0, 100.0, 600.0, 400.0]));
    frame(&mut app, finger(1, 900.0, 500.0, Start));
    assert!(value(&app, "jump").abs() < 1e-6, "the window's corner is not the game's");
    frame(&mut app, |input| {
        input.touch_event(1, 900.0, 500.0, End);
        input.touch_event(2, 700.0, 400.0, Start);
    });
    assert!((value(&app, "jump") - 1.0).abs() < 1e-6, "the viewport's corner is");
}

/// A host that switched the game's surface off, as the editor does while
/// nothing plays, leaves the controls dead.
#[test]
fn a_control_is_dead_while_the_game_area_is_off() {
    let (_dir, mut app) = app(MANIFEST);
    control(&app, "Jump", "touch_button", BUTTON);
    facts::update_device(&app.engine, |f| f.game_area = Some([0.0; 4]));
    frame(&mut app, finger(1, 900.0, 500.0, Start));
    assert!(value(&app, "jump").abs() < 1e-6);
}

/// A project that made the mouse a finger gets its touch-only controls on a
/// desktop too: the mouse can reach them now.
#[test]
fn emulating_touch_shows_touchscreen_only_controls() {
    let manifest = format!("{MANIFEST}\n[input]\nemulate_touch_from_mouse = true\n");
    let (_dir, mut app) = app(&manifest);
    let params = BUTTON.replace("visibility = \"always\"", "visibility = \"touchscreen\"");
    control(&app, "Jump", "touch_button", &params);
    frame(&mut app, |input| {
        input.set_mouse_pos(900.0, 500.0);
        input.mouse_button_event(0, true);
    });
    assert!((value(&app, "jump") - 1.0).abs() < 1e-6);
}
