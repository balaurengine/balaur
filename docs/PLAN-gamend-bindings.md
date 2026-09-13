> **Status:** steps 1, 2 and 3 built on 2026-09-13, and step 4 begun.
> `clients/generate_balaur.py` in the Gamend repository writes
> `addons/gamend`: 243 operations, 71 realtime events, over the hand-written
> client, auth and presence layer. Balaur carries the copy in
> `editor/library/addons/gamend`, offers it from the Library dock, and a
> test asserts the method, path and body a generated call puts on the wire.
> The port takes it with `port/sync_gamend.sh` and its scenarios still pass.
> What step 4 has left is the call shapes, which §3 now states. This is
> `docs/PLAN-gamend.md` step E1. Measured against Gamend's
> OpenAPI document (243 operations, 37 tags, at most two path parameters),
> its realtime protobuf (31 messages), the hand-written Godot façade that
> declares 84 realtime signals, and what `../polyglot-pirates-game` calls:
> 47 server hooks, 30 façade operations, 16 client verbs, 20 auth verbs.

# Plan: the Gamend SDK for balaur

Gamend generates an SDK per client from one OpenAPI document: a JavaScript
package and a Godot addon. Balaur gets the third, the same way: a generator
in Gamend's `clients/`, run by the same person at the same time as the other
two, emitting a Rune addon — `addons/gamend/` — that a game requires the way
Polyglot Pirates requires `addons/gamend` today in Godot. The engine's nine
`gamend::` calls are the transport underneath and do not change. Nothing is
curated out: every operation gets a function, because the next game uses the
one this game does not.

## 0. Where the three trees are today

Gamend, read on 2026-09-13:

- `clients/generate_godot.sh`: `mix openapi.spec.json` writes
  `clients/godot/openapi.json`; openapi-generator's `gdscript` target
  (Docker) writes `apis/`, `models/`, `core/`; ninety lines of `perl -i`
  repair its output; the hand-written layer in `clients/gamend_template/`
  is copied over it into `godot_addons/addons/gamend/`, which a game copies
  in whole. `mix host.proto.gen --only godot` writes the protobuf bindings.
- That hand-written layer is the SDK a game actually sees: `GamendClient`
  (HTTP, the socket, KV cache, hook RPC, network state), `GamendAuth`
  (device, email, six OAuth providers, session save and restore),
  `GamendPresence`, and `GamendApi` — a 307-function façade naming every
  operation `<tag>_<operationId>` (`quests_my_quests`,
  `parties_create_party`) and declaring 84 realtime signals
  (`lobby_member_joined`, `party_invite_accepted`, `kv_updated`, …), fed by
  a dispatch over about seventy server event names. No table holds that
  dispatch; it is code.
- The document: 243 operations over 207 paths, every one with a unique
  `operationId`, 144 with no path parameter, 91 with one, 8 with two; 98
  bodies, 93 of them with named fields; 55 with query parameters; one with
  both a body and a query; 34 with no argument at all. Four named schemas,
  so there is no model layer worth generating: every body and reply is a
  table.
- The proto: 31 messages mirroring the JSON payloads, timestamps as
  `<field>_at_ms`. JSON is the socket's default and what this plan speaks.

Balaur:

- `crates/balaur_gamend`: `configure`, `login` (device, email), `rest`,
  `connect`, `join`, `push`, `leave`, `call_hook`, `close`; one worker
  thread, delivery once per tick, every arrival an `ExternalIo` completion,
  so a recording replays an online session. This is the whole transport
  the SDK needs; it is the `core/` the Godot generator has to emit.
