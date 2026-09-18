//! Before the first tick: a host accepting and greeting joiners, a joiner
//! waiting to be let in, and the hand-over to a match.

use anyhow::{Result, bail};
use balaur_core::Engine;
use balaur_core::rollback::PlayerId;
use balaur_core::snapshot::Snapshot;
use balaur_core::time::Instant;
use balaur_core::transport::{LinkState, Transport};
use balaur_script::Value;

use crate::links::{self, Listener};
use crate::options::Options;
use crate::vocabulary::{EventKind, Role};
use crate::wire::{Control, Member, PROTOCOL};
use crate::{MultiplayerState, Phase, slot_value};

/// The most slots a lobby hands out, whatever `players` says.
const MOST_SLOTS: u32 = 64;

/// A host's lobby.
pub(crate) struct Lobby {
    listener: Listener,
    /// Links that connected and have not said hello yet.
    arriving: Vec<(Box<dyn Transport>, Instant)>,
    /// The joined players' links, by the slot each plays.
    links: Vec<(PlayerId, Box<dyn Transport>)>,
}

impl Lobby {
    pub(crate) fn address(&self) -> (String, Option<String>) {
        (self.listener.url.clone(), self.listener.cert_hash.clone())
    }
}

/// A joiner's link to its host, before the match.
pub(crate) struct Joiner {
    link: Box<dyn Transport>,
    since: Instant,
    said_hello: bool,
    pub(crate) welcomed: bool,
}

/// Everything the first frame of a match starts from.
pub(crate) struct Starting {
    /// Links by the slot at their far end: every joiner on a host, the
    /// host on a joiner.
    pub links: Vec<(PlayerId, Box<dyn Transport>)>,
    pub scene: String,
    pub depth: usize,
    /// The host's world, for a joiner to restore; `None` on the host.
    pub snapshot: Option<Snapshot>,
}

/// Listen, and take slot 0. Answers what a joiner needs.
///
/// # Errors
/// When this machine is already in a match, or the listener will not bind.
pub(crate) fn host(state: &mut MultiplayerState, eng: &Engine, options: Options) -> Result<Value> {
    if !matches!(state.phase, Phase::Idle) {
        bail!("already in a match; call multiplayer.leave first");
    }
    let listener = links::bind(eng, options.transport, &options.address)?;
    let answer = Value::Map(vec![
        (String::from("url"), Value::Str(listener.url.clone())),
        (
            String::from("cert_hash"),
            listener.cert_hash.clone().map_or(Value::Nil, Value::Str),
        ),
        (
            String::from("transport"),
            Value::Str(options.transport.name().into()),
        ),
    ]);
    state.reset();
    state.roster = vec![Member {
        slot: 0,
        name: options.name.clone(),
        bot: false,
    }];
    state.role = Some(Role::Host);
    state.local = Some(0);
    state.options = Some(options);
    state.phase = Phase::Hosting(Lobby {
        listener,
        arriving: Vec::new(),
        links: Vec::new(),
    });
    state.tell(EventKind::Connected, vec![("slot", slot_value(0))]);
    Ok(answer)
}

/// Dial a host.
///
/// # Errors
/// When this machine is already in a match, or the URL names no transport
/// this build has.
#[allow(
    clippy::disallowed_methods,
    reason = "a lobby's timeouts are wall time, never a simulation input"
)]
pub(crate) fn join(
    state: &mut MultiplayerState,
    eng: &Engine,
    url: &str,
    options: Options,
) -> Result<()> {
    if !matches!(state.phase, Phase::Idle) {
        bail!("already in a match; call multiplayer.leave first");
    }
    let link = links::connect(eng, url, options.cert_hash.as_deref())?;
    state.reset();
    state.role = Some(Role::Client);
    state.options = Some(options);
    state.phase = Phase::Joining(Joiner {
        link,
        since: Instant::now(),
        said_hello: false,
        welcomed: false,
    });
    Ok(())
}

/// Start with whoever is in. Answers whether a lobby was there to start.
pub(crate) fn start(state: &mut MultiplayerState) -> bool {
    let Phase::Hosting(lobby) = std::mem::replace(&mut state.phase, Phase::Idle) else {
        return false;
    };
    let Lobby {
        mut arriving,
        links,
        ..
    } = lobby;
    for (link, _) in &mut arriving {
        refuse(link.as_mut(), "the match has started");
    }
    let options = state.options.clone().unwrap_or_default();
    state.phase = Phase::Starting(Starting {
        links,
        scene: options.scene,
        depth: options.depth,
        snapshot: None,
    });
    true
}

/// A slot the host's own script plays. Answers the slot, while the lobby
/// has room.
pub(crate) fn add_bot(state: &mut MultiplayerState, name: &str) -> Option<PlayerId> {
    let Phase::Hosting(lobby) = &mut state.phase else {
        return None;
    };
    let slot = free_slot(&state.roster, cap(state.options.as_ref()))?;
    state.roster.push(Member {
        slot,
        name: name.to_string(),
        bot: true,
    });
    broadcast_roster(lobby, &state.roster);
    state.tell(
        EventKind::Joined,
        vec![
            ("slot", slot_value(slot)),
            ("name", Value::Str(name.into())),
        ],
    );
    Some(slot)
}

