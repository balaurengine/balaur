//! The two questions a layout asks about the screen it got: how it is driven,
//! and how much room there is.
//!
//! The first is a tag, because it cannot change while a run lasts and a
//! recording carries it. The second is not, because a rotation changes it.

use balaur_core::facts::{
    ClassLines, HEIGHT_CLASSES, MEDIUM, NARROW, SHORT, TALL, WIDE, WIDTH_CLASSES, height_class,
    width_class,
};
use balaur_core::tags::{POINTER, TOUCH, Tags};

/// Either side of each line, at the defaults Android's window size classes
/// put them at.
#[test]
fn each_width_answers_the_word_its_side_of_the_line_does() {
    let lines = ClassLines::default();
    for (width, want) in [
        (320.0, NARROW),
        (599.0, NARROW),
        (600.0, MEDIUM),
        (839.0, MEDIUM),
        (840.0, WIDE),
        (1920.0, WIDE),
    ] {
        assert_eq!(width_class(width, lines), want, "at {width} design pixels");
    }
}

#[test]
fn each_height_answers_the_word_its_side_of_the_line_does() {
    let lines = ClassLines::default();
    for (height, want) in [(390.0, SHORT), (479.0, SHORT), (480.0, TALL), (900.0, TALL)] {
        assert_eq!(height_class(height, lines), want, "at {height} design pixels");
    }
}

/// No window is nothing to fit, not the tightest fit there is. A headless run
/// reads what a desktop reads, which is what keeps a layout test neutral.
#[test]
fn a_surface_of_nothing_reads_wide_and_tall() {
    let lines = ClassLines::default();
    assert_eq!(width_class(0.0, lines), WIDE);
    assert_eq!(height_class(0.0, lines), TALL);
}

/// A project moves the lines; the words stay where they are.
#[test]
fn moving_a_line_moves_which_word_a_width_answers() {
    let tight = ClassLines {
        narrow_below: 400.0,
        ..ClassLines::default()
    };
    assert_eq!(width_class(500.0, ClassLines::default()), NARROW);
    assert_eq!(width_class(500.0, tight), MEDIUM);
}

/// Every word is in the list an override and a picker read, so a class table
/// nothing offers cannot exist.
#[test]
fn every_class_word_is_in_its_list() {
    assert_eq!(WIDTH_CLASSES, [WIDE, MEDIUM, NARROW]);
    assert_eq!(HEIGHT_CLASSES, [TALL, SHORT]);
    assert!(balaur_core::tags::ALL.contains(&TOUCH));
    assert!(balaur_core::tags::ALL.contains(&POINTER));
}

/// The input class is one tag or the other, never both and never neither, and
/// it sits between the kind of machine and the operating system.
#[test]
fn the_input_class_replaces_itself_and_keeps_its_place() {
    let mut tags = Tags(vec!["mobile".into(), "android".into()]);
    tags.set_input_class(true);
    assert_eq!(tags.0, vec!["mobile", TOUCH, "android"]);
    tags.set_input_class(false);
    assert_eq!(tags.0, vec!["mobile", POINTER, "android"]);
    assert!(!tags.has(TOUCH));
}

/// A page reports no input class until the browser answers, so a target's
/// tags name one only where the machine settles it.
#[test]
fn an_export_target_claims_an_input_class_only_where_it_knows_one() {
    assert!(Tags::for_target("ios").has(TOUCH));
    assert!(Tags::for_target("android").has(TOUCH));
    assert!(Tags::for_target("windows-x64").has(POINTER));
    let web = Tags::for_target("web");
    assert!(!web.has(TOUCH) && !web.has(POINTER));
}

/// The whole point of the input class being a tag: a session recorded on a
/// phone replays as a phone, on a desktop that has no touch screen at all.
#[test]
fn a_recorded_touch_screen_replays_as_the_touch_tag() {
    use balaur_core::replay::ReplaySetupRegistry;

    let recorded = {
        let app = balaur_core::App::new(balaur_core::AppConfig::bare(".")).unwrap();
        let mut facts = balaur_core::facts::platform(&app.engine);
        facts.touchscreen = true;
        app.engine
            .resource::<balaur_core::facts::Facts>()
            .borrow_mut()
            .0 = Some(facts);
        let registry = app.engine.resource::<ReplaySetupRegistry>();
        let registry = registry.borrow();
        let (_, capture, _) = registry
            .0
            .iter()
            .find(|(name, _, _)| name == "platform")
            .expect("core records the platform facts");
        capture(&app.engine)
    };

    let app = balaur_core::App::new(balaur_core::AppConfig::bare(".")).unwrap();
    app.engine
        .resource::<Tags>()
        .borrow_mut()
        .set_input_class(false);
    let registry = app.engine.resource::<ReplaySetupRegistry>();
    {
        let registry = registry.borrow();
        let (_, _, restore) = registry
            .0
            .iter()
            .find(|(name, _, _)| name == "platform")
            .expect("core records the platform facts");
        restore(&app.engine, &recorded);
    }
    let tags = app.engine.resource::<Tags>();
    let tags = tags.borrow();
    assert!(tags.has(TOUCH), "the recording's touch screen was lost");
    assert!(!tags.has(POINTER), "both classes answered at once");
}