- `script::require(path)` loads a module by project-relative path and hands
  back its `pub fn`s as fields, called `(m.f)(..)`. A required function takes
  at most five arguments (`shared.rs`'s trampoline).
- `editor/library/` holds what the Library dock offers a project —
  materials, models, rigs, scenes, scripts, shaders, skies, templates — each
  an entry in `manifest.toml` with a `kind`, a `file` and a card line.
  `balaur new <path> --template <id>` copies a template. Nothing there is a
  directory a project adds to itself; that is the one new kind this needs.
- The GDScript translator (`docs/PLAN-gdscript.md`) converts the game, and
  its `gamend_controller` is one of twelve files still ported by hand
  because every one of them reaches `addons/gamend`.

The port: the game's scripts reach the SDK through 137 members —
`gamend_api.<tag>_<op>(..)` for 30 operations, `client.*` for 16 verbs
(`rpc_call`, `register_kv`, `get_row`, `fetch_cached`, `fail_network`, …),
`auth.*` for 20 (`device_auth`, `provider_auth`, `restore_session`,
`go_offline`, …), and 47 server hooks through `rpc_call`, which is
`gamend::call_hook`.

## 1. Design

**The generator is a script in Gamend, not an openapi-generator target.**
openapi-generator has no Rune output, and a new language there is Java plus
a template set; the ninety lines of `perl` that repair the Godot output show
what fighting a near-fit generator costs. `clients/generate_balaur.py` reads
`clients/godot/openapi.json` (written by the same mix task), the proto and
an event table, and writes the addon. It is about six hundred lines, needs
no Docker, and `clients/generate_balaur.sh` wraps it the way
`generate_godot.sh` wraps the other.

**The addon is Rune modules, laid out like the Godot addon.** Generated:

- `addons/gamend/api.rn` — the façade. One `pub fn` per operation, named
  `<tag>_<operationId>` exactly as `GamendApi.gd` names it, so a game ported
  from Godot calls the same name and the translated Polyglot Pirates code
  needs no renaming. Naming lints do not apply: this is a project script,
  not engine API.
- `addons/gamend/events.rn` — one constant per realtime signal
  (`EVENT_LOBBY_MEMBER_JOINED = "lobby_member_joined"`, all 84) and
  `decode(topic, event, payload)`, which turns a raw socket message into
  `#{ kind, ..fields }` by the event table, normalising `_at_ms` to the REST
  name where the proto says so. A message the table does not name comes
  back with `kind = "message"`, as the engine delivers it today.
- `addons/gamend/README.md` — every function with the operation's own
  `summary`, grouped by tag: the SDK's reference, as `apis/*.md` is Godot's.
- `addons/gamend/version.rn` — `GAMEND_VERSION`, stamped by CI as the Godot
  template's is.

Hand-written, in `clients/balaur_template/`, copied over the generated
files exactly as `gamend_template` is:

- `client.rn` — `configure`, `connect`, the socket lifecycle and its
  `network_*` events, `rpc_call` / `rpc_send` / `rpc_push` over
  `gamend::call_hook` and `push`, `register_kv` as a `join` plus the
  `kv:subscribe` push and the row cache behind `get_row`, `has_row`,
  `fetch_cached`, `write_deduped`, `clear_row`. The sixteen verbs the game
  uses, over the nine calls.
- `auth.rn` — `device_auth` and email over `gamend::login`; `provider_auth`
  as `list_auth_providers`, `engine::open_url`, and a poll of the OAuth
  session operations until it resolves; `save_session` and
  `restore_session` through `save::`; `go_offline`, `link`, `unlink`,
  `state`, and a `state_changed` event on the node that owns it.
- `presence.rn` — the user cache, two verbs.

**One call shape, under the five-argument limit.** A façade function takes
the node, the path parameters in path order, a `params` table for the body,
and an `options` table for the query: `(api.lobbies_quick_join)(node,
#{ title: "duel", max_users: 2 })`, `(api.quests_my_quests)(node, (),
#{ category: "daily" })`. With at most two path parameters that is at most
five arguments, which is the trampoline's ceiling — measured, not assumed.
Each function checks the body's required fields against the document and
returns `()` with a logged error naming the field before any I/O; otherwise
it returns the id `gamend::rest` returns, so a caller awaits it with
`task::wait` exactly as it awaits `rest`. The 34 no-argument operations
take the node alone.

**The event table is data, checked in beside the spec.** OpenAPI does not
describe the socket. `clients/events.json` lists every (channel, event,
signal, proto message) row — seeded from `GamendApi.gd`'s dispatch, which
is the only place that mapping exists today — and the generator reads it.
A row the server stops sending, or an event it starts sending that the
table lacks, is what §4's live test exists to catch. The Godot façade
reading the same table instead of carrying the dispatch in code is Gamend's
own roadmap item, not this plan's.

**Copied into balaur as a library addon.** `editor/library/addons/gamend/`
is the checked-in copy, one `manifest.toml` entry of a new `kind = "addon"`
whose `file` is a directory. `scripts/sync_gamend.sh` refreshes it from a
Gamend checkout beside this one or from the addon artifact Gamend's CI
publishes, the way the website's `sync-docs.sh` refreshes from this
repository's `docs/generated`. The Library dock's card for an addon copies
the directory into the open project; `balaur new --addon gamend` does the
same for a new one. The engine's own tests exercise the copy against the
in-process Gamend stand-in in `crates/balaur_gamend/tests`.

**Copied into a game as `addons/gamend/`.** Polyglot Pirates' port lists
`/addons/gamend/` in `port/ported.txt` so `port/reimport.sh` never
translates the Godot SDK again, and `port/sync_gamend.sh` copies the Rune
addon in from the library. The translator then treats the SDK's classes as
modules: a member declared `GamendApi`, `GamendClient` or `GamendAuth`
resolves to `script::require("addons/gamend/<file>.rn")`, and a call on it
to `(m.f)(..)`.

Keeping the Godot names carries the call *sites*, not the call *shapes*:
the façade takes a request model or positional query parameters where this
SDK takes a table, so a game meets the SDK through one module of its own
that offers the old signatures over the new ones. For Polyglot Pirates
that is about thirty functions against 181 call sites, which is the
adapter this plan was asked for — much smaller than the SDK it replaces,
and not nothing.

**Determinism is unchanged.** Every SDK call is one of the nine, delivered
once per tick and recorded. The SDK adds no thread, no timer and no state
outside the node that called it.

## 2. The surface

| Piece | Count | Generated or written |
| --- | --: | --- |
| Façade functions, one per operation | 243 | generated, `api.rn` |
| Of which admin, answered 403 without an admin token | 89 | generated |
| Realtime event constants and their decoders | 84 | generated, `events.rn`, from `events.json` |
| Proto messages the decoders shape | 31 | generated |
| Client verbs | 16 | written, `client.rn` |
| Auth verbs | 20 | written, `auth.rn` |
| Presence verbs | 2 | written, `presence.rn` |
| Engine calls underneath | 9 | unchanged, `crates/balaur_gamend` |

## 3. Steps

Steps 1, 2 and 5 are Gamend-side and run in `../gamend`; 3 and 4 are here
and in the port. Both repositories see the same addon, so the split is by
where the file lives, not by who does it.

- **1. The generator (Gamend) — built.** `clients/generate_balaur.py`,
  `clients/generate_balaur.sh`, `clients/events.json` seeded from the Godot
  façade, `clients/balaur_template/` with `README.md` and `version.rn`, the
  output in `balaur_addons/addons/gamend/`. Ends with: `api.rn` carries 243
  functions with their summaries as doc comments, `events.rn` carries 84
  constants and a decoder, and a scratch project holding only the addon
  passes `balaur check` under the nightly `balaur` Gamend's CI downloads.
- **2. The written layer (Gamend) — built.** `client.rn`, `auth.rn` and
  `presence.rn` in `clients/balaur_template/`, joined by `core.rn`: the
  query string, the required-field check and the reply helpers every
  generated call needs. What is left is the live flow against
  `mix dev.start`: a device login, a hook through `rpc_call`, a KV key
  subscribed and its `kv_updated` decoded.
- **3. The library copy (balaur) — built.** `editor/library/addons/gamend/`, the
  `addon` kind in `manifest.toml` and the dock, `balaur new --addon`,
  `scripts/sync_gamend.sh`. Ends with: a new project from any template plus
  the addon passes `balaur check`, and a test in `crates/balaur_gamend/tests`
  boots it against the in-process stand-in and calls
  `users_get_current_user`, `rpc_call` and one decoded event through the
  addon rather than through `rest`.
- **4. The port.** `/addons/gamend/` in `ported.txt` and
  `port/sync_gamend.sh` are done: the SDK is copied in rather than
  translated, and the scenarios still pass. What is left is the call
  shapes. The names match — that was the point of §1's naming — but the
  arguments do not: Godot's façade takes a request model
  (`lobbies_quick_join(request)`) or positional query parameters
  (`quests_my_quests("achievement", "", page, page_size)`), and this SDK
  takes a table. The game has 181 such call sites over 24 files, using
  about 30 distinct operations. Rewriting 181 sites is the wrong shape of
  work; one module in the port that offers the Godot signatures over the
  Rune SDK, about 30 small functions, is the right one. With it and a
  translator rule that a member typed `GamendApi`, `GamendClient` or
  `GamendAuth` is a `script::require`, the 181 sites translate unchanged.
  Ends with: `gamend_controller` is removed from `ported.txt` and
  `login_offline` and `main_menu_ready` still pass; then
  `online_game_start` passes against a local `mix dev.start`.
- **5. The release path (Gamend).** CI runs the generator, stamps
  `GAMEND_VERSION`, publishes `balaur_addons/addons/gamend` as an artifact
  beside the Godot one. Ends with: a version bump in Gamend reaches a game
  with `sync_gamend.sh` and nothing typed by hand.

## 4. What CI can prove

- Gamend: the generator runs in CI on every change to the spec or the
  table, and `balaur check` over the scratch project, with the nightly
  engine, fails the build when the addon does not compile. The live flow of
  step 2 runs against `mix dev.start` in the same job, which is the check on
  `events.json`: a server event the table does not name fails it.
- Balaur: the step 3 test against the in-process stand-in, which grows a
  recording router so a test can assert the method, path and body any
  façade function sends without a server. `scripts/sync_gamend.sh --check`
  in precommit, so the library copy and the version it names never drift.
- The port: its scenarios, as today.
- What neither can: a real provider's OAuth page, and the socket under a
  real NAT.

## 5. Open questions

1. **Protobuf on the socket.** The server serves it on request; the Godot
   addon asks for it. This plan speaks JSON, which is the default and what
   the stand-in can fake. The generator could emit a decoder from the same
   proto when the bytes matter.
2. **Engine-level bindings too.** The earlier draft of this plan generated
   a Rust module into `crates/balaur_gamend` so every operation appeared in
   `docs/generated/api.json`, documented and lint-checked as engine API.
   That is still worth having — a script SDK is not in the reference, and
   a typo in a path is caught at run time rather than at `balaur check` —
   and the same generator can emit it from the same document. It is not a
   step here because the port does not need it; it is a step when the
   reference does.
3. **How the proto is read.** Parsing `gamend_realtime.proto` in the Python
   script is a few dozen lines for 31 flat messages; `mix host.proto.gen`
   gaining a `balaur` target is the other answer and keeps one proto
   reader. Start with the script; move when the task grows a second
   consumer.
4. **The Godot façade from the table.** Once `events.json` exists, the 84
   signals and their dispatch in `GamendApi.gd` could be generated from it
   too, retiring the one place the mapping lives as code. Gamend's roadmap.
