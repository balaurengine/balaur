> **Status:** not started. Written 2026-09-06, after "how safe is a shipped
> game against decompilation" got the honest answer: it is not, by design.
> The pack is length-prefixed raw bytes, the web pack carries the script
> sources, and the release binary keeps its symbol table. The steps are
> ordered by what each costs against what it buys: stripping is one line;
> the bytecode is portable already and only the web exception says
> otherwise; a cipher on the pack is a speed bump and is called one; nothing
> stronger is planned. No step here stops a determined reader, and every
> step says so, because a protection that claims more than it does is the
> one that costs a developer a launch.

# Plan: protecting a shipped game

What a player can take out of an exported game — textures, sounds, scenes,
scripts, the engine's own symbols — and how much harder each step makes it.
Not anti-cheat and not DRM: `docs/PLAN-steam.md` §2 rules out VAC, Easy
Anti-Cheat and the Steam DRM wrapper, and §1 below says why a server, not a
cipher, is the answer to a cheated score.

## 0. Where the tree is today

Built, and not built for this:

| Have | Where |
| --- | --- |
| One pack per export: manifest, scenes, scripts and assets, length-prefixed, in sorted order so two exports are byte-identical and CI can diff them | `balaur_core::pack`, `Pack::encode` |
| Scripts as a compiled Rune unit on native targets, debug info dropped | `balaur_script_rune::packed`, `options.debug_info(Purpose::Dev)` in `lib.rs` |
| A test that states what the unit still spells out: every function name, private ones included, field names, string literals | `crates/balaur_script_rune/tests/packed.rs` |
| A SHA-256 per asset, checked on decode | `pack::content_hash`, `Pack::decode` |
| Code signing on every target: `codesign` and notarization, Authenticode, `apksigner`, an `.ipa` | `balaur_export::sign`, `android.rs` |
| A stripped, `opt-level = "z"` web module | `[profile.web]` in `Cargo.toml` |
| Extensions that reach the host through a table of function pointers, so the executable exports no symbols of its own | `balaur_plugin::capi::BalaurApi` |
| Runtime templates verified against the release's `SHA256SUMS`, and provenance attested for what CI builds | `balaur_cli::templates`, `attest-build-provenance` in `build.yml` |

Missing:

- **Anything on the pack.** No compression and no cipher. A fused
  executable's pack starts where a `u64` at `len - 16` says, and each entry
  inside is the original file: `Pack::entries` is a public function that
  returns the project tree. `strings` on a pack prints every scene, comments
  included.
- **A compiled script on the web.** `export` forces `keep_sources` for
  `Bundle::Web` (`crates/balaur_export/src/lib.rs`), the browser exporter
  hard-codes it (`web_export.rs`), and `scripts/package_play.sh` asks for it
  so the editor's code panel has text to show. `strings dist/play/angrynerds.bpak`
  prints the game. The reason on record — `usize` sentinels that do not read
  back on 32-bit — predates `packed::FORMAT` 2, which sends stack offsets as
  `u32` for exactly that case, and `docs/PLAN-embed.md` §0 already lists
  compiled packs among what runs on the 32-bit runtime. One of the two is
  wrong; §5 asks which.
- **A stripped native binary.** `[profile.release]` sets `lto` and
  `codegen-units` and nothing else, so the release `balaur` carries its whole
  symbol table — some 53 000 symbols in 49 MB on this machine. Every runtime
  template CI builds with `cargo build --release` inherits that, and every
  fused game with it.
- **Names out of a unit.** Rune keeps function names as static strings in
  `Logic`, beside the string literals a script uses, whether or not debug
  info is dropped.
- **A page that says so.** The manual's shipping page reads "no script
  sources exist in the shipped build" — true of a desktop export, false of a
  web one.

## 1. Design

**The bar, stated once.** A shipped game holds every byte a player needs to
run it, so it holds every byte needed to read it; the key that opens a
sealed pack is in the file that opens it. Godot's PCK encryption, Unity's
IL2CPP and Unreal's pak encryption are each undone by a public tool for that
reason, and this engine will be no different. What the steps below buy is
that casual extraction stops working — `strings`, a hex editor, a
thirty-line script, a generic unpacker — and nothing more. The manual says
that in those words, next to the flag.

**Cheating has a different answer.** A pack nobody can read still runs on a
machine the player owns and can single-step. A score, an unlock or a match
result that matters is written by a server that saw it happen:
`docs/PLAN-gamend.md` §1 (leaderboard writes come from the game server, never
from the client), `docs/PLAN-sessions.md` (server-ordered inputs and digest
verification). Nothing here changes what a client is trusted with, which is
nothing.

