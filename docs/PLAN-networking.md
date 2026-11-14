# Plan: networking

What a networked game needs that the engine does not have: replication, the
latency hiding that rides on it, and the browser transports.
`ARCHITECTURE.md` "Networking and state sync" holds the design these steps
follow. The match a script opens is `docs/PLAN-multiplayer.md`, and the
server half is `docs/PLAN-gamend.md`.

## Transports

- WebTransport in the browser is step 13. Its client
  (`balaur_webtransport/src/browser.rs`) is a hand-written wasm-bindgen
  binding, because `web_sys::WebTransport` sits behind
  `web_sys_unstable_apis`. A browser accepts a self-signed host through
  `serverCertificateHashes` only while the certificate is valid for at most
  two weeks.
- WebRTC data channels are step 14, for one case: browser peer-to-peer
  without a relay. They need a signalling channel (a Gamend room) and
  STUN/TURN. Candidates are `matchbox` and `webrtc` (webrtc-rs), behind the
  same `Transport` trait.
- Steam networking sockets are `docs/PLAN-steam.md`, native only, behind the
  same trait.
- Not planned: raw UDP for scripts, ENet, laminar and other custom UDP
  protocols. QUIC datagrams already solve what they solve, and none of them
  runs in a browser.

## Steps

The list starts at 9 because other plans cite these steps by number. Two
engines on loopback in a headless CI run is the bar for 9 and 10.

9. Replication: a `replicate` key on a node (`authority`, `components`,
   `mode`), read off the engine's change log. It sends deltas against each
   observer's last acked tick, quantised off the schema's ranges, under an
   authority rule, with RPC by `StableId` and join in progress from a
   baseline snapshot. Ends with: two engines on a faulty loopback link
   (`balaur_net::Faulty`) agree on every replicated property.
10. Client prediction and server reconciliation: the client runs its own
    input on the tick it is pressed and holds it until a delta acks it. A
    correction restores that node alone, replays what is pending, and decays
    out of the render transform. Unowned nodes are drawn a send interval or
    two behind and interpolated, with extrapolation off by default. The clock
    offset rides the digest channel. Needs a per-body physics restore, since
    the rapier snapshot restores a whole world. Ends with: a predicted player
    under injected latency moves on the tick it is pressed and settles on the
    server's state.
11. Lag compensation: the server rewinds to the tick the shooter saw before
    testing a hit, off the snapshot ring, bounded by how far back a rewind
    may go. Ends with: a hit claimed at a tick past the bound is refused.
12. Interest management and a bandwidth budget: a per-observer filter over
    the replicated set, a send rate apart from the tick rate, and a cap that
    drops the least urgent deltas rather than growing a queue. Ends with: an
    observer is sent nothing outside its filter.
13. WebTransport in the browser: the client is built and in the default web
    build. Ends with: a browser joining a native host in a test.
14. WebRTC data channels, only when a game needs browser peer-to-peer without
    a relay. Ends with: the loopback tests pass over it.

## Open questions

1. Script state in rollback: a snapshot restores every script's plain
   fields. A script holding a handle to something outside the simulation has
   no way to say so. Whether to require one or warn waits for the first game
   that hits it.
2. Hot reload during a session: journaled inputs replayed against changed
   code cannot reproduce the recorded state. The proposal: a reload truncates
   the journal and re-baselines from the current snapshot.
3. Which nodes a client predicts: whether prediction follows from
   `authority = "owner"` or is its own key on `replicate`. Settle it before
   step 9, since it decides how change detection is scoped.
4. Where the send rate is set: a `[webtransport]` default, per node, or per
   observer. The answer decides whether interpolation delay can be a fixed
   number of frames.
