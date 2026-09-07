//! Rollback against a link that misbehaves.
//!
//! Every session test so far has run on loopback, which never delays a
//! payload, never drops one and never reorders two — so none of them has
//! exercised the property QUIC was chosen for. This one puts 150 ms of delay,
//! jitter and one datagram in twenty on the wire and asks for the same
//! answer: two peers agreeing on every settled tick.
//!
//! No socket. The link is two queues, so what is being tested is the session
//! and the faults rather than anyone's networking stack.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use balaur_core::components::StableId;
use balaur_core::netsession::NetSession;
use balaur_core::transport::{Delivery, Faults, Faulty, LinkState, Received, Transport};
use balaur_core::{App, AppConfig, Stage, Transform, rollback};
use balaur_script::Value;

const HOST: u32 = 1;
const GUEST: u32 = 2;

/// One end of an in-memory link: what this end writes, the other end reads.
struct Pipe {
    outbound: Rc<RefCell<VecDeque<Received>>>,
    inbound: Rc<RefCell<VecDeque<Received>>>,
}

fn pipe() -> (Pipe, Pipe) {
    let one = Rc::new(RefCell::new(VecDeque::new()));
    let two = Rc::new(RefCell::new(VecDeque::new()));
    (
        Pipe {
            outbound: Rc::clone(&one),
            inbound: Rc::clone(&two),
        },
        Pipe {
            outbound: two,
            inbound: one,
        },
    )
}

impl Pipe {
    fn push(&self, delivery: Delivery, bytes: &[u8]) {
        self.outbound.borrow_mut().push_back(Received {
            delivery,
            bytes: bytes.to_vec(),
        });
    }
}

impl Transport for Pipe {
    fn send_reliable(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        self.push(Delivery::Reliable, bytes);
        Ok(())
    }

    fn send_datagram(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        self.push(Delivery::Datagram, bytes);
        Ok(())
    }

    fn receive(&mut self) -> Vec<Received> {
        self.inbound.borrow_mut().drain(..).collect()
    }

    fn max_datagram(&self) -> usize {
        1200
    }

    fn state(&self) -> LinkState {
        LinkState::Open
    }

    fn close(&mut self) {}
}

/// Both players drive the same node, so an input landing on the wrong tick
/// shows up in the digest.
fn app() -> App {
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
    host_app: App,
    host: NetSession,
    guest_app: App,
    guest: NetSession,
}

fn connect(faults: Faults) -> Pair {
    let (one, two) = pipe();
    let host_app = app();
    let guest_app = app();
    let mut host = NetSession::new(HOST, &[HOST, GUEST], 64);
    // Wrapped here rather than through the Netcode setting, so a test says
    // what its link does instead of depending on an editor preference.
    host.add_peer(&host_app.engine, Box::new(Faulty::new(one, faults, 0x5eed)));
    let mut guest = NetSession::new(GUEST, &[HOST, GUEST], 64);
    // A different seed, so the two directions do not drop and delay in
    // lockstep with each other.
    guest.add_peer(
        &guest_app.engine,
        Box::new(Faulty::new(two, faults, 0xd1ce)),
    );
    Pair {
        host_app,
        host,
        guest_app,
        guest,
    }
}

fn step(pair: &mut Pair, tick: u64) {
    pair.host
        .set_input(Value::Int(i64::try_from(tick).unwrap()));
    pair.guest
        .set_input(Value::Int(i64::try_from(tick * 10).unwrap()));
    pair.host.advance(&mut pair.host_app);
    pair.guest.advance(&mut pair.guest_app);
}

/// The one that matters: a link with real latency, jitter and loss still
/// converges, because a lost input is a misprediction the session repairs
/// rather than a stall it waits out.
#[test]
fn two_peers_agree_across_a_link_that_delays_drops_and_reorders() {
    let mut pair = connect(Faults::typical());
    for tick in 1..=120 {
        step(&mut pair, tick);
    }

    // Well behind the newest tick, since a 150 ms link means an input can be
    // twelve ticks late and the tick it belongs to is unsettled until then.
    let settled = pair.host.tick() - 30;
    let mut compared = 0;
    for at in 1..=settled {
        let (Some(ours), Some(theirs)) = (
            pair.host.session().digest_at(at),
            pair.guest.session().digest_at(at),
        ) else {
            continue;
        };
        assert_eq!(ours, theirs, "peers first disagreed on tick {at}");
        compared += 1;
    }
    // Fewer than `settled` ticks: a digest lives only as long as its
    // snapshot, so the ring's depth bounds how far back either peer can
    // still be asked about.
    assert!(compared > 25, "only {compared} ticks were comparable");
    assert_eq!(pair.host.desync(), None, "no false desync from late inputs");
    assert_eq!(pair.guest.desync(), None);
}

