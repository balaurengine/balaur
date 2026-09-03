> **Status:** built on 2026-09-02 — format 2 with a session origin, optional
> per-tick digests, events and a trailer; the `replay` script module; the
> editor records every play and replays one; the Session dock. `balaur run
> --record` and `balaur replay --verify` now drive the same player the editor
> does. See [generated/](generated/) for what the code actually does.
>
> **Where the implementation decided differently:**
>
> 1. **The origin is settled by the first recorded frame, not by the call to
>    `record`.** Whether the frame a recording starts in is itself recorded
>    depends on where in that frame the call lands: from a script's `update`
>    the tick's own `Stage::Last` records it, from between two frames the next
>    one is first. The recorder therefore holds its header back until the
>    first frame is written and derives the origin tick and time from it. The
>    token counter is *not* deferred — it has to be the value in force when
>    recording began, because a request made between then and the first frame
>    belongs to that frame.
> 2. **The feed does not carry `dt`.** The plan put the recorded step on
>    `ReplayFeed`; `App::advance` both feeds a frame and runs it, so it passes
>    the step straight to `tick` and the feed keeps its one field.
> 3. **A paused replay is held apart from a debugger pause.**
>    `Engine::set_frozen` is the debugger's and only the Rune host touches it.
>    A second flag, `set_replay_hold`, holds the simulation for a paused
>    replay, and `frozen_root` answers for either — otherwise resuming one
>    would release the other, and a breakpoint hit during a replay is both at
>    once.
> 4. **The recorder writes the frames, not the caller.** Recording had been a
>    system the CLI added after boot, which a script could not start mid-run.
>    It is now a resource core writes at `Stage::Last`, so `replay.record(…)`
>    from the editor and `--record` from the command line are the same path.
> 5. **A recorder dropped without `finish` still closes its file.** A
>    recording with no trailer means the session crashed, and every run that
>    simply exited would otherwise look like one.
> 6. **Input lanes are derived by the editor, not by core.** `replay.marks`
>    answers "which ticks had a non-empty list under this key of this source",
>    and the dock asks for `("input", "just_pressed")`. Core never learns the
>    shape of the input plugin's snapshot, and a new source gets a lane
>    without a change here.
> 7. **"Save as…" is "export", and it has one destination.** The widget seam
>    has no file dialog, so a session is copied into the project root, which
>    is where a bug report or a `balaur replay --verify` can reach it.
> 8. **The trailer's digest is a note, not a check.** A stop lands wherever in
>    the frame the button was pressed, which is not a frame boundary, so it is
>    not comparable with anything a replay reaches. Only a frame's own digest
>    is compared, and per-tick digests are a Session-dock toggle (`verify`)
>    rather than always on.
>
> **Four things that had to be made reproducible before an editor replay
> could reproduce anything**, each found by the self-test and each a bug in
> its own right:
>
> - **A node with no stable id was labelled by its entity.** An editor tears
>   its scene down and builds it again, so the same world hashed differently
>   across a rebuild. The fallback label is now the node's path.
> - **The digest included the scope's container node.** The editor makes a
>   fresh one per run and its generated `local:N` id differs every time. The
>   container is the boundary, not game state, and is now excluded.
> - **Animation ignored the freeze.** It advances at `Stage::Update` on a
>   plugin-wide accumulator, so a held game kept animating and the accumulator
>   carried leftover time across a rebuild. It now stands down exactly as
>   `App::run_fixed_steps` does. This also fixes a debugger pause, which never
>   held animation still.
> - **`physics.clear` drained the world instead of replacing it.** Rapier
>   gives a freed handle's slot to the next body with the generation bumped,
>   and the solver works in handle order, so a rebuilt scene of stacked bodies
>   did not simulate the way a fresh process does. Both worlds are now
>   replaced outright, carrying gravity and the step across.

# Plan: session recording and replay in the editor

Every play-in-editor run records what the game was fed — keys, mouse,
touches, gamepads, HTTP replies, websocket traffic — and, once stopped, can
be played back, paused, and scrubbed on a timeline that shows what happened
and when.

## 0. Where the tree is today

Built, and built for the CLI:

