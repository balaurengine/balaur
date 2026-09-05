//! Rollback on one machine: an input that turns up after its tick already
//! ran has to leave the world exactly where it would have been had it
//! arrived on time. The digest is the judge, because it is what two peers
//! would compare.

use std::cell::RefCell;
use std::rc::Rc;

use balaur_core::components::StableId;
use balaur_core::rollback::{self, Session};
use balaur_core::{digest, App, AppConfig, Stage, Transform};
use balaur_script::Value;

const PLAYER: u32 = 1;

/// What one run saw, so a test can ask how many ticks actually ran and which
/// of them were re-runs.
#[derive(Default)]
struct Trace {
    ticks: u32,
    resimulated: u32,
}

/// An app with one node whose x position is driven by the player's input.
///
/// Movement rather than a bare counter: a position is what a snapshot
/// restores and what the digest hashes, so a rollback that half worked shows
/// up as a wrong number rather than as nothing.
fn app_driven_by_input() -> (App, Rc<RefCell<Trace>>) {
    let mut app = App::new(AppConfig::bare(".")).unwrap();
    let root = app.engine.root();
    let mover = {
        let mut world = app.engine.world_mut();
        let entity = balaur_core::scene::spawn_node(&mut world, "Mover", root);
        world
            .insert_one(entity, StableId(String::from("n_mover")))
            .unwrap();
        entity
    };
    let trace = Rc::new(RefCell::new(Trace::default()));
    let seen = Rc::clone(&trace);
    app.add_system(Stage::Update, move |eng, _| {
        let mut seen = seen.borrow_mut();
        seen.ticks += 1;
        if rollback::is_resimulating(eng) {
            seen.resimulated += 1;
        }
        let Some(Value::Int(step)) = rollback::input(eng, PLAYER) else {
            return;
        };
        let world = eng.world();
        let Ok(mut t) = world.get::<&mut Transform>(mover) else {
            return;
        };
        #[allow(clippy::cast_precision_loss, reason = "small whole numbers")]
        {
            t.position.x += step as f32;
        }
    });
    (app, trace)
}

/// Six ticks of input, with tick 3 unlike its neighbours so predicting it by
/// repetition is guaranteed to be wrong.
const INPUTS: [i64; 6] = [10, 20, 33, 40, 50, 60];

fn input_at(tick: u64) -> Value {
    Value::Int(INPUTS[tick as usize - 1])
}

/// Every input on time.
fn run_straight() -> (App, Rc<RefCell<Trace>>) {
    let (mut app, trace) = app_driven_by_input();
    let mut session = Session::new(&[PLAYER], 16);
    for tick in 1..=6 {
        session.submit(PLAYER, tick, input_at(tick));
        session.advance(&mut app);
    }
    (app, trace)
}

/// The load-bearing one: tick 3's input shows up after tick 5 has run, and
/// the run still ends where the unbroken run ended.
#[test]
fn a_late_input_rolls_back_to_the_digest_of_the_run_that_had_it_on_time() {
    let (straight, _) = run_straight();
    let expected = digest::digest(&straight.engine);

    let (mut app, trace) = app_driven_by_input();
    let mut session = Session::new(&[PLAYER], 16);
    for tick in [1, 2] {
        session.submit(PLAYER, tick, input_at(tick));
        session.advance(&mut app);
    }
    // Tick 3 arrives late, so this one runs on the prediction.
    session.advance(&mut app);
    for tick in [4, 5] {
        session.submit(PLAYER, tick, input_at(tick));
        session.advance(&mut app);
    }
    assert_ne!(
        digest::digest(&app.engine),
        digest::digest(&straight.engine),
        "the misprediction has to be visible, or the test proves nothing"
    );

    session.submit(PLAYER, 3, input_at(3));
    session.submit(PLAYER, 6, input_at(6));
    session.advance(&mut app);

    assert_eq!(digest::digest(&app.engine), expected);
    assert!(
        trace.borrow().resimulated > 0,
        "and got there by re-simulating, not by luck"
    );
}

/// A late input that matches what was predicted costs nothing: no restore,
/// no re-run.
#[test]
fn a_late_input_that_matches_the_prediction_does_not_roll_back() {
    let (mut app, trace) = app_driven_by_input();
    let mut session = Session::new(&[PLAYER], 16);
    session.submit(PLAYER, 1, Value::Int(7));
    session.advance(&mut app);
    // Tick 2 runs on the prediction, which repeats tick 1.
    session.advance(&mut app);

    session.submit(PLAYER, 2, Value::Int(7));
    session.advance(&mut app);

    assert_eq!(trace.borrow().ticks, 3, "three ticks, none of them twice");
    assert_eq!(trace.borrow().resimulated, 0);
}