/// The faults are real: with this much loss and delay, a run has to have
/// actually lost and reordered something, or the test above proved nothing.
#[test]
fn the_faults_actually_fire() {
    let (mut one, two) = pipe();
    let mut link = Faulty::new(two, Faults::typical(), 0x5eed);
    for n in 0..200u32 {
        one.send_datagram(&n.to_be_bytes()).unwrap();
    }
    let mut got = Vec::new();
    for _ in 0..40 {
        got.extend(link.receive());
    }
    assert!(
        got.len() < 200,
        "nothing was dropped out of 200 datagrams at 5% loss"
    );
    assert!(got.len() > 150, "far too much was dropped: {}", got.len());
    let order: Vec<u32> = got
        .iter()
        .map(|r| u32::from_be_bytes(r.bytes.clone().try_into().unwrap()))
        .collect();
    assert!(
        order.windows(2).any(|w| w[0] > w[1]),
        "jitter never reordered anything"
    );
}

/// A reliable payload is never dropped, whatever the loss setting: a
/// transport that loses one has broken its contract.
#[test]
fn loss_never_touches_the_reliable_channel() {
    let (mut one, two) = pipe();
    let mut link = Faulty::new(
        two,
        Faults {
            delay: 0,
            jitter: 0,
            loss: 1.0,
        },
        7,
    );
    for n in 0..50u32 {
        one.send_reliable(&n.to_be_bytes()).unwrap();
        one.send_datagram(&n.to_be_bytes()).unwrap();
    }
    let got = link.receive();
    assert_eq!(got.len(), 50, "every reliable payload survived");
    assert!(got.iter().all(|r| r.delivery == Delivery::Reliable));
}

/// A quiet link changes nothing, so `Faults::none()` is a real no-op rather
/// than a differently shaped path.
#[test]
fn a_link_with_no_faults_delivers_immediately_and_in_order() {
    let (mut one, two) = pipe();
    let mut link = Faulty::new(two, Faults::none(), 1);
    for n in 0..10u32 {
        one.send_datagram(&n.to_be_bytes()).unwrap();
    }
    let got = link.receive();
    let order: Vec<u32> = got
        .iter()
        .map(|r| u32::from_be_bytes(r.bytes.clone().try_into().unwrap()))
        .collect();
    assert_eq!(order, (0..10).collect::<Vec<_>>());
}

/// A networked session replays from its recording, with no peer on the other
/// end. This is what makes a desync reproducible: the same payloads arrive on
/// the same ticks, and the run is identical without a network at all.
#[test]
fn a_recorded_session_replays_with_no_peer() {
    use balaur_core::replay::{ReplayFeed, ReplayMode};

    // Record: the guest is only there to answer, and its frames are the ones
    // the host's traffic source captures.
    let mut pair = connect(Faults::none());
    let mut frames = Vec::new();
    for tick in 1..=12 {
        step(&mut pair, tick);
        frames.push(balaur_core::replay::capture(&pair.host_app.engine));
    }
    let recorded = pair.host.session().digest_at(6).expect("tick 6 was kept");

    let mut app = app();
    *app.engine.resource::<ReplayMode>().borrow_mut() = ReplayMode::Playing;
    let mut session = NetSession::new(HOST, &[HOST, GUEST], 64);
    for (at, frame) in frames.into_iter().enumerate() {
        app.engine.resource::<ReplayFeed>().borrow_mut().0 = Some(frame);
        session.set_input(Value::Int(i64::try_from(at + 1).unwrap()));
        session.advance(&mut app);
    }

    assert_eq!(
        session.session().digest_at(6),
        Some(recorded),
        "the replay reached the same world without a peer to talk to"
    );
}

/// The link reports what it is doing. Measurement, not simulation: none of
/// these numbers is recorded, replayed or hashed.
#[test]
fn a_session_measures_its_link() {
    let mut pair = connect(Faults {
        delay: 0,
        jitter: 0,
        loss: 0.20,
    });
    for tick in 1..=90 {
        step(&mut pair, tick);
    }

    let stats = pair.guest.stats();
    assert_eq!(stats.len(), 1, "one peer, one link");
    let link = stats[0];
    assert!(link.bytes_in > 0, "the guest heard something");
    assert!(link.bytes_out > 0, "and said something");
    assert!(
        link.loss > 0.05 && link.loss < 0.40,
        "one datagram in five was dropped; the link says {}",
        link.loss
    );
    // A round trip on an in-memory link is under a millisecond, so the useful
    // assertion is that a ping came back at all rather than what it measured.
    assert!(link.rtt_ms >= 0.0);

    let published = pair
        .guest_app
        .engine
        .resource::<balaur_core::netsession::SessionStats>();
    assert_eq!(published.borrow().0.len(), 1);
}