| Have | Where |
| --- | --- |
| Per-tick capture of every registered input source, dt bits, digest; JSON Lines with a header | `replay::{Recorder, Frame, Header}` |
| Keys, mouse buttons, position, delta, scroll, touches, dropped files | `InputSnapshot`, registered by `add_replay_resource::<InputSnapshot>("input")` |
| Gamepads | `"gamepad"` source in `balaur_input` |
| HTTP responses and errors; websocket open, message, closed, error | `ExternalIo<NetEvent>` in `balaur_net`, source `"net"` |
| Gamend replies and socket events | `ExternalIo<GamendEvent>` |
| Outbound I/O suppressed while playing: the only sender is behind `ExternalIo::start` | `replay::is_playing` |
| Restore before every plugin's `First` work, so the pump dispatches the recording, not the network | `App::new`, the first `Stage::First` system |
| `balaur run --record`, `balaur replay --verify`, `--entries-at` | `crates/balaur_cli/src/main.rs` `record_to`, `replay_session` |
| Play-in-editor: mirror rebuilt from the document, game scripts attached, a debug scope | `editor/scripts/shell.rn` `toggle_play`, `model.rn` `build_mirror` |
| An engine freeze that holds the scoped subtree still while the editor keeps drawing | `Engine::set_frozen`, `App::run_fixed_steps` |
| A timeline built from `ui::slider`, `ui::scroll`, `ui::dot`, `ui::pill` | the Animate dock, `editor/scripts/dock.rn` `timeline` |
| A 500-entry log ring, and a per-frame diff of it | `logbuf::recent`, `dap.rs` `forward_logs` |
| Dock, command, window and state registries a plugin adds to | `editor/plugins/counter.rn` |

Missing:

- Nothing in the editor records, and scripts cannot start a recording:
  the script API has no `replay` module.
- `Frame.tick` is the absolute engine tick, and `engine::tick`,
  `engine::time` and `Engine::next_token` keep counting across play
  sessions. A replay in the same process hands scripts different tick values
  and different HTTP request ids, so a recorded reply cannot find its
  handler (`request_handlers.shift_remove(request)` finds nothing).
- The CLI replays by calling `app.tick(frame.step())` from outside the
  loop. The windowed loop calls `app.advance(measured)` and nothing inside
  can substitute the recorded dt or run several ticks in one frame.
- `digest::entries` walks from the engine root, so the editor's own nodes
  (camera, chrome) are in the hash and a verify inside the editor fails for
  reasons that have nothing to do with the game.
- The restore at `Stage::First` replaces the whole `InputSnapshot`, and the
  editor's viewport tools read it some forty times (`gizmo.rn`,
  `gizmo2d.rn`, `polygon.rn`, `rig.rn`). While a replay plays, those tools
  would see the recorded pointer.
- The outbound side of a request is not recorded: only the reply, keyed by
  id. A timeline cannot show "GET /score sent at tick 40" from the file.
- Log lines carry wall time, not a tick. Script errors, debugger pauses,
  spawns and frees leave no trace in a recording.
- Snapshots do not cover spawn or free, so scrubbing backwards by restoring
  one is unsafe. A seek has to re-simulate from the session's start.
- Text input is not in `InputSnapshot`. A game text field does not replay.

## 1. Design

### What is recorded, and what it costs

A recording stores input, not state: the same rule as today. Each tick the
recorder asks every source for its value and writes one JSON line. For the
input snapshot that is the set of keys down, the frame's edges, the mouse and
the touches — a few hundred bytes, microseconds to encode, about 1.5 MB per
minute at 60 Hz. That cost is not worth engineering around in a first
version; delta encoding is listed under "later".

The cost that was worth removing is the digest. Today every frame carries
one, and computing it walks every node and hashes every component, every
tick. In the editor the digest is off by default: the recorder writes one at
stop, in the trailer, so a replay can say at its end whether it reproduced
what the user saw. Per-tick digests remain available (`replay::record(path,
#{ digest: true })`) for determinism debugging, where "which tick" is the
whole point. `Frame.digest` becomes optional; `replay::FORMAT` goes to 2.

### The file

Format 2 is still JSON Lines, and still `.blr`:

```
{"format":2,"project":"…","seed":…,"origin":{"tick":4210,"time":70.16,"tokens":31},"scripts":"<hash>","started":"2026-09-02T14:03:11Z"}
{"tick":4211,"dt":…,"sources":{"input":…,"gamepad":…,"net":…},"events":[…]}
…
{"end":{"reason":"stop","tick":6013,"digest":…}}
```

- `origin` is what the engine's counters read when the session started.
  Replay sets them back to exactly that, so `engine::tick()`,
  `engine::time()` and every `next_token` id come out as they did.
