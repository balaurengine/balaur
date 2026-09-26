> **Status:** written 2026-09-26 from an audit of every crate, done by reading
> the code. §2, §3 and §4.1 to §4.6 are built; §4.7 is next. The order is bugs
> first, then one delivery path, then the events the engine does not send yet.

# Plan: events, and one way to hear each of them

A game reacts to what happened: a body landed, a clip ended, a sound
finished, a button lost focus. Today the engine tells a script about it in
four different shapes, some of those shapes reach only one audience, and a
set of events the editor offers never fire at all.

## 0. Where the tree is today

A script hears the engine through four channels:

| Channel | What it is | Who hears it |
| --- | --- | --- |
| Hook | the engine calls `on_<name>(this, ..)` on one node's script, at once | that node's script |
| Emitted event | `events::emit_from(node, name, payload)`, delivered at the top of the next `Update` | `emitted:<name>` rows on the node, `events::listen` listeners, `task::wait(events::next(..))` |
| Binding row | `[[nodes.bindings.rows]]` naming an event in `hooks::BINDABLE`, run by `bindings::fire` | the scene, with no script |
| Poll | a function the script asks every frame | whoever asks |

Only the timer, the animation player, the state machine, `node.set_visible`
and widget click, change and submit reach more than one of those. Physics
contact force, joint break and tear, the tween, every network module and the
window focus, dark mode and quit notices are hooks only: a scene row cannot
answer them, another node cannot subscribe, and nothing can await them.

What a node sends is written down nowhere a tool can read. `ComponentDef`
has no list of events, so the Events view offers the same 16 names on every
node, and the generated reference names no event at all.

## 1. Design

**An engine event is one call:** `events::announce(eng, node, name,
payload)`. It runs the node's rows for the event, calls the node's own
`on_<name>(this, payload)` at once, and queues the event for subscribers and
awaiters. The pump then skips the rows it already ran and the emitter's own
subscription, so no handler runs twice.

**One payload.** Every engine event carries exactly one value: the other node
for a collision, the clip for `animation_finished`, a map when there is more
to say (`on_contact_force(this, contact)` with `other`, `force` and
`direction`). A node's own hook and a subscriber's handler then have the
same signature.

**Rows keep their two spellings, each with one meaning.** A name in
`hooks::BINDABLE` is a core interaction hook, run where it happens: pointer,
key, action, scroll, resize, `variable_changed`, `state_changed` and the
collision pair. Every other event a node sends is `emitted:<name>`, whether a
component or a script sent it. The pump never runs `emitted:` rows for a
`BINDABLE` name, so a collision has one spelling.

**A component declares what it sends.** `ComponentDef` gains `events`, a
list of `(name, payload)` pairs. The Events view offers the core hooks plus
the declared events of the components on the selected node. The generated
reference lists them per component, and `api.json` carries them for the
website.

**The core hooks stay where they are.** `interact.rs` keeps dispatching
pointer, key, action, scroll and resize with the handled-stops rule; those
are input routing, not node events.

## 2. Bugs

Each is a wrong result today, not a missing feature.

1. **Four rows never fire.** `collision_enter`, `collision_exit`,
   `variable_changed` and `state_changed` are in `BINDABLE` and the Events
   view offers them, but `balaur_physics/src/shared/events.rs`,
   `balaur_core/src/variables.rs:239` and `balaur_core/src/states.rs:84` call
   the script and skip `bindings::fire`. The Godot import writes
   `collision_enter` rows for `body_entered`.
2. **A collision exit is lost** when the collider being touched goes away:
   rapier reports the stop after the handle left the set, and the `let-else`
   in `shared/events.rs` drops it. A sensor keeps holding a freed node.
3. **Animation and state machine handlers can run twice.** Both call the hook
   and emit (`balaur_anim/src/system.rs:644`, `machine.rs:676`); a script
   that also subscribes to its own node runs the handler again a frame later.
4. **Paused scripts miss the window.** Focus, dark mode and quit use
   `call_all`, which skips paused scripts (`balaur_core/src/facts.rs:368`,
   `balaur_render/src/kiss3d_input.rs:109`); `on_paused_changed` already uses
   `announce`.
5. **Two awaits never return.** `websocket.connect` and `gamend` `connect`
   return an id that nothing wakes (`balaur_websocket/src/lib.rs:226`,
   `balaur_gamend/src/lib.rs:536`).
6. **Widget events that need a handler first.** A markup link emits only when
   `on_link` is set, and a label or panel with `on_click` ignores the pointer
   (`balaur_ui/src/widget/text.rs:383`, `layer.rs:856`). A flat menu pick overwrites its caption
   (`input.rs:290`). Accept on a focused `fold` does not flip it
   (`kinds.rs:509`).
