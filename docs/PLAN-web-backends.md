# Plan: WebGL2 beside WebGPU on the web

The `webgl` feature links wgpu's GL backend, 1.0 MB of a web module
(`docs/generated/features.md`). The editor's web module has it on and the web
game template leaves it off, so a page without WebGPU gets an error naming
the missing fallback (`drawable` in `crates/balaur_cli/src/web.rs`). What is
left is an exported game that draws there.

## Steps

1. **A game drawn on WebGL2.** Build the web runtime with `webgl` and run
   `examples/hello` and `examples/angrynerds` in a browser without WebGPU.
   WebGL2 has no compute shaders and no storage buffers, so whatever reaches
   for them is absent on this backend and its help says so, as the
   many-light pass in `light3d.rs` and the wireframe in `debug_draw.rs` do.
   The compute particle stepper in `docs/PLAN-particles.md` is the next case.
   Done: both examples draw in a browser without WebGPU, and every pass that
   does not draw there names WebGL2 in its help.
2. **Choose at boot and say which.** `navigator.gpu` present or not, decided
   once when the runtime starts, with the choice printed where the size
   report and the console can both show it. No per-frame switching and no
   user-facing toggle. Done: the console names the backend a page drew with.
3. **Ship it in the game template.** One wasm carrying both backends, two
   modules with a picker, or `docs/PLAN-build-size.md`'s shape: the GL
   backend as a part only a browser without WebGPU fetches. Measure each
   against the current web runtime and keep the comparison next to the
   numbers in `docs/BENCHMARKS.md`. Done: an exported web game draws on a
   page without WebGPU, at a size `docs/BENCHMARKS.md` records.

## What does not change

**The simulation.** The renderer is not in the tick digest, so a game running
on WebGL2 produces the same bits as the same game on WebGPU, and a recording
made on one replays and verifies on the other. Determinism is a property of the
fixed step, not of what draws afterwards. The manual says so on the page that
covers the fallback.

## What is worse on WebGL2

The manual names it before a player finds it: no compute, fewer render
targets, and the finishing passes show it first. The aim is a good picture on
WebGPU and a working picture on WebGL2, not parity.

Related: `docs/PLAN-embed.md` covers how a page loads the web module,
`docs/PLAN-build-size.md` what the module weighs, and the roadmap's `A game on
a small machine` row wants the same GL backend for `linux-arm64`, so the two
share whatever this turns up.
