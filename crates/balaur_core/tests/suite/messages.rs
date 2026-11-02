//! Typed messages (PLAN-ecs §2): sent by one system, read once by each
//! reader, kept for the frame they were sent in and the next.

use std::cell::RefCell;
use std::rc::Rc;

use balaur_core::messages::MessageCursor;
use balaur_core::{App, AppConfig, Engine, Stage, snapshot};

#[derive(Clone, Debug, PartialEq)]
struct Hit(u32);

fn app() -> App {
    let mut app = App::new(AppConfig::bare(".")).unwrap();
    app.add_messages::<Hit>();
    app
}

#[test]
fn each_reader_reads_each_message_once() {
    let app = app();
    let mut one = MessageCursor::<Hit>::default();
    let mut two = MessageCursor::<Hit>::default();
    app.engine.send(Hit(1));
    app.engine.send(Hit(2));
    assert_eq!(one.read_cloned(&app.engine), vec![Hit(1), Hit(2)]);
    app.engine.send(Hit(3));
    assert_eq!(one.read_cloned(&app.engine), vec![Hit(3)]);
    assert_eq!(two.read_cloned(&app.engine), vec![Hit(1), Hit(2), Hit(3)]);
    assert_eq!(two.read_cloned(&app.engine), vec![]);
}

#[test]
fn a_reader_before_the_sender_hears_it_the_next_frame() {
    let mut app = app();
    let heard = Rc::new(RefCell::new(Vec::new()));
    let kept = heard.clone();
    let mut cursor = MessageCursor::<Hit>::default();
    // Registered first, so it runs before the sender every frame.
    app.add_system(Stage::Update, move |eng: &Engine, _| {
        kept.borrow_mut().push(cursor.read_cloned(eng));
    });
    let mut sent = 0;
    app.add_system(Stage::Update, move |eng: &Engine, _| {
        sent += 1;
        eng.send(Hit(sent));
    });
    for _ in 0..3 {
        app.tick(0.0);
    }
    assert_eq!(
        *heard.borrow(),
        vec![vec![], vec![Hit(1)], vec![Hit(2)]],
        "nothing lost to the order of systems"
    );
}

#[test]
fn a_message_is_kept_two_frames_then_missed() {
    let mut app = app();
    let mut late = MessageCursor::<Hit>::default();
    app.engine.send(Hit(1));
    app.tick(0.0);
    app.tick(0.0);
    app.engine.send(Hit(2));
    app.tick(0.0);
    let mut heard = Vec::new();
    let missed = late.read(&app.engine, |hit| heard.push(hit.clone()));
    assert_eq!(heard, vec![Hit(2)]);
    assert_eq!(missed, 1, "the first went before this reader came");
}

#[test]
fn a_restore_drops_what_is_kept() {
    let app = app();
    let mut reader = MessageCursor::<Hit>::default();
    let taken = snapshot::capture(&app.engine);
    app.engine.send(Hit(1));
    snapshot::restore(&app.engine, &taken);
    assert_eq!(reader.read_cloned(&app.engine), vec![]);
    app.engine.send(Hit(2));
    assert_eq!(reader.read_cloned(&app.engine), vec![Hit(2)]);
}

#[test]
#[should_panic(expected = "never declared")]
fn sending_an_undeclared_type_says_so() {
    let app = App::new(AppConfig::bare(".")).unwrap();
    app.engine.send(Hit(1));
}