**Strip the shipped binary, keep the symbols beside it.** `strip = true` on
`[profile.release]`. The extension seam does not need exported symbols —
`capi.rs` hands an extension a table for exactly that reason — `panic =
"unwind"` is unaffected, since strip removes the symbol table and debug
info and not the unwind tables, and a Rune runtime error names script frames
from the unit, not from the binary. What is lost is a symbolised native
panic in a bug report. The report the roadmap wants (a crash report that
reproduces itself) is a recording plus a build id rather than a stack, and
for the stack CI keeps the symbols as a separate artifact per template — a
`dSYM` on macOS, the `.pdb` on Windows, `objcopy --only-keep-debug` on
Linux — so a developer symbolises a player's crash against their own build.

**The web exception goes, or gets a reason.** A test on
`wasm32-unknown-unknown` decodes and runs a unit compiled on the host. If it
passes, the three places that force sources lose their reason,
`--keep-sources` becomes the opt-in its name says (the site's editor keeps
asking for it), and a web game ships bytecode like every other target. If it
fails, the failing sentinel is named, fixed in the fork, and `FORMAT` moves
to 3. Either way the comment that says "until the bytecode format is
portable" stops being a guess.

**A sealed pack.** `BPAK\x03`: the v2 body under ChaCha20-Poly1305, and
`Pack::decode` reads both. The parts:

- `chacha20poly1305` from RustCrypto: pure Rust, builds on every target the
  engine has including wasm32, and its `chacha20` core is already in the
  lock through `symphonia` (one copy or two depends on the pins lining up).
  Not `ring`: it is in the tree through `rustls` and out of the web build,
  and its AEAD API is not the shape a file format wants.
- **The key** is 32 random bytes in a project-relative file,
  `[export] pack_key = "export.key"`, written by `balaur export --new-key`,
  which also adds the line to the project's `.gitignore`, creating one if
  there is none. Not in `project.toml`: that file is committed, and the
  editor shows it.
- **The nonce is derived, not drawn**: the first twelve bytes of SHA-256
  over the key and the plaintext body. One key and one body give one
  ciphertext, so two exports of the same project are still byte-identical on
  any machine, and the diff CI runs across the matrix keeps its meaning.
  What that leaks is that two packs are the same pack, which is the
  reproducible-build feature by another name.
- **Where the key rides.** By default the exporter writes it ahead of the
  pack, so a fused executable ends `[template][key][sealed pack][len]["BPAKSEAL"]`
  and a bundle's `game.bpak` starts with it. That is the speed bump in
  full: a generic unpacker and `strings` find nothing, and a reader of
  `standalone.rs` finds the key in one step. A developer who wants the key
  out of a documented offset builds their own template with
  `BALAUR_PACK_KEY` set, the runtime reads it through `option_env!`, and the
  exporter is told (`--key-in-template`) to write none. Still a speed bump —
  the key is in `.rodata` — but no longer a documented one.
- **On the web** the sealed `game.bpak` is fetched as before and opened in
  the module; the key is in the file or in the module, and either way in the
  browser's cache. A sealed web pack keeps scripts and scenes out of
  `strings` and out of the network tab's preview, and that is all it does.