- `scripts` is a hash of the game's script sources at start. The dock warns
  when the sources on disk no longer match: the replay will still play, but
  it may not reproduce what was seen.
- `events` is per-tick, written by the recorder from an `EventLog` resource
  (below). Not a replay source: nothing restores them, the subsystems
  re-emit them on replay, and the dock can compare.
- The trailer's `reason` is `stop`, `reload` or `quit`. A file with no
  trailer ended in a crash, which the dock says in as many words. A
  per-frame flush stays, as today: the session that crashes is the one worth
  having.

### Engine seams

Four additions in `balaur_core`, none of which change the CLI's behaviour:

1. **Session origin.** `Engine::set_clock(tick, time)` and
   `Engine::set_tokens(next)`. Public and documented as "for restoring a
   session origin"; nothing else should call them. Editor scripts use no
   `task::`, `http::` or `websocket::`, so putting the token counter back
   cannot collide with an editor operation in flight.
2. **The feed carries dt.** `ReplayFeed` holds the next frame's dt as well
   as its sources, and `App::advance` uses that dt when a frame is queued.
   A `ReplayPlayer` resource owns the loaded `Session`, a cursor, and a
   state (`stopped`, `playing`, `paused`, `seeking`). `advance` does the
   driving: playing feeds one frame per call; paused feeds nothing and holds
   the engine frozen through the existing `set_frozen`, so the scoped game
   stands still while the editor draws; seeking feeds up to a budget of
   frames per call (600 to start; tune once measured) and reports
   progress, so a long seek does not stall the window.
   `advance` needs `&mut App`, which is why this lives there and not in a
   system.
3. **A scoped digest.** `digest::entries` walks `debug_scope` when one is
   set and the root otherwise. `balaur run` sets none, so the CLI's traces
   are unchanged.
4. **An event log.** `EventLog` resource: `push(kind, label, data)`, cleared
   by the recorder after each frame's write. Kinds in the first cut:
   `net.request` (method, URL, id — pushed in `NetState::request`),
   `log` (level, tag, message — the recorder diffs `logbuf::recent` per
   tick the way the DAP server does), `script.error` and `debug.pause`
   (from `paused()` at `Stage::Last`). Input edges and net arrivals are
   derived from the sources when the file is read; recording them twice
   buys nothing. Spawn and free come later.

### The `replay` script module

Installed beside `debugger`, in `crates/balaur_core/src/replay_api.rs`:

| Function | Does |
| --- | --- |
| `record(path, opts)` | Start recording: write the header with the current counters as origin. `opts.digest` turns per-tick digests on. |
| `stop(reason)` | Write the trailer with a scoped digest, close the file. |
| `recording()` | The path being written, or `None`. |
| `load(path)` | Read a session, set `ReplayMode::Playing`, restore the origin counters and the RNG seed. Nothing has ticked yet: the editor attaches scripts after this call, so `init` runs under playback and any request it makes is suppressed and gets the recorded id. |
| `play()`, `pause()` | Drive the player. |
| `seek(tick)` | Forward only from the player's point of view. The dock handles backwards (below). |
| `position()`, `length()`, `state()` | Cursor, frame count, and one of the four states. |
| `header()` | Project, origin, script hash, started, and the trailer if there is one. |
| `events(from, to)` | The events between two ticks, input edges and net arrivals included, from an index built once at load. |
| `verified()` | After the last frame: `None`, or the first tick whose recorded digest differed. With end-only digests that is the last tick or nothing. |
| `unload()` | Back to `ReplayMode::Off`. |

### The editor

Recording is on for every play, no setting in the first version. Where:

```
<user data dir of the editor>/sessions/<game name>/<started>.blr
```

Outside the game's project directory, so nothing lands in version control by
accident. The ten newest are kept; older ones are pruned when a new one
starts. "Keep" renames a file to `kept-<started>.blr`, which pruning skips;
"Save as…" copies one anywhere, which is how a session reaches a bug report
or CI's `balaur replay --verify`. Overwriting a single file was considered
and rejected: the most common thing to do after seeing a bug is to press
Play again to check something, and that would erase the session worth
keeping.

Play, in `shell.rn`:

1. `replay::record(path)` **before** `model::attach_game_scripts`: the
   origin has to be taken before any `init` runs, since `init` may already
   issue a request and take a token.
2. Attach, unpause, run — unchanged.

