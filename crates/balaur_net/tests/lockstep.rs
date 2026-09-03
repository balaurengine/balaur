//! Two engines, one process, playing the same scene over a real socket.
//!
//! The bar step 5 of `PLAN-networking.md` sets: two peers exchange only
//! inputs, predict each other, roll back when a prediction was wrong, and
//! agree tick for tick. Nothing here compares positions — the digest is the
//! judge, because it is what the peers themselves compare.

use std::cell::RefCell;
use std::rc::Rc;

use balaur_core::components::StableId;
use balaur_core::netsession::NetSession;
use balaur_core::rollback;
use balaur_core::transport::{LinkState, Transport};
use balaur_core::{App, AppConfig, Stage, Transform};
use balaur_net::listener::WebsocketListener;
use balaur_net::transport::WebsocketTransport;
use balaur_net::SocketOptions;
use balaur_script::Value;

const HOST: u32 = 1;
const GUEST: u32 = 2;

/// Both players drive the same node, so either one's input reaching the
/// wrong tick shows up in the digest.
///
/// `bias` is added every tick. Zero on both peers is a matched pair; a
/// non-zero one is a peer whose simulation genuinely differs, which is what a
/// nondeterministic subsystem would look like from the outside. It has to
/// live in the tick rather than be poked into the world, because a rollback
/// restores the world and would quietly heal anything poked in.
fn app(bias: f32) -> App {
    let mut app = App::new(AppConfig {
        project_root: std::path::PathBuf::from("."),
        pack: None,
        watch: false,
        script_args: Vec::new(),
        script_backend: None,
    })
    .unwrap();
    let root = app.engine.root();
    let mover = {
        let mut world = app.engine.world_mut();
        let entity = balaur_core::scene::spawn_node(&mut world, "Mover", root);
        world
            .insert_one(entity, StableId(String::from("n_mover")))
            .unwrap();
        entity
    };
    app.add_system(Stage::Update, move |eng, _| {
        let mut delta = 0.0f32;
        for player in [HOST, GUEST] {
            if let Some(Value::Int(step)) = rollback::input(eng, player) {
                #[allow(clippy::cast_precision_loss, reason = "small whole numbers")]
                {
                    delta += step as f32;
                }
            }
        }
        let world = eng.world();
        let Ok(mut t) = world.get::<&mut Transform>(mover) else {
            return;
        };
        t.position.x += delta + bias;
    });
    app
}

/// Poll both links until each reports open, or give up.
fn wait_open(links: [&mut WebsocketTransport; 2]) {
    for link in links {
        for _ in 0..1000 {
            let _ = link.receive();
            if link.state() == LinkState::Open {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(link.state(), LinkState::Open, "a link never opened");
    }
}

/// A host and a guest, connected over loopback, each with a session.
struct Pair {
    host_app: App,
    host: NetSession,
    guest_app: App,
    guest: NetSession,
}

fn connect(guest_bias: f32) -> Pair {
    let host_app = app(0.0);
    let guest_app = app(guest_bias);
    let mut listener = WebsocketListener::bind(&host_app.engine, "127.0.0.1:0").unwrap();
    let mut dialled =
        WebsocketTransport::connect(&guest_app.engine, &listener.url(), SocketOptions::default());

    let mut accepted = None;
    for _ in 0..1000 {
        if let Some(peer) = listener.accept().into_iter().next() {
            accepted = Some(peer);
            break;
        }
        let _ = dialled.receive();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let mut accepted = accepted.expect("the listener never accepted the peer");
    wait_open([&mut accepted, &mut dialled]);

    let mut host = NetSession::new(HOST, &[HOST, GUEST], 32);
    host.add_peer(Box::new(accepted));
    let mut guest = NetSession::new(GUEST, &[HOST, GUEST], 32);
    guest.add_peer(Box::new(dialled));
    Pair {
        host_app,
        host,
        guest_app,
        guest,
    }
}

/// One tick on both sides, with a pause so loopback traffic lands before the
/// next one. A real game gets that for free from its frame budget.
fn step(pair: &mut Pair, tick: u64) {
    pair.host
        .set_input(Value::Int(i64::try_from(tick).unwrap()));
    pair.guest
        .set_input(Value::Int(i64::try_from(tick * 10).unwrap()));
    pair.host.advance(&mut pair.host_app);
    pair.guest.advance(&mut pair.guest_app);
    std::thread::sleep(std::time::Duration::from_millis(8));
}

#[test]
fn two_engines_on_loopback_agree_tick_for_tick() {
    let mut pair = connect(0.0);
    for tick in 1..=20 {
        step(&mut pair, tick);
    }

    // A settled tick, not the live world: the newest tick on each side still
    // rests on an uncorrected prediction of the other's input.
    let settled = pair.host.tick() - 6;
    assert!(
        pair.host.session().digest_at(settled).is_some(),
        "the host still holds tick {settled}"
    );
    // Every settled tick, not just the newest: agreeing only at the end could
    // be luck, and naming the first disagreement makes a failure diagnosable.
    for at in 1..=settled {
        let ours = pair.host.session().digest_at(at);
        let theirs = pair.guest.session().digest_at(at);
        assert_eq!(ours, theirs, "peers first disagreed on tick {at}");
    }
    assert_eq!(pair.host.desync(), None);
    assert_eq!(pair.guest.desync(), None);
    assert_eq!(pair.host.stale_inputs(), 0, "the ring was deep enough");
    assert_eq!(pair.guest.stale_inputs(), 0);
}

#[test]
fn an_injected_divergence_is_caught_and_named() {
    // The guest's tick does something the host's does not, so the two
    // disagree from the first tick and stay disagreeing through every
    // rollback.
    let mut pair = connect(1.0);
    for tick in 1..=20 {
        step(&mut pair, tick);
    }

    let host = pair.host.desync().expect("the host noticed");
    let guest = pair.guest.desync().expect("the guest noticed");
    assert_eq!(
        host.tick, guest.tick,
        "and both stopped on the same tick, which is what makes the report actionable"
    );
    assert_ne!(host.ours, host.theirs);
    assert_eq!(host.ours, guest.theirs, "each saw the other's number");
}

/// A session whose peer never says anything keeps running on predictions
/// rather than stalling. Lockstep here is optimistic, not blocking.
#[test]
fn a_silent_peer_does_not_stall_the_session() {
    let app_a = app(0.0);
    let mut solo = NetSession::new(HOST, &[HOST, GUEST], 32);
    let counted = Rc::new(RefCell::new(0u32));
    let seen = Rc::clone(&counted);
    let mut app_a = app_a;
    app_a.add_system(Stage::Update, move |_, _| *seen.borrow_mut() += 1);

    for tick in 1..=5 {
        solo.set_input(Value::Int(i64::from(tick)));
        solo.advance(&mut app_a);
    }
    assert_eq!(
        *counted.borrow(),
        5,
        "five ticks ran with nobody to talk to"
    );
    assert_eq!(solo.desync(), None);
}
