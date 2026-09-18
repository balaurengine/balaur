//! A session with a host in the middle: links bound to the players they
//! speak for, inputs relayed between the other ends, players leaving, and a
//! fast peer waiting for a slow one. In-memory links, so what is tested is
//! the session rather than a socket.

use balaur_core::components::StableId;
use balaur_core::netsession::NetSession;
use balaur_core::{App, AppConfig, Stage, Transform, rollback};
use balaur_script::Value;

use crate::faults::pipe;

/// Every player moves one node by its input, so an input on the wrong tick,
/// or for the wrong player, shows in the digest.
fn app(players: &'static [u32]) -> App {
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
    app.add_system(Stage::Update, move |eng, _| {
        let mut delta = 0.0f32;
        for (at, player) in players.iter().enumerate() {
            if let Some(Value::Int(step)) = rollback::input(eng, *player) {
                #[allow(clippy::cast_precision_loss, reason = "small whole numbers")]
                {
                    delta += step as f32 * (at as f32 + 1.0);
                }
            }
        }
        let world = eng.world();
        if let Ok(mut t) = world.get::<&mut Transform>(mover) {
            t.position.x += delta;
        }
    });
    app
}

const THREE: &[u32] = &[0, 1, 2];

/// A host and two players, each player linked to the host only.
struct Star {
    apps: Vec<App>,
    nets: Vec<NetSession>,
}

fn star() -> Star {
    let apps: Vec<App> = (0..3).map(|_| app(THREE)).collect();
    let mut host = NetSession::new(0, THREE, 64);
    let mut nets = Vec::new();
    for player in [1, 2] {
        let (near, far) = pipe();
        host.add_bound_peer(&apps[0].engine, Box::new(near), player, vec![player]);
        let mut net = NetSession::new(player, THREE, 64);
        let others = THREE.iter().copied().filter(|p| *p != player).collect();
        net.add_bound_peer(
            &apps[usize::try_from(player).unwrap()].engine,
            Box::new(far),
            0,
            others,
        );
        nets.push(net);
    }
    host.set_relay(true);
    nets.insert(0, host);
    Star { apps, nets }
}

fn step(star: &mut Star, tick: u64) {
    for (index, (net, app)) in star.nets.iter_mut().zip(&mut star.apps).enumerate() {
        let index = i64::try_from(index).unwrap();
        net.set_input(Value::Int(i64::try_from(tick).unwrap() % 4 + index));
        net.advance(app);
    }
}

#[test]
fn a_host_relays_each_players_inputs_to_the_others() {
    let mut star = star();
    for tick in 1..=90 {
        step(&mut star, tick);
    }
    let [host, one, two] = &star.nets[..] else {
        unreachable!()
    };
    let mut compared = 0;
    for at in 1..60 {
        if !(host.session().confirmed(at)
            && one.session().confirmed(at)
            && two.session().confirmed(at))
        {
            continue;
        }
        let digest = host.session().digest_at(at);
        assert_eq!(
            digest,
            one.session().digest_at(at),
            "player 1 differs on tick {at}"
        );
        assert_eq!(
            digest,
            two.session().digest_at(at),
            "player 2 differs on tick {at}"
        );
        compared += 1;
    }
    assert!(
        compared > 20,
        "only {compared} ticks were settled everywhere"
    );
    assert!(
        one.session().newest_input(2).is_some(),
        "player 1 heard player 2 through the host"
    );
    for net in &star.nets {
        assert_eq!(net.desync(), None);
    }
}

/// A link bound to player 1 cannot put words in player 2's mouth.
#[test]
fn a_link_speaks_only_for_the_players_it_is_bound_to() {
    let mut host_app = app(THREE);
    let mut far_app = app(THREE);
    let (near, far) = pipe();
    let mut host = NetSession::new(0, THREE, 64);
    host.add_bound_peer(&host_app.engine, Box::new(near), 1, vec![1]);
    let mut impostor = NetSession::new(2, THREE, 64);
    impostor.add_peer(&far_app.engine, Box::new(far));
    for tick in 1..=20 {
        impostor.set_input(Value::Int(i64::try_from(tick).unwrap()));
        impostor.advance(&mut far_app);
        host.advance(&mut host_app);
    }
    assert_eq!(
        host.session().newest_input(2),
        None,
        "the host took player 2's input from player 1's link"
    );
}

/// A player who leaves plays nil from the tick named, and nobody waits on
/// them to settle a tick.
#[test]
fn an_absent_player_plays_nil_and_holds_nothing_back() {
    const TWO: &[u32] = &[0, 1];
    let mut host_app = app(TWO);
    let mut guest_app = app(TWO);
    let (near, far) = pipe();
    let mut host = NetSession::new(0, TWO, 64);
    host.add_bound_peer(&host_app.engine, Box::new(near), 1, vec![1]);
    let mut guest = NetSession::new(1, TWO, 64);
    guest.add_bound_peer(&guest_app.engine, Box::new(far), 0, vec![0]);
    for tick in 1..=20 {
        host.set_input(Value::Int(1));
        guest.set_input(Value::Int(i64::try_from(tick).unwrap()));
        host.advance(&mut host_app);
        guest.advance(&mut guest_app);
    }
    let from = host.session().newest_input(1).unwrap() + 1;
    host.set_absent(1, from);
    for _ in 0..20 {
        host.advance(&mut host_app);
    }
    let late = host.tick() - 1;
    assert!(
        host.session().confirmed(late),
        "tick {late} waited on a player who left"
    );
    assert!(!host.session().is_present(1, late));
    assert!(host.session().is_present(1, from - 1));
}

/// A peer that hears nothing from the other side stops, rather than run so
/// far ahead that the other side's inputs land outside the ring.
#[test]
fn a_peer_far_ahead_waits() {
    const TWO: &[u32] = &[0, 1];
    let mut host_app = app(TWO);
    let guest_app = app(TWO);
    let (near, far) = pipe();
    let mut host = NetSession::new(0, TWO, 16);
    host.add_bound_peer(&host_app.engine, Box::new(near), 1, vec![1]);
    let mut guest = NetSession::new(1, TWO, 16);
    guest.add_bound_peer(&guest_app.engine, Box::new(far), 0, vec![0]);
    let mut ran = 0;
    while !host.should_wait() {
        host.advance(&mut host_app);
        ran += 1;
        assert!(
            ran < 16,
            "the host ran {ran} ticks with no word from its peer"
        );
    }
    assert!(ran >= 2, "the host waited before it had run ahead at all");
}