/// The clock goes back with the world. A re-run of tick 3 has to be tick 3
/// again, or a script reading the tick number sees one the first run never
/// did.
#[test]
fn a_resimulated_tick_carries_the_tick_number_it_had() {
    let (mut app, _) = app_driven_by_input();
    let seen: Rc<RefCell<Vec<u64>>> = Rc::new(RefCell::new(Vec::new()));
    let record = Rc::clone(&seen);
    app.add_system(Stage::Update, move |eng, _| {
        if rollback::is_resimulating(eng) {
            record.borrow_mut().push(eng.tick());
        }
    });
    let mut session = Session::new(&[PLAYER], 16);
    for tick in [1, 2] {
        session.submit(PLAYER, tick, input_at(tick));
        session.advance(&mut app);
    }
    session.advance(&mut app);
    session.submit(PLAYER, 3, input_at(3));
    session.advance(&mut app);

    assert_eq!(*seen.borrow(), vec![3], "tick 3 re-ran as tick 3");
}

/// An input older than the ring cannot be answered, and says so rather than
/// applying itself to whatever tick is still there.
#[test]
fn an_input_older_than_the_ring_is_refused() {
    let (mut app, trace) = app_driven_by_input();
    let mut session = Session::new(&[PLAYER], 2);
    for tick in 1..=5 {
        session.submit(PLAYER, tick, input_at(tick));
        session.advance(&mut app);
    }
    assert_eq!(session.earliest(), Some(4), "a ring of two holds 4 and 5");
    let before = trace.borrow().resimulated;

    session.submit(PLAYER, 1, Value::Int(-999));
    session.advance(&mut app);

    assert_eq!(
        trace.borrow().resimulated,
        before,
        "nothing was re-simulated for a tick the ring had dropped"
    );
    assert_eq!(
        session.stale_inputs(),
        1,
        "and the caller can see it happened"
    );
}

/// The journal is what a wire grows. A long session has to keep a bounded
/// number of inputs, not one per tick per player for as long as it runs.
#[test]
fn a_long_session_keeps_a_bounded_journal() {
    let (mut app, _) = app_driven_by_input();
    let mut session = Session::new(&[PLAYER], 8);
    for tick in 1..=3000u64 {
        session.submit(PLAYER, tick, Value::Int(1));
        session.advance(&mut app);
    }
    assert!(
        session.journal_len() <= 64,
        "the journal grew with the session: {} entries after 3000 ticks",
        session.journal_len()
    );
}

/// Both numbers in a peer's datagram are whatever the peer put there.
#[test]
fn an_input_for_an_unknown_player_or_an_impossible_tick_is_refused() {
    let (mut app, _) = app_driven_by_input();
    let mut session = Session::new(&[PLAYER], 8);
    session.submit(PLAYER, 1, Value::Int(1));
    session.advance(&mut app);
    let kept = session.journal_len();

    session.submit(PLAYER, u64::MAX, Value::Int(9));
    session.submit(PLAYER + 7, 2, Value::Int(9));

    assert_eq!(
        session.journal_len(),
        kept,
        "neither was journalled, so neither can grow the session"
    );
}

/// A burst of arrivals where one is too old to answer: the old one must not
/// take the rollback down with it, because `take` has already cleared the
/// correction for the tick that can still be re-run.
#[test]
fn a_stale_input_does_not_mask_a_correction_that_can_still_be_made() {
    let (straight, _) = run_straight();
    let expected = digest::digest(&straight.engine);

    let (mut app, trace) = app_driven_by_input();
    let mut session = Session::new(&[PLAYER], 3);
    for tick in [1, 2] {
        session.submit(PLAYER, tick, input_at(tick));
        session.advance(&mut app);
    }
    // Tick 3 arrives late, so this one runs on the prediction.
    session.advance(&mut app);
    for tick in [4, 5] {
        session.submit(PLAYER, tick, input_at(tick));
        session.advance(&mut app);
    }
    assert_eq!(session.earliest(), Some(3), "a ring of three holds 3 to 5");

    // Tick 1 is below the ring; tick 3's correction still is not.
    session.submit(PLAYER, 1, Value::Int(-999));
    session.submit(PLAYER, 3, input_at(3));
    session.submit(PLAYER, 6, input_at(6));
    session.advance(&mut app);

    assert_eq!(digest::digest(&app.engine), expected);
    assert!(trace.borrow().resimulated > 0, "and by re-simulating");
    assert_eq!(
        session.stale_inputs(),
        1,
        "the old one was counted, not lost"
    );
}