/// Say goodbye on every link this lobby holds and go idle.
pub(crate) fn leave(state: &mut MultiplayerState) {
    let links: Vec<Box<dyn Transport>> = match std::mem::replace(&mut state.phase, Phase::Idle) {
        Phase::Hosting(lobby) => lobby
            .links
            .into_iter()
            .map(|(_, link)| link)
            .chain(lobby.arriving.into_iter().map(|(link, _)| link))
            .collect(),
        Phase::Joining(joiner) => vec![joiner.link],
        Phase::Starting(starting) => starting.links.into_iter().map(|(_, link)| link).collect(),
        Phase::Idle | Phase::Playing => Vec::new(),
    };
    for mut link in links {
        let _ = link.send_reliable(&Control::Bye.bytes());
        link.close();
    }
}

/// One frame of a host's lobby.
#[allow(
    clippy::disallowed_methods,
    reason = "a lobby's timeouts are wall time, never a simulation input"
)]
pub(crate) fn poll_host(state: &mut MultiplayerState) {
    let timeout = state.options.as_ref().map_or(5.0, |o| o.timeout);
    let Phase::Hosting(lobby) = &mut state.phase else {
        return;
    };
    for link in lobby.listener.accept() {
        lobby.arriving.push((link, Instant::now()));
    }
    let mut hellos = Vec::new();
    for (mut link, since) in std::mem::take(&mut lobby.arriving) {
        let hello = link
            .receive()
            .iter()
            .find_map(|r| match Control::parse(&r.bytes) {
                Some(hello @ Control::Hello { .. }) => Some(hello),
                _ => None,
            });
        match hello {
            Some(hello) => hellos.push((link, hello)),
            None if is_closed(link.as_ref()) => {}
            None if since.elapsed().as_secs_f32() > timeout => link.close(),
            None => lobby.arriving.push((link, since)),
        }
    }
    let gone = departures(lobby);
    for (link, hello) in hellos {
        admit(state, link, hello);
    }
    for slot in gone {
        let name = state.name_of(slot);
        state.roster.retain(|member| member.slot != slot);
        if let Phase::Hosting(lobby) = &mut state.phase {
            broadcast_roster(lobby, &state.roster);
        }
        state.tell(
            EventKind::Left,
            vec![("slot", slot_value(slot)), ("name", Value::Str(name))],
        );
    }
    let cap = state.options.as_ref().map_or(0, |o| o.players);
    if cap > 0 && state.roster.len() >= usize::try_from(cap).unwrap_or(usize::MAX) {
        start(state);
    }
}

/// The joined players whose links closed or said goodbye, taken out.
fn departures(lobby: &mut Lobby) -> Vec<PlayerId> {
    let mut gone = Vec::new();
    lobby.links.retain_mut(|(slot, link)| {
        let bye = link
            .receive()
            .iter()
            .any(|r| matches!(Control::parse(&r.bytes), Some(Control::Bye)));
        let left = bye || is_closed(link.as_ref());
        if left {
            link.close();
            gone.push(*slot);
        }
        !left
    });
    gone
}

/// Let a joiner in, or tell it why not.
fn admit(state: &mut MultiplayerState, mut link: Box<dyn Transport>, hello: Control) {
    let Control::Hello {
        protocol,
        name,
        token,
    } = hello
    else {
        return;
    };
    let options = state.options.clone().unwrap_or_default();
    let slot = free_slot(&state.roster, cap(Some(&options)));
    let refusal = if protocol != PROTOCOL {
        Some(format!(
            "this host speaks protocol {PROTOCOL}, not {protocol}"
        ))
    } else if !options.token.is_empty() && token != options.token {
        Some(String::from("the token does not match"))
    } else if slot.is_none() {
        Some(String::from("the lobby is full"))
    } else {
        None
    };
    let (Some(slot), None) = (slot, refusal.as_ref()) else {
        refuse(link.as_mut(), &refusal.unwrap_or_default());
        return;
    };
    let Phase::Hosting(lobby) = &mut state.phase else {
        return;
    };
    state.roster.push(Member {
        slot,
        name: name.clone(),
        bot: false,
    });
    let welcome = Control::Welcome {
        slot,
        players: options.players,
        roster: state.roster.clone(),
    };
    let _ = link.send_reliable(&welcome.bytes());
    broadcast_roster(lobby, &state.roster);
    lobby.links.push((slot, link));
    state.tell(
        EventKind::Joined,
        vec![("slot", slot_value(slot)), ("name", Value::Str(name))],
    );
}