7. **`visibility_changed` from one writer in three.** Only `node.set_visible`
   emits it; the binding `visible` action and the animation `visible` track
   write the flag silently.
8. **Docs that name the wrong key.** `move_character` says `grounded` for
   `on_floor`, `wheel_state` says `grounded` for `in_contact`, and the joint
   break doc says both ends hear it while only the joint's node does.

## 3. One path

1. `events::announce` in `balaur_core/src/events.rs`, with the pump skipping
   what it already delivered; `announce` fixes bugs 1 and 3 on the way.
2. `ComponentDef::events`, filled for every component that sends one, with
   the Events view, `scene.bindable_events(node)`, `gen_docs.py` and
   `api.json` reading it.
3. Every engine event through `announce`: collisions, contact force, joint
   break, tear, tween, animation, state machine, timer, visibility. Contact
   force, joint break and tear carry one map. Widgets keep their handler keys
   (`on_click = "method"`) and emit `click`, `change` and the rest, since the
   key names the method and a second call to it would run it twice.
4. The network modules: an `EVENT_*` constant per `kind` in every crate, a
   `kind` on every http event, and `error` as the one failure kind. A web
   message stays the payload the parent frame posted.
5. A generated hooks page: every hook the engine calls, with its arguments
   and whether answering `true` stops it, from `balaur_core/src/hooks.rs`.

## 4. Events the engine does not send yet

In order of how often a game needs them.

1. **Sound finished:** built. The node whose `sound` played announces
   `finished` with the handle, from the decoded length counted on the fixed
   step, so a headless run and a replay end it on the same tick;
   `events::next("finished", node)` awaits it.
2. **Widget pointer and focus:** built. Every widget announces
   `pointer_enter`, `pointer_exit`, `pointer_down` and `pointer_up` with the
   button, as a world node does, and emits `double_click`, `focus` and
   `blur`; a click into a field is focus arriving. A field still submits on
   Enter and on a click away, and only the click away is a `blur`.
3. **Widget commits:** built. A slider, number field and colour picker emit
   `commit` once a drag or an edit ends. A double-clicked row emits
   `activate`, and a tree caret emits `fold` with the row and whether it is
   open. Menus, dropdowns and dialogs emit `opened` and `closed`; a scroll
   emits `scrolled` with its offset. A window's cross emits `close_request`,
   and with `hide_on_close = false` it only asks.
4. **The tree:** built. A parent announces `child_added` and `child_removed`
   for a node added, instantiated, moved or freed under it, and a freed child
   is still readable in the parent's own hook. A node announces `renamed` with
   the name it had and `reparented` with the parent it left. Loading a scene,
   switching one and restoring a snapshot stay silent.
5. **Physics:** built. A body and a soft body announce `sleeping_changed`
   with whether they sleep now. A body hears the collisions and contact
   forces of every collider under it. A soft body takes `events` and
   `contact_force_threshold` as a collider does, and its `tear` carries the
   torn edges. A joint break names both ends and the force. 2D gained
   `is_moving`, `contacts`, `bodies`, `active_bodies`, `potential_energy` and
   `effective_dominance`.
6. **Animation:** built. A player announces `animation_started`,
   `animation_changed` with `#{ from, to }` and `animation_looped`, whether
   `play` or a state machine started the clip. A tween announces
   `tween_looped` with `#{ tween, played }` and `tween_step` with
   `#{ tween, step }`. A method key hands its `args` to the method. A
   one-shot `particles` burst announces `finished`, timed from its settings
   on the fixed step so a headless run hears it too.
7. **The app and input:** suspend and resume, low memory, orientation and
   safe area; right and middle mouse buttons; action released; gamepad
   connected and disconnected; settings and language changed.
8. **Render:** camera became and stopped being current; a node entering and
   leaving the screen; a screenshot written.
9. **Network:** the Gamend addon's 19 unnamed server events, `match_found`
   first (regenerated from the gamend repo); `http.cancel` and upload
   progress; a websocket close code and state; `web.visible` changes and
   `web.unlisten`.
10. **The Godot import:** follows each of these: the signal map names what the
    engine now sends and drops what it does not.

## 5. Not planned

- **Apple arrivals** (a notification in front, a remote push payload, Game
  Center invites, iCloud changes): each needs Swift in `balaur_apple` and a
  device to test on; `docs/PLAN-apple.md` holds them.
- **A native plugin reporting to a script** waits on `docs/PLAN-c-api.md`.
- **Navigation** has no crate; `docs/PLAN-navigation.md` names its events.
