# Protecting a shipped game

How much harder each step makes it to take textures, scenes, scripts and the
engine's symbols out of an exported game. A shipped game holds the key that
opens it, so every step stops casual extraction and nothing more, and the
manual says so beside each flag. Not anti-cheat and not DRM: a score that
matters is written by a server.

## Steps

1. **Strip the shipped binary.** `strip = true` in `[profile.release]` of
   `Cargo.toml`, which sets only `lto` and `codegen-units` today, while
   `[profile.web]` already strips. `build.yml` uploads the symbols as their
   own artifact per template: a `dSYM` on macOS, the `.pdb` on Windows,
   `objcopy --only-keep-debug` on Linux. Done: `scripts/export_check.sh`
   fails a desktop template whose `nm --defined-only` prints a symbol, or a
   Windows download holding a `.pdb`.
2. **The seal.** `BPAK\x03` in `crates/balaur_core/src/pack.rs` (today
   `BPAK\x02`): the v2 body under ChaCha20-Poly1305 from the
   `chacha20poly1305` crate, which builds for wasm32. The key is 32 random
   bytes in a project-relative file, `[export] pack_key`, written by
   `balaur export --new-key`, which adds it to `.gitignore`. The nonce is the
   first twelve bytes of SHA-256 over key and body, so two exports stay
   byte-identical. The exporter writes the key ahead of the sealed pack.
   `Pack::decode` reads v2 and v3, the web runtime opens a sealed pack, and
   the editor's Export sheet gains a checkbox. Done: `strings game.bpak`
   prints only the magic, a wrong key fails with a named error, an `.app`
   holding a sealed pack passes `codesign --verify --deep`, and the matrix
   diff still holds.
3. **The key in the template.** `option_env!("BALAUR_PACK_KEY")` in the
   runtime, and `--key-in-template` on the exporter writing no key, for a
   developer who builds their own template. Done: a sealed game whose key is
   not at an offset the docs name.
4. **Names out of a stored unit.** `crates/balaur_bs/src/stored.rs` writes
   each unit's function keys, structs and fields into the pack, since the
   host finds `Type.update` by key. String literals stay, being data. When a
   game asks, measure which names a shipped unit can drop. Done: a test that
   a shipped unit holds no name the host never looks up.

Not planned: compression on its own, since zstd goes under the seal if web
size asks. Packers and virtualisers such as Denuvo or VMProtect rewrite the
executable a pack is appended to. GPU-compressed textures at export,
anti-debugging, encrypted saves, licence keys and Subresource Integrity on
the web shell are out too.

## Open questions

1. **A Rust game that embeds the engine** decodes its own pack from
   `include_bytes!`. Does the facade offer `Pack::open_sealed(bytes, key)`,
   or read the same `option_env!` the template does?
2. **One key per project or per release?** Per project keeps every export
   diffable against the last. Per project, unless a game asks.
