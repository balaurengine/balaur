# Plan: WebGL2 beside WebGPU on the web

`balaur export --target web` produces a WebGPU build. A browser without WebGPU
gets a blank canvas and an error, which is a worse first impression than a
slower picture would be.

wgpu already has a GL backend, so this is not a renderer to write. It is a
question of what the second backend costs in bytes, what it cannot do, and how
the runtime chooses.

## Steps

1. **Turn the backend on and see what breaks.** wgpu's GL backend targets
   WebGL2. Build the web template with it and run `examples/hello` and
   `examples/angrynerds`. The failures will be specific and worth listing
   before designing anything around them.
2. **Find the shaders that do not translate.** WESL compiles to WGSL and naga
   lowers it; WebGL2 has no compute shaders and no storage buffers, so
   anything that reaches for either needs a fallback path or is simply absent
   on this backend. The planned compute particle stepper in
   `docs/PLAN-particles.md` is the clearest case.
3. **Choose at boot.** `navigator.gpu` present or not, decided once when the
   runtime starts, with the choice printed where the size report and the
   console can both show it. No per-frame switching and no user-facing toggle.
4. **Measure the module.** One wasm carrying both backends is simpler to ship
   and larger to download; two modules with a picker is the opposite. Measure
   against the current web template before choosing, and keep the comparison
   next to the numbers in `docs/BENCHMARKS.md`.

## What does not change

**The simulation.** The renderer is not in the tick digest, so a game running
on WebGL2 produces the same bits as the same game on WebGPU, and a recording
made on one replays and verifies on the other. Determinism is a property of the
fixed step, not of what draws afterwards — this is the reassuring half of the
plan and worth saying out loud in the manual when it lands.

## What is worse on WebGL2

Say so plainly rather than letting a player discover it: no compute, fewer
render targets, and the finishing passes are the first place that will show.
The honest shape is a good picture on WebGPU and a working picture on WebGL2,
not parity.

Related: `docs/PLAN-embed.md` covers the web module and how a page loads it,
and the roadmap's `A game on a small machine` row wants the same GL backend for
`linux-arm64`, so the two share whatever this turns up.