**What a seal does not change.** The pack is still held whole in memory
(the roadmap's asset streaming item is unrelated and stays so), a `.app`
still signs the same way since the sealed file is a resource like the plain
one was, and `balaur play game.bpak` still opens a sealed pack, because the
key is in it. Compression is not part of the seal: the assets a pack carries
are already compressed formats, and text seals as well as it zips. If
download size asks, zstd goes under the seal, not instead of it.

## 2. The surface

Every measure a reader might ask about, and where each stands.

| Measure | Decision |
| --- | --- |
| A stripped release binary and template | Step 1 |
| Symbols kept for the developer | Step 1: a separate artifact per template, never in the download |
| Bytecode on the web | Step 2 |
| A sealed pack: ChaCha20-Poly1305, a project key, a derived nonce | Step 3 |
| The key compiled into a developer-built template | Step 4 |
| Function names out of a unit | Step 5, when a game asks. String literals stay: they are data, and a dialogue line or a URL has to be in the file somewhere |
| Code signing: `codesign`, notarization, Authenticode, `apksigner` | Have. It proves who built the file and that nobody changed it since; it hides nothing |
| A SHA-256 per asset | Have. It catches a truncated or corrupt entry. It is not a defence: whoever edits an asset recomputes the hash with the same public function |
| Build provenance | Have: `attest-build-provenance` on what a push to `main` exports |
| GPU-compressed textures at export | `docs/PLAN-textures.md` step 3. A side effect worth naming: a KTX2 in the pack is no longer the artist's PNG |
| Compression of the pack (`zstd`) | Not planned on its own; under the seal if web size asks. On its own it hides nothing from anyone with `zstd` installed |
| Steam DRM wrapper | Not planned, as `docs/PLAN-steam.md` §2 says: it rewrites the executable the exporter appends a pack to |
| Denuvo, VMProtect, Themida and other packers or virtualisers | Not planned. Each rewrites the executable, which breaks the trailer, the signature and the reproducible build, and each is a licensed product the engine cannot ship |
| Anti-debugging and run-time self-checks | Not planned. They stop `balaur`'s own debugger and the DAP, trip antivirus heuristics, and each is removed with one byte |
| VAC, Easy Anti-Cheat, BattlEye | Not planned; `docs/PLAN-steam.md` §2. Authority on a server is the engine's answer |
| Source-level script obfuscation | Not planned. The unit is already the form without source, and what it keeps is step 5's job |
| Native compilation of scripts | Not planned. Rune has no native backend, and interpretation is what iOS allows |
| Watermarking textures or audio | Not planned. A studio tracing a leak does it in its art pipeline, not in the exporter |
| Encrypting saves | Not planned. A save is the player's file; anything that matters lives on a server (`docs/PLAN-gamend.md`) |
| Licence keys, online activation | Not planned. That is a store's job, and playing offline is a feature |
| Subresource Integrity on the web shell | Not planned. The host serves the page, the glue, the module and the pack; a host that changes one can change the hashes with it |

## 3. Steps

Ordered by what each costs against what it buys; each leaves something a
developer can turn on.

1. **Strip.** `strip = true` in `[profile.release]`; `build.yml` uploads the
   symbols as their own artifact per template; a check in
   `scripts/export_check.sh` that `nm --defined-only` prints nothing for a
   desktop template on Linux and macOS, and that no `.pdb` sits in the
   Windows download (MSVC keeps names there, never in the executable). Ends
   with: a fused game with an empty symbol table.
2. **Bytecode on the web.** The wasm32 test from §1; then the force in
   `balaur_export`, the hard-coded `true` in `web_export.rs`, and the
   `--keep-sources` help text lose the pointer-width reason;
   `scripts/package_play.sh` keeps the flag, with its comment as the reason;
   the manual's shipping page says which targets ship source (none) and what
   the flag is for. Ends with: a web export whose `game.bpak` shows no
   `pub fn` under `strings`.
3. **The seal.** `BPAK\x03`, `chacha20poly1305`, `[export] pack_key`,
   `balaur export --new-key`, the key ahead of the pack, `Pack::decode`
   reading v2 and v3, `web.rs::start` opening a sealed pack, and a checkbox
   in the editor's Export sheet. A test that a sealed pack contains none of
   the fragments `tests/packed.rs` checks for, that a wrong key fails with a
   named error, and that the reproducibility diff still holds. The manual
   calls it a speed bump in those words. Ends with: `strings game.bpak`
   prints the magic.
4. **The key in the template.** `option_env!("BALAUR_PACK_KEY")` in the
   runtime, `--key-in-template` on the exporter, and a section under
   building your own template. Ends with: a sealed game whose key is not at
   an offset the docs name.
5. **Names out of a unit.** A `strip_names` option on the fork's compiler;
   measure what breaks (calls by name are hash lookups already; `Debug`
   output and runtime errors lose names), and `tests/packed.rs` flips its
   second test from "survives" to "is gone", as its own comment invites.
   When a game asks.

## 4. What CI can prove, and what it cannot

- the release binary and each desktop template carry no engine symbol
- a unit compiled on the host decodes and runs on wasm32 — through
  `wasm-bindgen-test` in Node, which the web template job has the toolchain
  for, or failing that in the headless page `export_check.sh web` drives
- a sealed pack contains no plaintext fragment, opens with its key, refuses a
  wrong one by name, still opens as v2 when unsealed, and exports
  byte-identical across the matrix as before
- an `.app` with a sealed resource still passes `codesign --verify --deep`

What it cannot: that any of it protects anything. A test can show `strings`
finds nothing; it cannot show a reader gives up, and no test here claims to.

## 5. Open questions

1. **Does a compiled unit load on wasm32 today?** `FORMAT` 2 and
   `docs/PLAN-embed.md` say yes; `balaur_export` and `web_export.rs` say no,
   one day later. The test in step 2 decides, and whichever way it goes, two
   comments are wrong today.
2. **A Rust game that embeds the engine.** Such a game hands
   `AppConfig::packed` a pack it decoded from `include_bytes!` itself, so
   the seal is opened before the engine sees it. The developer built the
   binary and the key is theirs to place; the open part is only whether the
   facade offers `Pack::open_sealed(bytes, key)` or reads the same
   `option_env!` the template does.
3. **Should the editor open a sealed pack?** It can, since the key is in the
   file, and refusing would protect nothing. Whether the Open dialog should
   list `.bpak` at all is an editor question, not this plan's.
4. **One key per project or per release?** Per project keeps every export
   diffable against the last; per release makes each build's bytes new,
   which is what the CI diff exists to catch. Per project, unless a game
   asks.