/// One frame of a joiner waiting on its host.
#[allow(
    clippy::disallowed_methods,
    reason = "a lobby's timeouts are wall time, never a simulation input"
)]
pub(crate) fn poll_join(state: &mut MultiplayerState) {
    let timeout = state.options.as_ref().map_or(5.0, |o| o.timeout);
    let Phase::Joining(joiner) = &mut state.phase else {
        return;
    };
    let arrivals = joiner.link.receive();
    if !joiner.said_hello && joiner.link.state() == LinkState::Open {
        let options = state.options.clone().unwrap_or_default();
        let hello = Control::Hello {
            protocol: PROTOCOL,
            name: options.name,
            token: options.token,
        };
        joiner.said_hello = joiner.link.send_reliable(&hello.bytes()).is_ok();
    }
    for control in arrivals.iter().filter_map(|r| Control::parse(&r.bytes)) {
        if !heard(state, control) {
            return;
        }
    }
    let Phase::Joining(joiner) = &mut state.phase else {
        return;
    };
    let reason = if let LinkState::Closed(why) = joiner.link.state() {
        Some(why)
    } else if !joiner.welcomed && joiner.since.elapsed().as_secs_f32() > timeout {
        Some(String::from("the host did not answer in time"))
    } else {
        None
    };
    if let Some(reason) = reason {
        let kind = if joiner.welcomed {
            EventKind::Closed
        } else {
            EventKind::Failed
        };
        joiner.link.close();
        state.reset();
        state.tell(kind, vec![("reason", Value::Str(reason))]);
    }
}

/// Act on one thing the host said. Answers whether to keep reading.
fn heard(state: &mut MultiplayerState, control: Control) -> bool {
    match control {
        Control::Welcome { slot, roster, .. } => {
            if let Phase::Joining(joiner) = &mut state.phase {
                joiner.welcomed = true;
            }
            state.local = Some(slot);
            state.roster = roster;
            state.tell(EventKind::Connected, vec![("slot", slot_value(slot))]);
        }
        Control::Refuse { reason } => {
            leave(state);
            state.reset();
            state.tell(EventKind::Failed, vec![("reason", Value::Str(reason))]);
            return false;
        }
        Control::Roster { roster } => changed_roster(state, roster),
        Control::Start {
            scene,
            depth,
            roster,
            snapshot,
        } => {
            let Phase::Joining(joiner) = std::mem::replace(&mut state.phase, Phase::Idle) else {
                return false;
            };
            state.roster = roster;
            let snapshot = match snapshot {
                serde_json::Value::Object(map) => Snapshot(map),
                _ => Snapshot::default(),
            };
            state.phase = Phase::Starting(Starting {
                links: vec![(0, joiner.link)],
                scene,
                depth,
                snapshot: Some(snapshot),
            });
            return false;
        }
        Control::Bye => {
            leave(state);
            state.reset();
            state.tell(
                EventKind::Closed,
                vec![(
                    "reason",
                    Value::Str(String::from("the host closed the lobby")),
                )],
            );
            return false;
        }
        Control::Hello { .. } | Control::Absent { .. } => {}
    }
    true
}

/// Take the host's new roster, telling scripts who came and went.
fn changed_roster(state: &mut MultiplayerState, roster: Vec<crate::wire::Member>) {
    let before = std::mem::take(&mut state.roster);
    for member in &roster {
        if Some(member.slot) != state.local && !before.iter().any(|m| m.slot == member.slot) {
            state.tell(
                EventKind::Joined,
                vec![
                    ("slot", slot_value(member.slot)),
                    ("name", Value::Str(member.name.clone())),
                ],
            );
        }
    }
    for member in &before {
        if !roster.iter().any(|m| m.slot == member.slot) {
            state.tell(
                EventKind::Left,
                vec![
                    ("slot", slot_value(member.slot)),
                    ("name", Value::Str(member.name.clone())),
                ],
            );
        }
    }
    state.roster = roster;
}

fn broadcast_roster(lobby: &mut Lobby, roster: &[Member]) {
    let bytes = Control::Roster {
        roster: roster.to_vec(),
    }
    .bytes();
    for (_, link) in &mut lobby.links {
        let _ = link.send_reliable(&bytes);
    }
}

fn refuse(link: &mut dyn Transport, reason: &str) {
    let _ = link.send_reliable(
        &Control::Refuse {
            reason: reason.to_string(),
        }
        .bytes(),
    );
    link.close();
}

fn is_closed(link: &dyn Transport) -> bool {
    matches!(link.state(), LinkState::Closed(_))
}

/// How many slots a lobby may hand out.
fn cap(options: Option<&Options>) -> u32 {
    match options.map_or(0, |o| o.players) {
        0 => MOST_SLOTS,
        players => players.min(MOST_SLOTS),
    }
}

/// The lowest slot nobody plays, while there is room.
fn free_slot(roster: &[Member], cap: u32) -> Option<PlayerId> {
    if roster.len() >= usize::try_from(cap).unwrap_or(usize::MAX) {
        return None;
    }
    (0..MOST_SLOTS).find(|slot| !roster.iter().any(|member| member.slot == *slot))
}