Stop: `replay::stop("stop")` before `build_mirror`. Quit while playing:
`replay::stop("quit")`.

Hot reload: the editor is the one reloading a game script during play
(`shell.rn` calls `engine::reload_script` on ⌘S). Before that call,
`replay::stop("reload")`. The recording ends there and nothing starts
another until the next Play. Continuing through a reload was considered and
rejected: the frames after it were produced by different code, and the
world they started from cannot be rebuilt from the recording alone. The
partial session is kept, not discarded — it is still a true record of what
the player did, and it replays up to the point where the code differs, which
the script hash makes visible.

Replay of a session, from the Session dock:

1. `stop_play`, so the mirror is rebuilt from the document — the same start
   the recording had.
2. `replay::load(path)`, then `model::attach_game_scripts`, then
   `replay::play()`.
3. While the state is `playing`, viewport tools are off: `editor.rn` skips
   the tool `update` handlers, and the gizmos draw nothing. Egui shortcuts
   and widgets take their input from egui, not the snapshot, so the dock
   stays usable. Paused, the tools come back — the engine is frozen and the
   live pointer is harmless.
4. The chrome's pause button drives `replay::pause` while a session is
   loaded, not `physics::set_paused`; a physics pause would diverge.
5. `engine::time` jumps back to the session's origin on load. The editor's
   TTL caches in `model::source` and the Assets dock would consider
   themselves fresh until time caught up; the dock clears them on load.

Seeking backwards is rebuild-and-fast-forward: steps 1 and 2 again, then
`replay::seek(tick)`. The player runs the budget per frame and the ruler
shows the cursor moving. Keyframe snapshots would make this instant and are
listed under "later", gated on snapshot covering spawn and free.

### The Session dock

A dock tab beside Output, Assets, Timeline and Debugger, drawn with the
Animate timeline's vocabulary:

- A session list: started, length in ticks and seconds, reason it ended,
  the script-hash warning when it applies, Keep, Save as, Delete.
- A ruler: `ui::slider` over `0..length`, showing the cursor; click or drag
  to seek. Play, pause and step-one-tick beside it.
- Lanes, one row each in a `ui::scroll`: **Input** (a dot per key or mouse
  edge, the name on hover), **Net** (request sent, reply arrived, with
  status), **Log** (warn and error as dots, info on hover), **Errors**
  (script errors and pauses). A lane is a single horizontal with dots placed
  by `tick / length * lane_w`, which is what the Animate lanes already do.
- A detail list for the selected tick: every event, in order, as text.
- A marker where `verified()` names a tick. A replay that diverged is
  information, not a failure.

## 2. Phases

1. **Core seams.** Format 2 with origin, script hash, optional digest,
   events and trailer; `set_clock`, `set_tokens`; the feed carries dt;
   `ReplayPlayer` in `App::advance` with play, pause and budgeted seek;
   scoped digest. A headless test in `crates/balaur_core/tests/replay.rs`
   records a session, resets the engine's counters, rebuilds the scene in
   the same process, replays, and checks the end digest — including a fake
   `ExternalIo` request whose reply must reach its handler by id.
2. **`replay` script module.** The table above, with `events` and
   `verified` stubbed until phase 5.
3. **Editor records.** Play, stop, quit and reload wired; the ring and its
   pruning; Keep and Save as. Nothing plays back yet.
4. **Editor replays.** Load, play, pause, seek; tools off while playing;
   the chrome's pause button; cache reset. Backwards seek by rebuild.
5. **Events.** `EventLog`, the four producers, the log diff, the index and
   `events(from, to)`, `verified`.
6. **Session dock.** Lanes, ruler, detail list, marker, and the `verify`
   toggle that turns per-tick digests on.
7. **`--state sessiondemo`**, in the editor's own self-test suite and in
   `scripts/e2e.sh`: play records, stop closes the file, the recording plays
   back into a rebuilt scene, reproduces it tick for tick, and closing hands
   the editor back. It is what found the four bugs above.

Built since: spawn and free events, from `scene.spawn` and the deferred free;
text input, as `input.typed`, fed from the window backend's character events
and recorded with the rest of the snapshot; `--verify` compares only the
frames that carry a digest and says so when a recording carries none.

Later, in no order: keyframe snapshots for instant seek, once a snapshot
covers nodes spawned and freed since it was taken; delta-encoded input, if
sessions ever grow enough to be worth it.
