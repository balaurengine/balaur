//! Step 5's bar, over QUIC: two engines exchanging only inputs, agreeing tick
//! for tick.
//!
//! The same assertions the websocket lockstep test makes, against a transport
//! where the inputs really do travel as unreliable datagrams and the digests
//! really do travel on a stream. Nothing in `NetSession` changed to allow
//! this, which is the point of the trait.

use balaur_core::components::StableId;
use balaur_core::netsession::NetSession;
use balaur_core::rollback;
use balaur_core::transport::{LinkState, Transport};
use balaur_core::{App, AppConfig, Stage, Transform};
use balaur_script::Value;
use balaur_webtransport::{Accept, WebTransportLink, WebTransportServer};

const HOST: u32 = 1;
const GUEST: u32 = 2;

/// Both players drive the same node, so either one's input landing on the
/// wrong tick shows up in the digest.
fn app() -> App {
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
        t.position.x += delta;
    });
    app
}

struct Pair {
    _server: WebTransportServer,
    host_app: App,
    host: NetSession,
    guest_app: App,
    guest: NetSession,
}

fn connect() -> Pair {
    let host_app = app();
    let guest_app = app();
    let mut server = WebTransportServer::bind(&host_app.engine, "127.0.0.1:0").unwrap();
    let accept = Accept::Hashes(server.certificate().hashes());
    let mut dialled = WebTransportLink::connect(&guest_app.engine, &server.url(), accept).unwrap();

    let mut accepted = None;
    for _ in 0..600 {
        if accepted.is_none() {
            accepted = server.accept().into_iter().next();
        }
        let _ = dialled.receive();
        if let Some(peer) = accepted.as_mut() {
            let _ = peer.receive();
            if peer.state() == LinkState::Open && dialled.state() == LinkState::Open {
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let accepted = accepted.expect("the server never accepted the peer");
    assert_eq!(accepted.state(), LinkState::Open, "the server side opened");
    assert_eq!(dialled.state(), LinkState::Open, "the client side opened");

    let mut host = NetSession::new(HOST, &[HOST, GUEST], 32);
    host.add_peer(&host_app.engine, Box::new(accepted));
    let mut guest = NetSession::new(GUEST, &[HOST, GUEST], 32);
    guest.add_peer(&guest_app.engine, Box::new(dialled));
    Pair {
        _server: server,
        host_app,
        host,
        guest_app,
        guest,
    }
}

/// One tick on both sides, with a pause so the datagrams land before the
/// next. A real game gets that for free from its frame budget.
fn step(pair: &mut Pair, tick: u64) {
    pair.host
        .set_input(Value::Int(i64::try_from(tick).unwrap()));
    pair.guest
        .set_input(Value::Int(i64::try_from(tick * 10).unwrap()));
    pair.host.advance(&mut pair.host_app);
    pair.guest.advance(&mut pair.guest_app);
    std::thread::sleep(std::time::Duration::from_millis(10));
}

#[test]
fn two_engines_agree_tick_for_tick_over_quic() {
    let mut pair = connect();
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
