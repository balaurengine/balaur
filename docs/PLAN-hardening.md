# Plan: hardening

The engine promises that the same inputs give the same bits everywhere, that
a bad file is an error and never a crash, and that CI proves both. These are
the gaps in that proof that are still open.

## Steps

1. **A GPU job.** A CI job on a software adapter renders frames, so a shader
   or a pass that breaks fails a push. `scripts/pictures.py` compares each
   example's offscreen frame with `scripts/pictures/`, but those are the
   release Mac's, so the job keeps its own set from the adapter it runs on.
   Where: `.github/workflows/test.yml`. Done: the job runs on every push and
   fails when a frame moves. It is not added until a runner is shown to get
   an adapter every time, since one that cannot turns `main` red for no fault
   of the commit.
2. **Both halves of the C ABI.** `sizeof(BalaurApi)` is asserted in neither
   `crates/balaur_plugin/include/balaur_extension.h` nor
   `the_rust_types_match_the_committed_header` in
   `crates/balaur_plugin/tests/suite/c_extension.rs`, and the Rust side does
   not assert `BalaurMapRef` or `BalaurPayload`. Done: every struct the header
   declares is pinned on both sides.
3. **Behaviours no test holds.** A desync detected: every test in
   `crates/balaur_net/tests/suite/` and `crates/balaur_multiplayer/tests/`
   asserts `desync() == None`. Also `presets.toml` in a packed run, a second
   `scene.instantiate` of one file, and `settings::faults` with integer
   values. Done: a test for each.
4. **A WebSocket handshake with a deadline.** A silent server leaves a
   connection `Connecting` with no event, since neither `frames.rs` nor
   `listener.rs` in `crates/balaur_websocket/src` bounds a handshake. The
   deadline is a `balaur_core::wake::at`, never a timed wait. Done: a test
   whose server never answers sees the link close with a reason.
